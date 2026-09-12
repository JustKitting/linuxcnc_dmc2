use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const REQUEST: &str = "circle-request.txt";
const ACTIVE: &str = "circle-active.txt";
const MAGIC: &str = "DMC2_CIRCLE_RECORD_V1";

fn io(context: &str, error: std::io::Error) -> String {
    format!("{context}: {error}")
}

fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|e| io("syncing capture directory", e))
}

fn save_active(output: &Path, file: &str, sequence: u64) -> Result<(), String> {
    let bytes = format!("{file}\n{sequence}\n");
    let pending = output.join("circle-active.pending");
    let mut f = File::create(&pending).map_err(|e| io("opening capture state", e))?;
    f.write_all(bytes.as_bytes())
        .map_err(|e| io("writing capture state", e))?;
    f.sync_all().map_err(|e| io("syncing capture state", e))?;
    fs::rename(&pending, output.join(ACTIVE)).map_err(|e| io("publishing capture state", e))?;
    sync_directory(output)
}

pub(super) fn begin(output: &Path) -> Result<(), String> {
    fs::create_dir_all(output.join("circle"))
        .map_err(|e| io("creating circle output directory", e))?;
    // Only this program's scratch request is removed; previous ledgers remain.
    match fs::remove_file(output.join(REQUEST)) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(io("discarding an uncommitted old capture request", e)),
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("reading capture timestamp: {e}"))?
        .as_nanos();
    let name = format!("circle-{stamp}-{}.txt", std::process::id());
    let mut ledger = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("circle").join(&name))
        .map_err(|e| io("creating a unique capture ledger", e))?;
    ledger.write_all(b"DMC2_CIRCLE_LEDGER_V1\nunits=mm,mm/min\nsource=LinuxCNC G38 trigger parameters; serialized decimal precision=9\nX_positive=physical LEFT / LinuxCNC +X\nX_negative=physical RIGHT / LinuxCNC -X\n")
        .map_err(|e| io("writing capture ledger header", e))?;
    ledger
        .sync_all()
        .map_err(|e| io("syncing capture ledger header", e))?;
    sync_directory(&output.join("circle"))?;
    save_active(output, &name, 0)?;
    println!(
        "DMC2 circle capture ledger: {}",
        output.join("circle").join(name).display()
    );
    Ok(())
}

pub(super) fn commit(
    output: &Path,
    sequence: u64,
    trigger: impl FnOnce() -> Result<[f64; 3], String>,
) -> Result<(), String> {
    let active = fs::read_to_string(output.join(ACTIVE))
        .map_err(|e| io("reading active capture state", e))?;
    let lines: Vec<_> = active.lines().collect();
    if lines.len() != 2
        || !lines[0].starts_with("circle-")
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
    let request = fs::read_to_string(output.join(REQUEST)).map_err(|e| {
        io(
            "reading staged contact; LinuxCNC may not have written circle-request.txt",
            e,
        )
    })?;
    validate(&request, sequence)?;
    let exact = if request.lines().any(|line| line == "kind=touch") {
        exact_trigger(&request, trigger()?)?
    } else {
        String::new()
    };
    let path = output.join("circle").join(lines[0]);
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
    fs::remove_file(output.join(REQUEST))
        .map_err(|e| io("consuming the saved capture request", e))?;
    save_active(output, lines[0], sequence + 1)?;
    println!(
        "DMC2 capture readback: {} sequence={sequence}\n{retained}",
        path.display()
    );
    Ok(())
}

fn validate(request: &str, sequence: u64) -> Result<(), String> {
    let mut lines = request.lines();
    if lines.next() != Some(MAGIC) || !request.ends_with('\n') {
        return Err("staged capture is missing its complete versioned header/terminator".into());
    }
    let mut values = BTreeMap::new();
    for line in lines {
        let (key, value) = line
            .split_once('=')
            .ok_or("malformed staged capture field")?;
        if values.insert(key, value).is_some() {
            return Err(format!("duplicate capture field {key}"));
        }
    }
    if values
        .remove("sequence")
        .and_then(|v| v.parse::<u64>().ok())
        != Some(sequence)
    {
        return Err("staged record has a stale or missing capture sequence".into());
    }
    let kind = values
        .remove("kind")
        .ok_or("capture record kind is missing")?;
    let required: &[&str] = match kind {
        "start" => &[
            "x",
            "y",
            "z",
            "offset_x",
            "offset_y",
            "offset_z",
            "feed",
            "search",
            "ball_diameter",
            "step_x",
            "step_y",
        ],
        "touch" => &[
            "pass",
            "axis",
            "direction",
            "success",
            "work_x",
            "work_y",
            "work_z",
            "machine_x",
            "machine_y",
            "machine_z",
            "feed",
        ],
        "sweep" => &[
            "pass",
            "x",
            "y",
            "center_x",
            "center_y",
            "dx",
            "dy",
            "span_error",
            "balance",
            "radius_x",
            "radius_y",
        ],
        "selection" | "result" => &[
            "reason",
            "x",
            "y",
            "machine_x",
            "machine_y",
            "dx",
            "dy",
            "span_error",
            "balance",
            "diameter",
            "pass",
        ],
        _ => return Err(format!("unknown capture record kind {kind}")),
    };
    if values.len() != required.len() {
        return Err(format!("{kind} capture has missing or unexpected fields"));
    }
    for key in required {
        let value = values
            .get(key)
            .ok_or_else(|| format!("missing {kind} field {key}"))?;
        if !value.parse::<f64>().map(|v| v.is_finite()).unwrap_or(false) {
            return Err(format!(
                "{kind} field {key} is not a finite number: {value}"
            ));
        }
    }
    if kind == "touch" && values["success"].parse::<f64>() != Ok(1.0) {
        return Err("G38 did not report a contact; no reconstructed position is accepted".into());
    }
    Ok(())
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
        assert!(validate("", 0).is_err());
        let record = "DMC2_CIRCLE_RECORD_V1\nsequence=1\nkind=touch\npass=1\naxis=1\ndirection=-1\nsuccess=1\nwork_x=1\nwork_y=2\nwork_z=3\nmachine_x=4\nmachine_y=5\nmachine_z=6\nfeed=50\n";
        assert!(validate(record, 1).is_ok());
        assert!(validate(record, 0).is_err());
        assert!(validate(&record.replace("success=1", "success=0"), 1).is_err());
        assert!(validate(&record.replace("machine_y=5", "machine_y=NaN"), 1).is_err());
        assert!(validate(&record.replace("machine_y=5\n", ""), 1).is_err());
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
        begin(&path).unwrap();
        let record = "DMC2_CIRCLE_RECORD_V1\nsequence=0\nkind=result\nreason=2\nx=1\ny=2\nmachine_x=1\nmachine_y=2\ndx=3\ndy=3\nspan_error=0\nbalance=0.0005\ndiameter=5\npass=1\n";
        fs::write(path.join(REQUEST), record).unwrap();
        commit(&path, 0, || panic!("result does not read a probe snapshot")).unwrap();
        let state = fs::read_to_string(path.join(ACTIVE)).unwrap();
        let ledger =
            fs::read_to_string(path.join("circle").join(state.lines().next().unwrap())).unwrap();
        assert!(ledger.ends_with(&format!("BEGIN 0\n{record}END 0\n")));
        assert!(commit(&path, 0, || unreachable!()).is_err());
        assert!(commit(&path, 1, || unreachable!()).is_err());
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
