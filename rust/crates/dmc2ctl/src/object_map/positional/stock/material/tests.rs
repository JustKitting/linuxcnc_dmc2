//! Analytical geometry cases, not observations of stock or machine behavior.
use super::super::super::{
    geometry::*,
    mesh::Triangle,
    probe::{Calibration, Probe, Sample},
    request::Use,
};
use super::*;
use crate::object_map::model::CaptureState;
fn source(check: Option<f64>) -> (Vec<Sample>, surface::request::Request) {
    let mut samples = Vec::new();
    for x in -2..=2 {
        for y in -2..=2 {
            samples.push(contact(samples.len(), [x as f64, y as f64, 1.], Use::Fit));
        }
    }
    if let Some(z) = check {
        samples.push(contact(samples.len(), [0., 0., z], Use::Check));
    }
    let r = surface::request::Request {
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
        selected: vec![],
        no_contact: surface::request::NoContactModel::Legacy,
    };
    (samples, r)
}
fn contact(sequence: usize, p: V, usage: Use) -> Sample {
    Sample {
        capture: Id::parse("synthetic").unwrap(),
        sequence,
        usage,
        state: CaptureState::Partial,
        trigger: p,
        center: p,
        approach: [0., 0., -1.],
        feed: 50.,
    }
}
fn settings() -> request::Request {
    request::Request {
        candidate: Id::parse("synthetic").unwrap(),
        surface: Id::parse("surface").unwrap(),
        clearance: 0.25,
        allowance: 0.1,
        band: 2.,
        radius: 0.25,
        samples: 10000,
        comparisons: 26 * 10000,
    }
}
fn region(p: V) -> cover::Sample {
    cover::Sample {
        triangle: 0,
        center: p,
        radius: 0.25,
    }
}
fn assess(points: &[cover::Sample], check: Option<f64>) -> Vec<query::Region> {
    let (s, sr) = source(check);
    let stations = surface::fit::run(&s, &sr, &[]);
    query::run(
        points,
        &s,
        &stations,
        &sr,
        Pose {
            r: IDENTITY,
            t: [0.; 3],
        },
        &settings(),
    )
    .unwrap()
}
#[test]
fn local_shortage_and_inwardness_do_not_become_a_solid() {
    let points = [
        region([0., 0., 1.]),
        region([0., 0., -1.]),
        region([0., 0., -0.4]),
    ];
    let result = assess(&points, Some(1.));
    assert_eq!(
        result.iter().map(|p| p.state).collect::<Vec<_>>(),
        vec![
            query::State::Shortage,
            query::State::LocallyInward,
            query::State::BoundaryBand
        ]
    );
    let (s, _) = source(Some(1.));
    let triangles = [Triangle {
        v: [[0.; 3], [1., 0., 0.], [0., 1., 0.]],
        n: [0., 0., 1.],
    }];
    let report = report::build(
        &points,
        &s,
        &result,
        &triangles,
        Pose {
            r: IDENTITY,
            t: [0.; 3],
        },
        &settings(),
    )
    .unwrap();
    assert!(report.json.contains("\"solid_stock\":null"));
    assert!(report.json.contains("\"cam_ready\":false"));
    assert!(report.needs.contains("closed-material-coverage-unresolved"));
    let fields = report.csv.lines().next().unwrap().split(',').count();
    assert!(report
        .csv
        .lines()
        .skip(1)
        .all(|l| l.split(',').count() == fields));
}
#[test]
fn full_cover_must_fit_support_and_normal_band() {
    let result = assess(
        &[
            region([1.9, 0., -1.]),
            region([0., 0., -5.]),
            region([4., 0., 0.]),
        ],
        Some(1.),
    );
    assert!(result
        .iter()
        .all(|p| p.state == query::State::Unsupported && p.comparisons.is_empty()));
}
#[test]
fn checks_are_required_and_disagreement_is_not_fitted_away() {
    let point = [region([0., 0., -1.])];
    assert_eq!(assess(&point, None)[0].state, query::State::Unchecked);
    let result = assess(&point, Some(2.));
    assert_eq!(result[0].state, query::State::CheckConflict);
    assert!(result[0]
        .comparisons
        .iter()
        .all(|c| c.checks == query::Checks::Disagrees && c.distance == -1.));
}
#[test]
fn rigid_frame_change_preserves_local_distances() {
    let (mut s, sr) = source(Some(1.));
    let pose = Pose::from_euler([30., 20., 10.], [20., 30., 40.])
        .validate()
        .unwrap();
    for sample in &mut s {
        sample.center = pose.point(sample.center);
        sample.trigger = sample.center;
        sample.approach = mv(pose.r, sample.approach);
    }
    let stations = surface::fit::run(&s, &sr, &[]);
    let result = query::run(
        &[region([0., 0., -1.])],
        &s,
        &stations,
        &sr,
        pose,
        &settings(),
    )
    .unwrap();
    assert_eq!(result[0].state, query::State::LocallyInward);
    assert!(result[0]
        .comparisons
        .iter()
        .all(|c| (c.distance + 1.).abs() < sr.convergence));
}
#[test]
fn vertical_geometry_gets_full_spatial_coverage() {
    let triangles = [Triangle {
        v: [[0., 0., 0.], [1., 0., 0.], [0., 0., 4.]],
        n: [0., -1., 0.],
    }];
    let r = settings();
    let xy = cover::build(&triangles, r.radius, r.samples, cover::Metric::Horizontal).unwrap();
    let full = cover::build(&triangles, r.radius, r.samples, cover::Metric::Spatial).unwrap();
    assert!(full.len() > xy.len());
    assert!(full.iter().all(|s| s.radius <= r.radius && s.triangle == 0));
    assert!(full.iter().any(|s| s.center[2] > 3.));
    assert!(full.iter().any(|s| s.center[2] < 1.));
}
#[test]
fn comparison_budget_cannot_drop_regions() {
    let (s, sr) = source(Some(1.));
    let stations = surface::fit::run(&s, &sr, &[]);
    let mut r = settings();
    r.comparisons = 1;
    let error = query::run(
        &[region([0., 0., -1.])],
        &s,
        &stations,
        &sr,
        Pose {
            r: IDENTITY,
            t: [0.; 3],
        },
        &r,
    )
    .err()
    .unwrap();
    assert!(format!("{error}").contains("no region was dropped"));
}
