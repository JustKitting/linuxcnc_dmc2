//! Runtime calibration contract, shared by capture, launch validation and AXIS.
//! Values are read from the selected INI, never compiled into the application.
use std::fmt;

pub const DATA_SCHEMA: &str = "DMC2_CALIBRATION_DATA_V1";
pub const VALID_PARAMETER: &str = "tool-reference-valid";

#[derive(Clone, Copy, Debug)]
pub enum Field {
    SetterHeight,
    HomeZ,
}

impl Field {
    pub const ALL: [Self; 2] = [Self::SetterHeight, Self::HomeZ];

    pub const fn parameter(self) -> &'static str {
        match self {
            Self::SetterHeight => "tool-reference-height-mm",
            Self::HomeZ => "tool-reference-home-z-mm",
        }
    }

    const fn source(self) -> (&'static str, &'static str) {
        match self {
            Self::SetterHeight => ("TOOL_SETTER", "HEIGHT_ABOVE_PLATE_MM"),
            Self::HomeZ => ("JOINT_2", "HOME"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Calibration {
    pub height_mm: f64,
    pub home_z_mm: f64,
}

impl Calibration {
    pub fn read(ini: &str) -> Result<Self, CalibrationError> {
        let height_mm = read_field(ini, Field::SetterHeight)?;
        if height_mm <= 0.0 {
            return Err(CalibrationError::NonpositiveHeight);
        }
        Ok(Self {
            height_mm,
            home_z_mm: read_field(ini, Field::HomeZ)?,
        })
    }

    pub fn value(self, field: Field) -> f64 {
        match field {
            Field::SetterHeight => self.height_mm,
            Field::HomeZ => self.home_z_mm,
        }
    }

    /// Presentation metadata only. AXIS creates exactly these data parameters.
    pub fn describe_data() -> String {
        let mut result = format!("{DATA_SCHEMA}\n{VALID_PARAMETER}\tbit\t0\n");
        for field in Field::ALL {
            result.push_str(&format!("{}\tfloat\t0\n", field.parameter()));
        }
        result
    }
}

#[derive(Clone, Copy, Debug)]
pub enum CalibrationError {
    Missing(Field),
    Duplicate(Field),
    Nonfinite(Field),
    NonpositiveHeight,
}

impl fmt::Display for CalibrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (field, reason) = match self {
            Self::Missing(field) => (*field, "is missing"),
            Self::Duplicate(field) => (*field, "is duplicated"),
            Self::Nonfinite(field) => (*field, "must be a finite number"),
            Self::NonpositiveHeight => (Field::SetterHeight, "must be positive"),
        };
        let (section, key) = field.source();
        write!(f, "CALIBRATION_INVALID: [{section}] {key} {reason}. Correct that entry in the selected INI before retrying; Clear Fault and Pendant Mode remain available.")
    }
}

impl std::error::Error for CalibrationError {}

fn read_field(text: &str, field: Field) -> Result<f64, CalibrationError> {
    let (section, key) = field.source();
    let mut selected = false;
    let mut value = None;
    for line in text.lines() {
        let line = line.split(['#', ';']).next().unwrap_or("").trim();
        if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            selected = name.trim().eq_ignore_ascii_case(section);
        } else if selected {
            if let Some((name, raw)) = line.split_once('=') {
                if name.trim().eq_ignore_ascii_case(key) {
                    let parsed = raw
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .filter(|v| v.is_finite())
                        .ok_or(CalibrationError::Nonfinite(field))?;
                    if value.replace(parsed).is_some() {
                        return Err(CalibrationError::Duplicate(field));
                    }
                }
            }
        }
    }
    value.ok_or(CalibrationError::Missing(field))
}
