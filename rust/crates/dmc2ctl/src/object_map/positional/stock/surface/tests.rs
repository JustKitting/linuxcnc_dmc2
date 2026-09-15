//! Numerical geometry only; these cases are not evidence of physical stock.
use super::super::super::{
    probe::{Calibration, Probe},
    request::Use,
};
use super::*;
use crate::object_map::model::{CaptureState, Id};
use geometry::*;
pub(super) fn settings() -> request::Request {
    // Existing unit-spaced stock fixtures use these mm-scale fit parameters.
    request::Request {
        probe: Probe {
            calibration: Calibration::Synthetic,
            radius: 1.,
            mount: [0.; 3],
            pretravel: 0.,
        },
        neighborhood: 4.,
        approach_cos: 90_f64.to_radians().cos(),
        huber: 0.1,
        iterations: 50,
        convergence: 0.000001,
        support_gap: 3.,
        max_residual: 0.1,
        selected: Vec::new(),
        no_contact: request::NoContactModel::Legacy,
        no_contact_sources: request::NoContactSources::ContributingContacts,
    }
}
fn sample(i: usize, p: V, approach: V, usage: Use) -> Sample {
    Sample {
        capture: Id::parse("synthetic").unwrap(),
        sequence: i,
        usage,
        state: CaptureState::Partial,
        trigger: p,
        center: p,
        approach,
        feed: 50.,
    }
}
pub(super) fn grid(height: impl Fn(f64, f64) -> f64) -> Vec<Sample> {
    let mut s = Vec::new();
    for x in -2..=2 {
        for y in -2..=2 {
            s.push(sample(
                s.len(),
                [x as f64, y as f64, height(x as f64, y as f64)],
                [0., 0., -1.],
                Use::Fit,
            ));
        }
    }
    s
}
#[test]
fn tilted_plane_uses_3d_normal_for_ball_correction() {
    let r = settings();
    let samples = grid(|x, y| 2. * x + 3. * y + 5.);
    let normal = [-2., -3., 1.].map(|x| x / 14_f64.sqrt());
    for station in fit::run(&samples, &r, &[]) {
        let patch = station.result.unwrap();
        assert!(norm(sub(patch.normal, normal)) < r.convergence);
        assert!(norm(sub(patch.center, samples[station.seed].center)) < r.convergence);
        assert!(
            norm(sub(
                patch.surface,
                sub(samples[station.seed].center, normal)
            )) < r.convergence
        );
        assert_eq!(patch.stop, Stop::Converged);
    }
}
#[test]
fn single_rim_line_does_not_invent_wall_slope() {
    let r = settings();
    let samples = (0..9)
        .map(|i| sample(i, [i as f64, 0., 5.], [0., -1., 0.], Use::Fit))
        .collect::<Vec<_>>();
    let stations = fit::run(&samples, &r, &[]);
    assert!(stations
        .iter()
        .all(|p| matches!(p.result, Err(Reason::UnobservedNormal))));
    let report = report::build(&samples, &stations, &r).unwrap();
    assert_eq!(report.csv.lines().count(), samples.len() + 1);
    assert!(report.refinements.contains("surface-normal-unobserved"));
    assert!(report.json.contains("\"solid_stock\":null"));
}
#[test]
fn withheld_contact_disagreement_does_not_pull_surface() {
    let r = settings();
    let mut samples = grid(|_, _| 5.);
    let before = fit::run(&samples, &r, &[]);
    samples.push(sample(
        samples.len(),
        [0., 0., 7.],
        [0., 0., -1.],
        Use::Check,
    ));
    let after = fit::run(&samples, &r, &[]);
    for (a, b) in before.iter().zip(&after) {
        assert_eq!(
            a.result.as_ref().unwrap().center,
            b.result.as_ref().unwrap().center
        );
        assert_eq!(
            a.result.as_ref().unwrap().normal,
            b.result.as_ref().unwrap().normal
        );
    }
    let report = report::build(&samples, &after, &r).unwrap();
    assert!(report
        .refinements
        .contains("independent-surface-check-disagreement"));
    assert!(report
        .json
        .contains("\"perpendicular_center_residual_mm\":2"));
}
#[test]
fn empty_space_between_neighborhoods_has_no_check_association() {
    let r = settings();
    let mut samples = grid(|_, _| 5.);
    let mut other = grid(|_, _| 5.);
    for p in &mut other {
        p.center[0] += 10.;
        p.trigger = p.center;
        p.sequence += samples.len();
    }
    samples.extend(other);
    samples.push(sample(
        samples.len(),
        [5., 0., 5.],
        [0., 0., -1.],
        Use::Check,
    ));
    let report = build(&samples, &r, &[]).unwrap();
    assert!(report.json.contains("\"associated_patch\":null"));
    assert!(report
        .refinements
        .contains("independent-surface-check-disagreement"));
}
#[test]
fn curved_surface_and_large_residual_remain_retained() {
    let r = settings();
    let mut samples = grid(|x, y| 5. + 0.1 * (x * x + y * y));
    let middle = samples.len() / 2;
    samples[middle].center[2] += 2.;
    samples[middle].trigger = samples[middle].center;
    let stations = fit::run(&samples, &r, &[]);
    assert_eq!(stations.len(), samples.len());
    assert!(stations
        .iter()
        .filter_map(|p| p.result.as_ref().ok())
        .any(|p| p.weights.iter().any(|w| *w < 1.)));
    for station in &stations {
        if let Ok(p) = &station.result {
            assert_eq!(p.residuals.len(), station.neighbours.len());
            assert_eq!(p.weights.len(), station.neighbours.len());
            assert!(p.objective_end <= p.objective_start + r.convergence);
        }
    }
    let report = report::build(&samples, &stations, &r).unwrap();
    assert_eq!(report.csv.lines().count(), samples.len() + 1);
    assert!(report.refinements.contains("surface-shape-unresolved"));
}
