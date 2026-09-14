//! Explicit analysis inputs. Unspecified calibration and tolerances have no defaults.
use super::{
    super::{model::Id, record, Error},
    geometry::{Pose, V},
    probe::Probe,
};
use std::collections::BTreeSet;
pub const SCHEMA: &str = "DMC2_POSITIONAL_REQUEST_V1";
pub const KEYS: &[&str] = &[
    "design",
    "model_role",
    "stl_mm_per_unit",
    "calibration_state",
    "calibration_reference",
    "frame_reference",
    "ball_radius_mm",
    "trigger_to_ball_mm",
    "pretravel_mm",
    "initial_translation_mm",
    "initial_rotation_xyz_deg",
    "solve",
    "max_iterations",
    "convergence_mm",
    "huber_mm",
    "correspondence_limit_mm",
    "max_translation_correction_mm",
    "max_rotation_correction_deg",
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
    Fit,
    Check,
    Observe,
    Face(usize, bool),
}
impl Use {
    pub fn parse(s: &str) -> Result<Self, Error> {
        match s {
            "fit" => Ok(Self::Fit),
            "check" => Ok(Self::Check),
            "observe" => Ok(Self::Observe),
            "x-min" => Ok(Self::Face(0, false)),
            "x-max" => Ok(Self::Face(0, true)),
            "y-min" => Ok(Self::Face(1, false)),
            "y-max" => Ok(Self::Face(1, true)),
            "z-min" => Ok(Self::Face(2, false)),
            "z-max" => Ok(Self::Face(2, true)),
            _ => Err(Error::Input(format!(
                "Unknown contact use {s:?}; select fit, check, observe or x/y/z-min/max."
            ))),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Fit => "fit",
            Self::Check => "check",
            Self::Observe => "observe",
            Self::Face(0, false) => "x-min",
            Self::Face(0, true) => "x-max",
            Self::Face(1, false) => "y-min",
            Self::Face(1, true) => "y-max",
            Self::Face(2, false) => "z-min",
            Self::Face(2, true) => "z-max",
            Self::Face(..) => unreachable!(),
        }
    }
}
pub struct Selection {
    pub capture: Id,
    pub sequence: usize,
    pub usage: Use,
}
pub struct Request {
    pub design: Id,
    pub model_role: String,
    pub units: f64,
    pub probe: Probe,
    pub initial: Pose,
    pub planar: bool,
    pub iterations: usize,
    pub convergence: f64,
    pub huber: f64,
    pub correspondence: f64,
    pub max_translation: f64,
    pub max_rotation: f64,
    pub selected: Vec<Selection>,
}
pub fn scalar(s: &str, label: &str) -> Result<f64, Error> {
    s.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| {
            Error::Input(format!(
                "{label} requires an explicit finite number; got {s:?}."
            ))
        })
}
pub fn vector(s: &str, label: &str) -> Result<V, Error> {
    let v = s
        .split(',')
        .map(|x| scalar(x, label))
        .collect::<Result<Vec<_>, _>>()?;
    v.try_into().map_err(|_| {
        Error::Input(format!(
            "{label} needs exactly three comma-separated numbers."
        ))
    })
}
impl Request {
    pub fn read(raw: &[u8]) -> Result<Self, Error> {
        let (f, body) = record::decode(raw, SCHEMA, KEYS)?;
        let positive = |key: &str| -> Result<f64, Error> {
            let v = scalar(&f[key], key)?;
            if v > 0. {
                Ok(v)
            } else {
                Err(Error::Input(format!("{key} must be positive.")))
            }
        };
        let units = positive("stl_mm_per_unit")?;
        let probe = Probe::read(&f)?;
        if !matches!(
            f["model_role"].as_str(),
            "finished-design" | "reference-stock"
        ) {
            return Err(Error::Input(
                "model_role must be finished-design or reference-stock.".into(),
            ));
        }
        let planar = match f["solve"].as_str() {
            "translation-yaw" => true,
            "rigid" => false,
            _ => {
                return Err(Error::Input(
                    "solve must be translation-yaw or rigid.".into(),
                ))
            }
        };
        let initial = Pose::from_euler(
            vector(&f["initial_rotation_xyz_deg"], "initial_rotation_xyz_deg")?,
            vector(&f["initial_translation_mm"], "initial_translation_mm")?,
        )
        .validate()?;
        let iterations = f["max_iterations"]
            .parse::<usize>()
            .ok()
            .filter(|v| *v > 0)
            .ok_or_else(|| {
                Error::Input(
                    "max_iterations requires a positive integer computational budget.".into(),
                )
            })?;
        let selected = read_selections(body)?;
        if selected.iter().filter(|s| s.usage == Use::Fit).count() < if planar { 4 } else { 6 } {
            return Err(Error::Input("Select enough independent fine contacts as fit rows for the requested pose; keep separate check rows and stock-face rows.".into()));
        }
        Ok(Self {
            design: Id::parse(&f["design"])?,
            model_role: f["model_role"].clone(),
            units,
            probe,
            initial,
            planar,
            iterations,
            convergence: positive("convergence_mm")?,
            huber: positive("huber_mm")?,
            correspondence: positive("correspondence_limit_mm")?,
            max_translation: positive("max_translation_correction_mm")?,
            max_rotation: positive("max_rotation_correction_deg")?.to_radians(),
            selected,
        })
    }
}

pub fn read_selections(body: &[u8]) -> Result<Vec<Selection>, Error> {
    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();
    let body = std::str::from_utf8(body)
        .map_err(|e| Error::Data(format!("Contact selections are not UTF-8: {e}.")))?;
    let mut lines = body.lines();
    if !body.ends_with('\n') || lines.next() != Some("capture,sequence,use") {
        return Err(Error::Data(
            "Request must end with a terminated capture,sequence,use CSV table.".into(),
        ));
    }
    for line in lines {
        let row = line.split(',').collect::<Vec<_>>();
        if row.len() != 3 {
            return Err(Error::Data(format!(
                "Selection row must have three fields: {line:?}."
            )));
        }
        let capture = Id::parse(row[0])?;
        let sequence = row[1]
            .parse::<usize>()
            .map_err(|e| Error::Data(format!("Invalid contact sequence: {e}.")))?;
        if !seen.insert((capture.as_str().to_string(), sequence)) {
            return Err(Error::Data(format!("Repeated selection {}:{sequence}; one trigger cannot fit and independently check the same result.",capture.as_str())));
        }
        selected.push(Selection {
            capture,
            sequence,
            usage: Use::parse(row[2])?,
        });
    }
    Ok(selected)
}
