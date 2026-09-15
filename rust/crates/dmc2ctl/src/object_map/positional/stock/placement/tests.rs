//! Numerical cases only. None establishes measured stock or machine behavior.
use super::super::super::{geometry::*, mesh::Triangle};
use super::*;
fn polygon(v: &[[f64; 2]]) -> polygon::Polygon {
    polygon::Polygon::new(v.iter().map(|p| [p[0], p[1], 0.]).collect()).unwrap()
}
fn corner() -> polygon::Polygon {
    polygon(&[
        [0., 0.],
        [10., 0.],
        [10., 4.],
        [4., 4.],
        [4., 10.],
        [0., 10.],
    ])
}
fn triangle(v: [V; 3]) -> Triangle {
    Triangle { v, n: [0., 0., 1.] }
}
fn bar(x: f64, y: f64) -> Vec<Triangle> {
    vec![
        triangle([[-x, -y, 0.], [x, -y, 0.], [x, y, 0.]]),
        triangle([[-x, -y, 0.], [x, y, 0.], [-x, y, 0.]]),
    ]
}
fn settings() -> request::Request {
    // Synthetic millimetre geometry: quarter-unit cover/clearance and a
    // hundredth-unit search bound resolve the deliberately coarse examples.
    request::Request {
        outline: Id::parse("synthetic-outline").unwrap(),
        design: Id::parse("synthetic-design").unwrap(),
        units: 1.,
        z: 0.,
        lo: [0.; 3],
        hi: [0.; 3],
        margin: 0.25,
        radius: 0.25,
        samples: 10000,
        resolution: 0.01,
        evaluations: 1000,
    }
}
#[test]
fn concavity_is_outside_even_inside_the_bounding_box() {
    let p = corner();
    assert_eq!(p.distance([8., 8., 0.]).unwrap().0, 4.);
    assert_eq!(p.distance([2., 8., 0.]).unwrap().0, -2.);
    let reverse = polygon::Polygon::new(p.vertices.into_iter().rev().collect()).unwrap();
    assert_eq!(reverse.distance([8., 8., 0.]).unwrap().0, 4.);
}
#[test]
fn triangle_interior_can_fail_when_all_vertices_are_inside() {
    let p = corner();
    let r = settings();
    let t = triangle([[1., 8., 0.], [8., 1., 0.], [1., 1., 0.]]);
    assert!(t.v.iter().all(|v| p.distance(*v).unwrap().0 < 0.));
    let s = cover::build(&[t], r.radius, r.samples, cover::Metric::Horizontal).unwrap();
    assert!(s.iter().any(|s| p.distance(s.center).unwrap().0 > 0.));
    assert!(search::score([0.; 3], r.z, &p, &s).unwrap() < 0.);
    assert!(s.iter().all(|s| s.radius <= r.radius && s.triangle == 0));
}
#[test]
fn translation_search_keeps_the_notch_and_improves_worst_clearance() {
    let p = corner();
    let mut r = settings();
    r.lo = [5., 1., 0.];
    r.hi = [8., 8., 0.];
    let s = cover::build(
        &bar(0.5, 0.5),
        r.radius,
        r.samples,
        cover::Metric::Horizontal,
    )
    .unwrap();
    let f = search::run(&p, &s, &r).unwrap();
    assert!(f.history[0].2 < 0.);
    assert_eq!(f.stop, search::Stop::Clearance);
    assert!(f.at[1] < 4. && f.clearance >= r.margin);
    assert!(f.history.windows(2).all(|w| w[1].2 > w[0].2));
    let report = report::build(&p, &s, &f, &r).unwrap();
    assert!(report
        .json
        .contains("\"three_dimensional_containment\":\"unresolved\""));
    assert_eq!(report.csv.lines().count(), s.len() + 1);
}
#[test]
fn yaw_search_preserves_bar_dimensions() {
    let p = polygon(&[[-1.5, -5.], [1.5, -5.], [1.5, 5.], [-1.5, 5.]]);
    let mut r = settings();
    r.hi[2] = std::f64::consts::FRAC_PI_2;
    let triangles = bar(3., 0.5);
    let s = cover::build(&triangles, r.radius, r.samples, cover::Metric::Horizontal).unwrap();
    let f = search::run(&p, &s, &r).unwrap();
    assert_eq!(f.stop, search::Stop::Clearance);
    assert!(f.clearance >= r.margin);
    let pose = search::pose(f.at, r.z);
    for t in &triangles {
        for i in 0..3 {
            let j = (i + 1) % 3;
            assert!(
                (norm(sub(pose.point(t.v[i]), pose.point(t.v[j]))) - norm(sub(t.v[i], t.v[j])))
                    .abs()
                    < f64::EPSILON * 16.
            );
        }
    }
}
#[test]
fn exhausted_budget_retains_a_candidate_and_remaining_bound() {
    let p = corner();
    let mut r = settings();
    r.lo = [5., 1., 0.];
    r.hi = [8., 8., 0.];
    r.evaluations = 1;
    let s = cover::build(
        &bar(0.5, 0.5),
        r.radius,
        r.samples,
        cover::Metric::Horizontal,
    )
    .unwrap();
    let f = search::run(&p, &s, &r).unwrap();
    assert_eq!(f.stop, search::Stop::Budget);
    assert_eq!(f.evaluations, 1);
    assert!(f.upper > f.clearance && f.clearance < 0.);
}
#[test]
fn invalid_outline_and_insufficient_cover_do_not_drop_geometry() {
    assert!(
        polygon::Polygon::new(vec![[0., 0., 0.], [2., 2., 0.], [0., 2., 0.], [2., 0., 0.]])
            .is_err()
    );
    let r = settings();
    let t = bar(3., 0.5);
    assert!(cover::build(&t, r.radius, t.len(), cover::Metric::Horizontal).is_err());
}
