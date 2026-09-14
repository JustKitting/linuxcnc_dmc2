use super::{exchange, model::Id, positional, store::Store, Error};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

pub const USAGE: &str = "Usage: dmc2ctl object-map [--store DIRECTORY] COMMAND\n\nCommands:\n  list\n  create OBJECT_ID LABEL\n  show OBJECT_ID\n  add-setup OBJECT_ID SETUP_ID LABEL\n  import-capture OBJECT_ID SETUP_ID CAPTURE_ID LEDGER\n  attach-design OBJECT_ID REVISION_ID FCSTD_STEP_OR_STL\n  export-freecad OBJECT_ID SETUP_ID NEW_DIRECTORY\n\nIDs use lowercase letters, digits, hyphens and underscores. Quote labels containing spaces.\nThe default store is var/objects beneath the DMC2 project.\nThese commands operate on retained files only. They issue no machine commands.\nCapture imports preserve original G38 ledgers; registration and CAM readiness remain unresolved.\n";

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
    // Development binaries are rooted at the same workspace, independent of cwd.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../var/objects")
}

#[derive(Debug)]
enum Command {
    Help,
    List,
    Create {
        object: Id,
        label: String,
    },
    Show {
        object: Id,
    },
    AddSetup {
        object: Id,
        setup: Id,
        label: String,
    },
    Import {
        object: Id,
        setup: Id,
        capture: Id,
        path: PathBuf,
    },
    AttachDesign {
        object: Id,
        revision: Id,
        path: PathBuf,
    },
    Export {
        object: Id,
        setup: Id,
        path: PathBuf,
    },
}

fn text(s: &OsString) -> Result<&str, Error> {
    s.to_str()
        .ok_or_else(|| Error::Input("Command, ID and label text must be UTF-8.".into()))
}

fn parse(args: &[OsString]) -> Result<Command, Error> {
    let first = args.first().map(text).transpose()?.unwrap_or("--help");
    let a = &args[usize::from(!args.is_empty())..];
    let id = |index| text(&a[index]).and_then(Id::parse);
    match (first, a.len()) {
        ("--help" | "-h", 0) => Ok(Command::Help),
        ("list", 0) => Ok(Command::List),
        ("create", 2) => Ok(Command::Create {
            object: id(0)?,
            label: text(&a[1])?.into(),
        }),
        ("show", 1) => Ok(Command::Show { object: id(0)? }),
        ("add-setup", 3) => Ok(Command::AddSetup {
            object: id(0)?,
            setup: id(1)?,
            label: text(&a[2])?.into(),
        }),
        ("import-capture", 4) => Ok(Command::Import {
            object: id(0)?,
            setup: id(1)?,
            capture: id(2)?,
            path: (&a[3]).into(),
        }),
        ("attach-design", 3) => Ok(Command::AttachDesign {
            object: id(0)?,
            revision: id(1)?,
            path: (&a[2]).into(),
        }),
        ("export-freecad", 3) => Ok(Command::Export {
            object: id(0)?,
            setup: id(1)?,
            path: (&a[2]).into(),
        }),
        _ => Err(Error::Input(format!(
            "Unknown object-map command or wrong argument count for {first:?}."
        ))),
    }
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
    let store = Store { root };
    if let Some(result) = positional::cli::dispatch(args, &store) {
        return result;
    }
    match parse(args)? {
        Command::Help => Ok(format!("{USAGE}{}", positional::cli::USAGE)),
        Command::List => store.list(),
        Command::Show { object } => store.show(&object),
        Command::Create { object, label } => {
            store.create(&object, &label)?;
            store.show(&object)
        }
        Command::AddSetup {
            object,
            setup,
            label,
        } => {
            store.add_setup(&object, &setup, &label)?;
            store.show(&object)
        }
        Command::Import {
            object,
            setup,
            capture,
            path,
        } => {
            store.import(&object, &setup, &capture, &path)?;
            store.show(&object)
        }
        Command::AttachDesign {
            object,
            revision,
            path,
        } => {
            store.attach_design(&object, &revision, &path)?;
            store.show(&object)
        }
        Command::Export {
            object,
            setup,
            path,
        } => exchange::export(&store, &object, &setup, &path),
    }
}
