//! Independent polygon-ray fixtures exercise the production planner only.
//! These numerical checks provide no evidence of physical probe behaviour.
use super::{
    model::{Mode, OutlinePolicy, OutlineRevision, Phase, Sample},
    outline,
    search::Progress,
    tests::fixture,
};

type P = [f64; 2];
fn cross(a: P, b: P) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}
fn hit(a: P, b: P, polygon: &[P]) -> Option<P> {
    let ray = sub(b, a);
    polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .take(polygon.len())
        .filter_map(|(&c, &d)| {
            let edge = sub(d, c);
            let denom = cross(ray, edge);
            if denom.abs() < f64::EPSILON {
                return None;
            }
            let t = cross(sub(c, a), edge) / denom;
            let u = cross(sub(c, a), ray) / denom;
            ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some(t)
        })
        .min_by(f64::total_cmp)
        .map(|t| [a[0] + t * ray[0], a[1] + t * ray[1]])
}
fn contains(p: P, polygon: &[P]) -> bool {
    let mut inside = false;
    for (&a, &b) in polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .take(polygon.len())
    {
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
    }
    inside
}
fn replay(polygon: &[P], revision: OutlineRevision) -> outline::Outline {
    let mut s = fixture(Mode::Outline);
    s.outline = Some(OutlinePolicy {
        revision,
        handoff_mm: 25.4,
    });
    s.grid = 1.0;
    s.origin[2] = 30.0;
    s.max[2] = 40.0;
    s.reach_floor = 0.0;
    s.side_depth = 12.7;
    let top = 24.0;
    let plane = s.trace_z(top);
    let mut samples = Vec::new();
    let mut tracing = false;
    loop {
        match outline::run(&s, &samples) {
            Ok(result) => {
                assert!(tracing);
                assert!(result.points.len() > polygon.len());
                let first = result.points[0];
                let last = *result.points.last().unwrap();
                assert!((first[0] - last[0]).hypot(first[1] - last[1]) <= s.resolution);
                assert!(result
                    .points
                    .iter()
                    .all(|p| (p[2] - s.offset[2] - plane).abs() < 1e-9));
                return result;
            }
            Err(Progress::Invalid(e)) => panic!("sample {}: {e}", samples.len()),
            Err(Progress::Need(request)) => {
                assert!(
                    samples.len() < 10000,
                    "offline fixture exceeded its execution budget"
                );
                assert!(!matches!(
                    request.phase,
                    Phase::Grid | Phase::Verify | Phase::Rim
                ));
                if tracing {
                    assert!(request.phase.is_outline());
                }
                tracing |= request.phase.is_outline();
                let target = [request.target[0], request.target[1]];
                let point = if request.phase.is_outline() {
                    assert_eq!(request.target[2], plane);
                    hit(request.approach, target, polygon)
                } else {
                    assert!(target[0] <= s.origin[0]); // no opposite-edge search
                    contains(target, polygon).then_some(target)
                };
                let trigger = point.map(|p| {
                    [
                        p[0] + s.offset[0],
                        p[1] + s.offset[1],
                        (if request.phase.is_outline() {
                            plane
                        } else {
                            top
                        }) + s.offset[2],
                    ]
                });
                let returned = if request.phase.is_outline() {
                    if let Some(p) = point {
                        let d = sub(request.approach, target);
                        let len = d[0].hypot(d[1]);
                        [
                            p[0] + d[0] / len
                                * if s.full_outline_backoff() {
                                    s.outline_backoff()
                                } else {
                                    s.step[0]
                                },
                            p[1] + d[1] / len
                                * if s.full_outline_backoff() {
                                    s.outline_backoff()
                                } else {
                                    s.step[1]
                                },
                            plane,
                        ]
                    } else {
                        request.target
                    }
                } else {
                    [target[0], target[1], s.origin[2]]
                };
                samples.push(Sample {
                    request,
                    trigger,
                    returned: Some(returned),
                });
            }
        }
    }
}
#[test]
fn traces_rotated_and_concave_outlines_without_opposite_search_or_grid() {
    let rectangle: Vec<P> = [[-11.0, -7.0], [11.0, -7.0], [11.0, 7.0], [-11.0, 7.0]]
        .into_iter()
        .map(|[x, y]| {
            let angle: f64 = 0.31;
            [
                x * angle.cos() - y * angle.sin(),
                x * angle.sin() + y * angle.cos(),
            ]
        })
        .collect();
    replay(&rectangle, OutlineRevision::ContactPlane);
    replay(&rectangle, OutlineRevision::BelowContact);
    let notch = [
        [-11.0, -7.0],
        [11.0, -7.0],
        [11.0, 7.0],
        [3.0, 7.0],
        [3.0, 2.0],
        [-3.0, 2.0],
        [-3.0, 7.0],
        [-11.0, 7.0],
    ];
    let result = replay(&notch, OutlineRevision::BelowContact);
    let work: Vec<P> = result
        .points
        .iter()
        .map(|p| [p[0] - 20.0, p[1] - 30.0])
        .collect();
    assert!(
        work.iter()
            .any(|p| p[0].abs() < 2.0 && (p[1] - 2.0).abs() < 1e-9),
        "trace must visit the recessed face instead of fitting a rectangle"
    );
}
