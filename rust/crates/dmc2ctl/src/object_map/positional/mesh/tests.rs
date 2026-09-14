//! Numerical lookup checks, not evidence of physical registration accuracy.
use super::*;

const STL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../examples/positional-mapper/reference.stl"
));

fn linear(mesh: &Mesh, p: V) -> Nearest {
    mesh.triangles
        .iter()
        .enumerate()
        .map(|(i, t)| t.closest(p, i).unwrap())
        .reduce(|best, next| {
            if next.distance.abs() < best.distance.abs() {
                next
            } else {
                best
            }
        })
        .unwrap()
}

#[test]
fn cube_vertices_faces_inside_and_outside_retain_linear_results() {
    let mesh = Mesh::read(STL, 1.).unwrap();
    let coordinates: [Vec<f64>; 3] = std::array::from_fn(|i| {
        let lo = mesh.min[i];
        let hi = mesh.max[i];
        let span = hi - lo;
        vec![lo - span, lo, lo + span / 2., hi, hi + span]
    });
    for &x in &coordinates[0] {
        for &y in &coordinates[1] {
            for &z in &coordinates[2] {
                let p = [x, y, z];
                let result = mesh.nearest(p).unwrap();
                assert!(finite(result.normal));
                assert_eq!(result, linear(&mesh, p), "query {p:?}");
            }
        }
    }
    // Neighboring representable points at each surface exercise the pruning
    // boundary without introducing a dimensional tolerance.
    for triangle in &mesh.triangles {
        for vertex in triangle.v {
            for axis in 0..3 {
                for bits in [
                    vertex[axis].to_bits().saturating_sub(1),
                    vertex[axis].to_bits() + 1,
                ] {
                    let mut p = vertex;
                    p[axis] = f64::from_bits(bits);
                    if finite(p) {
                        let result = mesh.nearest(p).unwrap();
                        assert!(finite(result.normal));
                        assert_eq!(result, linear(&mesh, p), "query {p:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn tiny_finite_geometry_and_distances_keep_finite_normals() {
    // Derive the numerical stress case from the floating-point format, not a
    // modeled or measured physical feature size.
    let side = f64::MIN_POSITIVE.sqrt() / 2.;
    let triangle = Triangle::new([[0.; 3], [side, 0., 0.], [0., side, 0.]]).unwrap();
    assert_eq!(triangle.n, [0., 0., 1.]);
    let mesh = Mesh::read(STL, 1.).unwrap();
    let result = mesh.nearest([f64::from_bits(1), 0., 0.]).unwrap();
    assert_eq!(result.distance, f64::from_bits(1));
    assert_eq!(result.normal, [1., 0., 0.]);
}

#[test]
fn equal_distance_keeps_original_id_even_when_other_leaf_is_visited_first() {
    let triangles = [1., -1.]
        .map(|x| Triangle::new([[x, -0.25, -0.25], [x, 0.25, -0.25], [x, 0., 0.25]]).unwrap());
    let index = spatial::Index::new(&triangles).unwrap();
    let result = index.nearest(&triangles, [0.; 3]).unwrap();
    assert_eq!(result.triangle, 0);
    assert_eq!(result.point, [1., 0., 0.]);
    assert_eq!(result.distance, -1.);
}

#[test]
fn empty_geometry_and_nonfinite_queries_remain_errors() {
    assert!(spatial::Index::new(&[]).is_err());
    let mesh = Mesh::read(STL, 1.).unwrap();
    for coordinate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for axis in 0..3 {
            let mut query = [0.; 3];
            query[axis] = coordinate;
            assert!(mesh.nearest(query).is_err());
        }
    }
}
