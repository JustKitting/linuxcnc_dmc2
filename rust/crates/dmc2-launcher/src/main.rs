use std::ffi::OsString;
use std::io;

fn main() {
    let platform = dmc2_launcher::RealPlatform;
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    match dmc2_launcher::run(&platform, &arguments, &mut io::stdout().lock()) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("LIVE LAUNCH REFUSED: {error}");
            std::process::exit(2);
        }
    }
}
