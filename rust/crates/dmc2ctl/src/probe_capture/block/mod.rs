//! Adaptive cuboid metrology: Rust plans, the typed G-code script executes.
mod exchange;
use super::geometry;
mod model;
mod report;
#[cfg(test)]
mod tests;

use super::{ledger, schema::Workflow, storage};
use std::path::Path;

pub(super) use exchange::invalidate;
pub(super) use report::export;

pub(super) fn publish_next(output: &Path, sequence: u64) -> Result<(), String> {
    exchange::with_bank(|bank| {
        bank.invalidate()?;
        let path = storage::active_path(output, Workflow::Block, sequence)?;
        let records = ledger::records(
            &std::fs::read_to_string(&path).map_err(|e| e.to_string())?,
            Workflow::Block,
        )?;
        if records.len() as u64 != sequence {
            return Err("Block ledger length disagrees with the requested plan sequence.".into());
        }
        let state = model::State::read(&records)?;
        let plan = state.next()?;
        let values = plan.values(sequence);
        let saved = values
            .iter()
            .map(|(k, v)| format!("{k}={v}\n"))
            .collect::<String>();
        ledger::publish(
            &path.with_extension(format!("plan-{sequence}.txt")),
            saved.as_bytes(),
        )?;
        bank.publish(&values)
    })
}
