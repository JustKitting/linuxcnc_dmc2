use super::schema::{validate, Workflow};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn io(context: &str, error: std::io::Error) -> String {
    format!("{context}: {error}")
}

fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|e| io("syncing capture directory", e))
}

fn save_active(output: &Path, workflow: Workflow, file: &str, sequence: u64) -> Result<(), String> {
    let bytes = format!("{file}\n{sequence}\n");
    let pending = output.join(format!("{}-active.pending", workflow.name()));
    let mut f = File::create(&pending).map_err(|e| io("opening capture state", e))?;
    f.write_all(bytes.as_bytes())
        .map_err(|e| io("writing capture state", e))?;
    f.sync_all().map_err(|e| io("syncing capture state", e))?;
    fs::rename(&pending, output.join(workflow.active()))
        .map_err(|e| io("publishing capture state", e))?;
    sync_directory(output)
}

pub(super) fn begin(output: &Path, workflow: Workflow) -> Result<(), String> {
    fs::create_dir_all(output.join(workflow.name()))
        .map_err(|e| io("creating probe output directory", e))?;
    // Only this program's scratch request is removed; previous ledgers remain.
    match fs::remove_file(output.join(workflow.request())) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(io("discarding an uncommitted old capture request", e)),
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("reading capture timestamp: {e}"))?
        .as_nanos();
    let name = format!("{}-{stamp}-{}.txt", workflow.name(), std::process::id());
    let mut ledger = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join(workflow.name()).join(&name))
        .map_err(|e| io("creating a unique capture ledger", e))?;
    let header = format!("{}\nunits=mm,mm/min\nsource=LinuxCNC G38 trigger parameters; serialized decimal precision=9\nstage=0:coarse-location,1:fine-measurement\nX_positive=physical LEFT / LinuxCNC +X\nX_negative=physical RIGHT / LinuxCNC -X\n", workflow.ledger_magic());
    ledger
        .write_all(header.as_bytes())
        .map_err(|e| io("writing capture ledger header", e))?;
    ledger
        .sync_all()
        .map_err(|e| io("syncing capture ledger header", e))?;
    sync_directory(&output.join(workflow.name()))?;
    save_active(output, workflow, &name, 0)?;
    println!(
        "DMC2 capture ledger: {}",
        output.join(workflow.name()).join(name).display()
    );
    Ok(())
}

pub(super) fn commit(
    output: &Path,
    workflow: Workflow,
    sequence: u64,
    trigger: impl FnOnce() -> Result<[f64; 3], String>,
) -> Result<(), String> {
    let active = fs::read_to_string(output.join(workflow.active()))
        .map_err(|e| io("reading active capture state", e))?;
    let lines: Vec<_> = active.lines().collect();
    if lines.len() != 2
        || !lines[0].starts_with(&format!("{}-", workflow.name()))
        || !lines[0].ends_with(".txt")
        || lines[0].contains('/')
        || lines[0].contains('\\')
    {
        return Err(
            "active capture state is malformed; start the script again after recovery".into(),
        );
    }
    let expected = lines[1]
        .parse::<u64>()
        .map_err(|e| format!("invalid capture sequence state: {e}"))?;
    if sequence != expected {
        return Err(format!("capture sequence {sequence} does not match expected {expected}; no return move is permitted"));
    }
    let request = fs::read_to_string(output.join(workflow.request())).map_err(|e| {
        io(
            "reading staged contact; LinuxCNC may not have written the request file",
            e,
        )
    })?;
    validate(workflow, &request, sequence)?;
    let exact = if request
        .lines()
        .any(|line| matches!(line, "kind=touch" | "kind=obstruction"))
    {
        exact_trigger(&request, trigger()?)?
    } else {
        String::new()
    };
    let path = output.join(workflow.name()).join(lines[0]);
    let bytes = format!("BEGIN {sequence}\n{request}{exact}END {sequence}\n");
    let mut ledger = OpenOptions::new()
        .read(true)
        .append(true)
        .open(&path)
        .map_err(|e| io("opening existing capture ledger", e))?;
    let start = ledger
        .seek(SeekFrom::End(0))
        .map_err(|e| io("locating capture append position", e))?;
    ledger
        .write_all(bytes.as_bytes())
        .map_err(|e| io("writing contact to ledger", e))?;
    ledger
        .sync_all()
        .map_err(|e| io("syncing contact to storage", e))?;
    ledger
        .seek(SeekFrom::Start(start))
        .map_err(|e| io("seeking saved contact for readback", e))?;
    let mut retained = String::new();
    ledger
        .read_to_string(&mut retained)
        .map_err(|e| io("reading saved contact back", e))?;
    if retained != bytes {
        return Err("saved contact readback differs from the staged trigger record; retained result is quarantined".into());
    }
    if workflow == Workflow::Surface && request.lines().any(|line| line == "kind=result") {
        super::surface::export(&path, true)?;
    }
    if workflow == Workflow::Mapper && request.lines().any(|line| line == "kind=result") {
        super::mapper::export(&path, true)?;
    }
    if workflow == Workflow::Block && request.lines().any(|line| line == "kind=result") {
        super::block::export(&path, true)?;
    }
    if workflow == Workflow::ToolSetter
        && request.lines().any(|line| line == "kind=touch")
        && request.lines().any(|line| line == "stage=1")
    {
        // Export only after the original fine trigger has been synced/read back.
        // This adds no machine command and does not apply the saved offset.
        super::tool_setter::export(&path)?;
    }
    fs::remove_file(output.join(workflow.request()))
        .map_err(|e| io("consuming the saved capture request", e))?;
    save_active(output, workflow, lines[0], sequence + 1)?;
    println!(
        "DMC2 capture readback: {} sequence={sequence}\n{retained}",
        path.display()
    );
    Ok(())
}

pub(super) fn active_path(
    output: &Path,
    workflow: Workflow,
    expected: u64,
) -> Result<PathBuf, String> {
    let text = fs::read_to_string(output.join(workflow.active()))
        .map_err(|e| io("reading active capture state", e))?;
    let lines: Vec<_> = text.lines().collect();
    if lines.len() != 2
        || lines[1].parse::<u64>() != Ok(expected)
        || !lines[0].starts_with(&format!("{}-", workflow.name()))
        || !lines[0].ends_with(".txt")
        || lines[0].contains(['/', '\\'])
    {
        return Err("The active capture ledger or sequence does not match this script. Reopen the script after Abort and Pendant Mode.".into());
    }
    Ok(output.join(workflow.name()).join(lines[0]))
}

fn exact_trigger(request: &str, xyz: [f64; 3]) -> Result<String, String> {
    let mut retained =
        String::from("exact_source=emcStatus.motion.traj.probedPosition;machine-mm\n");
    for (axis, raw) in ["x", "y", "z"].into_iter().zip(xyz) {
        let key = format!("machine_{axis}=");
        let staged = request
            .lines()
            .find_map(|line| line.strip_prefix(&key))
            .ok_or_else(|| format!("missing staged trigger {axis}"))?
            .parse::<f64>()
            .map_err(|e| format!("reading staged trigger {axis}: {e}"))?;
        // Half of the last serialized decimal place, plus f64 arithmetic error
        // from subtracting/adding work offsets. This is not a motion tolerance.
        let allowance = 0.5e-9 + 4.0 * f64::EPSILON * raw.abs().max(staged.abs()).max(1.0);
        if !raw.is_finite() || (raw - staged).abs() > allowance {
            return Err(format!("original G38 trigger {axis}={raw} disagrees with script capture {staged}; keep the setup in place"));
        }
        retained.push_str(&format!(
            "machine_{axis}_exact={raw}\nmachine_{axis}_f64_bits={:016x}\n",
            raw.to_bits()
        ));
    }
    Ok(retained)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_incomplete_stale_and_nonfinite_trigger_records() {
        assert!(validate(Workflow::Circle, "", 0).is_err());
        let record = "DMC2_CIRCLE_RECORD_V2\nsequence=1\nkind=touch\nstage=1\npass=1\naxis=1\ndirection=-1\nsuccess=1\nwork_x=1\nwork_y=2\nwork_z=3\nmachine_x=4\nmachine_y=5\nmachine_z=6\nfeed=50\n";
        assert!(validate(Workflow::Circle, record, 1).is_ok());
        assert!(validate(Workflow::Circle, &record.replace("stage=1", "stage=2"), 1).is_err());
        assert!(validate(Workflow::Circle, record, 0).is_err());
        assert!(validate(
            Workflow::Circle,
            &record.replace("success=1", "success=0"),
            1
        )
        .is_err());
        assert!(validate(
            Workflow::Circle,
            &record.replace("machine_y=5", "machine_y=NaN"),
            1
        )
        .is_err());
        assert!(validate(Workflow::Circle, &record.replace("machine_y=5\n", ""), 1).is_err());
    }

    #[test]
    fn commit_preserves_readback_and_rejects_a_second_use() {
        let path = std::env::temp_dir().join(format!(
            "dmc2-circle-capture-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        begin(&path, Workflow::Circle).unwrap();
        let record = "DMC2_CIRCLE_RECORD_V2\nsequence=0\nkind=result\nreason=2\nx=1\ny=2\nmachine_x=1\nmachine_y=2\ndx=3\ndy=3\nspan_error=0\nbalance=0.0005\ndiameter=5\npass=1\n";
        fs::write(path.join(Workflow::Circle.request()), record).unwrap();
        commit(&path, Workflow::Circle, 0, || {
            panic!("result does not read a probe snapshot")
        })
        .unwrap();
        let state = fs::read_to_string(path.join(Workflow::Circle.active())).unwrap();
        let ledger =
            fs::read_to_string(path.join("circle").join(state.lines().next().unwrap())).unwrap();
        assert!(ledger.ends_with(&format!("BEGIN 0\n{record}END 0\n")));
        assert!(commit(&path, Workflow::Circle, 0, || unreachable!()).is_err());
        assert!(commit(&path, Workflow::Circle, 1, || unreachable!()).is_err());
        fs::remove_dir_all(&path).unwrap();
    }

    #[test]
    fn retain_original_bits_and_reject_a_stale_probe_position() {
        let request = "machine_x=293.526025375\nmachine_y=166.847919281\nmachine_z=33.291491486\n";
        let xyz: [f64; 3] = [293.5260253753662, 166.84791928100586, 33.2914914855957];
        let retained = exact_trigger(request, xyz).unwrap();
        assert!(retained.contains(&format!("machine_x_f64_bits={:016x}", xyz[0].to_bits())));
        assert!(exact_trigger(request, [xyz[0], xyz[1] + 0.001, xyz[2]]).is_err());
        assert!(exact_trigger(request, [f64::NAN, xyz[1], xyz[2]]).is_err());
    }
}
