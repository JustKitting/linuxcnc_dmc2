//! Triangle intersections beyond an exact shared vertex or shared edge.
//! Adaptive orientation signs avoid a dimensional intersection tolerance.
use super::{Error, Triangle, V, data};
use robust::{Coord, Coord3D, orient2d, orient3d};

fn coord(p: V) -> Coord3D<f64> {
    Coord3D {
        x: p[0],
        y: p[1],
        z: p[2],
    }
}
fn sign(v: f64) -> Result<i8, Error> {
    if !v.is_finite() {
        return Err(data(
            "Mesh orientation arithmetic overflowed. Inspect source coordinates and units before repeating the topology analysis.",
        ));
    }
    Ok(if v > 0. {
        1
    } else if v < 0. {
        -1
    } else {
        0
    })
}
fn side(a: V, b: V, c: V, p: V) -> Result<i8, Error> {
    sign(orient3d(coord(a), coord(b), coord(c), coord(p)))
}
fn turn(a: V, b: V, p: V, drop: usize) -> Result<i8, Error> {
    let project = |v: V| Coord {
        x: v[(drop + 1) % 3],
        y: v[(drop + 2) % 3],
    };
    sign(orient2d(project(a), project(b), project(p)))
}
fn consistent(s: [i8; 3]) -> bool {
    s.iter().all(|x| *x >= 0) || s.iter().all(|x| *x <= 0)
}
fn in_triangle(p: V, t: &Triangle, drop: usize) -> Result<bool, Error> {
    Ok(consistent([
        turn(t.v[0], t.v[1], p, drop)?,
        turn(t.v[1], t.v[2], p, drop)?,
        turn(t.v[2], t.v[0], p, drop)?,
    ]))
}
fn directed(a: &Triangle, b: &Triangle) -> Result<bool, Error> {
    let s = [
        side(b.v[0], b.v[1], b.v[2], a.v[0])?,
        side(b.v[0], b.v[1], b.v[2], a.v[1])?,
        side(b.v[0], b.v[1], b.v[2], a.v[2])?,
    ];
    if s.iter().all(|x| *x > 0) || s.iter().all(|x| *x < 0) {
        return Ok(false);
    }
    let drop = (0..3)
        .max_by(|i, j| b.n[*i].abs().total_cmp(&b.n[*j].abs()))
        .unwrap();
    for i in 0..3 {
        if s[i] == 0 && !b.v.contains(&a.v[i]) && in_triangle(a.v[i], b, drop)? {
            return Ok(true);
        }
        let j = (i + 1) % 3;
        if s[i] * s[j] < 0 {
            // Edge crosses the plane strictly. Oriented edge/triangle tests
            // locate the crossing without constructing rounded coordinates.
            if consistent([
                side(a.v[i], a.v[j], b.v[0], b.v[1])?,
                side(a.v[i], a.v[j], b.v[1], b.v[2])?,
                side(a.v[i], a.v[j], b.v[2], b.v[0])?,
            ]) {
                return Ok(true);
            }
        } else if s[i] == 0 && s[j] == 0 {
            for k in 0..3 {
                let l = (k + 1) % 3;
                let ab = [
                    turn(a.v[i], a.v[j], b.v[k], drop)?,
                    turn(a.v[i], a.v[j], b.v[l], drop)?,
                ];
                let ba = [
                    turn(b.v[k], b.v[l], a.v[i], drop)?,
                    turn(b.v[k], b.v[l], a.v[j], drop)?,
                ];
                if ab[0] * ab[1] < 0 && ba[0] * ba[1] < 0 {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}
pub(super) fn beyond_shared_simplex(a: &Triangle, b: &Triangle) -> Result<bool, Error> {
    // Coincident triangles are not a permitted shared simplex. Endpoint
    // inclusion in both directions also catches collinear edge overlaps.
    Ok(a.v.iter().all(|p| b.v.contains(p)) || directed(a, b)? || directed(b, a)?)
}
