use std::env;

const DEFAULT_COMPONENT: &str = "dmc2-task-monitor";
const DEFAULT_NML_FILE: &str = "/usr/share/linuxcnc/linuxcnc.nml";

#[derive(Debug)]
pub(super) struct Arguments {
    pub(super) component: String,
    pub(super) nml_file: String,
    pub(super) validate: bool,
}

pub(super) fn arguments() -> Result<Arguments, String> {
    let mut component = DEFAULT_COMPONENT.to_owned();
    let mut nml_file = env::var("EMC2_NMLFILE").unwrap_or_else(|_| DEFAULT_NML_FILE.to_owned());
    let mut validate = false;
    let mut items = env::args().skip(1);
    while let Some(argument) = items.next() {
        match argument.as_str() {
            "--component" => {
                component = items
                    .next()
                    .ok_or_else(|| "--component requires a value".to_owned())?;
            }
            "--nml-file" => {
                nml_file = items
                    .next()
                    .ok_or_else(|| "--nml-file requires a value".to_owned())?;
            }
            "--validate" => validate = true,
            "--help" | "-h" => {
                println!(
                    "Usage: dmc2-task-monitor [--component NAME] [--nml-file PATH] [--validate]"
                );
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(Arguments {
        component,
        nml_file,
        validate,
    })
}
