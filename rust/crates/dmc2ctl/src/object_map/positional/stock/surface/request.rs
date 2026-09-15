use super::super::super::{
    Error,
    probe::Probe,
    request::{Selection, Use, read_selections, scalar},
};
use crate::object_map::{
    capture_selection::{self, Entry},
    record,
};
pub const LEGACY_SCHEMA: &str = "DMC2_STOCK_SURFACE_REQUEST_V1";
pub const CONTRIBUTING_SCHEMA: &str = "DMC2_STOCK_SURFACE_REQUEST_V2";
pub const SCHEMA: &str = "DMC2_STOCK_SURFACE_REQUEST_V3";
pub fn legacy_keys() -> Vec<&'static str> {
    super::super::super::probe::keys(&[
        "neighborhood_mm",
        "max_approach_angle_deg",
        "huber_mm",
        "max_iterations",
        "convergence_mm",
        "max_support_gap_mm",
        "max_fit_residual_mm",
    ])
}
pub fn keys() -> Vec<&'static str> {
    legacy_keys()
        .into_iter()
        .chain(["no_contact_model", "no_contact_allowance_mm"])
        .collect()
}
pub fn decode(raw: &[u8]) -> Result<(std::collections::BTreeMap<String, String>, &[u8]), Error> {
    if raw.starts_with(format!("{LEGACY_SCHEMA}\n").as_bytes()) {
        record::decode(raw, LEGACY_SCHEMA, &legacy_keys())
    } else if raw.starts_with(format!("{CONTRIBUTING_SCHEMA}\n").as_bytes()) {
        record::decode(raw, CONTRIBUTING_SCHEMA, &keys())
    } else {
        record::decode(raw, SCHEMA, &keys())
    }
}
#[derive(Clone, Copy)]
pub enum NoContactModel {
    Legacy,
    ErodedProbeSweep { allowance: f64 },
}
pub enum NoContactSources {
    ContributingContacts,
    Explicit(Vec<Entry>),
}
pub struct Request {
    pub probe: Probe,
    pub neighborhood: f64,
    pub approach_cos: f64,
    pub huber: f64,
    pub iterations: usize,
    pub convergence: f64,
    pub support_gap: f64,
    pub max_residual: f64,
    pub selected: Vec<Selection>,
    pub no_contact: NoContactModel,
    pub no_contact_sources: NoContactSources,
}
impl Request {
    pub fn read(raw: &[u8]) -> Result<Self, Error> {
        let (f, body) = decode(raw)?;
        let probe = Probe::read(&f)?;
        let no_contact = if let Some(model) = f.get("no_contact_model") {
            if model != "eroded-probe-sweep" {
                return Err(Error::Input("no_contact_model must be eroded-probe-sweep. This assumes declared pretravel plus allowance bounds undetected contact and reported-position error; review that model in the request.".into()));
            }
            let allowance = scalar(&f["no_contact_allowance_mm"], "no_contact_allowance_mm")?;
            if allowance < 0. || allowance + probe.pretravel >= probe.radius {
                return Err(Error::Input("no_contact_allowance_mm must be nonnegative and, together with declared pretravel, smaller than ball_radius_mm. Enter an evidence-based allowance; no radius or measurement was substituted.".into()));
            }
            NoContactModel::ErodedProbeSweep { allowance }
        } else {
            NoContactModel::Legacy
        };
        let positive = |k: &str| -> Result<f64, Error> {
            let v = scalar(&f[k], k)?;
            if v > 0. {
                Ok(v)
            } else {
                Err(Error::Input(format!(
                    "{k} must be positive; edit this analysis request."
                )))
            }
        };
        let angle = positive("max_approach_angle_deg")?;
        if angle > 180. {
            return Err(Error::Input("max_approach_angle_deg cannot exceed a half-turn; edit the neighborhood selection in this request.".into()));
        }
        let iterations = f["max_iterations"].parse::<usize>().ok().filter(|n|*n>0)
            .ok_or_else(|| Error::Input("max_iterations needs a positive integer computational budget; edit the request.".into()))?;
        let (contacts, no_contact_sources) = if raw.starts_with(format!("{SCHEMA}\n").as_bytes()) {
            let boundary = body.windows(2).position(|v| v == b"\n\n").ok_or_else(|| Error::Input("Surface V3 needs the contact CSV followed by a blank line and the tab-separated no-contact capture decisions. Prepare a new surface request and retain both tables.".into()))?;
            (
                &body[..boundary + 1],
                NoContactSources::Explicit(capture_selection::read(&body[boundary + 2..])?),
            )
        } else {
            (body, NoContactSources::ContributingContacts)
        };
        let selected = read_selections(contacts)?;
        if selected.iter().any(|s| matches!(s.usage, Use::Face(..))) {
            return Err(Error::Input("3D stock surface rows use fit, check or observe. Named box faces do not define this surface model.".into()));
        }
        if selected.iter().filter(|s| s.usage == Use::Fit).count() < 3 {
            return Err(Error::Input("Select at least three fine contacts for local plane fitting; each neighborhood also needs noncollinear support. Keep independent check contacts.".into()));
        }
        Ok(Self {
            probe,
            no_contact,
            no_contact_sources,
            neighborhood: positive("neighborhood_mm")?,
            approach_cos: angle.to_radians().cos(),
            huber: positive("huber_mm")?,
            iterations,
            convergence: positive("convergence_mm")?,
            support_gap: positive("max_support_gap_mm")?,
            max_residual: positive("max_fit_residual_mm")?,
            selected,
        })
    }
}
