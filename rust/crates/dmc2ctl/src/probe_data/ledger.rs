//! Shared validated ledger reader and durable, no-overwrite export.
use super::schema::{validate, Workflow};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub type Fields = BTreeMap<String, String>;

pub fn number(fields: &Fields, key: &str) -> Result<f64, String> {
    fields
        .get(key)
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("probe record lacks finite {key}"))
}

pub fn records(text: &str, workflow: Workflow) -> Result<Vec<Fields>, String> {
    if text.lines().next() != Some(workflow.ledger_magic()) {
        return Err("this is not a versioned probe ledger".into());
    }
    let mut lines = text.lines();
    let mut result = Vec::new();
    while let Some(line) = lines.next() {
        let Some(raw) = line.strip_prefix("BEGIN ") else {
            continue;
        };
        let sequence = raw
            .parse::<u64>()
            .map_err(|e| format!("ledger sequence: {e}"))?;
        if sequence != result.len() as u64 {
            return Err("probe ledger sequence is discontinuous".into());
        }
        let mut body = Vec::new();
        let end = format!("END {sequence}");
        let mut terminated = false;
        for line in lines.by_ref() {
            if line == end {
                terminated = true;
                break;
            }
            body.push(line);
        }
        if !terminated {
            return Err("probe ledger has an unfinished record; no complete map is claimed".into());
        }
        let staged_end = body
            .iter()
            .position(|line| line.starts_with("exact_source="))
            .unwrap_or(body.len());
        validate(workflow, &(body[..staged_end].join("\n") + "\n"), sequence)?;
        let mut fields = Fields::new();
        for line in body.into_iter().skip(1) {
            let (key, value) = line.split_once('=').ok_or("invalid retained probe field")?;
            if fields.insert(key.to_owned(), value.to_owned()).is_some() {
                return Err(format!("duplicate retained field {key}"));
            }
        }
        if matches!(fields["kind"].as_str(), "touch" | "obstruction") {
            for axis in ["x", "y", "z"] {
                let exact = number(&fields, &format!("machine_{axis}_exact"))?;
                let bits = fields
                    .get(&format!("machine_{axis}_f64_bits"))
                    .and_then(|v| u64::from_str_radix(v, 16).ok());
                if bits != Some(exact.to_bits()) {
                    return Err(format!(
                        "original {axis} trigger bits are missing or disagree"
                    ));
                }
            }
        }
        result.push(fields);
    }
    Ok(result)
}

pub fn publish(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path.exists() {
        return if fs::read(path).map_err(|e| format!("reading existing map: {e}"))? == bytes {
            Ok(())
        } else {
            Err(format!(
                "{} already contains different output; it was preserved",
                path.display()
            ))
        };
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let pending = path.with_extension(format!("pending-{stamp}-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|e| format!("creating map output: {e}"))?;
    file.write_all(bytes)
        .map_err(|e| format!("writing map output: {e}"))?;
    file.sync_all()
        .map_err(|e| format!("syncing map output: {e}"))?;
    if fs::read(&pending).map_err(|e| format!("reading map back: {e}"))? != bytes {
        return Err("map readback differs; output is quarantined".into());
    }
    fs::hard_link(&pending, path)
        .map_err(|e| format!("publishing map without overwriting existing data: {e}"))?;
    fs::remove_file(&pending).map_err(|e| format!("removing this export's temporary link: {e}"))?;
    File::open(path.parent().ok_or("map has no parent directory")?)
        .and_then(|f| f.sync_all())
        .map_err(|e| format!("syncing map directory: {e}"))?;
    Ok(())
}
