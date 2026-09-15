use super::{
    capture::Capture,
    capture_bundle::{self, Context},
    model::{named_json, DesignFormat, Id, Registration},
    record::{self, quote},
    Error,
};
use crate::probe_data::ledger;
use std::{
    fs,
    path::{Path, PathBuf},
};

const OBJECT: &str = "DMC2_OBJECT_V1";
const SETUP: &str = "DMC2_OBJECT_SETUP_V1";
const DESIGN: &str = "DMC2_OBJECT_DESIGN_V1";

pub struct Store {
    pub root: PathBuf,
}

pub fn read(path: &Path) -> Result<Vec<u8>, Error> {
    fs::read(path).map_err(|e| Error::Storage(format!("Reading {}: {e}.", path.display())))
}

pub fn save(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Storage("Output has no parent directory.".into()))?;
    ensure_directory(parent)?;
    ledger::publish(path, bytes).map_err(Error::Storage)
}

fn ensure_directory(path: &Path) -> Result<(), Error> {
    if path.as_os_str().is_empty() {
        return Ok(());
    }
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => Ok(()),
        Ok(_) => Err(Error::Storage(format!(
            "{} is not a normal directory.",
            path.display()
        ))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or_else(|| {
                Error::Storage(format!("{} has no parent directory.", path.display()))
            })?;
            ensure_directory(parent)?;
            match fs::create_dir(path) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    return ensure_directory(path)
                }
                Err(e) => return Err(Error::Storage(format!("Creating {}: {e}.", path.display()))),
            }
            sync_directory(if parent.as_os_str().is_empty() {
                Path::new(".")
            } else {
                parent
            })
        }
        Err(e) => Err(Error::Storage(format!(
            "Inspecting {}: {e}.",
            path.display()
        ))),
    }
}

pub fn sync_directory(path: &Path) -> Result<(), Error> {
    fs::File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(|e| Error::Storage(format!("Syncing directory {}: {e}.", path.display())))
}

fn ids_in(path: &Path, directories: bool) -> Result<Vec<Id>, Error> {
    let entries = match fs::read_dir(path) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::Storage(format!("Listing {}: {e}.", path.display()))),
    };
    let mut ids = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|e| Error::Storage(format!("Reading {} entry: {e}.", path.display())))?;
        let kind = entry.file_type().map_err(|e| {
            Error::Storage(format!("Reading {} type: {e}.", entry.path().display()))
        })?;
        if kind.is_symlink() {
            return Err(Error::Data(format!(
                "Object-store entry {} is a symbolic link.",
                entry.path().display()
            )));
        }
        let p = entry.path();
        let name = if directories && kind.is_dir() {
            p.file_name()
        } else if !directories
            && kind.is_file()
            && p.extension().and_then(|v| v.to_str()) == Some("dmc2")
        {
            p.file_stem()
        } else {
            continue;
        };
        ids.push(Id::parse(name.and_then(|v| v.to_str()).ok_or_else(
            || Error::Data("Object-store ID is not UTF-8.".into()),
        )?)?);
    }
    ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    Ok(ids)
}

fn named(path: &Path, schema: &str, expected: &Id) -> Result<String, Error> {
    let bytes = read(path)?;
    let (fields, payload) = record::decode(&bytes, schema, &["id", "label"])?;
    if fields["id"] != expected.as_str() || !payload.is_empty() {
        return Err(Error::Data(format!(
            "ID or payload disagrees with named record {}.",
            path.display()
        )));
    }
    Ok(fields["label"].clone())
}

pub struct CaptureSnapshot {
    pub id: Id,
    pub source_path: String,
    pub raw: Vec<u8>,
    pub capture: Capture,
    pub context: Context,
}

impl CaptureSnapshot {
    pub fn export(&self, path: &Path) -> Result<(), Error> {
        save(path, &self.raw)?;
        self.context.export(path, &self.capture)
    }
}

pub struct DesignSnapshot {
    pub id: Id,
    pub source_path: String,
    pub format: DesignFormat,
    pub raw: Vec<u8>,
}

impl DesignSnapshot {
    pub fn json(&self) -> String {
        format!("{{\"revision\":{},\"format\":{},\"source_path\":{},\"bytes\":{},\"geometry_inspected\":false}}", quote(self.id.as_str()), quote(self.format.name()), quote(&self.source_path), self.raw.len())
    }
}

impl Store {
    fn object_path(&self, id: &Id) -> PathBuf {
        self.root.join(id.as_str())
    }
    pub(super) fn setup_path(&self, object: &Id, setup: &Id) -> PathBuf {
        self.object_path(object).join("setups").join(setup.as_str())
    }

    pub fn object_label(&self, id: &Id) -> Result<String, Error> {
        named(&self.object_path(id).join("object.dmc2"), OBJECT, id)
    }
    pub fn setup_label(&self, object: &Id, setup: &Id) -> Result<String, Error> {
        self.object_label(object)?;
        named(
            &self.setup_path(object, setup).join("setup.dmc2"),
            SETUP,
            setup,
        )
    }

    pub fn create(&self, id: &Id, label: &str) -> Result<(), Error> {
        let bytes = record::encode(OBJECT, &[("id", id.as_str()), ("label", label)], &[])?;
        save(&self.object_path(id).join("object.dmc2"), &bytes)
    }

    pub fn add_setup(&self, object: &Id, id: &Id, label: &str) -> Result<(), Error> {
        self.object_label(object)?;
        let bytes = record::encode(SETUP, &[("id", id.as_str()), ("label", label)], &[])?;
        save(&self.setup_path(object, id).join("setup.dmc2"), &bytes)
    }

    pub fn import(&self, object: &Id, setup: &Id, id: &Id, source: &Path) -> Result<(), Error> {
        self.setup_label(object, setup)?;
        let raw = read(source)?;
        let capture = Capture::read(
            std::str::from_utf8(&raw)
                .map_err(|e| Error::Data(format!("Capture is not UTF-8: {e}.")))?,
        )?;
        let source = source
            .canonicalize()
            .map_err(|e| Error::Storage(format!("Resolving {}: {e}.", source.display())))?;
        let context = Context::source(&source, capture.workflow)?;
        let source = source
            .to_str()
            .ok_or_else(|| Error::Input("Capture path must be UTF-8.".into()))?;
        let bytes = capture_bundle::encode(id, source, &raw, &context)?;
        save(
            &self
                .setup_path(object, setup)
                .join("captures")
                .join(format!("{}.dmc2", id.as_str())),
            &bytes,
        )
    }

    pub fn attach_design(&self, object: &Id, id: &Id, source: &Path) -> Result<(), Error> {
        self.object_label(object)?;
        let format = DesignFormat::parse_extension(
            source.extension().and_then(|s| s.to_str()).unwrap_or(""),
        )?;
        let raw = read(source)?;
        if raw.is_empty() {
            return Err(Error::Data("Design file is empty.".into()));
        }
        let source = source
            .canonicalize()
            .map_err(|e| Error::Storage(format!("Resolving design path: {e}.")))?;
        let source = source
            .to_str()
            .ok_or_else(|| Error::Input("Design path must be UTF-8.".into()))?;
        let bytes = record::encode(
            DESIGN,
            &[
                ("id", id.as_str()),
                ("source_path", source),
                ("format", format.name()),
            ],
            &raw,
        )?;
        save(
            &self
                .object_path(object)
                .join("designs")
                .join(format!("{}.dmc2", id.as_str())),
            &bytes,
        )
    }

    pub fn captures(&self, object: &Id, setup: &Id) -> Result<Vec<CaptureSnapshot>, Error> {
        self.setup_label(object, setup)?;
        let path = self.setup_path(object, setup).join("captures");
        ids_in(&path, false)?
            .into_iter()
            .map(|id| {
                let bytes = read(&path.join(format!("{}.dmc2", id.as_str())))?;
                let retained = capture_bundle::decode(&bytes, &id)?;
                let capture =
                    Capture::read(std::str::from_utf8(&retained.raw).map_err(|e| {
                        Error::Data(format!("Retained capture is not UTF-8: {e}."))
                    })?)?;
                Ok(CaptureSnapshot {
                    id,
                    source_path: retained.source,
                    raw: retained.raw,
                    context: retained.context,
                    capture,
                })
            })
            .collect()
    }

    pub fn designs(&self, object: &Id) -> Result<Vec<DesignSnapshot>, Error> {
        self.object_label(object)?;
        let path = self.object_path(object).join("designs");
        ids_in(&path, false)?
            .into_iter()
            .map(|id| {
                let bytes = read(&path.join(format!("{}.dmc2", id.as_str())))?;
                let (fields, raw) =
                    record::decode(&bytes, DESIGN, &["id", "source_path", "format"])?;
                if fields["id"] != id.as_str() || raw.is_empty() {
                    return Err(Error::Data(
                        "Design snapshot ID or payload is invalid.".into(),
                    ));
                }
                let format = DesignFormat::parse_extension(&fields["format"])?;
                Ok(DesignSnapshot {
                    id,
                    format,
                    source_path: fields["source_path"].clone(),
                    raw: raw.into(),
                })
            })
            .collect()
    }

    pub fn list(&self) -> Result<String, Error> {
        let objects = ids_in(&self.root, true)?
            .into_iter()
            .map(|id| Ok(format!("{{{}}}", named_json(&id, &self.object_label(&id)?))))
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(format!(
            "{{\"schema\":\"dmc2.objects.v1\",\"objects\":[{}]}}",
            objects.join(",")
        ))
    }

    pub fn show(&self, object: &Id) -> Result<String, Error> {
        let label = self.object_label(object)?;
        let mut setups = Vec::new();
        for id in ids_in(&self.object_path(object).join("setups"), true)? {
            let name = self.setup_label(object, &id)?;
            let captures = self
                .captures(object, &id)?
                .into_iter()
                .map(|c| {
                    format!(
                        "{{\"id\":{},\"source_path\":{},\"acquisition_context\":{},{}}}",
                        quote(c.id.as_str()),
                        quote(&c.source_path),
                        c.context.json(&c.capture),
                        c.capture.summary()
                    )
                })
                .collect::<Vec<_>>();
            setups.push(format!(
                "{{{},\"registration\":{},\"captures\":[{}],\"analysis_candidates\":[{}]}}",
                named_json(&id, &name),
                Registration::Unresolved.json(),
                captures.join(","),
                ids_in(&self.setup_path(object, &id).join("analyses"), true)?
                    .iter()
                    .map(|analysis| format!(
                        "{{\"id\":{},\"manifest_present\":{}}}",
                        quote(analysis.as_str()),
                        self.setup_path(object, &id)
                            .join("analyses")
                            .join(analysis.as_str())
                            .join("manifest.json")
                            .is_file()
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        let designs = self
            .designs(object)?
            .iter()
            .map(DesignSnapshot::json)
            .collect::<Vec<_>>();
        Ok(format!("{{\"schema\":\"dmc2.object.v1\",{},\"design_revisions\":[{}],\"setups\":[{}],\"reconstructed_stock\":null,\"cam_ready\":false}}", named_json(object, &label), designs.join(","), setups.join(",")))
    }
}
