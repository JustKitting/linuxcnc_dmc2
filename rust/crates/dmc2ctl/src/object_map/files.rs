//! File-selection and draft exchange for the offline AXIS editor.
use super::{
    Error,
    record::{self, quote},
    store::{read, save},
};
use std::{fs, path::Path};

pub fn browse(path: &Path) -> Result<String, Error> {
    let path = fs::canonicalize(path)
        .map_err(|e| Error::Storage(format!("Opening directory {}: {e}.", path.display())))?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(&path)
        .map_err(|e| Error::Storage(format!("Reading directory {}: {e}.", path.display())))?
    {
        let entry = entry
            .map_err(|e| Error::Storage(format!("Reading entry in {}: {e}.", path.display())))?;
        let metadata = entry
            .metadata()
            .map_err(|e| Error::Storage(format!("Reading {}: {e}.", entry.path().display())))?;
        let name = entry.file_name().into_string().map_err(|_| Error::Input("The directory contains a filename that cannot be displayed as UTF-8. Enter the intended source path directly.".into()))?;
        entries.push((!metadata.is_dir(), name, entry.path()));
    }
    entries.sort();
    Ok(format!(
        "{{\"schema\":\"dmc2.directory.v1\",\"directory\":{},\"parent\":{},\"entries\":[{}]}}",
        quote(&path.display().to_string()),
        quote(&path.parent().unwrap_or(&path).display().to_string()),
        entries
            .iter()
            .map(|(file, name, path)| format!(
                "{{\"name\":{},\"path\":{},\"directory\":{}}}",
                quote(name),
                quote(&path.display().to_string()),
                !file
            ))
            .collect::<Vec<_>>()
            .join(",")
    ))
}
fn draft(raw: &[u8]) -> Result<&str, Error> {
    let schemas = [
        (
            super::positional::request::SCHEMA,
            super::positional::request::keys(),
        ),
        super::positional::stock::Model::Outline.request_schema(),
        super::positional::stock::Model::Surface.request_schema(),
        (
            super::positional::stock::surface::request::LEGACY_SCHEMA,
            super::positional::stock::surface::request::legacy_keys(),
        ),
        (
            super::positional::stock::surface::request::CONTRIBUTING_SCHEMA,
            super::positional::stock::surface::request::keys(),
        ),
        (
            super::positional::stock::placement::request::SCHEMA,
            super::positional::stock::placement::request::KEYS.to_vec(),
        ),
        (
            super::positional::stock::material::request::SCHEMA,
            super::positional::stock::material::request::KEYS.to_vec(),
        ),
        (
            super::positional::stock::reconstruction::request::SCHEMA,
            super::positional::stock::reconstruction::request::KEYS.to_vec(),
        ),
        (
            super::positional::stock::volume::request::SCHEMA,
            super::positional::stock::volume::request::KEYS.to_vec(),
        ),
        (
            super::positional::stock::observation::request::SCHEMA,
            super::positional::stock::observation::request::KEYS.to_vec(),
        ),
        (
            super::positional::stock::observation::spatial::request::SCHEMA,
            super::positional::stock::observation::spatial::request::KEYS.to_vec(),
        ),
        (
            super::positional::stock::observation::spatial::request::HISTORY_SCHEMA,
            super::positional::stock::observation::spatial::request::KEYS.to_vec(),
        ),
    ];
    let (schema, keys) = schemas.iter().find(|(schema, _)| raw.starts_with(format!("{schema}\n").as_bytes()))
        .ok_or_else(|| Error::Input("Unsupported analysis draft. Prepare a registration, stock outline, stock surface, machining footprint, material-check, stock-mesh, volume-placement or follow-up-observation request through Object Mapper.".into()))?;
    record::decode(raw, schema, keys)?;
    std::str::from_utf8(raw).map_err(|e| Error::Input(format!("Request text is not UTF-8: {e}.")))
}
pub fn load_request(path: &Path) -> Result<String, Error> {
    Ok(draft(&read(path)?)?.into())
}
pub fn save_request(path: &Path, raw: &[u8]) -> Result<String, Error> {
    draft(raw)?;
    save(path, raw)?;
    Ok(format!(
        "{{\"request_file\":{},\"state\":\"draft-retained\",\"message\":\"Request text retained. The selected analysis operation validates calibration, settings and selected contacts.\"}}",
        quote(&path.display().to_string())
    ))
}
