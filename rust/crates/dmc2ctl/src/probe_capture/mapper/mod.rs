//! Automatic stock geometry. M190 publishes data; LinuxCNC owns motion.
mod dimensions;
mod model;
mod report;
mod search;
mod state;
#[cfg(test)]
mod tests;

use super::{ledger, plan_bank, schema::Workflow, storage};
use model::{data, Phase, Request, Settings};
pub(super) use report::export;
use std::{fs, path::Path};

const PLATE_FIELDS: &[&str] = &["x_min", "x_max", "y_min", "y_max", "ball_diameter"];
fn policy(text: &str) -> Result<ledger::Fields, String> {
    match text.lines().next() {
        Some("DMC2_MAPPER_FEEDS_V1") => {
            let mut fields = data(text, "DMC2_MAPPER_FEEDS_V1", &["coarse_feed", "fine_feed", "travel_feed"])?;
            // Older recorded runs used the same coarse feed in Z and XY.
            fields.insert("downward_feed".into(), fields["coarse_feed"].clone());
            Ok(fields)
        }
        Some("DMC2_MAPPER_FEEDS_V2") => data(text, "DMC2_MAPPER_FEEDS_V2", &["coarse_feed", "downward_feed", "fine_feed", "travel_feed"]),
        _ => Err("Mapper feed settings need a supported versioned header; correct config/mapper-feeds.txt before Run.".into()),
    }
}
const PLAN_FIELDS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../config/mapper-plan-fields.txt"
));
const BANK: plan_bank::Spec = plan_bank::Spec {
    name: "mapper",
    fields: PLAN_FIELDS,
};

pub(super) fn begin(root: &Path, output: &Path) -> Result<(), String> {
    plan_bank::with_bank(BANK, |b| b.invalidate())?;
    let plate = fs::read_to_string(root.join("config/metrology/plate-envelope.txt"))
        .map_err(|e| format!("Reading the retained plate envelope: {e}"))?;
    data(&plate, "DMC2_PLATE_ENVELOPE_V1", PLATE_FIELDS)?;
    let feeds = fs::read_to_string(root.join("config/mapper-feeds.txt"))
        .map_err(|e| format!("Reading the mapper feed settings: {e}"))?;
    policy(&feeds)?;
    storage::begin(output, Workflow::Mapper)?;
    let path = storage::active_path(output, Workflow::Mapper, 0)?;
    ledger::publish(&path.with_extension("plate.txt"), plate.as_bytes())?;
    ledger::publish(&path.with_extension("feeds.txt"), feeds.as_bytes())
}

fn read(path: &Path) -> Result<(Vec<ledger::Fields>, Settings), String> {
    let records = ledger::records(
        &fs::read_to_string(path).map_err(|e| format!("Reading mapper ledger: {e}"))?,
        Workflow::Mapper,
    )?;
    let start = records
        .first()
        .ok_or("No mapper start settings are retained.")?;
    let plate = data(
        &fs::read_to_string(path.with_extension("plate.txt"))
            .map_err(|e| format!("Reading this run's plate snapshot: {e}"))?,
        "DMC2_PLATE_ENVELOPE_V1",
        PLATE_FIELDS,
    )?;
    let policy = policy(
        &fs::read_to_string(path.with_extension("feeds.txt"))
            .map_err(|e| format!("Reading this run's feed snapshot: {e}"))?,
    )?;
    let settings = Settings::read(start, &plate, &policy)?;
    Ok((records, settings))
}

pub(super) fn publish_next(output: &Path, sequence: u64) -> Result<(), String> {
    plan_bank::with_bank(BANK, |bank| {
        bank.invalidate()?;
        let path = storage::active_path(output, Workflow::Mapper, sequence)?;
        let (records, settings) = read(&path)?;
        if records.len() as u64 != sequence || records.last().is_some_and(|r| r["kind"] == "result")
        {
            return Err("The mapper sequence is stale or the run has already ended. Start a new Run after recovery.".into());
        }
        let samples = state::samples(&records, &settings, false)?;
        let mut survey = search::Survey::new(&settings, &samples);
        let request = match survey.run() {
            Err(search::Progress::Need(p)) => p,
            Err(search::Progress::Invalid(error)) => {
                return match export(&path, false) {
                    Ok(()) => Err(format!("{error} Partial CSV/JSON saved beside {}.", path.display())),
                    Err(export_error) => Err(format!("{error} Partial export also failed: {export_error}. Original ledger retained at {}.",path.display())),
                };
            }
            Ok(_) => Request {
                phase: Phase::Finished,
                edge: -1,
                approach: [settings.origin[0], settings.origin[1]],
                target: settings.origin,
            },
        };
        let values = request.values(&settings, samples.len(), sequence);
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
