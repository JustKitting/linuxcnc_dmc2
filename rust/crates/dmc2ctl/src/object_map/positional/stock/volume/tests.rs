use super::super::{
    super::{
        cover,
        geometry::*,
        mesh::{
            solid::Solid,
            solid_tests::{box_mesh, concave},
        },
    },
    optimize::{Objective, Stop},
};
use super::{
    request::{Occupancy, Request},
    search,
};
use crate::object_map::model::Id;
fn request() -> Request {
    Request {
        stock: Id::parse("synthetic").unwrap(),
        design: Id::parse("required").unwrap(),
        occupancy: Occupancy::SingleShellEnclosedSolid,
        units: 1.,
        lo: [0.; 6],
        hi: [0.; 6],
        margin: 0.1,
        allowance: 0.,
        radius: 0.1,
        samples: 4096,
        resolution: 0.01,
        evaluations: 31,
        topology_visits: 1024,
        winding_terms: usize::MAX,
    }
}
#[test]
fn placement_moves_required_geometry_out_of_concave_empty_region() {
    let stock = concave();
    let solid = Solid::new(&stock, stock.triangles().len().pow(2) * 2).unwrap();
    let required = box_mesh([-0.2; 3], [0.2; 3]);
    let mut r = request();
    r.lo = [0., 2., 1., 0., 0., 0.];
    r.hi = [2., 2., 1., 0., 0., 0.];
    let cover = cover::build(
        required.triangles(),
        r.radius,
        r.samples,
        cover::Metric::Spatial,
    )
    .unwrap();
    let fitted = search::run(&solid, &cover, &r).unwrap();
    assert_eq!(fitted.stop, Stop::Clearance);
    assert!(fitted.history.len() > 1);
    assert!(fitted.at[0] > 0.2 && fitted.at[0] < 0.8);
    assert_eq!(&fitted.at[1..], &[2., 1., 0., 0., 0.]);
}
#[test]
fn pitch_search_fits_long_axis_without_resizing_it() {
    let stock = box_mesh([-1., -1., -3.], [1., 1., 3.]);
    let solid = Solid::new(&stock, 288).unwrap();
    let required = box_mesh([-2., -0.25, -0.25], [2., 0.25, 0.25]);
    let mut r = request();
    r.hi[4] = std::f64::consts::FRAC_PI_2;
    let cover = cover::build(
        required.triangles(),
        r.radius,
        r.samples,
        cover::Metric::Spatial,
    )
    .unwrap();
    let f = search::run(&solid, &cover, &r).unwrap();
    assert_eq!(f.stop, Stop::Clearance);
    assert!(f.at[4] > std::f64::consts::FRAC_PI_4);
    let p = search::pose(f.at);
    let span = norm(sub(p.point([-2., 0., 0.]), p.point([2., 0., 0.])));
    assert!((span - 4.).abs() < 1e-12);
}
#[test]
fn bounds_cover_coupled_translation_and_rotation() {
    let stock = box_mesh([-4.; 3], [4.; 3]);
    let solid = Solid::new(&stock, 288).unwrap();
    let samples = [cover::Sample {
        triangle: 0,
        vertices: [[1., 2., 3.]; 3],
        center: [1., 2., 3.],
        radius: 0.1,
    }];
    let objective = search::Enclosed {
        stock: &solid,
        samples: &samples,
        radius: norm(samples[0].center),
        allowance: 0.,
    };
    let lo = [-1., -2., -3., -0.7, -0.8, -0.9];
    let hi = [2., 3., 4., 0.5, 0.6, 0.7];
    let mid = std::array::from_fn(|i| (lo[i] + hi[i]) / 2.);
    let bound = objective.movement(lo, hi).1;
    let center = search::pose(mid).point(samples[0].center);
    for bits in 0..64 {
        let at = std::array::from_fn(|i| if bits & (1 << i) == 0 { lo[i] } else { hi[i] });
        assert!(norm(sub(search::pose(at).point(samples[0].center), center)) <= bound);
        assert!(objective.value(at).unwrap() <= objective.value(mid).unwrap() + bound);
    }
}
#[test]
fn failed_clearance_keeps_best_bound_and_does_not_enlarge_domain() {
    let stock = box_mesh([-1.; 3], [1.; 3]);
    let solid = Solid::new(&stock, 288).unwrap();
    let required = box_mesh([-2.; 3], [2.; 3]);
    let mut r = request();
    r.radius = 1.;
    r.evaluations = 1;
    r.lo[0] = 1.;
    r.hi[0] = 2.;
    let cover = cover::build(
        required.triangles(),
        r.radius,
        r.samples,
        cover::Metric::Spatial,
    )
    .unwrap();
    let f = search::run(&solid, &cover, &r).unwrap();
    assert_eq!(f.stop, Stop::Budget);
    assert_eq!(f.at[0], 1.5);
    assert!(f.clearance < 0.);
    assert!(f.upper >= f.clearance);
    r.winding_terms = 1;
    assert!(search::run(&solid, &cover, &r)
        .err()
        .unwrap()
        .to_string()
        .contains("max_winding_terms"));
}
