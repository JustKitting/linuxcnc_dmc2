//! Offline positional mapper, reached through the standard dmc2ctl object-map path.
mod fit;
mod geometry;
mod mesh;
mod report;
pub(super) mod request;
#[cfg(test)]
mod tests;
use super::{
    model::{CaptureState, DesignFormat, Id, Stage},
    record::{self, quote},
    store::{read, save, Store},
    Error,
};
use geometry::*;
use request::Request;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

pub fn inspect(path: &Path, units: &str) -> Result<String, Error> {
    let units = request::scalar(units, "millimetres per STL unit")?;
    let raw = read(path)?;
    let mesh = mesh::Mesh::read(&raw, units)?;
    Ok(format!("{{\"schema\":\"dmc2.stl-inspection.v1\",\"source\":{},\"stl_mm_per_unit\":{},\"geometry\":{},\"cam_ready\":false}}",quote(&path.display().to_string()),units,mesh.json()))
}
pub fn prepare(store: &Store, object: &Id, setup: &Id, design: &Id) -> Result<String, Error> {
    let designs = store.designs(object)?;
    if !designs
        .iter()
        .any(|d| d.id == *design && d.format == DesignFormat::Stl)
    {
        return Err(Error::Input(
            "Attach the selected design as an STL revision before preparing its fit.".into(),
        ));
    }
    let captures = store.captures(object, setup)?;
    let fields = request::KEYS
        .iter()
        .map(|k| {
            (
                *k,
                if *k == "design" {
                    design.as_str()
                } else {
                    "REQUIRED"
                },
            )
        })
        .collect::<Vec<_>>();
    let mut rows = String::from("capture,sequence,use\n");
    for c in captures {
        if c.capture.state != CaptureState::Quarantined {
            for contact in c.capture.contacts.iter().filter(|p| p.stage == Stage::Fine) {
                rows.push_str(&format!("{},{},observe\n", c.id.as_str(), contact.sequence));
            }
        }
    }
    String::from_utf8(record::encode(request::SCHEMA, &fields, rows.as_bytes())?)
        .map_err(|e| Error::Data(e.to_string()))
}
fn folder(store: &Store, object: &Id, setup: &Id, id: &Id) -> Result<PathBuf, Error> {
    store.setup_label(object, setup)?;
    Ok(store
        .setup_path(object, setup)
        .join("analyses")
        .join(id.as_str()))
}
fn pose_record(p: Pose) -> Result<Vec<u8>, Error> {
    record::encode(
        "DMC2_POSE_CANDIDATE_V1",
        &[
            ("state", "unreviewed-local-proposal"),
            ("frame", "model-mm-to-machine-mm"),
            ("rotation_0", &csv(p.r[0])),
            ("rotation_1", &csv(p.r[1])),
            ("rotation_2", &csv(p.r[2])),
            ("translation", &csv(p.t)),
        ],
        &[],
    )
}
fn read_pose(path: &Path) -> Result<Pose, Error> {
    let bytes = read(path)?;
    let (f, payload) = record::decode(
        &bytes,
        "DMC2_POSE_CANDIDATE_V1",
        &[
            "state",
            "frame",
            "rotation_0",
            "rotation_1",
            "rotation_2",
            "translation",
        ],
    )?;
    if !payload.is_empty()
        || f["state"] != "unreviewed-local-proposal"
        || f["frame"] != "model-mm-to-machine-mm"
    {
        return Err(Error::Data(
            "Placement candidate has an unsupported frame or state.".into(),
        ));
    }
    Pose {
        r: [
            request::vector(&f["rotation_0"], "rotation_0")?,
            request::vector(&f["rotation_1"], "rotation_1")?,
            request::vector(&f["rotation_2"], "rotation_2")?,
        ],
        t: request::vector(&f["translation"], "translation")?,
    }
    .validate()
}
pub fn run(store: &Store, object: &Id, setup: &Id, id: &Id, path: &Path) -> Result<String, Error> {
    let output = folder(store, object, setup, id)?;
    if output.exists() {
        return Err(Error::Storage(format!(
            "Analysis {} already exists; choose a new analysis ID.",
            output.display()
        )));
    }
    let raw = read(path)?;
    let req = Request::read(&raw)?;
    let designs = store.designs(object)?;
    let design = designs
        .iter()
        .find(|d| d.id == req.design && d.format == DesignFormat::Stl)
        .ok_or_else(|| {
            Error::Input("Requested STL design revision is not attached to this object.".into())
        })?;
    let mesh = mesh::Mesh::read(&design.raw, req.units)?;
    let captures = store.captures(object, setup)?;
    let mut samples = Vec::new();
    let mut used = BTreeSet::new();
    for selected in &req.selected {
        let c = captures
            .iter()
            .find(|c| c.id == selected.capture)
            .ok_or_else(|| {
                Error::Data(format!(
                    "Capture {} is absent from this setup.",
                    selected.capture.as_str()
                ))
            })?;
        if c.capture.state == CaptureState::Quarantined {
            return Err(Error::Data(format!("Capture {} is quarantined. Preserve it for inspection; recapture the required geometry before fitting.",c.id.as_str())));
        }
        let contact=c.capture.contacts.iter().find(|p| p.sequence==selected.sequence && p.stage==Stage::Fine).ok_or_else(|| Error::Data(format!("{}:{} is not an original fine contact; endpoints, coarse touches, releases and misses cannot substitute.",c.id.as_str(),selected.sequence)))?;
        let center = sub(
            add(contact.trigger_mm, req.mount),
            scale(contact.direction, req.pretravel),
        );
        if !finite(center) {
            return Err(Error::Data(
                "Probe correction overflowed; check the request calibration.".into(),
            ));
        }
        samples.push(fit::Sample {
            capture: c.id.clone(),
            sequence: contact.sequence,
            usage: selected.usage,
            state: c.capture.state,
            trigger: contact.trigger_mm,
            center,
            approach: contact.direction,
            feed: contact.commanded_feed_mm_min,
        });
        used.insert(c.id.as_str());
    }
    let fitted = fit::run(&mesh, &samples, &req)?;
    let reports = report::build(&mesh, &samples, &fitted, &req)?;
    let stl = mesh.transformed_stl(fitted.model_to_machine)?;
    // All computation/validation precedes publication. Inputs are retained along
    // with outputs; the manifest is the last marker. No result replaces another.
    save(&output.join("request.txt"), &raw)?;
    save(&output.join("source.stl"), &design.raw)?;
    for c in &captures {
        if used.contains(c.id.as_str()) {
            save(
                &output.join(format!("capture-{}.txt", c.id.as_str())),
                &c.raw,
            )?;
        }
    }
    save(&output.join("residuals.csv"), reports.csv.as_bytes())?;
    save(
        &output.join("ball-centres.machine-mm.asc"),
        reports.centers.as_bytes(),
    )?;
    save(
        &output.join("estimated-surfaces.machine-mm.asc"),
        reports.surfaces.as_bytes(),
    )?;
    save(
        &output.join("model-candidate.machine-mm.stl"),
        stl.as_bytes(),
    )?;
    if fitted.outcome == fit::Outcome::Converged {
        save(
            &output.join("pose-candidate.txt"),
            &pose_record(fitted.model_to_machine)?,
        )?;
    }
    let references = used
        .iter()
        .map(|id| quote(id))
        .collect::<Vec<_>>()
        .join(",");
    let manifest=format!("{{\"schema\":\"dmc2.positional-bundle.v1\",\"object\":{},\"setup\":{},\"analysis\":{},\"design\":{},\"captures\":[{}],\"result\":{},\"cam_ready\":false}}\n",quote(object.as_str()),quote(setup.as_str()),quote(id.as_str()),quote(req.design.as_str()),references,reports.json);
    save(&output.join("manifest.json"), manifest.as_bytes())?;
    Ok(format!(
        "{{\"analysis_directory\":{},\"state\":{},\"message\":{},\"cam_ready\":false}}",
        quote(&output.display().to_string()),
        quote(fitted.outcome.name()),
        quote(fitted.outcome.message())
    ))
}
pub fn locate(
    store: &Store,
    object: &Id,
    setup: &Id,
    id: &Id,
    input: &Path,
    output: &Path,
) -> Result<String, Error> {
    let dir = folder(store, object, setup, id)?;
    read(&dir.join("manifest.json"))?;
    let pose = read_pose(&dir.join("pose-candidate.txt"))?;
    let raw = read(input)?;
    let body = std::str::from_utf8(&raw)
        .map_err(|e| Error::Input(format!("Location CSV is not UTF-8: {e}.")))?;
    let mut lines = body.lines();
    if !body.ends_with('\n') || lines.next() != Some("id,model_x_mm,model_y_mm,model_z_mm") {
        return Err(Error::Input(
            "Location CSV needs id,model_x_mm,model_y_mm,model_z_mm and terminated rows.".into(),
        ));
    }
    let mut out=String::from("id,model_x_mm,model_y_mm,model_z_mm,machine_x_mm,machine_y_mm,machine_z_mm,object,setup,analysis,state\n");
    let mut ids = BTreeSet::new();
    for l in lines {
        let v = l.split(',').collect::<Vec<_>>();
        if v.len() != 4 {
            return Err(Error::Input(format!(
                "Location row needs four fields: {l:?}."
            )));
        }
        let point_id = Id::parse(v[0])?;
        if !ids.insert(point_id.as_str().to_string()) {
            return Err(Error::Input("Location IDs must be unique.".into()));
        }
        let p = request::vector(&v[1..].join(","), "model point in mm")?;
        let mapped = pose.point(p);
        if !finite(mapped) {
            return Err(Error::Data(
                "Location transform overflowed; inspect the input coordinates.".into(),
            ));
        }
        out.push_str(&format!(
            "{},{},{},{},{},{},unreviewed-coordinate-proposal\n",
            point_id.as_str(),
            csv(p),
            csv(mapped),
            object.as_str(),
            setup.as_str(),
            id.as_str()
        ));
    }
    if ids.is_empty() {
        return Err(Error::Input("Location CSV has no points.".into()));
    }
    save(output, out.as_bytes())?;
    Ok(format!(
        "{{\"locations\":{},\"output\":{},\"machine_commands_issued\":false}}",
        ids.len(),
        quote(&output.display().to_string())
    ))
}
pub fn show(store: &Store, object: &Id, setup: &Id, id: &Id) -> Result<String, Error> {
    let dir = folder(store, object, setup, id)?;
    let text = |name: &str| -> Result<String, Error> {
        String::from_utf8(read(&dir.join(name))?)
            .map_err(|e| Error::Data(format!("Analysis {name} is not UTF-8: {e}.")))
    };
    let manifest = text("manifest.json")?;
    Ok(format!("{{\"schema\":\"dmc2.analysis-inspection.v1\",\"analysis_directory\":{},\"manifest_text\":{},\"residuals_csv\":{},\"request_text\":{},\"cam_ready\":false}}",quote(&dir.display().to_string()),quote(&manifest),quote(&text("residuals.csv")?),quote(&text("request.txt")?)))
}
pub fn export(
    store: &Store,
    object: &Id,
    setup: &Id,
    id: &Id,
    output: &Path,
) -> Result<String, Error> {
    let input = folder(store, object, setup, id)?;
    let manifest = read(&input.join("manifest.json"))?;
    if output.exists() {
        return Err(Error::Storage(format!(
            "Export {} already exists; use a new directory.",
            output.display()
        )));
    }
    let mut payloads = Vec::new();
    for entry in fs::read_dir(&input)
        .map_err(|e| Error::Storage(format!("Reading analysis {}: {e}.", input.display())))?
    {
        let e = entry.map_err(|e| Error::Storage(e.to_string()))?;
        let kind = e.file_type().map_err(|e| Error::Storage(e.to_string()))?;
        if !kind.is_file() {
            return Err(Error::Data(
                "Analysis bundle contains a non-file entry.".into(),
            ));
        }
        if e.file_name() != "manifest.json" {
            payloads.push((e.file_name(), read(&e.path())?));
        }
    }
    for (name, bytes) in payloads {
        save(&output.join(name), &bytes)?;
    }
    save(&output.join("manifest.json"), &manifest)?;
    Ok(format!(
        "{{\"export\":{},\"cam_ready\":false}}",
        quote(&output.display().to_string())
    ))
}
