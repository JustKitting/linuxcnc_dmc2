pub(super) mod fit;
pub(super) mod local;
pub(super) mod no_contact;
mod plane;
pub(super) mod report;
pub(in crate::object_map) mod request;
pub(super) mod support;
#[cfg(test)]
mod tests;
use super::super::{geometry, probe::Sample, Error};
use super::fit::Stop;
use geometry::V;
pub struct Patch {
    pub center: V,
    pub surface: V,
    pub normal: V,
    pub variance: V,
    pub residuals: Vec<f64>,
    pub weights: Vec<f64>,
    pub iterations: usize,
    pub stop: Stop,
    pub objective_start: f64,
    pub objective_end: f64,
}
pub struct Station {
    pub seed: usize,
    pub neighbours: Vec<usize>,
    pub result: Result<Patch, Reason>,
    pub no_contact: std::sync::Arc<[no_contact::Sweep]>,
}
#[derive(Clone, Copy, Debug)]
pub enum Reason {
    FewContacts,
    UnobservedNormal,
    Approach,
    EigenBudget,
    Arithmetic,
}
impl Reason {
    fn description(self) -> (&'static str, &'static str) {
        match self {
            Self::FewContacts=>("insufficient-local-contacts","This neighborhood has fewer than three fitting contacts. Select additional retained contacts or acquire a supported local surface."),
            Self::UnobservedNormal=>("surface-normal-unobserved","These contacts do not distinguish a surface normal. A single line cannot determine wall slope; add independent spatial support without inventing a normal."),
            Self::Approach=>("surface-approach-conflict","The fitted plane cannot face all recorded approaches in this neighborhood. Resolve the local grouping or acquire the separate surfaces; every original contact is retained."),
            Self::EigenBudget=>("plane-solve-budget","The local covariance solve exhausted its computational budget. Inspect the request budget and neighborhood before reusing this patch."),
            Self::Arithmetic=>("surface-arithmetic-unrepresentable","This neighborhood cannot be evaluated with finite arithmetic. Inspect coordinate units and fit scales; no patch was substituted."),
        }
    }
}
pub fn build(
    samples: &[Sample],
    r: &request::Request,
    misses: &[no_contact::Sweep],
) -> Result<super::report::Report, Error> {
    report::build(samples, &fit::run(samples, r, misses), r)
}

pub struct Loaded {
    pub source: super::super::retained::Bundle,
    pub request: request::Request,
    pub contacts: Vec<Sample>,
    pub stations: Vec<Station>,
}
pub fn load(
    store: &crate::object_map::store::Store,
    object: &crate::object_map::model::Id,
    setup: &crate::object_map::model::Id,
    id: &crate::object_map::model::Id,
) -> Result<Loaded, Error> {
    let source =
        super::super::retained::Bundle::read(&super::super::folder(store, object, setup, id)?)?;
    let request = request::Request::read(source.get("request.txt")?)?;
    let captures = store.captures(object, setup)?;
    let contacts = request.probe.samples(&captures, &request.selected)?;
    source.check_captures(&captures, &contacts)?;
    if matches!(
        request.no_contact,
        request::NoContactModel::ErodedProbeSweep { .. }
    ) {
        for c in &captures {
            if contacts.iter().any(|s| s.capture == c.id) {
                source.check_context(c)?;
            }
        }
    }
    let misses = no_contact::read(&captures, &contacts, &request)?;
    let stations = fit::run(&contacts, &request, &misses);
    let report = report::build(&contacts, &stations, &request)?;
    source.require_equal("stock-surface.machine-mm.json", report.json.as_bytes())?;
    Ok(Loaded {
        source,
        request,
        contacts,
        stations,
    })
}
