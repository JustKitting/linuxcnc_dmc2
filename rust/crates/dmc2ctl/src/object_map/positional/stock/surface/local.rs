//! One local fit/support/check interpretation for dependent analyses.
use super::super::super::{geometry::*, probe::Sample, request::Use};
use super::super::fit::Stop;
use super::{request::Request, support::Support, Patch, Station};
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Checks {
    Missing,
    Disagrees,
    Within,
}
impl Checks {
    pub fn name(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Disagrees => "disagrees",
            Self::Within => "within-requested-residual",
        }
    }
}
pub struct Local<'a> {
    pub station: &'a Station,
    pub patch: &'a Patch,
    pub support: Support,
    pub checks: Checks,
    pub check_sources: Vec<usize>,
}
pub fn build<'a>(samples: &[Sample], stations: &'a [Station], sr: &Request) -> Vec<Local<'a>> {
    let mut local = Vec::new();
    for station in stations {
        let Ok(patch) = &station.result else { continue };
        if patch.stop != Stop::Converged
            || patch.residuals.iter().any(|d| d.abs() > sr.max_residual)
        {
            continue;
        }
        let support = Support::new(samples, station, patch, sr);
        let check_sources = samples
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                s.usage == Use::Check
                    && dot(s.approach, patch.normal) < 0.
                    && support.contains(s.center, patch, sr)
            })
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        let checks = if check_sources.is_empty() {
            Checks::Missing
        } else if check_sources.iter().any(|i| {
            dot(sub(samples[*i].center, patch.center), patch.normal).abs() > sr.max_residual
        }) {
            Checks::Disagrees
        } else {
            Checks::Within
        };
        local.push(Local {
            station,
            patch,
            support,
            checks,
            check_sources,
        });
    }
    local
}
