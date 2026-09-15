use crate::object_map::{
    model::Id,
    positional::request::{scalar, vector},
    record, Error,
};
pub const SCHEMA: &str = "DMC2_ADAPTIVE_STOCK_REQUEST_V1";
pub const KEYS: &[&str] = &[
    "surface_analysis",
    "source_capture",
    "design",
    "stl_mm_per_unit",
    "model_origin_x_bounds_mm",
    "model_origin_y_bounds_mm",
    "model_origin_z_bounds_mm",
    "roll_bounds_deg",
    "pitch_bounds_deg",
    "yaw_bounds_deg",
    "required_clearance_mm",
    "field_length_mm",
    "field_scale_mm",
    "contact_sigma_mm",
    "normal_sigma",
    "uncertainty_multiplier",
    "basis_pairs",
    "max_matrix_entries",
    "cover_radius_mm",
    "max_cover_samples",
    "population",
    "max_evaluations",
    "mutation_scale",
    "crossover_probability",
    "random_seed",
    "observation_candidates",
    "max_observations",
    "ray_resolution_mm",
    "empty_space_sigma_mm",
    "posterior_iterations",
    "posterior_weight_tolerance",
    "max_conditioning_observations",
    "acquisition_start_work_mm",
    "acquisition_drop_mm",
    "interior_samples",
    "interior_candidates",
];
pub struct Request {
    pub surface: Id,
    pub capture: Id,
    pub design: Id,
    pub units: f64,
    pub lo: [f64; 6],
    pub hi: [f64; 6],
    pub clearance: f64,
    pub length: f64,
    pub scale: f64,
    pub contact_sigma: f64,
    pub normal_sigma: f64,
    pub confidence: f64,
    pub basis_pairs: usize,
    pub matrix_entries: usize,
    pub radius: f64,
    pub covers: usize,
    pub population: usize,
    pub evaluations: usize,
    pub mutation: f64,
    pub crossover: f64,
    pub seed: u64,
    pub observation_candidates: usize,
    pub observations: usize,
    pub ray_resolution: f64,
    pub empty_sigma: f64,
    pub posterior_iterations: usize,
    pub posterior_tolerance: f64,
    pub conditioning: usize,
    pub acquisition_start: [f64; 3],
    pub acquisition_drop: f64,
    pub interior_samples: usize,
    pub interior_candidates: usize,
}
impl Request {
    pub fn read(raw: &[u8]) -> Result<Self, Error> {
        let (f, body) = record::decode(raw, SCHEMA, KEYS)?;
        if !body.is_empty() {
            return Err(Error::Input("The adaptive request uses a retained surface revision. Put new capture selections in its next surface revision, not in an extra request payload.".into()));
        }
        let positive = |k: &str| -> Result<f64, Error> {
            let x = scalar(&f[k], k)?;
            if x > 0. {
                Ok(x)
            } else {
                Err(Error::Input(format!(
                    "{k} must be positive. Correct the adaptive request."
                )))
            }
        };
        let count = |k: &str| -> Result<usize, Error> {
            f[k].parse::<usize>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| {
                    Error::Input(format!(
                        "{k} requires a positive computation or observation budget."
                    ))
                })
        };
        let mut lo = [0.; 6];
        let mut hi = [0.; 6];
        for (i, k) in KEYS[4..10].iter().enumerate() {
            let (a, b) = f[*k]
                .split_once(',')
                .ok_or_else(|| Error::Input(format!("{k} requires lower,upper bounds.")))?;
            let (a, b) = (scalar(a, k)?, scalar(b, k)?);
            if a > b || !(b - a).is_finite() || (i >= 3 && b - a > 360.) {
                return Err(Error::Input(format!("{k} requires finite ordered bounds, at most one full rotation. Equal endpoints freeze that coordinate.")));
            }
            lo[i] = if i < 3 { a } else { a.to_radians() };
            hi[i] = if i < 3 { b } else { b.to_radians() };
        }
        let clearance = scalar(&f["required_clearance_mm"], "required_clearance_mm")?;
        if clearance < 0. {
            return Err(Error::Input("required_clearance_mm cannot be negative. Required geometry is never reduced to obtain a fit.".into()));
        }
        let population = count("population")?;
        let evaluations = count("max_evaluations")?;
        if population < 4 || evaluations < population {
            return Err(Error::Input("Differential evolution needs a population containing a parent and three distinct donors, and enough evaluations to initialize it. Increase population/max_evaluations.".into()));
        }
        let crossover = positive("crossover_probability")?;
        if crossover > 1. {
            return Err(Error::Input(
                "crossover_probability must be in (0,1].".into(),
            ));
        }
        let seed = f["random_seed"]
            .parse::<u64>()
            .ok()
            .filter(|n| *n != 0)
            .ok_or_else(|| {
                Error::Input(
                    "random_seed must be a nonzero unsigned integer, retained for replay.".into(),
                )
            })?;
        let observations = count("max_observations")?;
        let observation_candidates = count("observation_candidates")?;
        if observations > observation_candidates {
            return Err(Error::Input("max_observations exceeds observation_candidates. Increase the candidate budget or reduce this batch size.".into()));
        }
        let interior_samples = count("interior_samples")?;
        let interior_candidates = count("interior_candidates")?;
        if interior_candidates < interior_samples {
            return Err(Error::Input("interior_candidates must cover the requested interior_samples and rejected outside points. Increase that numerical budget.".into()));
        }
        Ok(Self {
            interior_samples,
            interior_candidates,
            acquisition_start: vector(
                &f["acquisition_start_work_mm"],
                "acquisition_start_work_mm",
            )?,
            acquisition_drop: positive("acquisition_drop_mm")?,
            empty_sigma: positive("empty_space_sigma_mm")?,
            posterior_iterations: count("posterior_iterations")?,
            posterior_tolerance: positive("posterior_weight_tolerance")?,
            conditioning: count("max_conditioning_observations")?,
            surface: Id::parse(&f["surface_analysis"])?,
            capture: Id::parse(&f["source_capture"])?,
            design: Id::parse(&f["design"])?,
            units: positive("stl_mm_per_unit")?,
            lo,
            hi,
            clearance,
            length: positive("field_length_mm")?,
            scale: positive("field_scale_mm")?,
            contact_sigma: positive("contact_sigma_mm")?,
            normal_sigma: positive("normal_sigma")?,
            confidence: positive("uncertainty_multiplier")?,
            basis_pairs: count("basis_pairs")?,
            matrix_entries: count("max_matrix_entries")?,
            radius: positive("cover_radius_mm")?,
            covers: count("max_cover_samples")?,
            population,
            evaluations,
            mutation: positive("mutation_scale")?,
            crossover,
            seed,
            observation_candidates,
            observations,
            ray_resolution: positive("ray_resolution_mm")?,
        })
    }
}
