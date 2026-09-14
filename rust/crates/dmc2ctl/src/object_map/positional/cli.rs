use super::{
    super::{model::Id, store::Store, Error},
    export, inspect, locate, prepare, run,
};
use std::{ffi::OsString, path::Path};
#[derive(Clone, Copy)]
enum Operation {
    Inspect,
    Prepare,
    Fit,
    Export,
    Locate,
}
const COMMANDS: &[(&str, Operation, usize)] = &[
    ("inspect-stl", Operation::Inspect, 2),
    ("prepare-fit", Operation::Prepare, 3),
    ("fit", Operation::Fit, 4),
    ("export-fit", Operation::Export, 4),
    ("locate", Operation::Locate, 5),
];
pub const USAGE:&str="\nPositional analysis (offline, no machine connection):\n  inspect-stl FILE MM_PER_FILE_UNIT\n  prepare-fit OBJECT SETUP STL_REVISION\n  fit OBJECT SETUP ANALYSIS REQUEST_FILE\n  export-fit OBJECT SETUP ANALYSIS NEW_DIRECTORY\n  locate OBJECT SETUP ANALYSIS MODEL_POINTS_CSV NEW_OUTPUT_CSV\n\nprepare-fit emits a request with explicit REQUIRED settings and fine-trigger references.\nfit retains source geometry, source ledgers, calibration inputs, candidate placement,\nall residuals, separate check points and stock-face dimensions. Results remain proposals.\nlocate produces coordinate CSV only; it never applies offsets or emits G-code.\n";
pub fn dispatch(args: &[OsString], store: &Store) -> Option<Result<String, Error>> {
    let command = args.first()?.to_str()?;
    let (_, operation, count) = *COMMANDS.iter().find(|(name, _, _)| *name == command)?;
    Some((|| {
        if args.len() != count + 1 {
            return Err(Error::Input(format!(
                "{command} needs {count} arguments. {USAGE}"
            )));
        }
        let text = |i: usize| {
            args[i]
                .to_str()
                .ok_or_else(|| Error::Input("IDs and numeric arguments must be UTF-8.".into()))
        };
        if matches!(operation, Operation::Inspect) {
            return inspect(Path::new(&args[1]), text(2)?);
        }
        let object = Id::parse(text(1)?)?;
        let setup = Id::parse(text(2)?)?;
        let id = Id::parse(text(3)?)?;
        match operation {
            Operation::Prepare => prepare(store, &object, &setup, &id),
            Operation::Fit => run(store, &object, &setup, &id, Path::new(&args[4])),
            Operation::Export => export(store, &object, &setup, &id, Path::new(&args[4])),
            Operation::Locate => locate(
                store,
                &object,
                &setup,
                &id,
                Path::new(&args[4]),
                Path::new(&args[5]),
            ),
            Operation::Inspect => unreachable!("handled before parsing object IDs"),
        }
    })())
}
