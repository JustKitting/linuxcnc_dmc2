//! Shared retained-data planning and selection; no NML/HAL or execution path.
mod adaptive;
pub mod observation;
pub mod outline;
pub mod state;
mod withdrawal;
use super::mapper_settings::Request;
pub enum Progress {
    Need(Request),
    Invalid(String),
}
impl From<String> for Progress {
    fn from(s: String) -> Self {
        Self::Invalid(s)
    }
}
