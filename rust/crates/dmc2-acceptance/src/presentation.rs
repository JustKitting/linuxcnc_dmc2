use std::path::Path;
use std::process::Command;

use crate::failure::{Failure, FailureCode, Result};

const FORBIDDEN_NATIVE_LIMIT_ERROR: &str = "on limit switch error";
const CONTROLLED_STOP_MESSAGE: &str = "Jog aborted by jog-stop";
const IMMEDIATE_STOP_MESSAGE: &str = "Jog aborted by jog-stop-immediate";

pub(crate) fn validate_error_journal(
    project_root: &Path,
    run_directory: &Path,
    expected_immediate_stops: usize,
) -> Result<()> {
    let python_path = project_root.join("python");
    let journal_path = run_directory.join("error-channel.tsv");
    let output = Command::new("python3")
        .args(["-B", "-m", "dmc2_axis.journal_validator"])
        .arg(&journal_path)
        .arg("--minimum-events")
        .arg((expected_immediate_stops + 1).to_string())
        .arg("--forbid-substring")
        .arg(FORBIDDEN_NATIVE_LIMIT_ERROR)
        .arg("--require-message-count")
        .arg(CONTROLLED_STOP_MESSAGE)
        .arg("1")
        .arg("--require-message-count")
        .arg(IMMEDIATE_STOP_MESSAGE)
        .arg(expected_immediate_stops.to_string())
        .arg("--required-messages-must-be-suppressed")
        .env("PYTHONPATH", python_path)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .map_err(|error| {
            Failure::io(
                FailureCode::JournalPresentation,
                "execute production AXIS error-journal validator",
                error,
            )
        })?;
    if !output.status.success() {
        return Err(Failure::new(
            FailureCode::JournalPresentation,
            format!(
                "journal={}; expected_controlled_stops=1; expected_immediate_stops={expected_immediate_stops}; exit={}; stdout={:?}; stderr={:?}",
                journal_path.display(),
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            ),
        ));
    }
    Ok(())
}

pub(crate) fn validate_diagnostic_journal(project_root: &Path, run_directory: &Path) -> Result<()> {
    let python_path = project_root.join("python");
    let journal_path = run_directory.join("diagnostics.tsv");
    let output = Command::new("python3")
        .args(["-B", "-m", "dmc2_axis.diagnostic_validator"])
        .arg(&journal_path)
        .arg("--forbid-source")
        .arg("joints[0].hard_limit")
        .arg("--forbid-source")
        .arg("joints[1].hard_limit")
        .arg("--forbid-source")
        .arg("joints[2].hard_limit")
        .env("PYTHONPATH", python_path)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .map_err(|error| {
            Failure::io(
                FailureCode::JournalPresentation,
                "execute production AXIS diagnostic-journal validator",
                error,
            )
        })?;
    if !output.status.success() {
        return Err(Failure::new(
            FailureCode::JournalPresentation,
            format!(
                "journal={}; forbidden_sources=joints[0..2].hard_limit; exit={}; stdout={:?}; stderr={:?}",
                journal_path.display(),
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            ),
        ));
    }
    Ok(())
}
