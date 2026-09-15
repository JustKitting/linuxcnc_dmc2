//! Finite numerical sweeps only. No physical clearance or stock evidence.
use super::super::{
    fit,
    support::Support,
    tests::{grid, settings},
};
use super::*;

fn sweep() -> Sweep {
    Sweep {
        capture: Id::parse("synthetic").unwrap(),
        sequence: 100,
        from: [1., 1., 7.],
        reported_end: [1., 1., 3.],
        requested_end: [1., 1., 3.],
        center_from: [1., 1., 7.],
        center_end: [1., 1., 3.],
        radius: 1.,
        feed: 400.,
    }
}
#[test]
fn point_constraint_stops_at_the_reported_segment_and_ball_ends() {
    let m = sweep();
    assert!(m.excludes_point([1., 1., 4.]).unwrap());
    assert!(m.excludes_point([1., 1., 2.]).unwrap());
    assert!(!m.excludes_point([1., 1., 1.]).unwrap());
    assert!(!m.excludes_point([3., 1., 4.]).unwrap());
}
#[test]
fn full_facet_test_finds_a_sweep_between_clear_vertices() {
    let m = sweep();
    let v = [[0., 0., 4.], [2., 0., 4.], [0., 2., 4.]];
    assert!(v.iter().all(|p| !m.excludes_point(*p).unwrap()));
    assert!(m
        .overlaps_triangle(Triangle { v, n: [0., 0., 1.] })
        .unwrap());
    assert!(!m
        .overlaps_triangle(Triangle {
            v: [[-2., -2., 4.], [0., -2., 4.], [-2., 0., 4.]],
            n: [0., 0., 1.]
        })
        .unwrap());
}
#[test]
fn finite_sweep_geometry_handles_crossing_parallel_and_endpoint_cases() {
    let t = Triangle {
        v: [[-2., -2., 0.], [2., -2., 0.], [0., 2., 0.]],
        n: [0., 0., 1.],
    };
    assert_eq!(
        geometry::segment_triangle([0., 0., -1.], [0., 0., 1.], t).unwrap(),
        0.
    );
    assert_eq!(
        geometry::segment_triangle([-1., 0., 0.], [1., 0., 0.], t).unwrap(),
        0.
    );
    let distance = geometry::segment_triangle([-1., 0., 2.], [1., 0., 2.], t).unwrap();
    assert!((distance - 2.).abs() <= f64::EPSILON * distance);
    assert_eq!(
        geometry::point_segment([0., 0., 3.], [0., 0., -1.], [0., 0., 1.]).unwrap(),
        2.
    );
}
#[test]
fn excluded_gap_does_not_remove_supported_surface_around_it() {
    let mut r = settings();
    r.no_contact = NoContactModel::ErodedProbeSweep { allowance: 0. };
    let samples = grid(|_, _| 5.)
        .into_iter()
        .filter(|s| s.center[0] % 2. == 0. && s.center[1] % 2. == 0.)
        .collect::<Vec<_>>();
    let stations = fit::run(&samples, &r, &[sweep()]);
    let station = stations
        .iter()
        .find(|s| samples[s.seed].center == [0., 0., 5.])
        .unwrap();
    let patch = station.result.as_ref().unwrap();
    let support = Support::new(&samples, station, patch, &r).unwrap();
    assert!(!support.contains([1., 1., 5.], patch, &r).unwrap());
    assert!(support.contains([-1., -1., 5.], patch, &r).unwrap());
    assert!(!support
        .covers([1., 1., 4.], r.probe.radius, patch, &r)
        .unwrap());
    assert!(!support
        .covers_points(&[[0., 0., 4.], [2., 0., 4.], [0., 2., 4.]], patch, &r)
        .unwrap());
    assert!(support
        .covers_points(&[[-2., -2., 4.], [0., -2., 4.], [-2., 0., 4.]], patch, &r)
        .unwrap());
}
#[test]
fn rigid_change_of_frame_preserves_finite_sweep_intersection() {
    let m = sweep();
    let pose = Pose::from_euler([30., 20., 10.], [25., 28., 48.]);
    let transformed = Sweep {
        center_from: pose.point(m.center_from),
        center_end: pose.point(m.center_end),
        ..m.clone()
    };
    let t = Triangle {
        v: [[0., 0., 4.], [2., 0., 4.], [0., 2., 4.]],
        n: [0., 0., 1.],
    };
    let rotated = Triangle {
        v: t.v.map(|p| pose.point(p)),
        n: mv(pose.r, t.n),
    };
    assert_eq!(
        m.overlaps_triangle(t).unwrap(),
        transformed.overlaps_triangle(rotated).unwrap()
    );
    for p in [[1., 1., 4.], [-1., -1., 4.]] {
        assert_eq!(
            m.excludes_point(p).unwrap(),
            transformed.excludes_point(pose.point(p)).unwrap()
        );
    }
}

#[test]
fn contradictory_contacts_keep_both_sources_and_prevent_checked_support() {
    use super::super::{local, report};
    let mut r = settings();
    r.no_contact = NoContactModel::ErodedProbeSweep { allowance: 0. };
    let samples = grid(|_, _| 5.);
    let stations = fit::run(&samples, &r, &[sweep()]);
    let local = local::build(&samples, &stations, &r).unwrap();
    assert!(!local.is_empty());
    assert!(local
        .iter()
        .all(|l| l.checks == local::Checks::NoContactConflict));
    let report = report::build(&samples, &stations, &r).unwrap();
    assert!(report.refinements.contains(Issue::Conflict.description().0));
    assert!(report
        .refinements
        .contains("\"no_contact_source\":{\"capture\":\"synthetic\",\"sequence\":100}"));
    assert!(report.refinements.contains("\"outward_normal\":[0,0,1]"));
    assert_eq!(report.csv.lines().count(), samples.len() + 1);
}

#[test]
fn unrepresentable_distance_is_an_error_with_source_and_recovery() {
    let error = sweep()
        .excludes_point([f64::MAX; 3])
        .unwrap_err()
        .to_string();
    assert!(error.contains("synthetic:100"));
    assert!(error.contains("Inspect coordinate units"));
}
