use super::{record::quote, Error};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Id(String);

impl Id {
    pub fn parse(s: &str) -> Result<Self, Error> {
        if s.is_empty()
            || !s
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
        {
            return Err(Error::Input(format!(
                "Invalid ID {s:?}; use lowercase letters, digits, hyphens or underscores."
            )));
        }
        Ok(Self(s.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registration {
    Unresolved,
}

impl Registration {
    pub fn json(self) -> &'static str {
        match self {
            Self::Unresolved => {
                "{\"state\":\"unresolved\",\"object_to_machine\":null,\"residual_mm\":null}"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureState {
    ResultUnreviewed,
    Partial,
    Quarantined,
}

impl CaptureState {
    pub fn name(self) -> &'static str {
        match self {
            Self::ResultUnreviewed => "recorded-result-unreviewed",
            Self::Partial => "partial",
            Self::Quarantined => "quarantined",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CaptureIssueKind {
    Probe(crate::probe_data::mapper_schema::CaptureFailure),
    GaugeBlockSideMiss,
}
impl CaptureIssueKind {
    fn name(self) -> &'static str {
        match self {
            Self::Probe(failure) => failure.name(),
            Self::GaugeBlockSideMiss => "gauge-block-side-miss",
        }
    }
    fn message(self) -> &'static str {
        match self {
            Self::Probe(failure) => failure.message(),
            Self::GaugeBlockSideMiss => "An expected gauge-block side contact was not captured.",
        }
    }
}
#[derive(Debug)]
pub struct CaptureIssue {
    pub sequence: usize,
    pub kind: CaptureIssueKind,
}
impl CaptureIssue {
    pub fn json(&self) -> String {
        format!("{{\"sequence\":{},\"kind\":{},\"message\":{},\"recovery\":\"Preserve this ledger for diagnosis. Recapture the required geometry after operator recovery; this snapshot cannot supply fitted geometry.\"}}", self.sequence, quote(self.kind.name()), quote(self.kind.message()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Coarse,
    Fine,
}

impl Stage {
    pub fn name(self) -> &'static str {
        match self {
            Self::Coarse => "coarse",
            Self::Fine => "fine",
        }
    }
}

#[derive(Debug)]
pub struct Contact {
    pub sequence: usize,
    pub stage: Stage,
    pub trigger_mm: [f64; 3],
    pub direction: [f64; 3],
    pub commanded_feed_mm_min: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignFormat {
    FreeCad,
    Step,
    Stl,
}

impl DesignFormat {
    pub fn parse_extension(s: &str) -> Result<Self, Error> {
        match s.to_ascii_lowercase().as_str() {
            "fcstd" => Ok(Self::FreeCad),
            "step" | "stp" => Ok(Self::Step),
            "stl" => Ok(Self::Stl),
            _ => Err(Error::Input(
                "Design must be a native .FCStd, STEP or STL file.".into(),
            )),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::FreeCad => "FCStd",
            Self::Step => "STEP",
            Self::Stl => "stl",
        }
    }
}

pub fn named_json(id: &Id, label: &str) -> String {
    format!("\"id\":{},\"label\":{}", quote(id.as_str()), quote(label))
}
