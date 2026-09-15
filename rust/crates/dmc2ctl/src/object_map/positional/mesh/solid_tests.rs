use super::super::geometry::*;
use super::{Mesh, Triangle, intersection, solid::Solid};

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
    assert!(
        Solid::new(&mesh(&a, &tetra[..3]), 128)
            .err()
            .unwrap()
            .to_string()
            .contains("open edges")
    );
    let mut v = a.to_vec();
    v.extend(a.map(|p| p.map(|x| -x)));
    let mut f = tetra.to_vec();
    f.extend(tetra.map(|[a, b, c]| [a + 4, c + 4, b + 4]));
    assert!(
        Solid::new(&mesh(&v, &f), 128)
            .err()
            .unwrap()
            .to_string()
            .contains("pinches")
    );
    let mut v = a.to_vec();
    v.extend(a.map(|p| p.map(|x| x + 10.)));
    let mut f = tetra.to_vec();
    f.extend(tetra.map(|p| p.map(|i| i + 4)));
    assert!(
        Solid::new(&mesh(&v, &f), 128)
            .err()
            .unwrap()
            .to_string()
            .contains("disconnected")
    );
    let base = box_mesh([0.; 3], [1.; 3]);
    let mut raw = base
        .transformed_stl(Pose::from_euler([0.; 3], [0.; 3]))
        .unwrap();
    raw = raw.replace("vertex 1 1 1\n", "vertex 0.5 0.5 -0.5\n");
    let folded = Mesh::read(raw.as_bytes(), 1.).unwrap();
    assert!(
        Solid::new(&folded, 288)
            .err()
            .unwrap()
            .to_string()
            .contains("intersect beyond")
    );
}
#[test]
fn topology_budget_is_not_an_acceptance_shortcut() {
    let mesh = box_mesh([-1.; 3], [1.; 3]);
    assert!(
        Solid::new(&mesh, 1)
            .err()
            .unwrap()
            .to_string()
            .contains("max_topology_visits")
    );
}
