//! Analytical numerical cases; these are not stock or machine observations.
use super::super::super::{
    geometry::*,
    probe::{Calibration, Probe, Sample},
    request::Use,
};
use super::*;
use crate::object_map::model::CaptureState;
fn settings() -> request::Request {
    request::Request {
        surface: Id::parse("synthetic").unwrap(),
        min: [-1.; 3],
        max: [1.; 3],
        spacing: 0.5,
        band: 1.,
        residual: 0.1,
        vertices: 125,
        comparisons: 100000,
        triangles: 10000,
    }
}
fn field(grid: &grid::Grid, f: impl Fn(V) -> f64) -> Vec<field::Node> {
    (0..grid.count)
        .map(|i| field::Node {
            seed: 0,
            value: Ok(f(grid.point(i))),
        })
        .collect()
}
fn plane_area(mesh: &extract::Mesh) -> f64 {
    mesh.facets
        .iter()
        .filter(|f| f.source.is_some())
        .map(|f| {
            let p = f.vertices.map(|i| mesh.vertices[i].p);
            norm(cross(sub(p[1], p[0]), sub(p[2], p[0]))) / 2.
        })
        .sum()
}
#[test]
fn plane_intersections_share_edges_and_keep_original_grid_sources() {
    let r = settings();
    let grid = grid::Grid::new(&r).unwrap();
    let mesh = extract::run(&grid, &field(&grid, |p| p[2] - 0.125), r.triangles, |_| {
        Some(0)
    })
    .unwrap();
    assert!((plane_area(&mesh) - 4.).abs() < f64::EPSILON * mesh.facets.len() as f64);
    assert_eq!(mesh.topology().nonmanifold_edges, 0);
    assert_eq!(mesh.topology().orientation_conflicts, 0);
    for f in &mesh.facets {
        let p = f.vertices.map(|i| mesh.vertices[i].p);
        assert!(cross(sub(p[1], p[0]), sub(p[2], p[0]))[2] > 0.);
    }
    for v in &mesh.vertices {
        assert_eq!(v.p[2], 0.125);
        assert_eq!(
            v.p,
            add(
                grid.point(v.a),
                scale(sub(grid.point(v.b), grid.point(v.a)), v.fraction)
            )
        );
    }
    // Every singly incident edge is on the crop boundary, never a tetra seam.
    let mut edges = std::collections::BTreeMap::new();
    for f in &mesh.facets {
        for j in 0..3 {
            let (a, b) = (f.vertices[j], f.vertices[(j + 1) % 3]);
            *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    for ((a, b), count) in edges {
        if count == 1 {
            let (a, b) = (mesh.vertices[a].p, mesh.vertices[b].p);
            assert!((0..2).any(|i| a[i].abs() == 1. && a[i] == b[i]));
        }
    }
}
#[test]
fn zeros_on_lattice_nodes_do_not_duplicate_coplanar_facets() {
    let r = settings();
    let grid = grid::Grid::new(&r).unwrap();
    let mesh = extract::run(&grid, &field(&grid, |p| p[2]), r.triangles, |_| Some(0)).unwrap();
    assert_eq!(plane_area(&mesh), 4.);
    assert_eq!(mesh.topology().nonmanifold_edges, 0);
    assert_eq!(mesh.topology().orientation_conflicts, 0);
    for v in &mesh.vertices {
        assert_eq!(v.a, v.b);
        assert_eq!(v.fraction, 0.);
    }
}
#[test]
fn analytic_sphere_has_closed_outward_edge_incidence() {
    let r = settings();
    let grid = grid::Grid::new(&r).unwrap();
    let mesh = extract::run(
        &grid,
        &field(&grid, |p| norm(p) - 0.75),
        r.triangles,
        |_| Some(0),
    )
    .unwrap();
    let t = mesh.topology();
    assert!(t.triangles > 0);
    assert_eq!(
        (
            t.boundary_edges,
            t.nonmanifold_edges,
            t.orientation_conflicts
        ),
        (0, 0, 0)
    );
    for f in &mesh.facets {
        let p = f.vertices.map(|i| mesh.vertices[i].p);
        assert!(dot(cross(sub(p[1], p[0]), sub(p[2], p[0])), p[0]) > 0.);
    }
}
#[test]
fn missing_field_and_unsupported_facets_remain_holes() {
    let r = settings();
    let grid = grid::Grid::new(&r).unwrap();
    let mut nodes = field(&grid, |p| p[2] - 0.125);
    nodes[grid.index([2, 2, 2])].value = Err(field::Missing::Support);
    let mesh = extract::run(&grid, &nodes, r.triangles, |_| Some(0)).unwrap();
    assert!(plane_area(&mesh) < 4.);
    assert!(!mesh.unresolved_cells.is_empty());
    let unsupported = extract::run(&grid, &field(&grid, |p| p[2]), r.triangles, |_| None).unwrap();
    assert!(!unsupported.facets.is_empty());
    assert_eq!(unsupported.topology().triangles, 0);
    assert!(!unsupported.stl().contains("facet normal"));
}
#[test]
fn explicit_budgets_reject_incomplete_requested_calculation() {
    let mut r = settings();
    r.vertices = 124;
    assert!(format!("{}", grid::Grid::new(&r).err().unwrap()).contains("needs 125 vertices"));
    r.vertices = 125;
    let grid = grid::Grid::new(&r).unwrap();
    assert!(format!(
        "{}",
        extract::run(&grid, &field(&grid, |p| p[2]), 1, |_| Some(0))
            .err()
            .unwrap()
    )
    .contains("max_mesh_triangles"));
    assert!(extract::run(&grid, &[], r.triangles, |_| Some(0)).is_err());
}
fn source() -> (Vec<Sample>, surface::request::Request) {
    let samples = [
        [-1., -1., 1.],
        [1., -1., 1.],
        [1., 1., 1.],
        [-1., 1., 1.],
        [0., 0., 1.],
    ]
    .into_iter()
    .enumerate()
    .map(|(sequence, p)| Sample {
        capture: Id::parse("synthetic").unwrap(),
        sequence,
        usage: if sequence == 4 { Use::Check } else { Use::Fit },
        state: CaptureState::Partial,
        trigger: p,
        center: p,
        approach: [0., 0., -1.],
        feed: 50.,
    })
    .collect();
    let request = surface::request::Request {
        probe: Probe {
            calibration: Calibration::Synthetic,
            radius: 1.,
            mount: [0.; 3],
            pretravel: 0.,
        },
        neighborhood: 4.,
        approach_cos: 0.,
        huber: 0.1,
        iterations: 50,
        convergence: 0.000001,
        support_gap: 3.,
        max_residual: 0.1,
        selected: vec![],
    };
    (samples, request)
}
#[test]
fn nearest_unresolved_source_is_not_replaced_by_a_farther_plane() {
    let (samples, sr) = source();
    let mut stations = surface::fit::run(&samples, &sr);
    stations[0].result = Err(surface::Reason::FewContacts);
    let r = settings();
    let grid = grid::Grid::new(&r).unwrap();
    let result = field::run(&grid, &samples, &stations, &sr, &r).unwrap();
    let n = result.nodes[grid.index([0, 0, 2])];
    assert_eq!(n.seed, 0);
    assert_eq!(n.value, Err(field::Missing::Fit));
    assert!(result.nodes[grid.index([4, 0, 2])].value.is_ok());
}
#[test]
fn field_requires_independent_checks_and_outward_full_facet_support() {
    let (mut samples, sr) = source();
    let stations = surface::fit::run(&samples, &sr);
    let r = settings();
    let grid = grid::Grid::new(&r).unwrap();
    let result = field::run(&grid, &samples, &stations, &sr, &r).unwrap();
    assert_eq!(result.nodes[grid.index([2, 2, 2])].value, Ok(0.));
    let facet = [[-0.5, -0.5, 0.], [0.5, -0.5, 0.], [0., 0.5, 0.]];
    assert!(result.facet_support(&facet, &sr, &r).is_some());
    assert!(result
        .facet_support(&[facet[0], facet[2], facet[1]], &sr, &r)
        .is_none());
    samples[4].center[2] = 2.;
    let conflict = field::run(&grid, &samples, &stations, &sr, &r).unwrap();
    assert_eq!(
        conflict.nodes[grid.index([2, 2, 2])].value,
        Err(field::Missing::CheckConflict)
    );
    samples.pop();
    let unchecked = field::run(&grid, &samples, &stations, &sr, &r).unwrap();
    assert_eq!(
        unchecked.nodes[grid.index([2, 2, 2])].value,
        Err(field::Missing::Check)
    );
}
#[test]
fn full_facet_cannot_bridge_disjoint_sample_support_disks() {
    let (samples, mut sr) = source();
    sr.support_gap = 0.25;
    let stations = surface::fit::run(&samples, &sr);
    let p = stations[0].result.as_ref().unwrap();
    let support = surface::support::Support::new(&samples, &stations[0], p, &sr);
    let vertices = [[-1., -1., 0.], [1., -1., 0.], [1., 1., 0.]];
    assert!(vertices.iter().all(|v| support.contains(*v, p, &sr)));
    assert!(!support.covers_points(&vertices, p, &sr));
}
