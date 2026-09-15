use super::super::super::{geometry::*, request::Use};
use super::{
    material::{self, query::State},
    request::Request,
    surface,
};
use crate::{
    object_map::{store::CaptureSnapshot, Error},
    probe_data::{
        mapper_settings::{Sample, Settings},
        mapper_trace::{observation::Retouch, state},
    },
};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    MissingCheck,
    ConflictingCheck,
    NoContactConflict,
    Shortage,
}
impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::MissingCheck => "acquire-independent-patch-check",
            Self::ConflictingCheck => "repeat-disagreeing-check",
            Self::NoContactConflict => "review-contact-no-contact-conflict",
            Self::Shortage => "remeasure-local-shortage",
        }
    }
}
pub struct Need {
    pub seed: usize,
    pub kind: Kind,
    pub regions: BTreeSet<usize>,
}
pub struct Run {
    pub settings: Settings,
    pub samples: Vec<Sample>,
}
pub struct Candidate {
    pub sources: Vec<usize>,
    pub proposal: Result<Retouch, String>,
    pub needs: BTreeSet<usize>,
}
pub struct Selection {
    pub needs: Vec<Need>,
    pub candidates: Vec<Candidate>,
    pub chosen: Vec<usize>,
    pub pending: Vec<usize>,
    pub runs: BTreeMap<String, Result<Run, String>>,
}
fn needs(a: &material::Assessment) -> Vec<Need> {
    let mut grouped: BTreeMap<(usize, Kind), BTreeSet<usize>> = BTreeMap::new();
    for (region, r) in a.regions.iter().enumerate() {
        for c in &r.comparisons {
            use surface::local::Checks;
            let kind = match c.checks {
                Checks::Missing => Some(Kind::MissingCheck),
                Checks::Disagrees => Some(Kind::ConflictingCheck),
                Checks::NoContactConflict => Some(Kind::NoContactConflict),
                Checks::Within if r.state == State::Shortage && c.upper < a.request.clearance => {
                    Some(Kind::Shortage)
                }
                Checks::Within => None,
            };
            if let Some(kind) = kind {
                grouped.entry((c.source, kind)).or_default().insert(region);
            }
        }
    }
    grouped
        .into_iter()
        .map(|((seed, kind), regions)| Need {
            seed,
            kind,
            regions,
        })
        .collect()
}

/// Greedy coverage of distinct patch requirements, not mesh-cover density.
/// Every incidence is retired once. Equal gains select the first original
/// source deterministically; there is no physical-distance weighting.
pub(super) fn choose(
    links: &[BTreeSet<usize>],
    count: usize,
    budget: usize,
) -> (Vec<usize>, Vec<usize>) {
    let mut owners = vec![Vec::new(); count];
    let mut gains = Vec::with_capacity(links.len());
    let mut heap = BinaryHeap::new();
    for (i, needs) in links.iter().enumerate() {
        gains.push(needs.len());
        heap.push((needs.len(), Reverse(i)));
        for &n in needs {
            owners[n].push(i);
        }
    }
    let mut covered = vec![false; count];
    let mut chosen = Vec::new();
    while chosen.len() < budget {
        let Some((gain, Reverse(i))) = heap.pop() else {
            break;
        };
        if gain != gains[i] {
            continue;
        }
        if gain == 0 {
            break;
        }
        chosen.push(i);
        for &n in &links[i] {
            if covered[n] {
                continue;
            }
            covered[n] = true;
            for &j in &owners[n] {
                gains[j] -= 1;
                heap.push((gains[j], Reverse(j)));
            }
        }
    }
    (
        chosen,
        covered
            .iter()
            .enumerate()
            .filter_map(|(i, yes)| (!yes).then_some(i))
            .collect(),
    )
}
pub fn run(
    a: &material::Assessment,
    captures: &[CaptureSnapshot],
    r: &Request,
) -> Result<Selection, Error> {
    let needs = needs(a);
    let samples = &a.surface.contacts;
    let required = samples.len().checked_mul(needs.len());
    if required.is_none_or(|n| n > r.comparisons) {
        return Err(Error::Input("max_candidate_need_comparisons cannot cover every original contact and distinct patch requirement. Increase this computation budget or select another retained analysis; no requirement was dropped.".into()));
    }
    let mut runs = BTreeMap::new();
    for c in captures
        .iter()
        .filter(|c| samples.iter().any(|s| s.capture == c.id))
    {
        a.surface.source.check_context(c)?;
        let parsed = c.context.settings(&c.capture).and_then(|settings| {
            state::samples(&c.capture.records, &settings, false)
                .map(|samples| Run { settings, samples })
        });
        runs.insert(c.id.as_str().to_owned(), parsed);
    }
    let local = surface::local::build(samples, &a.surface.stations, &a.surface.request)?;
    let by_seed = local
        .iter()
        .map(|l| (l.station.seed, l))
        .collect::<BTreeMap<_, _>>();
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut repeated = BTreeMap::new();
    for (index, s) in samples.iter().enumerate() {
        let mut addresses = BTreeSet::new();
        for (i, need) in needs.iter().enumerate() {
            let l = by_seed.get(&need.seed).ok_or_else(|| Error::Data("A material requirement has no matching reproduced local patch. Recalculate its source analysis; no substitute normal was inferred.".into()))?;
            let residual = dot(sub(s.center, l.patch.center), l.patch.normal).abs();
            let relevant = match need.kind {
                // Repeating a positive contact alone cannot resolve the
                // contradictory no-contact path/model. Retain this need as
                // pending; do not count a proposed retouch as its resolution.
                Kind::NoContactConflict => false,
                Kind::ConflictingCheck => {
                    l.check_sources.contains(&index) && residual > a.surface.request.max_residual
                }
                Kind::MissingCheck | Kind::Shortage => {
                    s.usage != Use::Check
                        && residual <= a.surface.request.max_residual
                        && dot(s.approach, l.patch.normal) < 0.
                        && l.support.contains(s.center, l.patch, &a.surface.request)?
                }
            };
            if relevant {
                addresses.insert(i);
            }
        }
        if addresses.is_empty() {
            continue;
        }
        let proposal = (|| -> Result<Retouch, String> {
            let run = runs.get(s.capture.as_str()).ok_or_else(|| "The original acquisition context is missing. Import the intact original run under a new capture ID before planning.".to_string())?.as_ref().map_err(Clone::clone)?;
            let sample = run.samples.iter().find(|p| p.sequence == s.sequence).ok_or_else(|| "The source fine contact does not have a validated acquisition cycle. Retain a complete cycle before planning its repeat.".to_string())?;
            Retouch::from_sample(&run.settings, sample)
        })();
        if let Ok(p) = &proposal {
            // Alias only identical requests in the same retained run. All
            // original measurements survive in sources and the source bundle.
            let q = p.request;
            let bits = q
                .target
                .into_iter()
                .chain(q.approach)
                .chain(p.start)
                .map(|v| if v == 0. { 0 } else { v.to_bits() })
                .collect::<Vec<_>>();
            let key = (s.capture.as_str().to_owned(), q.phase as u8, q.edge, bits);
            if let Some(&prior) = repeated.get(&key) {
                let c: &mut Candidate = &mut candidates[prior];
                c.sources.push(index);
                c.needs.extend(addresses);
                continue;
            }
            repeated.insert(key, candidates.len());
        }
        candidates.push(Candidate {
            sources: vec![index],
            proposal,
            needs: addresses,
        });
    }
    let links = candidates
        .iter()
        .map(|c| {
            if c.proposal.is_ok() {
                c.needs.clone()
            } else {
                BTreeSet::new()
            }
        })
        .collect::<Vec<_>>();
    let (chosen, pending) = choose(&links, needs.len(), r.observations);
    Ok(Selection {
        needs,
        candidates,
        chosen,
        pending,
        runs,
    })
}
