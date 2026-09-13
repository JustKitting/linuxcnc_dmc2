//! FreeCAD Points ASC files are raw fine-trigger envelopes, in machine mm.
use super::{
    model::{CaptureState, Id, Registration, Stage},
    record::quote,
    store::{save, sync_directory, Store},
    Error,
};
use std::{fs, path::Path};

pub fn export(store: &Store, object: &Id, setup: &Id, output: &Path) -> Result<String, Error> {
    let object_label = store.object_label(object)?;
    let setup_label = store.setup_label(object, setup)?;
    let captures = store.captures(object, setup)?;
    let designs = store.designs(object)?;
    // All inputs are validated before output creation. An existing export stays intact.
    fs::create_dir(output).map_err(|e| {
        Error::Storage(format!(
            "Creating new export directory {}: {e}.",
            output.display()
        ))
    })?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    sync_directory(parent)?;
    let mut manifest_captures = Vec::new();
    for snapshot in &captures {
        let id = snapshot.id.as_str();
        let capture = &snapshot.capture;
        save(&output.join(format!("{id}.ledger.txt")), &snapshot.raw)?;
        let mut csv = String::from("sequence,stage,capture_state,source,machine_x_mm,machine_y_mm,machine_z_mm,commanded_direction_x,commanded_direction_y,commanded_direction_z,commanded_feed_mm_min\n");
        let mut points = String::new();
        let mut point_sequences = Vec::new();
        for contact in &capture.contacts {
            let xyz = contact.trigger_mm.map(|v| v.to_string()).join(",");
            let direction = contact.direction.map(|v| v.to_string()).join(",");
            csv.push_str(&format!(
                "{},{},{},original_G38_trigger_f64,{xyz},{direction},{}\n",
                contact.sequence,
                contact.stage.name(),
                capture.state.name(),
                contact.commanded_feed_mm_min
            ));
            if contact.stage == Stage::Fine && capture.state != CaptureState::Quarantined {
                points.push_str(&contact.trigger_mm.map(|v| v.to_string()).join(" "));
                points.push('\n');
                point_sequences.push(contact.sequence.to_string());
            }
        }
        save(&output.join(format!("{id}.contacts.csv")), csv.as_bytes())?;
        let cloud = if points.is_empty() {
            "null".into()
        } else {
            let name = format!("{id}.trigger-envelope.asc");
            save(&output.join(&name), points.as_bytes())?;
            quote(&name)
        };
        manifest_captures.push(format!("{{\"id\":{},\"source_path\":{},\"ledger\":{},\"contacts_csv\":{},\"fine_trigger_cloud\":{cloud},\"cloud_record_sequences\":[{}],{}}}", quote(id), quote(&snapshot.source_path), quote(&format!("{id}.ledger.txt")), quote(&format!("{id}.contacts.csv")), point_sequences.join(","), capture.summary()));
    }
    let mut manifest_designs = Vec::new();
    for design in &designs {
        let name = format!("design-{}.{}", design.id.as_str(), design.format.name());
        save(&output.join(&name), &design.raw)?;
        manifest_designs.push(format!(
            "{{\"file\":{},\"revision\":{},\"geometry_inspected\":false}}",
            quote(&name),
            quote(design.id.as_str())
        ));
    }
    let manifest = format!("{{\n\"schema\":\"dmc2.freecad-exchange.v1\",\n\"object_id\":{},\n\"object_label\":{},\n\"setup_id\":{},\n\"setup_label\":{},\n\"registration\":{},\n\"reconstructed_stock\":null,\n\"cam_ready\":false,\n\"captures\":[{}],\n\"design_revisions\":[{}],\n\"interpretation\":\"Each ASC is an individual capture's fine machine G38 trigger envelope in mm, including reference contacts. No ball-radius, mounting, object-frame or cross-setup correction is applied. Captures are not merged. Misses have no assigned surface points. Quarantined captures retain their raw ledger and labelled contact CSV but no ASC. Recorded results are not an acceptance of shape, dimensions or alignment.\"\n}}\n", quote(object.as_str()), quote(&object_label), quote(setup.as_str()), quote(&setup_label), Registration::Unresolved.json(), manifest_captures.join(","), manifest_designs.join(","));
    save(&output.join("README.txt"), b"DMC2 / FreeCAD measurement exchange\nOpen *.trigger-envelope.asc using FreeCAD's Points workbench Import points command. The coordinates are in millimetres and are original LinuxCNC trigger positions. Open a retained design file separately when present.\n\nThese are probe trigger envelopes, not reconstructed stock solids. No CAD placement or machining correction has been applied. Inspect manifest.json for per-capture state, source and point-to-ledger record mapping. Separate captures have not been established to share a probe mounting or machine reference.\n\nmanifest.json is published last. A directory without it is an interrupted export: choose a new output directory and retry from the retained object store.\n")?;
    // Final commit marker, only after all payloads have been flushed and read back.
    save(&output.join("manifest.json"), manifest.as_bytes())?;
    Ok(format!(
        "{{\"export\":{},\"manifest\":{},\"cam_ready\":false}}",
        quote(&output.display().to_string()),
        quote(&output.join("manifest.json").display().to_string())
    ))
}
