use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use dmc2_diagnostics::RecoveryDisplay;

const FAILURE_REPORT: &str = "/tmp/linuxcnc.report";

fn main() {
    let platform = dmc2_launcher::RealPlatform;
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    match dmc2_launcher::run(&platform, &arguments, &mut io::stdout().lock()) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            let message = format!(
                "DMC2 LIVE LAUNCH FAILURE\nLIVE LAUNCH REFUSED: {}\n",
                RecoveryDisplay(&error),
            );
            match write_failure_report(Path::new(FAILURE_REPORT), message.as_bytes()) {
                Ok(()) => eprintln!("{message}Full report: {FAILURE_REPORT}"),
                Err(report_error) => {
                    let report_error = dmc2_launcher::Error::os(
                        "write launch failure report",
                        PathBuf::from(FAILURE_REPORT),
                        report_error,
                    );
                    eprintln!(
                        "{message}FAILURE REPORT WRITE FAILED: {}",
                        RecoveryDisplay(&report_error),
                    );
                }
            }
            std::process::exit(2);
        }
    }
}

fn write_failure_report(path: &Path, contents: &[u8]) -> io::Result<()> {
    let temporary = temporary_report_path(path);
    let mut report = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)?;
    report.write_all(contents)?;
    report.flush()?;
    report.sync_all()?;
    fs::rename(temporary, path)
}

fn temporary_report_path(report: &Path) -> PathBuf {
    let mut path = report.as_os_str().to_os_string();
    path.push(format!(".launcher-partial-{}", std::process::id()));
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_report_is_replaced_atomically_with_readable_text() {
        let directory =
            std::env::temp_dir().join(format!("dmc2-launcher-report-test-{}", std::process::id()));
        fs::create_dir_all(&directory).expect("create report test directory");
        let report = directory.join("linuxcnc.report");
        fs::write(&report, b"stale\n").expect("seed stale report");
        write_failure_report(&report, b"HAL_PIN_SIGNAL_CONFLICT: machine.hal:27\n")
            .expect("write failure report");
        assert_eq!(
            fs::read(&report).expect("read failure report"),
            b"HAL_PIN_SIGNAL_CONFLICT: machine.hal:27\n"
        );
        fs::remove_dir_all(directory).expect("remove report test directory");
    }
}
