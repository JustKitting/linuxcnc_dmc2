//! Local tactile contour following. Plans data only; LinuxCNC owns each move.
use super::Progress;
use crate::probe_data::mapper_settings::{
    close, BoundarySearch, LocalSearch, Phase, Request, Sample, Settings,
};
use std::{
    collections::BTreeSet,
    f64::consts::{FRAC_PI_2, TAU},
    fmt,
};

type Point = [f64; 2];

#[derive(Debug)]
pub(super) enum TraceError {
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
    NoOutwardStep,
}
impl fmt::Display for TraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NoOutwardStep => "No full X step remains to the next bounded search point in physical RIGHT / LinuxCNC -X. No outside point was inferred; choose a starting point with room inside the plate envelope before a new Run.",
            Self::SearchPrecision => "The selected search geometry exhausted representable coordinate precision. Correct the outline policy or resolution before Run.",
            Self::NoTop => "No starting top contact was retained within the descent budget.",
            Self::PlateContact => "The first outward search still touches at the plate envelope; there is no measured outside point.",
            Self::MissingContact => "The search toward the retained inside point did not retain a fine edge contact.",
            Self::MissingReturn => "The last local search has no retained released endpoint.",
            Self::ChangedPlan(i) => return write!(f, "OUTLINE_PLAN_MISMATCH: retained sample {i} differs from its planned local path. Use Abort then Pendant Mode; begin a new Run."),
            Self::WrongPlane => "A local edge contact or released endpoint differs from the retained tracing Z plane.",
            Self::BlockedBackoff => "The withdrawal or return along a retained clear path encountered another surface before reaching its endpoint.",
            Self::EmptySweep => "The entire local search circle was traversed without another edge contact. Review Initial / minimum trace interval in Scripts before another Run.",
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
    pub points: Vec<[f64; 3]>, // selected original fine machine triggers, in contour order
    pub sequences: Vec<usize>, // original fine record identities, not sampling order
    pub plane: Option<f64>,
    pub refinements: Vec<Refinement>,
    pub result: Result<(), Progress>,
}
pub struct Refinement {
    pub from_sequence: usize,
    pub coarse_sequence: usize,
    pub midpoint_sequence: Option<usize>,
    pub radius_mm: f64,
    pub error_mm: Option<f64>,
    pub resolved: bool,
}
impl Refinement {
    pub fn json(&self) -> String {
        format!("{{\"from_sequence\":{},\"coarse_sequence\":{},\"midpoint_sequence\":{},\"radius_mm\":{},\"midpoint_chord_error_mm\":{},\"minimum_spacing_reached\":{},\"resolved\":{}}}",self.from_sequence,self.coarse_sequence,self.midpoint_sequence.map(|v|v.to_string()).unwrap_or_else(||"null".into()),self.radius_mm,self.error_mm.map(|v|v.to_string()).unwrap_or_else(||"null".into()),self.midpoint_sequence.is_none(),self.resolved)
    }
}

pub(super) struct Reader<'a> {
    pub(super) s: &'a Settings,
    samples: &'a [Sample],
    cursor: usize,
    pub(super) points: Vec<[f64; 3]>,
    pub(super) sequences: Vec<usize>,
    plane: Option<f64>,
    pub(super) refinements: Vec<Refinement>,
}
impl<'a> Reader<'a> {
    pub(super) fn select(&mut self, sample: &Sample) -> Result<(), Progress> {
        self.points
            .push(sample.trigger.ok_or(TraceError::MissingContact)?);
        self.sequences.push(sample.sequence);
        Ok(())
    }
    pub(super) fn finished(&self) -> Result<(), Progress> {
        if self.cursor == self.samples.len() {
            Ok(())
        } else {
            Err(TraceError::ChangedPlan(self.cursor).into())
        }
    }
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
    pub(super) fn local(
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
pub(super) fn xy(p: [f64; 3]) -> Point {
    [p[0], p[1]]
}
pub(super) fn add(a: Point, b: Point) -> Point {
    [a[0] + b[0], a[1] + b[1]]
}
pub(super) fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}
pub(super) fn scale(p: Point, k: f64) -> Point {
    [p[0] * k, p[1] * k]
}
pub(super) fn length(p: Point) -> f64 {
    p[0].hypot(p[1])
}
pub(super) fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
pub(super) fn unit(p: Point) -> Result<Point, Progress> {
    let d = length(p);
    if !d.is_finite() || d == 0.0 {
        return Err(TraceError::RepeatedRegion.into());
    }
    Ok(scale(p, 1.0 / d))
}
pub(super) fn rotate(p: Point, angle: f64) -> Point {
    [
        p[0] * angle.cos() - p[1] * angle.sin(),
        p[0] * angle.sin() + p[1] * angle.cos(),
    ]
}
pub(super) fn point(sample: &Sample, s: &Settings) -> Result<[f64; 3], Progress> {
    let p = sample.trigger.ok_or(TraceError::MissingContact)?;
    Ok([0, 1, 2].map(|i| p[i] - s.offset[i]))
}
pub(super) fn endpoint(sample: &Sample) -> Result<Point, Progress> {
    Ok(xy(sample.returned.ok_or(TraceError::MissingReturn)?))
}

pub fn run(s: &Settings, samples: &[Sample]) -> Outline {
    let mut r = Reader {
        s,
        samples,
        cursor: 0,
        points: Vec::new(),
        sequences: Vec::new(),
        plane: None,
        refinements: Vec::new(),
    };
    let result = walk(&mut r);
    Outline {
        points: r.points,
        sequences: r.sequences,
        plane: r.plane,
        refinements: r.refinements,
        result,
    }
}
fn walk(r: &mut Reader<'_>) -> Result<(), Progress> {
    let s = r.s;
    let policy = s.outline.ok_or_else(|| {
        Progress::Invalid(
            "The outline policy snapshot is missing. Reopen the script and start a new Run.".into(),
        )
    })?;
    let seed = xy(s.origin);
    let reference = r.top(seed, Phase::Reference)?;
    if reference.trigger.is_none() {
        return Err(TraceError::NoTop.into());
    }
    let mut inside = seed;
    let mut top = point(reference, s)?;
    // Preserve the existing first search direction: physical RIGHT / LinuxCNC -X.
    // Only this one edge bracket is located; no opposite-side search or grid.
    let mut outside = match policy.boundary_search {
        BoundarySearch::EnvelopeThenBisect => {
            let edge = [s.min[0], seed[1]];
            if r.top(edge, Phase::Boundary)?.trigger.is_some() {
                return Err(TraceError::PlateContact.into());
            }
            edge
        }
        BoundarySearch::ExponentialOffsets { initial_mm } => {
            let available = seed[0] - s.min[0];
            if available < s.step[0] && !close(available, s.step[0]) {
                return Err(TraceError::NoOutwardStep.into());
            }
            let mut offset = initial_mm.min(available);
            loop {
                // Physical RIGHT / LinuxCNC -X. Offset is measured from seed;
                // the final bounded sample is exactly the retained plate edge.
                let candidate = [
                    if offset == available {
                        s.min[0]
                    } else {
                        seed[0] - offset
                    },
                    seed[1],
                ];
                if candidate[0] >= inside[0] {
                    return Err(TraceError::SearchPrecision.into());
                }
                let travel = inside[0] - candidate[0];
                if travel < s.step[0] && !close(travel, s.step[0]) {
                    return Err(TraceError::NoOutwardStep.into());
                }
                let sample = r.top(candidate, Phase::Boundary)?;
                if sample.trigger.is_none() {
                    break candidate;
                }
                inside = candidate;
                top = point(sample, s)?;
                if offset == available {
                    return Err(TraceError::PlateContact.into());
                }
                // The user's 1, 2, 4, ... sequence; plate bounds cap growth.
                let next = (offset * 2.0).min(available);
                if next <= offset {
                    return Err(TraceError::SearchPrecision.into());
                }
                offset = next;
            }
        }
    };
    while length(sub(inside, outside)) > policy.handoff_mm {
        // Refine only the observed bracket. Physical RIGHT / LinuxCNC -X;
        // physical LEFT / LinuxCNC +X, according to the previous sample.
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
    let plane = s.trace_z(top[2]);
    r.plane = Some(plane);
    let first = r.local(Phase::OutlineEnter, outside, inside, plane)?;
    r.select(first)?;
    if matches!(policy.local_search, LocalSearch::GrowingRefinement) {
        return super::adaptive::walk(r, first, outside, inside, plane);
    }
    let mut contact = xy(point(first, s)?);
    let start = contact;
    let first_outward = unit(sub(outside, inside))?;
    let mut outward = first_outward;
    let mut current = endpoint(first)?;
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
        let anchor = add(contact, scale(outward, s.outline_backoff()));
        if s.full_outline_backoff() {
            if !s.endpoint_matches(
                [current[0], current[1], plane],
                [anchor[0], anchor[1], plane],
            ) {
                return Err(Progress::Invalid("The last contact has not completed its full probe-diameter backoff. No next candidate was issued; Abort then Pendant Mode.".into()));
            }
        } else {
            // Historical ledgers retain their original, separately planned retreat.
            let retreat = r.local(Phase::OutlineBackoff, current, anchor, plane)?;
            if retreat.trigger.is_some() {
                return Err(TraceError::BlockedBackoff.into());
            }
            current = endpoint(retreat)?;
        }
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
        r.select(sample)?;
        // A local revisit cannot close the contour. Require travel beyond a
        // complete search-circle circumference, a compatible approach, then an
        // independent fine re-touch of the original seam.
        if distance > TAU * s.grid
            && length(sub(contact, start)) <= s.grid
            && dot(outward, first_outward) > 0.0
        {
            let approach = add(start, scale(first_outward, s.outline_backoff()));
            let target = sub(start, scale(first_outward, s.resolution));
            let closing = r.local(Phase::OutlineClose, approach, target, plane)?;
            let measured = point(closing, s)?;
            if length(sub(xy(measured), start)) > s.resolution {
                return Err(TraceError::ClosureMismatch.into());
            }
            r.select(closing)?;
            return r.finished();
        }
    }
}
