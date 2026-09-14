//! File-selection and draft exchange for the offline AXIS editor.
use super::{
    record::{self, quote},
    store::{read, save},
    Error,
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
    record::decode(
        raw,
        super::positional::request::SCHEMA,
        super::positional::request::KEYS,
    )?;
    std::str::from_utf8(raw).map_err(|e| Error::Input(format!("Request text is not UTF-8: {e}.")))
}
pub fn load_request(path: &Path) -> Result<String, Error> {
    Ok(draft(&read(path)?)?.into())
}
pub fn save_request(path: &Path, raw: &[u8]) -> Result<String, Error> {
    draft(raw)?;
    save(path, raw)?;
    Ok(format!("{{\"request_file\":{},\"state\":\"draft-retained\",\"message\":\"Request text retained. Calculate placement validates calibration, settings and selected contacts.\"}}",quote(&path.display().to_string())))
}
