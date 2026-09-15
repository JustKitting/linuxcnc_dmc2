//! Automatic stock geometry. M190 publishes data; LinuxCNC owns motion.
mod dimensions;
mod free_report;
mod free_surface;
pub(super) mod followup;
#[cfg(test)]
mod free_tests;
use dmc2ctl::probe_data::mapper_settings as model;
use dmc2ctl::probe_data::mapper_trace::{outline, state};
mod outline_report;
#[cfg(test)]
mod outline_tests;
mod report;
mod search;
#[cfg(test)]
mod tests;

use super::{ledger, plan_bank, schema::Workflow, storage};
use model::{data, policy, Mode, OutlinePolicy, Phase, Request, Settings, PLATE_FIELDS};
pub(super) use report::export;
use std::{fs, path::Path};

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
    let outline = fs::read_to_string(root.join("config/mapper-outline.txt"))
        .map_err(|e| format!("Reading the outline search policy: {e}"))?;
    OutlinePolicy::read(&outline)?;
    storage::begin(output, Workflow::Mapper)?;
    let path = storage::active_path(output, Workflow::Mapper, 0)?;
    ledger::publish(&path.with_extension("plate.txt"), plate.as_bytes())?;
    ledger::publish(&path.with_extension("feeds.txt"), feeds.as_bytes())?;
    ledger::publish(&path.with_extension("outline.txt"), outline.as_bytes())
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
    let outline = if Mode::read(ledger::number(start, "mode")?)? == Mode::Outline {
        Some(OutlinePolicy::read(
            &fs::read_to_string(path.with_extension("outline.txt"))
                .map_err(|e| format!("Reading this run's outline policy snapshot: {e}"))?,
        )?)
    } else {
        None
    };
    let settings = Settings::read(start, &plate, &policy, outline)?;
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
        let progress = if settings.mode == Mode::TopFollowup {
            followup::next(&path, &records).map_err(search::Progress::Invalid).and_then(|next| {
                next.map_or(Ok(()), |q| Err(search::Progress::Need(q)))
            })
        } else if settings.mode == Mode::Outline {
            outline::run(&settings, &samples).result
        } else if settings.mode == Mode::FreeSurface {
            search::Survey::new(&settings, &samples).free_surface()
        } else {
            search::Survey::new(&settings, &samples).run().map(|_| ())
        };
        let request = match progress {
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
