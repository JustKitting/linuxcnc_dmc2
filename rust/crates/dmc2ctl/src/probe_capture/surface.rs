//! Export retained trigger envelopes; never infer a surface at a missed point.
use super::schema::{validate, Workflow};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

type Fields = BTreeMap<String, String>;

fn number(fields: &Fields, key: &str) -> Result<f64, String> {
    fields
        .get(key)
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("surface record lacks finite {key}"))
}

fn records(text: &str) -> Result<Vec<Fields>, String> {
    if text.lines().next() != Some(Workflow::Surface.ledger_magic()) {
        return Err("this is not a versioned surface ledger".into());
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
            return Err("surface ledger sequence is discontinuous".into());
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
            return Err(
                "surface ledger has an unfinished record; no complete map is claimed".into(),
            );
        }
        let staged_end = body
            .iter()
            .position(|line| line.starts_with("exact_source="))
            .unwrap_or(body.len());
        validate(
            Workflow::Surface,
            &(body[..staged_end].join("\n") + "\n"),
            sequence,
        )?;
        let mut fields = Fields::new();
        for line in body.into_iter().skip(1) {
            let (key, value) = line
                .split_once('=')
                .ok_or("invalid retained surface field")?;
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

#[derive(Debug)]
struct Export {
    csv: String,
    metadata: String,
}

fn render(text: &str, require_result: bool) -> Result<Export, String> {
    let records = records(text)?;
    let start = records
        .first()
        .filter(|r| r["kind"] == "start")
        .ok_or("surface start metadata is missing")?;
    if records.iter().skip(1).any(|r| r["kind"] == "start") {
        return Err("surface ledger repeats its start record".into());
    }
    let reference_records: Vec<_> = records
        .iter()
        .filter(|r| r["kind"] == "touch" && r["stage"] == "1" && r["point"] == "-1")
        .collect();
    if reference_records.len() != 1 {
        return Err("exactly one retained slow high-reference contact is required".into());
    }
    let reference = reference_records[0];
    let reference_z = number(reference, "machine_z_exact")?;
    let floor = reference_z - number(start, "max_drop")?;
    let columns = number(start, "columns")? as i64;
    let rows = number(start, "rows")? as i64;
    if columns <= 0 || rows <= 0 {
        return Err("surface grid dimensions must be positive".into());
    }
    let expected = columns
        .checked_mul(rows)
        .ok_or("surface grid count overflow")?;
    let mut points = BTreeMap::new();
    let mut result_record = None;
    let mut obstructed = false;
    for record in &records {
        if result_record.is_some() {
            return Err("surface ledger contains records after its result".into());
        }
        match record["kind"].as_str() {
            "result" => result_record = Some(record),
            "obstruction" => obstructed = true,
            "miss" | "touch" => {
                if record["kind"] == "touch" && record["stage"] != "1" {
                    continue;
                }
                let point = number(record, "point")? as i64;
                if point == -1 {
                    continue;
                }
                let row = number(record, "row")? as i64;
                let column = number(record, "column")? as i64;
                if row < 0
                    || row >= rows
                    || column < 0
                    || column >= columns
                    || point < 0
                    || point >= expected
                {
                    return Err("surface sample lies outside its declared grid".into());
                }
                // Serpentine order is part of the saved program, not inferred geometry.
                let walk = if row % 2 == 0 {
                    column
                } else {
                    columns - 1 - column
                };
                if point != row * columns + walk {
                    return Err("surface sample identifier disagrees with its grid cell".into());
                }
                if points.insert(point, record).is_some() {
                    return Err("surface grid repeats an accepted cell".into());
                }
            }
            _ => (),
        }
    }
    if require_result && result_record.is_none() {
        return Err("surface run has no result record; export is provisional only".into());
    }
    let hits = points.values().filter(|r| r["kind"] == "touch").count();
    let misses = points.len() - hits;
    if let Some(result) = result_record {
        if obstructed
            || number(result, "points")? != expected as f64
            || points.len() as i64 != expected
            || number(result, "hits")? != hits as f64
            || number(result, "misses")? != misses as f64
        {
            return Err("surface result conflicts with retained contacts, misses or obstruction; map is quarantined".into());
        }
    }
    let mut csv = String::from("point,row,column,status,machine_x_mm,machine_y_mm,probe_trigger_machine_z_mm,relative_trigger_z_mm,search_floor_machine_z_mm,commanded_feed_mm_min\n");
    for (point, record) in points {
        let row = &record["row"];
        let col = &record["column"];
        let feed = &record["feed"];
        if record["kind"] == "touch" {
            let x = number(record, "machine_x_exact")?;
            let y = number(record, "machine_y_exact")?;
            let z = number(record, "machine_z_exact")?;
            csv.push_str(&format!(
                "{point},{row},{col},measured,{x},{y},{z},{},{floor},{feed}\n",
                z - reference_z
            ));
        } else {
            let x = number(record, "machine_x")?;
            let y = number(record, "machine_y")?;
            let searched_floor = number(record, "floor_machine_z")?;
            csv.push_str(&format!(
                "{point},{row},{col},unmeasured,{x},{y},,,{searched_floor},{feed}\n"
            ));
        }
    }
    let mut metadata = format!("{{\n  \"schema\": \"dmc2.surface-map.v1\",\n  \"units\": \"mm\",\n  \"coordinate_frame\": \"LinuxCNC machine trigger coordinates\",\n  \"geometry\": \"top contact envelope of mounted probe\",\n  \"program_result_present\": {},\n  \"obstruction_recorded\": {obstructed},\n  \"absolute_surface_z_calibrated\": false,\n  \"ball_radius_compensation_applied\": false,\n  \"plate_alignment_applied\": false,\n  \"plate_reference_path\": \"config/metrology/known-plate-position.json\",\n  \"missing_points\": \"unmeasured within bounded descent; no surface height assigned\",\n  \"reference_trigger_machine_mm\": [{},{},{}],\n  \"measured_points\": {hits},\n  \"unmeasured_points\": {misses},\n  \"expected_points\": {expected},\n  \"relative_z_formula\": \"fine machine trigger z minus fine reference machine trigger z\",\n  \"work_to_machine_translation_mm\": [{},{},{}],\n  \"settings\": {{\n", result_record.is_some(), number(reference,"machine_x_exact")?, number(reference,"machine_y_exact")?, reference_z, number(start,"offset_x")?, number(start,"offset_y")?, number(start,"offset_z")?);
    let keys = [
        "x_min",
        "x_max",
        "y_min",
        "y_max",
        "spacing",
        "columns",
        "rows",
        "reference_search",
        "max_drop",
        "clearance",
        "usable_reach",
        "reach_reserve",
        "feed",
        "coarse_feed",
        "ball_diameter",
    ];
    for (index, key) in keys.iter().enumerate() {
        metadata.push_str(&format!(
            "    \"{key}\": {}{}\n",
            number(start, key)?,
            if index + 1 == keys.len() { "" } else { "," }
        ));
    }
    metadata.push_str("  }\n}\n");
    Ok(Export { csv, metadata })
}

fn publish(path: &Path, bytes: &[u8]) -> Result<(), String> {
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

pub(super) fn export(path: &Path, require_result: bool) -> Result<(), String> {
    let text =
        fs::read_to_string(path).map_err(|e| format!("reading retained surface ledger: {e}"))?;
    let output = render(&text, require_result)?;
    let csv = path.with_extension("csv");
    let metadata = path.with_extension("json");
    publish(&csv, output.csv.as_bytes())?;
    publish(&metadata, output.metadata.as_bytes())?;
    println!(
        "DMC2 retained surface data: {} and {}",
        csv.display(),
        metadata.display()
    );
    Ok(())
}
