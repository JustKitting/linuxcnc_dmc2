use super::{
    Error,
    catalog::{self, Operation},
    exchange, files,
    model::Id,
    positional,
    store::Store,
};
use std::{
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
};

pub fn default_store() -> PathBuf {
    if let Ok(executable) = std::env::current_exe() {
        if let Some(bin) = executable.parent() {
            if let Some(native) = bin.parent() {
                if bin.file_name().and_then(|s| s.to_str()) == Some("bin")
                    && native.file_name().and_then(|s| s.to_str()) == Some("native")
                {
                    if let Some(project) = native.parent() {
                        return project.join("var/objects");
                    }
                }
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../var/objects")
}
fn text(s: &OsString) -> Result<&str, Error> {
    s.to_str()
        .ok_or_else(|| Error::Input("Command, ID and label text must be UTF-8.".into()))
}
pub fn run(args: &[OsString], default_store: &Path) -> Result<String, Error> {
    let (root, args) = if args.first().and_then(|s| s.to_str()) == Some("--store") {
        let path = args
            .get(1)
            .ok_or_else(|| Error::Input("--store requires a directory.".into()))?;
        (PathBuf::from(path), &args[2..])
    } else {
        (default_store.into(), args)
    };
    let first = args.first().map(text).transpose()?.unwrap_or("--help");
    let a = &args[usize::from(!args.is_empty())..];
    if a.is_empty() {
        match first {
            "--help" | "-h" => return Ok(catalog::usage()),
            "catalog" => return Ok(catalog::json(&root)),
            _ => (),
        }
    }
    let spec = catalog::OPERATIONS
        .iter()
        .find(|s| s.name == first)
        .ok_or_else(|| Error::Input(format!("Unknown object-map command {first:?}.")))?;
    if a.len() != spec.fields.len() {
        return Err(Error::Input(format!(
            "{} needs {} arguments. {}",
            spec.name,
            spec.fields.len(),
            catalog::usage()
        )));
    }
    let store = Store { root };
    let id = |i| text(&a[i]).and_then(Id::parse);
    let path = |i| Path::new(&a[i]);
    use Operation::*;
    match spec.operation {
        List => store.list(),
        Show => store.show(&id(0)?),
        Create => {
            let object = id(0)?;
            store.create(&object, text(&a[1])?)?;
            store.show(&object)
        }
        AddSetup => {
            let object = id(0)?;
            store.add_setup(&object, &id(1)?, text(&a[2])?)?;
            store.show(&object)
        }
        Import => {
            let object = id(0)?;
            store.import(&object, &id(1)?, &id(2)?, path(3))?;
            store.show(&object)
        }
        AttachDesign => {
            let object = id(0)?;
            store.attach_design(&object, &id(1)?, path(2))?;
            store.show(&object)
        }
        ExportCaptures => exchange::export(&store, &id(0)?, &id(1)?, path(2)),
        Inspect => positional::inspect(path(0), text(&a[1])?),
        Prepare => positional::prepare(&store, &id(0)?, &id(1)?, &id(2)?),
        PrepareStock => positional::stock::prepare(&store, &id(0)?, &id(1)?, &id(2)?),
        FitStock => positional::stock::run(
            &store,
            &id(0)?,
            &id(1)?,
            &id(2)?,
            path(3),
            positional::stock::Model::Outline,
        ),
        PrepareSurface => positional::stock::prepare_surface(&store, &id(0)?, &id(1)?),
        FitSurface => positional::stock::run(
            &store,
            &id(0)?,
            &id(1)?,
            &id(2)?,
            path(3),
            positional::stock::Model::Surface,
        ),
        PrepareFootprint => {
            positional::stock::placement::prepare(&store, &id(0)?, &id(1)?, &id(2)?, &id(3)?)
        }
        FitFootprint => {
            positional::stock::placement::run(&store, &id(0)?, &id(1)?, &id(2)?, path(3))
        }
        PrepareMaterial => positional::stock::material::prepare(&store, &id(0)?, &id(1)?, &id(2)?),
        CheckMaterial => {
            positional::stock::material::run(&store, &id(0)?, &id(1)?, &id(2)?, path(3))
        }
        PrepareObservations => {
            positional::stock::observation::prepare(&store, &id(0)?, &id(1)?, &id(2)?)
        }
        PrepareSpatialObservations => positional::stock::observation::spatial::prepare(
            &store,
            &id(0)?,
            &id(1)?,
            &id(2)?,
            &id(3)?,
        ),
        PlanObservations => {
            positional::stock::observation::run(&store, &id(0)?, &id(1)?, &id(2)?, path(3))
        }
        ExportTopObservations => positional::stock::observation::spatial::export::run(
            &store, &id(0)?, &id(1)?, &id(2)?, path(3),
        ),
        PrepareStockMesh => {
            positional::stock::reconstruction::prepare(&store, &id(0)?, &id(1)?, &id(2)?)
        }
        ReconstructStockMesh => {
            positional::stock::reconstruction::run(&store, &id(0)?, &id(1)?, &id(2)?, path(3))
        }
        PrepareVolume => {
            positional::stock::volume::prepare(&store, &id(0)?, &id(1)?, &id(2)?, &id(3)?)
        }
        FitVolume => positional::stock::volume::run(&store, &id(0)?, &id(1)?, &id(2)?, path(3)),
        ExportStockScene => {
            positional::stock::scene::export(&store, &id(0)?, &id(1)?, &id(2)?, &id(3)?, path(4))
        }
        Fit => positional::run(&store, &id(0)?, &id(1)?, &id(2)?, path(3)),
        ShowFit => positional::show(&store, &id(0)?, &id(1)?, &id(2)?),
        ExportFit => positional::export(&store, &id(0)?, &id(1)?, &id(2)?, path(3)),
        Locate => positional::locate(&store, &id(0)?, &id(1)?, &id(2)?, path(3), path(4)),
        Browse => files::browse(path(0)),
        LoadRequest => files::load_request(path(0)),
        SaveRequest => {
            let mut raw = Vec::new();
            std::io::stdin()
                .read_to_end(&mut raw)
                .map_err(|e| Error::Input(format!("Reading request draft: {e}.")))?;
            files::save_request(path(0), &raw)
        }
    }
}
