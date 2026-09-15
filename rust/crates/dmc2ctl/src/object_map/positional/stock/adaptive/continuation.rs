//! Incorporate an actual completed acquisition and rerun the joint calculation.
use super::{request, surface};
use crate::object_map::{
    model::{Id, Stage},
    positional::{folder, retained::Bundle},
    record,
    store::{save, Store},
    Error,
};

pub fn run(
    store: &Store,
    object: &Id,
    setup: &Id,
    previous: &Id,
    capture: &Id,
    next: &Id,
) -> Result<String, Error> {
    let original = Bundle::read(&folder(store, object, setup, previous)?)?;
    let raw = original.get("request.txt")?;
    let r = request::Request::read(raw)?;
    let surface = surface::load(store, object, setup, &r.surface)?;
    original.require_source(&surface.source, "surface-source-")?;
    let captures = store.captures(object, setup)?;
    let fresh=captures.iter().find(|c|c.id==*capture).ok_or_else(||Error::Input("Import the new exact probe ledger and companions into this setup before continuing the adaptive cycle.".into()))?;
    let plan=fresh.context.followup_plan(&fresh.capture).map_err(Error::Data)?.ok_or_else(||Error::Data("This capture has no retained adaptive acquisition plan. Select the completed capture produced by the preceding adaptive cycle.".into()))?;
    if plan.source[..3] != [object.as_str(), setup.as_str(), previous.as_str()]
        || !plan.rows.directed()
    {
        return Err(Error::Data("The new capture belongs to a different object, setup or adaptive cycle. Select the matching capture; no frame or ancestry was inferred.".into()));
    }
    let expected=crate::probe_data::top_followup::Plan::from_program(std::str::from_utf8(original.get("adaptive-probe.ngc")?).map_err(|e|Error::Data(format!("The retained adaptive program is not UTF-8: {e}. Preserve it and select an intact prior cycle.")))?).map_err(Error::Data)?;
    if plan.encode().map_err(Error::Data)? != expected.encode().map_err(Error::Data)? {
        return Err(Error::Data("The new capture's exact acquisition plan differs from the preceding cycle's exported plan. Preserve both and select the matching capture; matching labels alone cannot establish its movements or frame.".into()));
    }
    plan.samples(&fresh.capture.records, true)
        .map_err(Error::Data)?;
    let role=plan.role.ok_or_else(||Error::Data("The adaptive capture has no measurement role. Preserve its plan and regenerate a role-bearing acquisition.".into()))?;
    let source_raw = surface.source.get("request.txt")?;
    let (fields, body) = surface::request::decode(source_raw)?;
    let version = std::str::from_utf8(source_raw.split(|b| *b == b'\n').next().unwrap_or_default())
        .map_err(|e| Error::Data(e.to_string()))?;
    let mut body = String::from_utf8(body.to_vec()).map_err(|e| Error::Data(e.to_string()))?;
    let boundary = body.find("\n\n");
    let mut contacts = boundary
        .map(|i| body[..i + 1].to_string())
        .unwrap_or_else(|| body.clone());
    for sample in &surface.request.selected {
        if sample.capture == *capture {
            return Err(Error::Input("This capture already contributes to the current surface revision. Import new measurements or continue from the later adaptive cycle.".into()));
        }
    }
    for p in fresh
        .capture
        .contacts
        .iter()
        .filter(|p| p.stage == Stage::Fine)
    {
        contacts.push_str(&format!(
            "{},{},{}\n",
            capture.as_str(),
            p.sequence,
            role.name()
        ));
    }
    body = if let Some(i) = boundary {
        let mut entries = crate::object_map::capture_selection::read(&body.as_bytes()[i + 2..])?;
        if !entries.iter().any(|e| e.capture == *capture) {
            let entry=super::super::surface::no_contact::prepare(std::slice::from_ref(fresh)).pop().ok_or_else(||Error::Data("The fresh capture's empty-space disposition is missing. Inspect its retained acquisition context before continuing.".into()))?;
            entries.push(entry);
        }
        format!(
            "{contacts}\n{}",
            crate::object_map::capture_selection::encode(&entries)
        )
    } else {
        contacts
    };
    let source_id = Id::parse(&format!("{}-surface", next.as_str()))?;
    let next_folder = folder(store, object, setup, next)?;
    if next_folder.exists() || folder(store, object, setup, &source_id)?.exists() {
        return Err(Error::Storage("The requested next cycle or its surface revision already exists. Select a new cycle ID; previous measurements are preserved.".into()));
    }
    let inputs = next_folder
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| Error::Storage("The next analysis has no setup directory.".into()))?
        .join("adaptive-inputs")
        .join(next.as_str());
    if inputs.exists() {
        return Err(Error::Storage("The next cycle's input record exists. Choose a new cycle ID to preserve that interrupted attempt.".into()));
    }
    let fields = fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect::<Vec<_>>();
    let surface_raw = record::encode(version, &fields, body.as_bytes())?;
    let surface_input = inputs.join("surface-request.txt");
    save(&surface_input, &surface_raw)?;
    super::super::run(
        store,
        object,
        setup,
        &source_id,
        &surface_input,
        super::super::Model::Surface,
    )?;
    let (mut fields, payload) = record::decode(raw, request::SCHEMA, request::KEYS)?;
    fields.insert("surface_analysis".into(), source_id.as_str().into());
    let fields = fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect::<Vec<_>>();
    let cycle_input = inputs.join("adaptive-request.txt");
    save(
        &cycle_input,
        &record::encode(request::SCHEMA, &fields, payload)?,
    )?;
    super::run(store, object, setup, next, &cycle_input)
}
