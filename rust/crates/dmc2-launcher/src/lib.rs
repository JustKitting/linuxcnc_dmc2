mod cli;
mod deployment;
mod embedded;
mod error;
mod hal_validation;
mod integrity;
mod launch;
mod layout;
mod owner;
mod platform;
mod validation;

use std::ffi::OsString;
use std::io::Write;

pub use error::Error;
pub use platform::RealPlatform;

pub fn run(
    platform: &dyn platform::Platform,
    arguments: &[OsString],
    output: &mut dyn Write,
) -> Result<i32, Error> {
    let cli::Command::Launch(mode) = cli::parse(arguments)? else {
        writeln!(output, "{}", cli::USAGE)
            .map_err(|error| Error::os("write launcher output", "/dev/stdout".into(), error))?;
        return Ok(0);
    };
    let layout = layout::discover(platform)?;
    let plan = launch::prepare(platform, &layout, mode)?;
    launch::execute(platform, plan, output)
}
