//! Atomic ledger + original companion snapshots. Never consult current config.
use super::{capture::Capture, model::Id, record, store::save, Error};
use crate::probe_data::{
    mapper_settings::{data, policy, Mode, OutlinePolicy, Settings, PLATE_FIELDS},
    mapper_trace::{outline, state, Progress},
    schema::Workflow,
};
use std::{fs, path::Path};
const V1: &str = "DMC2_OBJECT_CAPTURE_V1";
const V2: &str = "DMC2_OBJECT_CAPTURE_V2";
const OLD_KEYS: &[&str] = &["id", "source_path"];
const KEYS: &[&str] = &[
    "id",
    "source_path",
    "ledger_bytes",
    "plate_bytes",
    "feeds_bytes",
    "outline_bytes",
];
#[derive(Clone, Copy)]
enum Kind {
    Plate,
    Feeds,
    Outline,
}
impl Kind {
    const ALL: [Self; 3] = [Self::Plate, Self::Feeds, Self::Outline];
    fn name(self) -> &'static str {
        match self {
            Self::Plate => "plate",
            Self::Feeds => "feeds",
            Self::Outline => "outline",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Self::Plate => "plate_bytes",
            Self::Feeds => "feeds_bytes",
            Self::Outline => "outline_bytes",
        }
    }
}
#[derive(Default)]
pub struct Context {
    parts: [Option<Vec<u8>>; 3],
}
impl Context {
    /// Original bytes, including explicit absence, for dependent planning.
    pub fn snapshots(&self) -> impl Iterator<Item = (&'static str, Option<&[u8]>)> {
        Kind::ALL
            .into_iter()
            .zip(&self.parts)
            .map(|(kind, bytes)| (kind.name(), bytes.as_deref()))
    }
    pub fn source(path: &Path, workflow: Workflow) -> Result<Self, Error> {
        let mut result = Self::default();
        if workflow == Workflow::Mapper {
            for (i, kind) in Kind::ALL.into_iter().enumerate() {
                let path = path.with_extension(format!("{}.txt", kind.name()));
                result.parts[i] = match fs::read(&path) {
                    Ok(bytes) => Some(bytes),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    Err(e) => {
                        return Err(Error::Storage(format!(
                            "Reading original {} snapshot {}: {e}.",
                            kind.name(),
                            path.display()
                        )))
                    }
                };
            }
        }
        Ok(result)
    }
    fn text(&self, kind: Kind) -> Result<&str, String> {
        let bytes=self.parts[kind as usize].as_deref().ok_or_else(||format!("The original {} snapshot was not retained. Import the original ledger with its companion files under a new capture ID; current configuration cannot substitute.",kind.name()))?;
        std::str::from_utf8(bytes).map_err(|e|format!("The retained {} snapshot is not UTF-8: {e}. Preserve the source and import an intact run under a new ID.",kind.name()))
    }
    pub fn settings(&self, capture: &Capture) -> Result<Settings, String> {
        if capture.workflow != Workflow::Mapper {
            return Err("This capture does not use mapper companion settings.".into());
        }
        let start = &capture.records[0];
        let plate = data(
            self.text(Kind::Plate)?,
            "DMC2_PLATE_ENVELOPE_V1",
            PLATE_FIELDS,
        )?;
        let feeds = policy(self.text(Kind::Feeds)?)?;
        let outline =
            if Mode::read(crate::probe_data::ledger::number(start, "mode")?)? == Mode::Outline {
                Some(OutlinePolicy::read(self.text(Kind::Outline)?)?)
            } else {
                None
            };
        let settings = Settings::read(start, &plate, &feeds, outline)?;
        if settings
            .min
            .iter()
            .chain(&settings.max)
            .any(|v| !v.is_finite())
        {
            return Err("The retained run's derived bounds overflow. Inspect its source units and reference translation; no substitute bounds were inferred.".into());
        }
        Ok(settings)
    }
    pub fn json(&self, capture: &Capture) -> String {
        let parts = Kind::ALL
            .iter()
            .zip(&self.parts)
            .map(|(kind, bytes)| {
                format!(
                    "{{\"kind\":{},\"retained\":{},\"bytes\":{}}}",
                    record::quote(kind.name()),
                    bytes.is_some(),
                    bytes
                        .as_ref()
                        .map(|b| b.len().to_string())
                        .unwrap_or_else(|| "null".into())
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let mut selection = String::from("null");
        let (settings, issue) = if capture.workflow != Workflow::Mapper {
            (String::from("null"), String::from("null"))
        } else {
            match self.settings(capture) {
                Ok(s) => {
                    if s.mode == Mode::Outline {
                        selection = match state::samples(&capture.records,&s,false) {
                            Ok(samples)=>{
                                let traced=outline::run(&s,&samples);
                                let issue=match &traced.result {
                                    Ok(())=>String::from("null"),
                                    Err(Progress::Need(_))=>record::quote("The outline is partial; only selected retained contacts are ordered. Resume measurement through an operator-approved run."),
                                    Err(Progress::Invalid(e))=>record::quote(e),
                                };
                                let refinements=traced.refinements.iter().map(outline::Refinement::json).collect::<Vec<_>>().join(",");
                                format!("{{\"selected_original_sequences\":{:?},\"refinements\":[{refinements}],\"issue\":{issue},\"interpretation\":\"Selected contour order from this capture's retained policy; sampling decisions do not establish unsampled material or a solid volume.\"}}",traced.sequences)
                            }
                            Err(e)=>format!("{{\"selected_original_sequences\":null,\"refinements\":null,\"issue\":{}}}",record::quote(&e)),
                        };
                    }
                    let mode = match s.mode {
                        Mode::Outline => "outline",
                        Mode::Rim => "rim",
                        Mode::Surface => "surface",
                        Mode::FreeSurface => "free-surface",
                    };
                    (format!("{{\"mode\":{},\"frame\":\"retained work coordinates in mm\",\"work_to_machine_translation_mm\":{:?},\"bounded_min_mm\":{:?},\"bounded_max_mm\":{:?},\"local_spacing_mm\":{},\"resolution_mm\":{},\"nominal_ball_radius_mm\":{},\"horizontal_fine_travel_feeds_mm_min\":{:?},\"downward_feed_mm_min\":{}}}",record::quote(mode),s.offset,s.min,s.max,s.grid,s.resolution,s.radius,s.feeds,s.downward_feed),String::from("null"))
                }
                Err(e) => (String::from("null"), record::quote(&e)),
            }
        };
        format!("{{\"schema\":\"dmc2.capture-context.v1\",\"snapshots\":[{parts}],\"settings\":{settings},\"outline_selection\":{selection},\"issue\":{issue},\"interpretation\":\"Recorded acquisition settings, not calibration or current machine state. Companion files are retained as found beside the source ledger; legacy records have no inferred companions.\"}}")
    }
    pub fn export(&self, ledger: &Path, capture: &Capture) -> Result<(), Error> {
        for (kind, bytes) in Kind::ALL.iter().zip(&self.parts) {
            if let Some(bytes) = bytes {
                save(
                    &ledger.with_extension(format!("{}.txt", kind.name())),
                    bytes,
                )?;
            }
        }
        save(
            &ledger.with_extension("context.json"),
            self.json(capture).as_bytes(),
        )
    }
}
pub fn encode(id: &Id, source: &str, raw: &[u8], context: &Context) -> Result<Vec<u8>, Error> {
    let mut fields = vec![
        ("id", id.as_str().to_string()),
        ("source_path", source.to_string()),
        ("ledger_bytes", raw.len().to_string()),
    ];
    let mut payload = raw.to_vec();
    for (kind, bytes) in Kind::ALL.iter().zip(&context.parts) {
        fields.push((
            kind.key(),
            bytes
                .as_ref()
                .map(|b| b.len().to_string())
                .unwrap_or_else(|| "absent".into()),
        ));
        if let Some(bytes) = bytes {
            payload.extend_from_slice(bytes);
        }
    }
    record::encode(
        V2,
        &fields
            .iter()
            .map(|(k, v)| (*k, v.as_str()))
            .collect::<Vec<_>>(),
        &payload,
    )
}
pub struct Decoded {
    pub source: String,
    pub raw: Vec<u8>,
    pub context: Context,
}
pub fn decode(bytes: &[u8], id: &Id) -> Result<Decoded, Error> {
    let old = bytes.starts_with(format!("{V1}\n").as_bytes());
    let (fields, body) = record::decode(
        bytes,
        if old { V1 } else { V2 },
        if old { OLD_KEYS } else { KEYS },
    )?;
    if fields["id"] != id.as_str() {
        return Err(Error::Data(
            "Capture snapshot ID disagrees with its filename.".into(),
        ));
    }
    if old {
        return Ok(Decoded {
            source: fields["source_path"].clone(),
            raw: body.into(),
            context: Context::default(),
        });
    }
    let mut remaining = body;
    let mut take = |key: &str| -> Result<Vec<u8>, Error> {
        let n=fields[key].parse::<usize>().map_err(|_|Error::Data(format!("Retained capture {key} is not a byte count. Import an intact bundle under a new ID.")))?;
        if n > remaining.len() {
            return Err(Error::Data(format!(
                "Retained capture is truncated at {key}. Import an intact bundle under a new ID."
            )));
        }
        let (part, rest) = remaining.split_at(n);
        remaining = rest;
        Ok(part.to_vec())
    };
    let raw = take("ledger_bytes")?;
    let mut context = Context::default();
    for (i, kind) in Kind::ALL.into_iter().enumerate() {
        if fields[kind.key()] != "absent" {
            context.parts[i] = Some(take(kind.key())?);
        }
    }
    if !remaining.is_empty() {
        return Err(Error::Data("Retained capture has bytes outside its declared ledger and companion snapshots. Import an intact bundle under a new ID.".into()));
    }
    Ok(Decoded {
        source: fields["source_path"].clone(),
        raw,
        context,
    })
}
#[cfg(test)]
mod tests;
