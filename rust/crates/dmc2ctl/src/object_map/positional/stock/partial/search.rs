//! Bounded refinement of fixed measured-support constraints, never stock filling.
use super::super::{
    material,
    optimize::{self, Domain, Objective},
    surface,
};
use super::request::Request;
use crate::object_map::{
    positional::{geometry::*, mesh::solid::Solid},
    Error,
};

#[derive(Clone, Copy)]
pub enum Kind {
    Clearance,
    Support,
    EmptySweep,
}
impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Clearance => "measured-plane-separation",
            Self::Support => "measured-support",
            Self::EmptySweep => "retained-empty-sweep",
        }
    }
}
pub struct Row {
    pub kind: Kind,
    pub region: Option<usize>,
    pub source: usize,
    pub sample: Option<usize>,
    pub slack: f64,
}
struct Anchor {
    region: usize,
    local: usize,
}
pub struct Ball {
    pub source: usize,
    pub part: usize,
    pub parts: usize,
    pub center: V,
    pub radius: f64,
}
pub struct ObjectiveData<'a> {
    a: &'a material::Assessment,
    local: Vec<surface::local::Local<'a>>,
    anchors: Vec<Anchor>,
    required: Solid<'a>,
    pub balls: Vec<Ball>,
    radius: f64,
    pub reserved_winding: usize,
    pub reserved_patches: usize,
}
pub fn pose(base: Pose, correction: [f64; 6]) -> Pose {
    let d = Pose::from_euler(
        [
            correction[3].to_degrees(),
            correction[4].to_degrees(),
            correction[5].to_degrees(),
        ],
        [correction[0], correction[1], correction[2]],
    );
    // Corrections rotate about the retained MODEL origin, not machine zero.
    Pose {
        r: mm(d.r, base.r),
        t: add(base.t, d.t),
    }
}
impl<'a> ObjectiveData<'a> {
    pub fn new(a: &'a material::Assessment, r: &Request) -> Result<Self, Error> {
        let material::request::EmptySpace::RequiredVolume {
            topology_visits, ..
        } = a.request.empty
        else {
            return Err(Error::Input("Partial placement requires a V3 material assessment with the explicit required-material boundary and retained no-contact model. Prepare and run that material check first.".into()));
        };
        let local =
            surface::local::build(&a.surface.contacts, &a.surface.stations, &a.surface.request)?;
        if local.iter().any(|l| {
            matches!(
                l.checks,
                surface::local::Checks::Disagrees | surface::local::Checks::NoContactConflict
            )
        }) {
            return Err(Error::Data("Source surface checks conflict. Resolve those original observations and run a new material assessment before refining placement; fitting cannot discard the disagreement.".into()));
        }
        let mut anchors = Vec::new();
        for (region, result) in a.regions.iter().enumerate() {
            for c in &result.comparisons {
                if c.checks != surface::local::Checks::Within {
                    continue;
                }
                let index=local.iter().position(|l|l.station.seed==c.source).ok_or_else(||Error::Data("An original supported comparison has no retained patch. Recalculate the material assessment from its original source.".into()))?;
                anchors.push(Anchor {
                    region,
                    local: index,
                });
            }
        }
        if anchors.is_empty() {
            return Err(Error::Input("This placement has no independently checked local surface support to retain during refinement. Acquire the missing support or choose a measured initial placement and reassess; empty space alone cannot locate material.".into()));
        }
        let passes = r.evaluations.checked_add(2).ok_or_else(|| {
            Error::Input("Refinement computation count overflows. Reduce max_evaluations.".into())
        })?;
        let reserved_patches = anchors.len().checked_mul(passes).ok_or_else(|| {
            Error::Input(
                "Patch comparison count overflows. Reduce the explicit computation domain/budget."
                    .into(),
            )
        })?;
        if reserved_patches > r.comparisons {
            return Err(Error::Input(format!("Refinement and reporting require {reserved_patches} patch comparisons, exceeding max_patch_comparisons={}. Increase this computation budget; no supported comparison was dropped.",r.comparisons)));
        }
        let mut balls = Vec::new();
        for (source, sweep) in a.surface.no_contact.iter().enumerate() {
            let delta = sub(sweep.center_end, sweep.center_from);
            let count = (norm(delta) / a.request.radius / 2.).ceil().max(1.);
            if !count.is_finite()
                || count >= usize::MAX as f64
                || count > r.sweeps.saturating_sub(balls.len()) as f64
            {
                return Err(Error::Input("max_sweep_samples cannot cover all finite clear paths at the retained cover radius. Increase that computation budget or explicitly refine the source material request; no path segment was skipped.".into()));
            }
            let parts = count as usize;
            for part in 0..parts {
                let start = add(sweep.center_from, scale(delta, part as f64 / parts as f64));
                let end = add(
                    sweep.center_from,
                    scale(delta, (part + 1) as f64 / parts as f64),
                );
                let center = add(start, scale(sub(end, start), 0.5));
                let radius = norm(sub(end, start)) / 2.;
                if !finite(center) || !radius.is_finite() {
                    return Err(Error::Data("Finite clear-path coverage overflowed. Inspect source coordinates and the retained coverage radius before refinement.".into()));
                }
                balls.push(Ball {
                    source,
                    part,
                    parts,
                    center,
                    radius,
                });
            }
        }
        let required = Solid::required(&a.candidate.mesh, topology_visits, r.winding)?;
        let reserved_winding = balls
            .len()
            .checked_mul(required.triangle_count())
            .and_then(|n| n.checked_mul(passes))
            .and_then(|n| n.checked_add(required.structure_winding_terms))
            .ok_or_else(|| {
                Error::Input(
                    "Refinement winding count overflows. Reduce the explicit search budget.".into(),
                )
            })?;
        if reserved_winding > r.winding {
            return Err(Error::Input(format!("Refinement and reporting require {reserved_winding} solid-angle terms, exceeding max_winding_terms={}. Increase this computation budget; required triangles and finite clear paths were not omitted.",r.winding)));
        }
        let mut radius = a
            .covers
            .iter()
            .map(|s| norm(s.center))
            .fold(0_f64, f64::max);
        // For inverse-pose empty-space queries, bound each fixed sweep point's
        // distance from every allowed translated model origin. Rotation
        // preserves this radius; the same Euler chord bound then applies.
        for ball in &balls {
            let far = std::array::from_fn(|i| {
                (ball.center[i] - a.candidate.pose.t[i] - r.lo[i])
                    .abs()
                    .max((ball.center[i] - a.candidate.pose.t[i] - r.hi[i]).abs())
            });
            radius = radius.max(norm(far));
        }
        if !radius.is_finite() {
            return Err(Error::Input("Placement movement bound overflows. Restrict source units and the explicit correction domain.".into()));
        }
        Ok(Self {
            a,
            local,
            anchors,
            required,
            balls,
            radius,
            reserved_winding,
            reserved_patches,
        })
    }
    pub fn rows(&self, at: [f64; 6]) -> Result<Vec<Row>, Error> {
        let transform = pose(self.a.candidate.pose, at).validate()?;
        let mut rows = Vec::new();
        for anchor in &self.anchors {
            let cover = &self.a.covers[anchor.region];
            let l = &self.local[anchor.local];
            let p = transform.point(cover.center);
            let d = dot(sub(p, l.patch.surface), l.patch.normal);
            let values = [
                (
                    Kind::Clearance,
                    -d - cover.radius - self.a.request.allowance - self.a.request.clearance,
                ),
                (
                    Kind::Support,
                    l.support
                        .margin(p, cover.radius, l.patch, &self.a.surface.request)?,
                ),
            ];
            for (kind, slack) in values {
                if !slack.is_finite() {
                    return Err(Error::Data("A retained local constraint overflowed. Inspect placement bounds and source units; no comparison was dropped.".into()));
                }
                rows.push(Row {
                    kind,
                    region: Some(anchor.region),
                    source: l.station.seed,
                    sample: None,
                    slack,
                });
            }
        }
        let inverse = transform.inverse();
        for (i, ball) in self.balls.iter().enumerate() {
            // Signed OUTSIDE distance is positive only in required-material
            // empty space. Its 1-Lipschitz bound covers the complete finite
            // subsegment, including a sweep wholly inside required material.
            let distance = self.required.distance(inverse.point(ball.center))?;
            let slack = -distance.inward
                - ball.radius
                - self.a.surface.no_contact[ball.source].radius
                - self.a.request.clearance;
            if !slack.is_finite() {
                return Err(Error::Data("A retained empty-space constraint overflowed. Inspect source geometry and placement bounds before refinement.".into()));
            }
            rows.push(Row {
                kind: Kind::EmptySweep,
                region: None,
                source: ball.source,
                sample: Some(i),
                slack,
            });
        }
        Ok(rows)
    }
    pub fn run(&self, r: &Request) -> Result<optimize::Fitted<6>, Error> {
        optimize::run_seeded(
            self,
            &Domain {
                lo: r.lo,
                hi: r.hi,
                margin: 0.,
                resolution: r.resolution,
                evaluations: r.evaluations,
            },
            Some([0.; 6]),
        )
    }
}
impl Objective<6> for ObjectiveData<'_> {
    fn value(&self, at: [f64; 6]) -> Result<f64, Error> {
        Ok(self
            .rows(at)?
            .iter()
            .map(|r| r.slack)
            .fold(f64::INFINITY, f64::min))
    }
    fn movement(&self, lo: [f64; 6], hi: [f64; 6]) -> ([f64; 6], f64) {
        optimize::rigid_movement(self.radius, lo, hi)
    }
}
