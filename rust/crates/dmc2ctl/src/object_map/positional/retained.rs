//! Immutable analysis inputs shared by dependent offline calculations.
use super::super::{
    store::{read, save, CaptureSnapshot},
    Error,
};
use super::probe::Sample;
use std::{collections::BTreeMap, fs, path::Path};
pub struct Bundle {
    files: BTreeMap<String, Vec<u8>>,
}
impl Bundle {
    pub fn read(path: &Path) -> Result<Self, Error> {
        let mut files = BTreeMap::new();
        for entry in fs::read_dir(path).map_err(|e| {
            Error::Storage(format!(
                "Reading analysis {}: {e}. Select a published analysis.",
                path.display()
            ))
        })? {
            let entry = entry.map_err(|e| {
                Error::Storage(format!(
                    "Reading analysis entry: {e}. Inspect its retained files."
                ))
            })?;
            if !entry
                .file_type()
                .map_err(|e| {
                    Error::Storage(format!(
                        "Reading analysis file type: {e}. Inspect its retained files."
                    ))
                })?
                .is_file()
            {
                return Err(Error::Data("An analysis bundle contains a non-file entry. Preserve and inspect the bundle before reuse.".into()));
            }
            let name = entry.file_name().into_string().map_err(|_| Error::Data("An analysis bundle filename is not UTF-8. Preserve and inspect it before reuse.".into()))?;
            files.insert(name, read(&entry.path())?);
        }
        let result = Self { files };
        result.get("manifest.json")?;
        Ok(result)
    }
    pub fn get(&self, name: &str) -> Result<&[u8], Error> {
        self.optional(name).ok_or_else(|| Error::Data(format!("The source analysis is missing {name}. Select the intended published analysis; do not reconstruct missing source data.")))
    }
    pub fn optional(&self, name: &str) -> Option<&[u8]> {
        self.files.get(name).map(Vec::as_slice)
    }
    pub fn require_equal(&self, name: &str, bytes: &[u8]) -> Result<(), Error> {
        if self.get(name)? != bytes {
            return Err(Error::Data(format!("{name} differs from the source analysis's retained bytes or calculation. Preserve the original and calculate a new source analysis before reuse.")));
        }
        Ok(())
    }
    pub fn check_captures(
        &self,
        captures: &[CaptureSnapshot],
        samples: &[Sample],
    ) -> Result<(), Error> {
        for c in captures {
            if samples.iter().any(|s| s.capture == c.id) {
                self.require_equal(&format!("capture-{}.txt", c.id.as_str()), &c.raw)?;
            }
        }
        Ok(())
    }
    pub fn check_context(&self, capture: &CaptureSnapshot) -> Result<(), Error> {
        self.check_context_at(capture, &format!("capture-{}", capture.id.as_str()))
    }
    pub fn check_capture(&self, capture: &CaptureSnapshot, prefix: &str) -> Result<(), Error> {
        let stem = format!("{prefix}capture-{}", capture.id.as_str());
        self.require_equal(&format!("{stem}.txt"), &capture.raw)?;
        self.check_context_at(capture, &stem)?;
        self.require_equal(
            &format!("{stem}.context.json"),
            capture.context.json(&capture.capture).as_bytes(),
        )
    }
    fn check_context_at(&self, capture: &CaptureSnapshot, stem: &str) -> Result<(), Error> {
        for (kind, bytes) in capture.context.snapshots() {
            let name = format!("{stem}.{kind}.txt");
            if self.files.get(&name).map(Vec::as_slice) != bytes {
                return Err(Error::Data(format!(
                    "The retained {name} snapshot differs from this setup's capture context. Select matching original revisions or recalculate the source analysis; current acquisition settings cannot substitute."
                )));
            }
        }
        Ok(())
    }
    pub fn copy_to(&self, output: &Path, prefix: &str) -> Result<(), Error> {
        for (name, bytes) in &self.files {
            save(&output.join(format!("{prefix}{name}")), bytes)?;
        }
        Ok(())
    }
    pub fn require_source(&self, source: &Self, prefix: &str) -> Result<(), Error> {
        let names = self
            .files
            .keys()
            .filter_map(|name| name.strip_prefix(prefix))
            .collect::<Vec<_>>();
        if names != source.files.keys().map(String::as_str).collect::<Vec<_>>() {
            return Err(Error::Data(format!("The retained {prefix} file set differs from the selected source analysis. Select matching revisions or recalculate the dependent analysis; no source files were added or discarded.")));
        }
        for (name, bytes) in &source.files {
            self.require_equal(&format!("{prefix}{name}"), bytes)?;
        }
        Ok(())
    }
}
