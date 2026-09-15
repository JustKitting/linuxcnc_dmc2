//! Declared use of fresh fine contacts, retained before acquisition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Fit,
    Check,
}
impl Role {
    pub fn read(value: &str) -> Result<Self, String> {
        match value {
            "fit" => Ok(Self::Fit),
            "check" => Ok(Self::Check),
            _ => Err("contact_role must be fit or check. Choose whether fresh fine contacts extend the surface fit or remain withheld checks, then retry.".into()),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Fit => "fit",
            Self::Check => "check",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Fit => "Fresh fine contacts supply fit rows; existing withheld check rows remain unchanged.",
            Self::Check => "Fresh fine contacts supply check rows, withheld from fitting. Actual check residuals determine agreement; predicted positions do not.",
        }
    }
}
