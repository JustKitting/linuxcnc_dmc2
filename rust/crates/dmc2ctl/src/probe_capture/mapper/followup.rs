//! Follow-up data comes from the actual loaded program, never a global plan ID.
use super::{ledger, plan_bank, storage, Workflow, BANK};
use dmc2ctl::probe_data::{mapper_settings::Request, top_followup::Plan};
use std::{
    env,
    ffi::{c_char, CString},
    fs,
    path::{Path, PathBuf},
};

extern "C" {
    fn dmc2_probe_capture_program(nml: *const c_char, path: *mut c_char, capacity: usize) -> i32;
}
fn loaded_program() -> Result<PathBuf, String> {
    let nml = CString::new(
        env::var("EMC2_NMLFILE").unwrap_or_else(|_| "/usr/share/linuxcnc/linuxcnc.nml".into()),
    )
    .map_err(|e| format!("Invalid status-channel path: {e}. Reopen the standard application."))?;
    let error = || {
        "LinuxCNC did not supply an intact loaded-program filename. Use Abort then Pendant Mode and open the exported follow-up program through File Open.".to_string()
    };
    let length = unsafe { dmc2_probe_capture_program(nml.as_ptr(), std::ptr::null_mut(), 0) };
    if length <= 0 {
        return Err(error());
    }
    let mut bytes = vec![0u8; length as usize];
    let copied =
        unsafe { dmc2_probe_capture_program(nml.as_ptr(), bytes.as_mut_ptr().cast(), bytes.len()) };
    if copied != length {
        return Err(error());
    }
    use std::os::unix::ffi::OsStringExt;
    Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}
pub(crate) fn begin(output: &Path) -> Result<(), String> {
    plan_bank::with_bank(BANK, |b| b.invalidate())?;
    let program = loaded_program()?;
    let text = fs::read_to_string(&program).map_err(|e|format!("Reading the loaded follow-up program {}: {e}. Re-export and open its intact file before Run.",program.display()))?;
    let plan = Plan::from_program(&text)?;
    storage::begin(output, Workflow::Mapper)?;
    let path = storage::active_path(output, Workflow::Mapper, 0)?;
    for (extension, text) in [
        ("plate.txt", plan.plate.clone()),
        ("feeds.txt", plan.feeds.clone()),
        ("followup.txt", plan.encode()?),
    ] {
        ledger::publish(&path.with_extension(extension), text.as_bytes())?;
    }
    Ok(())
}
pub(super) fn read(path: &Path) -> Result<Plan, String> {
    let plan = Plan::read(&fs::read_to_string(path.with_extension("followup.txt")).map_err(|e|format!("Reading this run's original follow-up plan: {e}. Preserve the ledger; Abort then Pendant Mode. Re-export its source observation program before a new Run."))?)?;
    for (extension, expected) in [("plate.txt", &plan.plate), ("feeds.txt", &plan.feeds)] {
        let actual = fs::read_to_string(path.with_extension(extension)).map_err(|e| format!("Reading this follow-up's {extension}: {e}. Preserve the run and use Abort then Pendant Mode."))?;
        if actual != *expected {
            return Err(format!("This follow-up's {extension} differs from its original plan. Preserve the ledger and use Abort then Pendant Mode; re-export the source program before a new Run."));
        }
    }
    Ok(plan)
}
pub(super) fn next(path: &Path, records: &[ledger::Fields]) -> Result<Option<Request>, String> {
    let plan = read(path)?;
    let samples = plan.samples(records, false)?;
    Ok(plan.requests()?.get(samples.len()).copied())
}
pub(super) fn export(
    path: &Path,
    records: &[ledger::Fields],
    require_result: bool,
) -> Result<(), String> {
    let plan = read(path)?;
    let samples = plan.samples(records, require_result)?;
    let ended = records.last().is_some_and(|r| r["kind"] == "result");
    let rows = samples.iter().enumerate().map(|(i,s)|format!("{{\"row\":{i},\"source_sequence\":{},\"original_trigger_machine_mm\":{},\"miss\":{}}}",s.sequence,s.trigger.map(|p|format!("{p:?}")).unwrap_or_else(||"null".into()),s.trigger.is_none())).collect::<Vec<_>>().join(",");
    let json = format!("{{\"schema\":\"dmc2.top-followup-capture.v1\",\"program_result_present\":{ended},\"requested_rows\":{},\"retained_rows\":[{rows}],\"interpretation\":\"Fresh top samples with the original acquisition frame and explicit plan order. Misses have no surface height. Import this ledger with its companions into the original object/setup, then prepare a new stock surface and material assessment. Coverage and cutting remain unresolved.\",\"cam_ready\":false}}\n",plan.points.len());
    let stem = if require_result {
        String::new()
    } else {
        format!("partial-{}.", records.len())
    };
    ledger::publish(
        &path.with_extension(format!("{stem}csv")),
        super::report::event_csv(records)?.as_bytes(),
    )?;
    ledger::publish(&path.with_extension(format!("{stem}json")), json.as_bytes())
}
