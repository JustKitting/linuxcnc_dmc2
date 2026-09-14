//! Synthetic numerical checks. No physical stock, probing or machining proof.
use super::super::{
    geometry::*,
    probe::{Calibration, Probe, Sample},
    request::{Selection, Use},
};
use super::*;
use request::{Closure, Request, Surface};

fn settings() -> Request {
    // Same mm-scale radius/Huber and numerical convergence as the existing
    // positional example. Local span/gap relate only to these unit-spaced rows.
    Request {
        probe: Probe {
            calibration: Calibration::Synthetic,
            radius: 1.,
            mount: [0.; 3],
            pretravel: 0.,
        },
        closure: Closure::Open,
        surface: Surface::VerticalSides,
        span: 4.,
        huber: 0.1,
        iterations: 50,
        convergence: 0.000001,
        max_gap: 3.,
        max_z_span: 0.1,
        max_residual: 0.1,
        selected: Vec::new(),
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
#[test]
fn normal_correction_follows_measured_edge_not_oblique_approach() {
    let r = settings();
    let s = (0..9)
        .map(|i| {
            sample(
                i,
                [i as f64, 2. * i as f64 + 3., 5.],
                [0., -1., 0.],
                Use::Fit,
            )
        })
        .collect::<Vec<_>>();
    let c = fit::run(&s, &r).unwrap();
    let n = [-2. / 5_f64.sqrt(), 1. / 5_f64.sqrt(), 0.];
    for p in c.stations {
        assert!(norm(sub(p.normal, n)) < r.convergence);
        assert!(norm(sub(p.center, s[p.sample].center)) < r.convergence);
        assert!(norm(sub(p.surface.unwrap(), sub(s[p.sample].center, n))) < r.convergence);
        assert_eq!(p.stop, fit::Stop::Converged);
    }
}
#[test]
fn independently_perturbed_check_does_not_pull_the_contour() {
    let r = settings();
    let mut s = (0..9)
        .map(|i| sample(i, [i as f64, 0., 5.], [0., -1., 0.], Use::Fit))
        .collect::<Vec<_>>();
    let a = fit::run(&s, &r).unwrap();
    s.push(sample(9, [4., 2., 5.], [0., -1., 0.], Use::Check));
    let b = fit::run(&s, &r).unwrap();
    assert_eq!(
        a.stations.iter().map(|p| p.center).collect::<Vec<_>>(),
        b.stations.iter().map(|p| p.center).collect::<Vec<_>>()
    );
    let report = report::build(&s, &b, &r).unwrap();
    assert!(report
        .refinements
        .contains("independent-check-disagreement"));
    assert!(report.json.contains("\"solid_stock\":null"));
}
#[test]
fn unsupported_gap_is_not_used_as_a_check_surface() {
    let r = settings();
    let mut s = [0., 1., 2., 10., 11., 12.]
        .iter()
        .enumerate()
        .map(|(i, x)| sample(i, [*x, 0., 5.], [0., -1., 0.], Use::Fit))
        .collect::<Vec<_>>();
    s.push(sample(6, [6., 0., 5.], [0., -1., 0.], Use::Check));
    let c = fit::run(&s, &r).unwrap();
    let report = report::build(&s, &c, &r).unwrap();
    assert!(report.refinements.contains("rim-gap"));
    assert!(report.json.contains("\"horizontal_center_residual_mm\":4"));
}
#[test]
fn curved_indented_outline_keeps_free_shape_and_retained_rows() {
    let mut r = settings();
    r.closure = Closure::Closed;
    r.surface = Surface::ProbeCentres;
    // A synthetic three-lobed contour. Its radii are unrelated to real stock.
    let s = (0..96)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / 96.;
            let radius = 10. + 2. * (3. * a).cos();
            sample(
                i,
                [radius * a.cos(), radius * a.sin(), 5.],
                [-a.cos(), -a.sin(), 0.],
                Use::Fit,
            )
        })
        .collect::<Vec<_>>();
    let c = fit::run(&s, &r).unwrap();
    assert_eq!(c.stations.len(), s.len());
    let radii = c
        .stations
        .iter()
        .map(|p| p.center[0].hypot(p.center[1]))
        .collect::<Vec<_>>();
    assert!(
        radii.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - radii.iter().copied().fold(f64::INFINITY, f64::min)
            > 2.
    );
    for p in &c.stations {
        assert!(p.surface.is_none());
        assert_eq!(p.neighbours.len(), p.residuals.len());
        assert_eq!(p.neighbours.len(), p.weights.len());
        assert!(p.objective_end <= p.objective_start + r.convergence);
    }
    assert!(report::build(&s, &c, &r)
        .unwrap()
        .refinements
        .contains("local-shape-unresolved"));
}
#[test]
fn ambiguous_geometry_is_an_error_instead_of_an_arbitrary_normal() {
    let r = settings();
    let s = (0..3)
        .map(|i| sample(i, [1., 1., 5.], [0., -1., 0.], Use::Fit))
        .collect::<Vec<_>>();
    assert!(fit::run(&s, &r).is_err());
}
#[test]
fn exact_capture_rules_are_shared_with_stl_fitting() {
    let raw = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../examples/positional-mapper/synthetic-ledger.txt"
    ));
    let mut c = crate::object_map::capture::Capture::read(raw).unwrap();
    let r = settings();
    let id = Id::parse("synthetic").unwrap();
    let p = c.contacts.iter().find(|p| p.stage == Stage::Fine).unwrap();
    let selected = Selection {
        capture: id.clone(),
        sequence: p.sequence,
        usage: Use::Fit,
    };
    let measured = r.probe.measure(&id, &c, &selected).unwrap();
    assert_eq!(measured.trigger, p.trigger_mm);
    c.state = CaptureState::Quarantined;
    assert!(r.probe.measure(&id, &c, &selected).is_err());
}
