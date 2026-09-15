//! Distances to finite reported sweeps; no extrapolation beyond their endpoints.
use super::super::geometry::*;
use crate::object_map::positional::mesh::Triangle;

fn checked(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}
pub(super) fn point_segment(p: V, a: V, b: V) -> Option<f64> {
    if ![p, a, b].into_iter().all(finite) {
        return None;
    }
    let d = sub(b, a);
    let length = checked(norm(d))?;
    if length == 0. {
        return checked(norm(sub(p, a)));
    }
    let u = d.map(|x| x / length);
    let t = checked(dot(sub(p, a), u))?.clamp(0., length);
    checked(norm(sub(p, add(a, scale(u, t)))))
}
fn segment_segment(a: V, b: V, c: V, d: V) -> Option<f64> {
    let u = sub(b, a);
    let v = sub(d, c);
    let w = sub(a, c);
    let extent = checked(norm(u))?
        .max(checked(norm(v))?)
        .max(checked(norm(w))?);
    if extent == 0. {
        return Some(0.);
    }
    let [u, v, w] = [u, v, w].map(|p| p.map(|x| x / extent));
    let aa = dot(u, u);
    let bb = dot(u, v);
    let cc = dot(v, v);
    let dd = dot(u, w);
    let ee = dot(v, w);
    if aa == 0. {
        return point_segment(a, c, d);
    }
    if cc == 0. {
        return point_segment(c, a, b);
    }
    let denominator = aa * cc - bb * bb;
    let mut s = if denominator > 0. {
        ((bb * ee - cc * dd) / denominator).clamp(0., 1.)
    } else {
        0.
    };
    let mut t = (bb * s + ee) / cc;
    if t < 0. {
        t = 0.;
        s = (-dd / aa).clamp(0., 1.);
    } else if t > 1. {
        t = 1.;
        s = ((bb - dd) / aa).clamp(0., 1.);
    }
    checked(norm(sub(add(w, scale(u, s)), scale(v, t))) * extent)
}
fn inside(p: V, t: Triangle) -> bool {
    let axis = (0..3)
        .max_by(|a, b| t.n[*a].abs().total_cmp(&t.n[*b].abs()))
        .unwrap();
    let xy = |p: V| robust::Coord {
        x: p[(axis + 1) % 3],
        y: p[(axis + 2) % 3],
    };
    let signs = [0, 1, 2].map(|i| robust::orient2d(xy(t.v[i]), xy(t.v[(i + 1) % 3]), xy(p)));
    signs.iter().all(|d| *d >= 0.) || signs.iter().all(|d| *d <= 0.)
}
pub(super) fn segment_triangle(a: V, b: V, t: Triangle) -> Option<f64> {
    if ![a, b, t.v[0], t.v[1], t.v[2], t.n].into_iter().all(finite) {
        return None;
    }
    let anchor = t.v[0];
    let extent = [a, b, t.v[0], t.v[1], t.v[2]]
        .into_iter()
        .map(|p| checked(norm(sub(p, anchor))))
        .try_fold(0_f64, |a, d| Some(a.max(d?)))?;
    if extent == 0. {
        return Some(0.);
    }
    let scaled = |p: V| sub(p, anchor).map(|x| x / extent);
    let a = scaled(a);
    let b = scaled(b);
    let t = Triangle {
        v: t.v.map(scaled),
        n: t.n,
    };
    let da = dot(sub(a, t.v[0]), t.n);
    let db = dot(sub(b, t.v[0]), t.n);
    if (da <= 0. && db >= 0.) || (da >= 0. && db <= 0.) {
        if da != db {
            let p = add(a, scale(sub(b, a), da / (da - db)));
            if inside(p, t) {
                return Some(0.);
            }
        }
    }
    let mut distance =
        checked(norm(sub(a, t.nearest(a))))?.min(checked(norm(sub(b, t.nearest(b))))?);
    for i in 0..3 {
        distance = distance.min(segment_segment(a, b, t.v[i], t.v[(i + 1) % 3])?);
    }
    checked(distance * extent)
}
