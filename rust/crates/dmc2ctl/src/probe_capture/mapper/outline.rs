//! Local tactile contour following. Plans data only; LinuxCNC owns each move.
use super::{
    model::{close, Phase, Request, Sample, Settings},
    search::Progress,
};
use std::{
    collections::BTreeSet,
    f64::consts::{FRAC_PI_2, TAU},
    fmt,
};

type Point = [f64; 2];

#[derive(Debug)]
enum TraceError {
    NoTop,
    PlateContact,
    MissingContact,
    MissingReturn,
    ChangedPlan(usize),
    WrongPlane,
    BlockedBackoff,
    EmptySweep,
    RepeatedRegion,
    ClosureMismatch,
    SearchPrecision,
}
impl fmt::Display for TraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::SearchPrecision => "The selected search geometry exhausted representable coordinate precision. Correct the outline policy or resolution before Run.",
            Self::NoTop => "No starting top contact was retained within the descent budget.",
            Self::PlateContact => "The first outward search still touches at the plate envelope; there is no measured outside point.",
            Self::MissingContact => "The search toward the retained inside point did not retain a fine edge contact.",
            Self::MissingReturn => "The last local search has no retained released endpoint.",
            Self::ChangedPlan(i) => return write!(f, "OUTLINE_PLAN_MISMATCH: retained sample {i} differs from its planned local path. Use Abort then Pendant Mode; begin a new Run."),
            Self::WrongPlane => "A local edge contact or released endpoint differs from the retained tracing Z plane.",
            Self::BlockedBackoff => "The radial withdrawal encountered another surface before reaching the local search circle.",
            Self::EmptySweep => "The entire local search circle was traversed without another edge contact. Adjust Trace step in Scripts before another Run.",
            Self::RepeatedRegion => "The trace revisited the same local contact and direction before confirming the starting seam.",
            Self::ClosureMismatch => "The closing touch did not match the original edge contact within Outline resolution.",
        };
        write!(f, "OUTLINE_{self:?}: {message} Partial contacts are retained. Use Abort then Pendant Mode.")
    }
}
impl From<TraceError> for Progress {
    fn from(e: TraceError) -> Self {
        Self::Invalid(e.to_string())
    }
}

pub struct Outline {
    pub points: Vec<[f64; 3]>, // original fine machine-coordinate triggers, ordered
    pub plane: f64,            // work-coordinate Z of the last top contact
}

struct Reader<'a> {
    s: &'a Settings,
    samples: &'a [Sample],
    cursor: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, request: Request) -> Result<&'a Sample, Progress> {
        self.s.bounds(request.target)?;
        self.s
            .bounds([request.approach[0], request.approach[1], request.target[2]])?;
        let Some(sample) = self.samples.get(self.cursor) else {
            return Err(Progress::Need(request));
        };
        if request.phase != sample.request.phase
            || request.edge != sample.request.edge
            || !(0..3).all(|i| close(request.target[i], sample.request.target[i]))
            || !(0..2).all(|i| close(request.approach[i], sample.request.approach[i]))
        {
            return Err(TraceError::ChangedPlan(self.cursor).into());
        }
        self.cursor += 1;
        Ok(sample)
    }
    fn top(&mut self, xy: Point, phase: Phase) -> Result<&'a Sample, Progress> {
        self.take(Request::top(self.s, phase, xy))
    }
    fn local(
        &mut self,
        phase: Phase,
        from: Point,
        to: Point,
        z: f64,
    ) -> Result<&'a Sample, Progress> {
        let sample = self.take(Request {
            phase,
            edge: -1,
            approach: from,
            target: [to[0], to[1], z],
        })?;
        let returned = sample.returned.ok_or(TraceError::MissingReturn)?;
        let tolerance = self.s.step[2] / 2.0 + 1e-9;
        if (returned[2] - z).abs() > tolerance
            || sample
                .trigger
                .is_some_and(|p| (p[2] - self.s.offset[2] - z).abs() > tolerance)
        {
            return Err(TraceError::WrongPlane.into());
        }
        Ok(sample)
    }
}
fn xy(p: [f64; 3]) -> Point {
    [p[0], p[1]]
}
fn add(a: Point, b: Point) -> Point {
    [a[0] + b[0], a[1] + b[1]]
}
fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}
fn scale(p: Point, k: f64) -> Point {
    [p[0] * k, p[1] * k]
}
fn length(p: Point) -> f64 {
    p[0].hypot(p[1])
}
fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn unit(p: Point) -> Result<Point, Progress> {
    let d = length(p);
    if !d.is_finite() || d == 0.0 {
        return Err(TraceError::RepeatedRegion.into());
    }
    Ok(scale(p, 1.0 / d))
}
fn rotate(p: Point, angle: f64) -> Point {
    [
        p[0] * angle.cos() - p[1] * angle.sin(),
        p[0] * angle.sin() + p[1] * angle.cos(),
    ]
}
fn point(sample: &Sample, s: &Settings) -> Result<[f64; 3], Progress> {
    let p = sample.trigger.ok_or(TraceError::MissingContact)?;
    Ok([0, 1, 2].map(|i| p[i] - s.offset[i]))
}
fn endpoint(sample: &Sample) -> Result<Point, Progress> {
    Ok(xy(sample.returned.ok_or(TraceError::MissingReturn)?))
}

pub fn run(s: &Settings, samples: &[Sample]) -> Result<Outline, Progress> {
    let handoff = s.outline_handoff.ok_or_else(|| {
        Progress::Invalid(
            "The outline policy snapshot is missing. Reopen the script and start a new Run.".into(),
        )
    })?;
    let mut r = Reader {
        s,
        samples,
        cursor: 0,
    };
    let seed = xy(s.origin);
    let reference = r.top(seed, Phase::Reference)?;
    if reference.trigger.is_none() {
        return Err(TraceError::NoTop.into());
    }
    let mut inside = seed;
    let mut top = point(reference, s)?;
    // Preserve the existing first search direction: physical RIGHT / LinuxCNC -X.
    // Only this one edge bracket is located; no opposite-side search or grid.
    let mut outside = [s.min[0], seed[1]];
    if r.top(outside, Phase::Boundary)?.trigger.is_some() {
        return Err(TraceError::PlateContact.into());
    }
    while length(sub(inside, outside)) > handoff {
        let mid = scale(add(inside, outside), 0.5);
        if mid == inside || mid == outside {
            return Err(TraceError::SearchPrecision.into());
        }
        let sample = r.top(mid, Phase::Boundary)?;
        if sample.trigger.is_some() {
            inside = mid;
            top = point(sample, s)?;
        } else {
            outside = mid;
        }
    }
    let plane = top[2];
    let first = r.local(Phase::OutlineEnter, outside, inside, plane)?;
    let mut contact = xy(point(first, s)?);
    let start = contact;
    let first_outward = unit(sub(outside, inside))?;
    let mut outward = first_outward;
    let mut current = endpoint(first)?;
    let mut points = vec![first.trigger.ok_or(TraceError::MissingContact)?];
    let mut distance = 0.0;
    // Chord sagitta <= selected resolution. A sector never spans more than a
    // quadrant, so a candidate chord cannot run through its contact pivot.
    let angle = (2.0 * (1.0 - s.resolution / s.grid).acos()).min(FRAC_PI_2);
    if !angle.is_finite() || angle <= 0.0 || TAU / angle > i64::MAX as f64 {
        return Err(TraceError::SearchPrecision.into());
    }
    let sectors = (TAU / angle).ceil() as usize;
    let mut visited = BTreeSet::new();
    loop {
        let direction_bin = ((outward[1].atan2(outward[0]).rem_euclid(TAU) / TAU) * sectors as f64)
            .round() as i64
            % sectors as i64;
        let key = (
            (contact[0] / s.resolution).round() as i64,
            (contact[1] / s.resolution).round() as i64,
            direction_bin,
        );
        if !visited.insert(key) {
            return Err(TraceError::RepeatedRegion.into());
        }
        // Withdraw along the reverse of the actual last approach, at fixed Z.
        let anchor = add(contact, scale(outward, s.grid));
        let retreat = r.local(Phase::OutlineBackoff, current, anchor, plane)?;
        if retreat.trigger.is_some() {
            return Err(TraceError::BlockedBackoff.into());
        }
        current = endpoint(retreat)?;
        let mut found = None;
        for sector in 1..=sectors {
            let candidate = add(
                contact,
                scale(
                    rotate(outward, TAU * sector as f64 / sectors as f64),
                    s.grid,
                ),
            );
            let sample = r.local(Phase::OutlineAdvance, current, candidate, plane)?;
            if sample.trigger.is_some() {
                found = Some((sample, current, candidate));
                break;
            }
            current = endpoint(sample)?;
        }
        let (sample, from, to) = found.ok_or(TraceError::EmptySweep)?;
        let next = xy(point(sample, s)?);
        distance += length(sub(next, contact));
        outward = unit(sub(from, to))?;
        contact = next;
        current = endpoint(sample)?;
        points.push(sample.trigger.ok_or(TraceError::MissingContact)?);
        // A local revisit cannot close the contour. Require travel beyond a
        // complete search-circle circumference, a compatible approach, then an
        // independent fine re-touch of the original seam.
        if distance > TAU * s.grid
            && length(sub(contact, start)) <= s.grid
            && dot(outward, first_outward) > 0.0
        {
            let approach = add(start, scale(first_outward, s.grid));
            let target = sub(start, scale(first_outward, s.resolution));
            let closing = r.local(Phase::OutlineClose, approach, target, plane)?;
            let measured = point(closing, s)?;
            if length(sub(xy(measured), start)) > s.resolution {
                return Err(TraceError::ClosureMismatch.into());
            }
            points.push(closing.trigger.ok_or(TraceError::MissingContact)?);
            if r.cursor != samples.len() {
                return Err(TraceError::ChangedPlan(r.cursor).into());
            }
            return Ok(Outline { points, plane });
        }
    }
}
