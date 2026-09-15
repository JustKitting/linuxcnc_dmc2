use super::super::geometry::*;
use super::{intersection, solid::Solid, Mesh, Triangle};

pub(in crate::object_map::positional) fn mesh(v: &[V], faces: &[[usize; 3]]) -> Mesh {
    let mut raw = String::from("solid numerical\n");
    for face in faces {
        raw.push_str("facet normal 0 0 0\nouter loop\n");
        for i in face {
            raw.push_str(&format!("vertex {} {} {}\n", v[*i][0], v[*i][1], v[*i][2]));
        }
        raw.push_str("endloop\nendfacet\n");
    }
    raw.push_str("endsolid numerical\n");
    Mesh::read(raw.as_bytes(), 1.).unwrap()
}
pub(in crate::object_map::positional) fn box_mesh(lo: V, hi: V) -> Mesh {
    let v = std::array::from_fn::<_, 8, _>(|i| {
        std::array::from_fn(|j| if i & (1 << j) == 0 { lo[j] } else { hi[j] })
    });
    mesh(
        &v,
        &[
            [0, 2, 3],
            [0, 3, 1],
            [4, 5, 7],
            [4, 7, 6],
            [0, 1, 5],
            [0, 5, 4],
            [2, 6, 7],
            [2, 7, 3],
            [0, 4, 6],
            [0, 6, 2],
            [1, 3, 7],
            [1, 7, 5],
        ],
    )
}
pub(in crate::object_map::positional) fn concave() -> Mesh {
    let xy = [[0., 0.], [3., 0.], [3., 1.], [1., 1.], [1., 3.], [0., 3.]];
    let v = (0..12)
        .map(|i| [xy[i % 6][0], xy[i % 6][1], if i < 6 { 0. } else { 2. }])
        .collect::<Vec<_>>();
    let mut faces = Vec::new();
    for [a, b, c] in [[0, 1, 2], [0, 2, 3], [0, 3, 5], [3, 4, 5]] {
        faces.push([a, c, b]);
        faces.push([a + 6, b + 6, c + 6]);
    }
    for i in 0..6 {
        let j = (i + 1) % 6;
        faces.push([i, j, j + 6]);
        faces.push([i, j + 6, i + 6]);
    }
    mesh(&v, &faces)
}
#[test]
fn cube_sign_and_distance_match_analytic_solid() {
    let mesh = box_mesh([-2., -3., -4.], [2., 3., 4.]);
    let solid = Solid::new(&mesh, mesh.triangles().len().pow(2) * 2).unwrap();
    for p in [
        [0_f64, 0., 0.],
        [1., 2., 3.],
        [2., 0., 0.],
        [3., 5., 7.],
        [-4., 0., -5.],
    ] {
        let gap = std::array::from_fn::<_, 3, _>(|i| p[i].abs() - [2., 3., 4.][i]);
        let expected = if gap.iter().any(|x| *x > 0.) {
            -norm(gap.map(|x| x.max(0.)))
        } else {
            -gap.into_iter().fold(f64::NEG_INFINITY, f64::max)
        };
        assert!((solid.distance(p).unwrap().inward - expected).abs() < 1e-12);
    }
}
#[test]
fn concavity_is_outside_despite_enclosing_bounds() {
    let mesh = concave();
    let solid = Solid::new(&mesh, mesh.triangles().len().pow(2) * 2).unwrap();
    assert!((solid.distance([2., 2., 1.]).unwrap().inward + 1.).abs() < 1e-12);
    assert!((solid.distance([0.5, 2., 1.]).unwrap().inward - 0.5).abs() < 1e-12);
}
#[test]
fn shared_simplex_only_and_intersection_degeneracies() {
    let t = |v| Triangle::new(v).unwrap();
    let a = t([[0., 0., 0.], [2., 0., 0.], [0., 2., 0.]]);
    for v in [
        [[0., 0., 0.], [0., 2., 0.], [-2., 0., 0.]], // shared edge
        [[0., 0., 0.], [-2., 0., 0.], [0., -2., 0.]], // shared vertex
        [[0., 0., 1.], [2., 0., 1.], [0., 2., 1.]],
        [[0., 0., 0.], [2., 0., 0.], [0., 0., 2.]], // folded shared edge
    ] {
        assert!(!intersection::beyond_shared_simplex(&a, &t(v)).unwrap());
    }
    for v in [
        a.v,
        [[0., 0., 0.], [2., 0., 0.], [1., 1., 0.]], // overlapping shared edge
        [[0.25, 0.25, 0.], [1., 0.25, 0.], [0.25, 1., 0.]], // contained
        [[1., 0., 0.], [2., -1., 0.], [0., -1., 0.]], // T junction
        [[0.5, -1., 0.], [0.5, 2., 0.], [1., 2., 0.]], // planar crossing
        [[0.5, 0.5, -1e-12], [0.5, 0.5, 1.], [1., 1., 1.]], // strict plane crossing
    ] {
        assert!(intersection::beyond_shared_simplex(&a, &t(v)).unwrap());
    }
}
#[test]
fn open_pinched_separate_and_intersecting_boundaries_are_rejected() {
    let tetra = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
    let a = [[0., 0., 0.], [4., 0., 0.], [0., 4., 0.], [0., 0., 4.]];
    assert!(Solid::new(&mesh(&a, &tetra[..3]), 128)
        .err()
        .unwrap()
        .to_string()
        .contains("open edges"));
    let mut v = a.to_vec();
    v.extend(a.map(|p| p.map(|x| -x)));
    let mut f = tetra.to_vec();
    f.extend(tetra.map(|[a, b, c]| [a + 4, c + 4, b + 4]));
    assert!(Solid::new(&mesh(&v, &f), 128)
        .err()
        .unwrap()
        .to_string()
        .contains("pinches"));
    let mut v = a.to_vec();
    v.extend(a.map(|p| p.map(|x| x + 10.)));
    let mut f = tetra.to_vec();
    f.extend(tetra.map(|p| p.map(|i| i + 4)));
    assert!(Solid::new(&mesh(&v, &f), 128)
        .err()
        .unwrap()
        .to_string()
        .contains("disconnected"));
    let base = box_mesh([0.; 3], [1.; 3]);
    let mut raw = base
        .transformed_stl(Pose::from_euler([0.; 3], [0.; 3]))
        .unwrap();
    raw = raw.replace("vertex 1 1 1\n", "vertex 0.5 0.5 -0.5\n");
    let folded = Mesh::read(raw.as_bytes(), 1.).unwrap();
    assert!(Solid::new(&folded, 288)
        .err()
        .unwrap()
        .to_string()
        .contains("intersect beyond"));
}
#[test]
fn topology_budget_is_not_an_acceptance_shortcut() {
    let mesh = box_mesh([-1.; 3], [1.; 3]);
    assert!(Solid::new(&mesh, 1)
        .err()
        .unwrap()
        .to_string()
        .contains("max_topology_visits"));
}

fn assembled(parts: &[(&Mesh, bool, V)]) -> Mesh {
    let mut vertices = Vec::new();
    let mut faces = Vec::new();
    for (part, reverse, translation) in parts {
        for triangle in part.triangles() {
            let first = vertices.len();
            vertices.extend(triangle.v.map(|p| add(p, *translation)));
            faces.push(if *reverse {
                [first, first + 2, first + 1]
            } else {
                [first, first + 1, first + 2]
            });
        }
    }
    mesh(&vertices, &faces)
}

#[test]
fn required_material_keeps_separate_bodies_and_the_gap_between_them() {
    let unit = box_mesh([-1.; 3], [1.; 3]);
    let shift = [2. * (unit.max[0] - unit.min[0]), 0., 0.];
    let mesh = assembled(&[(&unit, false, [0.; 3]), (&unit, false, shift)]);
    let n = mesh.triangles().len();
    let solid = Solid::required(&mesh, 2 * n * n, n).unwrap();
    assert_eq!(solid.shells.len(), 2);
    assert_eq!(solid.structure_winding_terms, n);
    for p in [[0.; 3], shift] {
        assert_eq!(solid.distance(p).unwrap().inward, 1.);
    }
    assert_eq!(solid.distance(scale(shift, 0.5)).unwrap().inward, -1.);
    assert!(Solid::new(&mesh, 2 * n * n)
        .err()
        .unwrap()
        .to_string()
        .contains("disconnected"));
}

#[test]
fn required_cavity_orientation_is_retained_instead_of_filled() {
    let outer = box_mesh([-2.; 3], [2.; 3]);
    let inner = box_mesh([-1.; 3], [1.; 3]);
    let hollow = assembled(&[(&outer, false, [0.; 3]), (&inner, true, [0.; 3])]);
    let n = hollow.triangles().len();
    let solid = Solid::required(&hollow, 2 * n * n, n).unwrap();
    assert_eq!(solid.distance([0.; 3]).unwrap().inward, -1.);
    assert_eq!(solid.distance([1.5, 0., 0.]).unwrap().inward, 0.5);
    assert_eq!(solid.distance([3., 0., 0.]).unwrap().inward, -1.);
    assert!(solid.shells[0].signed_volume > 0.);
    assert!(solid.shells[1].signed_volume < 0.);
    let filled_twice = assembled(&[(&outer, false, [0.; 3]), (&inner, false, [0.; 3])]);
    assert!(Solid::required(&filled_twice, 2 * n * n, n)
        .err()
        .unwrap()
        .to_string()
        .contains("surrounding winding"));
    assert!(Solid::required(&hollow, 2 * n * n, n - 1)
        .err()
        .unwrap()
        .to_string()
        .contains("max_winding_terms"));
}
