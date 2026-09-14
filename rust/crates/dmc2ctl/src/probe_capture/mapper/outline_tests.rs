//! Independent polygon-ray fixtures exercise the production planner only.
//! These numerical checks provide no evidence of physical probe behaviour.
use super::{
    model::{
        BoundarySearch, Mode, OutlinePolicy, OutlineRevision, Phase, Request, Sample, Settings,
    },
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
const CURRENT_POLICY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../config/mapper-outline.txt"
));

fn replay(polygon: &[P], policy: OutlinePolicy) -> outline::Outline {
    let mut s = fixture(Mode::Outline);
    s.outline = Some(policy);
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
    for policy in [
        "DMC2_OUTLINE_POLICY_V1\nhandoff_mm=25.4\n",
        "DMC2_OUTLINE_POLICY_V2\nhandoff_mm=25.4\n",
        CURRENT_POLICY,
    ] {
        replay(&rectangle, OutlinePolicy::read(policy).unwrap());
    }
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
    let result = replay(&notch, OutlinePolicy::read(CURRENT_POLICY).unwrap());
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

/// Mathematical top samples only. No NML, HAL, controller or machine action.
fn top_prefix(
    s: &Settings,
    has_top: impl Fn(f64) -> bool,
) -> (Vec<Request>, Result<Request, String>) {
    let mut samples = Vec::new();
    let mut requests = Vec::new();
    loop {
        let request = match outline::run(s, &samples) {
            Err(Progress::Need(request)) if request.phase == Phase::OutlineEnter => {
                return (requests, Ok(request));
            }
            Err(Progress::Need(request)) => request,
            Err(Progress::Invalid(error)) => return (requests, Err(error)),
            Ok(_) => panic!("An outline cannot finish from top samples alone"),
        };
        assert!(matches!(request.phase, Phase::Reference | Phase::Boundary));
        assert!(requests.len() < 10000, "offline fixture execution budget");
        // First direction: physical RIGHT / LinuxCNC -X; refinement may return
        // physical LEFT / LinuxCNC +X. This fixture sends no motion commands.
        s.bounds(request.target).unwrap();
        assert_eq!(request.target[1], s.origin[1]);
        assert!(request.target[0] <= s.origin[0]);
        requests.push(request);
        samples.push(Sample {
            request,
            trigger: has_top(request.target[0]).then_some([
                request.target[0] + s.offset[0],
                request.target[1] + s.offset[1],
                s.origin[2] - s.side_depth + s.offset[2],
            ]),
            returned: Some([request.target[0], request.target[1], s.origin[2]]),
        });
    }
}

#[test]
fn exponential_first_edge_refines_only_the_observed_bracket() {
    let mut s = fixture(Mode::Outline);
    let mut policy = OutlinePolicy::read(CURRENT_POLICY).unwrap();
    // Use the existing fixture resolution to exercise the binary-refinement
    // branch. This is not a change to the installed handoff distance.
    policy.handoff_mm = s.resolution;
    s.outline = Some(policy);
    let edge = -11.0; // Existing rectangle fixture's physical RIGHT / LinuxCNC -X face.
    let (requests, entry) = top_prefix(&s, |x| x >= edge);
    let entry = entry.unwrap();
    let x: Vec<_> = requests.iter().map(|r| r.target[0]).collect();
    assert_eq!(&x[..6], &[0.0, -1.0, -2.0, -4.0, -8.0, -16.0]);
    assert!(x[6..].iter().all(|x| (-16.0..=-8.0).contains(x)));
    assert!(entry.approach[0] < edge && entry.target[0] >= edge);
    assert!(entry.target[0] - entry.approach[0] <= s.resolution);
    assert_eq!(entry.target[2], s.trace_z(s.origin[2] - s.side_depth));
}

#[test]
fn exponential_plate_contact_is_terminal_and_does_not_invent_an_outside() {
    let mut s = fixture(Mode::Outline);
    s.outline = Some(OutlinePolicy::read(CURRENT_POLICY).unwrap());
    let (requests, result) = top_prefix(&s, |_| true);
    let x: Vec<_> = requests.iter().map(|r| r.target[0]).collect();
    assert_eq!(x, [0.0, -1.0, -2.0, -4.0, -8.0, -16.0, s.min[0]]);
    assert!(result.unwrap_err().contains("no measured outside point"));
    s.min[0] = s.origin[0];
    let (requests, result) = top_prefix(&s, |_| true);
    assert_eq!(requests.len(), 1); // No repeated zero-distance boundary sample.
    assert!(result.is_err());
    s.min[0] = s.origin[0] - s.step[0] / 2.0;
    let (requests, result) = top_prefix(&s, |_| true);
    assert_eq!(requests.len(), 1); // No sub-step target promoted to a miss.
    assert!(result.unwrap_err().contains("No full X step"));
}

#[test]
fn historical_search_policy_is_not_reinterpreted_as_exponential() {
    for (text, revision) in [
        (
            "DMC2_OUTLINE_POLICY_V1\nhandoff_mm=25.4\n",
            OutlineRevision::ContactPlane,
        ),
        (
            "DMC2_OUTLINE_POLICY_V2\nhandoff_mm=25.4\n",
            OutlineRevision::BelowContact,
        ),
    ] {
        let policy = OutlinePolicy::read(text).unwrap();
        assert_eq!(policy.revision, revision);
        assert!(matches!(
            policy.boundary_search,
            BoundarySearch::EnvelopeThenBisect
        ));
        let mut s = fixture(Mode::Outline);
        s.outline = Some(policy);
        let (requests, _) = top_prefix(&s, |x| x >= -11.0);
        assert_eq!(requests[1].target[0], s.min[0]);
    }
    for text in [
        "DMC2_OUTLINE_POLICY_V3\nhandoff_mm=25.4\n",
        "DMC2_OUTLINE_POLICY_V2\nhandoff_mm=25.4\ninitial_edge_offset_mm=1\n",
        "DMC2_OUTLINE_POLICY_V3\nhandoff_mm=25.4\ninitial_edge_offset_mm=0\n",
    ] {
        assert!(OutlinePolicy::read(text).is_err());
    }
}
