//! Offline object records and FreeCAD exchange. No NML/HAL or machine commands.
mod capture;
mod capture_bundle;
mod capture_selection;
mod catalog;
pub mod cli;
mod exchange;
mod files;
mod model;
pub mod pipeline;
mod positional;
mod record;
mod store;

use std::fmt;

#[derive(Debug)]
pub enum Error {
    PipelinePaused(pipeline::Pause),
    Input(String),
    Data(String),
    Storage(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (detail, recovery) = match self {
            Self::PipelinePaused(pause) => return pause.fmt(f),
            Self::Input(s) => (s, "Correct the arguments and retry; object-map --help lists the commands."),
            Self::Data(s) => (s, "Use an intact source record or correct the named input, then retry. Retained measurements are preserved."),
            Self::Storage(s) => (s, "Check the named path and write access, then retry. For conflicting existing data, choose a new ID or output directory; existing data is preserved."),
        };
        write!(f, "{detail} {recovery}")
    }
}

#[cfg(test)]
mod tests;
