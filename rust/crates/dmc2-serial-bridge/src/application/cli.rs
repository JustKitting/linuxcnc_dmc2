use std::env;

const DEFAULT_COMPONENT: &str = "dmc2-pendant";
const DEFAULT_PORT: &str = "/dev/ttyUSB0";
const DEFAULT_BAUD: u32 = 115_200;
pub(super) const DEFAULT_TIMEOUT_MS: u64 = 100;

#[derive(Debug)]
pub(super) struct Arguments {
    pub(super) component: String,
    pub(super) port: String,
    pub(super) baud: u32,
    pub(super) timeout_ms: u64,
    pub(super) validate: bool,
}

pub(super) fn arguments() -> Result<Arguments, String> {
    let mut result = Arguments {
        component: DEFAULT_COMPONENT.to_owned(),
        port: DEFAULT_PORT.to_owned(),
        baud: DEFAULT_BAUD,
        timeout_ms: DEFAULT_TIMEOUT_MS,
        validate: false,
    };
    let mut items = env::args().skip(1);
    while let Some(argument) = items.next() {
        let value = |items: &mut std::iter::Skip<std::env::Args>| {
            items
                .next()
                .ok_or_else(|| format!("{argument} requires a value"))
        };
        match argument.as_str() {
            "--component" => result.component = value(&mut items)?,
            "--port" => result.port = value(&mut items)?,
            "--baud" => {
                result.baud = value(&mut items)?
                    .parse()
                    .map_err(|_| "--baud must be an unsigned integer".to_owned())?;
            }
            "--packet-timeout-ms" => {
                result.timeout_ms = value(&mut items)?
                    .parse()
                    .map_err(|_| "--packet-timeout-ms must be an unsigned integer".to_owned())?;
            }
            "--validate" => result.validate = true,
            "--help" | "-h" => {
                println!(
                    "Usage: dmc2-serial-bridge [--component NAME] [--port PATH] \
                     [--baud 115200] [--packet-timeout-ms N] [--validate]"
                );
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    if result.timeout_ms == 0 {
        return Err("--packet-timeout-ms must be positive".to_owned());
    }
    if result.baud != DEFAULT_BAUD {
        return Err("this audited bridge accepts exactly 115200 baud".to_owned());
    }
    Ok(result)
}
