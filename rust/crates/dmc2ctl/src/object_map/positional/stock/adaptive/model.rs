//! Bayesian implicit surface in a finite random Fourier basis.
//! All selected fit contacts contribute; no initial placement/support gate.
use super::super::surface;
use super::{random::Random, request::Request};
use crate::object_map::{
    positional::{geometry::*, request::Use},
    Error,
};

pub struct Prediction {
    pub mean: f64,
    pub variance: f64,
}
pub struct Model {
    frequencies: Vec<V>,
    amplitude: f64,
    pub origin: V,
    lower: Vec<Vec<f64>>,
    weights: Vec<f64>,
    pub observations: usize,
    pub gradients: usize,
    pub checks: Vec<(usize, f64, f64)>,
    pub empty_checks: Vec<(usize, V, f64, f64)>,
    pub discrepancy: f64,
    pub empty_observations: usize,
    pub posterior_iterations: usize,
    pub posterior_converged: bool,
}
impl Model {
    pub fn json(&self) -> String {
        let rows = self
            .lower
            .iter()
            .enumerate()
            .map(|(i, row)| {
                format!(
                    "[{}]",
                    row.iter()
                        .enumerate()
                        .map(|(j, x)| if j <= i { x.to_string() } else { "0".into() })
                        .collect::<Vec<_>>()
                        .join(",")
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("{{\"schema\":\"dmc2.bayesian-stock-field.v1\",\"frame\":\"LinuxCNC machine-mm\",\"origin_machine_mm\":{},\"frequencies_per_mm\":[{}],\"feature_amplitude_mm\":{},\"weights\":{:?},\"posterior_precision_cholesky\":[{rows}],\"discrepancy_variance_mm2\":{},\"converged\":{},\"definition\":\"For each frequency w, append amplitude*cos(dot(w,p-origin)), amplitude*sin(dot(w,p-origin)) to phi. Mean signed distance = dot(phi,weights); positive is estimated air, negative is estimated material. Solve L*y=phi, then variance = dot(y,y)+discrepancy_variance_mm2. L is the lower-triangular posterior precision Cholesky factor; it is not covariance. This is a noisy approximate field, not a certified stock boundary.\"}}",json(self.origin),self.frequencies.iter().map(|w|json(*w)).collect::<Vec<_>>().join(","),self.amplitude,self.weights,self.discrepancy,self.posterior_converged)
    }
    fn features(&self, p: V, derivative: Option<usize>) -> Result<Vec<f64>, Error> {
        let d = sub(p, self.origin);
        let mut values = Vec::with_capacity(self.frequencies.len() * 2);
        for w in &self.frequencies {
            let angle = dot(*w, d);
            if !angle.is_finite() {
                return Err(Error::Data("Adaptive surface coordinates overflow the basis. Inspect units and frame before recalculating.".into()));
            }
            let (s, c) = angle.sin_cos();
            let (a, b) = match derivative {
                None => (c, s),
                Some(axis) => (-s * w[axis], c * w[axis]),
            };
            values.extend([a * self.amplitude, b * self.amplitude]);
        }
        Ok(values)
    }
    pub fn fit(source: &surface::Loaded, r: &Request, random: &mut Random) -> Result<Self, Error> {
        let n = r.basis_pairs.checked_mul(2).ok_or_else(|| {
            Error::Input("The adaptive basis size overflows. Reduce basis_pairs.".into())
        })?;
        n.checked_mul(n).filter(|&size| size <= r.matrix_entries).ok_or_else(|| Error::Input("The Bayesian basis exceeds max_matrix_entries. Increase that computation budget or explicitly select a smaller basis; contacts were not discarded.".into()))?;
        let origin = source.contacts.first().map(|s| s.center).unwrap_or([0.; 3]);
        let mut model = Self {
            frequencies: (0..r.basis_pairs)
                .map(|_| std::array::from_fn(|_| random.normal() / r.length))
                .collect(),
            amplitude: r.scale / (r.basis_pairs as f64).sqrt(),
            origin,
            lower: vec![vec![0.; n]; n],
            weights: vec![0.; n],
            observations: 0,
            gradients: 0,
            checks: Vec::new(),
            empty_checks: Vec::new(),
            discrepancy: 0.,
            empty_observations: 0,
            posterior_iterations: 0,
            posterior_converged: false,
        };
        // Unit-normal prior on basis coefficients; physical scale belongs to
        // the explicit field_scale_mm, not an invented pseudo-measurement.
        for i in 0..n {
            model.lower[i][i] = 1.;
        }
        let mut rhs = vec![0.; n];
        let accumulate = |a: &mut Vec<Vec<f64>>,
                          b: &mut Vec<f64>,
                          x: Vec<f64>,
                          y: f64,
                          sigma: f64|
         -> Result<(), Error> {
            let x = x.into_iter().map(|v| v / sigma).collect::<Vec<_>>();
            let y = y / sigma;
            if !y.is_finite() || x.iter().any(|v| !v.is_finite()) {
                return Err(Error::Input("Adaptive likelihood scaling overflowed. Inspect the stated measurement noise and field scale.".into()));
            }
            for i in 0..x.len() {
                b[i] += x[i] * y;
                for j in 0..=i {
                    a[i][j] += x[i] * x[j];
                }
            }
            Ok(())
        };
        for (index, s) in source
            .contacts
            .iter()
            .enumerate()
            .filter(|(_, s)| s.usage == Use::Fit)
        {
            // A corrected contacting sphere centre is one ball radius outside
            // the surface. It does not require an assumed approach normal.
            let x = model.features(s.center, None)?;
            accumulate(
                &mut model.lower,
                &mut rhs,
                x,
                source.request.probe.radius,
                r.contact_sigma,
            )?;
            model.observations += 1;
            if let Some(patch) = source
                .stations
                .iter()
                .find(|t| t.seed == index)
                .and_then(|t| t.result.as_ref().ok())
            {
                // Estimated normal is a noisy derivative observation. Failed
                // normal fits leave this derivative unknown, retaining contact.
                for axis in 0..3 {
                    let x = model.features(s.center, Some(axis))?;
                    accumulate(
                        &mut model.lower,
                        &mut rhs,
                        x,
                        patch.normal[axis],
                        r.normal_sigma,
                    )?;
                    model.gradients += 1;
                }
            }
        }
        let mut censored = Vec::new();
        let mut empty_checks = Vec::new();
        for (index, sweep) in source.no_contact.iter().enumerate() {
            let checking=source.source.optional(&format!("capture-{}.followup.txt",sweep.capture.as_str())).map(|raw| {
                let plan=crate::probe_data::top_followup::Plan::read(std::str::from_utf8(raw).map_err(|e|Error::Data(format!("Retained follow-up plan is not UTF-8: {e}. Select the intact acquisition source.")))?).map_err(Error::Data)?;
                Ok::<bool,Error>(plan.role==Some(crate::probe_data::top_followup::Role::Check))
            }).transpose()?.unwrap_or(false);
            let length = norm(sub(sweep.center_end, sweep.center_from));
            let count = (length / r.length).ceil().max(1.);
            if !count.is_finite() || count >= usize::MAX as f64 {
                return Err(Error::Input("Empty-path conditioning count overflows. Inspect the modelling length and retained path units.".into()));
            }
            let parts = count as usize;
            let added = parts
                .checked_add(1)
                .ok_or_else(|| Error::Input("Empty-path conditioning count overflows.".into()))?;
            if censored
                .len()
                .checked_add(added)
                .and_then(|n| n.checked_add(empty_checks.len()))
                .and_then(|n| n.checked_add(model.observations + model.gradients))
                .is_none_or(|n| n > r.conditioning)
            {
                return Err(Error::Input("All contacts and finite empty paths exceed max_conditioning_observations. Increase that computation budget; no path or contact was silently dropped.".into()));
            }
            for i in 0..=parts {
                let point = add(
                    sweep.center_from,
                    scale(
                        sub(sweep.center_end, sweep.center_from),
                        i as f64 / parts as f64,
                    ),
                );
                if checking {
                    empty_checks.push((index, point, sweep.radius));
                    continue;
                }
                censored.push(super::posterior::Censored {
                    x: model
                        .features(point, None)?
                        .into_iter()
                        .map(|v| v / r.empty_sigma)
                        .collect(),
                    lower: sweep.radius / r.empty_sigma,
                    weight: 1. / added as f64,
                });
            }
        }
        if model.observations + model.gradients > r.conditioning {
            return Err(Error::Input("Contact conditioning exceeds max_conditioning_observations. Increase the explicit computation budget; the source selection was preserved.".into()));
        }
        let posterior = super::posterior::fit(
            &model.lower,
            &rhs,
            &censored,
            r.posterior_iterations,
            r.posterior_tolerance,
        )?;
        model.empty_observations = censored.len();
        model.lower = posterior.lower;
        model.weights = posterior.mean;
        model.posterior_iterations = posterior.iterations;
        model.posterior_converged = posterior.converged;
        for (index, point, lower) in empty_checks {
            let prediction = model.predict(point)?;
            model.empty_checks.push((
                index,
                point,
                (lower - prediction.mean).max(0.),
                prediction.variance,
            ));
        }
        // Checks do not train the mean. Their residuals provide a retained
        // model-discrepancy term rather than being thrown away as bad data.
        for (i, s) in source
            .contacts
            .iter()
            .enumerate()
            .filter(|(_, s)| s.usage == Use::Check)
        {
            let p = model.predict(s.center)?;
            let residual = p.mean - source.request.probe.radius;
            model.checks.push((i, residual, p.variance));
        }
        if !model.checks.is_empty() {
            model.discrepancy = model
                .checks
                .iter()
                .map(|(_, d, v)| (d * d - v - r.contact_sigma * r.contact_sigma).max(0.))
                .sum::<f64>()
                / model.checks.len() as f64;
        }
        if !model.discrepancy.is_finite() {
            return Err(Error::Data("Adaptive independent residuals overflowed. Inspect the original check captures and their shared frame.".into()));
        }
        Ok(model)
    }
    fn forward(&self, x: &[f64]) -> Vec<f64> {
        let mut y = vec![0.; x.len()];
        for i in 0..x.len() {
            y[i] = (x[i] - (0..i).map(|j| self.lower[i][j] * y[j]).sum::<f64>()) / self.lower[i][i];
        }
        y
    }
    pub fn predict(&self, p: V) -> Result<Prediction, Error> {
        let x = self.features(p, None)?;
        let mean = x.iter().zip(&self.weights).map(|(a, b)| a * b).sum::<f64>();
        let y = self.forward(&x);
        let variance = y.iter().map(|x| x * x).sum::<f64>() + self.discrepancy;
        if !mean.is_finite() || !variance.is_finite() {
            return Err(Error::Data("Adaptive field prediction overflowed. Inspect the model scales and requested coordinates.".into()));
        }
        Ok(Prediction { mean, variance })
    }
    /// Expected reduction in latent variance at target after one noisy scalar
    /// contact at observation. Ranking is conditional on contact being possible.
    pub fn information(&self, target: V, observation: V, noise: f64) -> Result<f64, Error> {
        let a = self.forward(&self.features(target, None)?);
        let b = self.forward(&self.features(observation, None)?);
        let covariance = a.iter().zip(&b).map(|(a, b)| a * b).sum::<f64>();
        let variance = b.iter().map(|v| v * v).sum::<f64>() + noise * noise + self.discrepancy;
        let gain = covariance * covariance / variance;
        if !gain.is_finite() {
            return Err(Error::Data("Adaptive information gain overflowed. Inspect the stated noise before planning new observations.".into()));
        }
        Ok(gain)
    }
}
