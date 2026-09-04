use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::{self, JoinHandle};

pub(super) struct PreparedCapture {
    report_path: PathBuf,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    stdout_log: File,
    stderr_log: File,
}

pub(super) struct RunningCapture {
    report_path: PathBuf,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    stdout_thread: JoinHandle<StreamOutcome>,
    stderr_thread: JoinHandle<StreamOutcome>,
}

#[derive(Debug, Default)]
struct StreamOutcome {
    read_error: Option<String>,
    log_error: Option<String>,
    forward_error: Option<String>,
}

impl PreparedCapture {
    pub(super) fn prepare(
        report_path: &Path,
        journal_path: &Path,
        role_name: &str,
        command: &mut Command,
    ) -> io::Result<Self> {
        remove_if_present(report_path)?;
        let log_directory = journal_path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("journal path has no parent: {}", journal_path.display()),
            )
        })?;
        let stdout_path = log_directory.join(format!("{role_name}.stdout.log"));
        let stderr_path = log_directory.join(format!("{role_name}.stderr.log"));
        let stdout_log = truncate_log(&stdout_path)?;
        let stderr_log = truncate_log(&stderr_path)?;
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        Ok(Self {
            report_path: report_path.to_path_buf(),
            stdout_path,
            stderr_path,
            stdout_log,
            stderr_log,
        })
    }

    pub(super) fn start(self, child: &mut Child) -> io::Result<RunningCapture> {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("configured LinuxCNC stdout pipe was not created"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("configured LinuxCNC stderr pipe was not created"))?;
        let stdout_thread = thread::Builder::new()
            .name("dmc2-linuxcnc-stdout".to_owned())
            .spawn(move || copy_stream(stdout, self.stdout_log, io::stdout()))?;
        let stderr_thread = thread::Builder::new()
            .name("dmc2-linuxcnc-stderr".to_owned())
            .spawn(move || copy_stream(stderr, self.stderr_log, io::stderr()))?;
        Ok(RunningCapture {
            report_path: self.report_path,
            stdout_path: self.stdout_path,
            stderr_path: self.stderr_path,
            stdout_thread,
            stderr_thread,
        })
    }
}

impl RunningCapture {
    pub(super) fn finish(
        self,
        status: ExitStatus,
        program: &OsStr,
        arguments: &[impl AsRef<OsStr>],
        session_started_ns: u128,
    ) -> io::Result<Option<PathBuf>> {
        let stdout = join_stream("stdout", self.stdout_thread)?;
        let stderr = join_stream("stderr", self.stderr_thread)?;
        if status.success() {
            return Ok(None);
        }

        let temporary_path = temporary_report_path(&self.report_path);
        let mut report = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary_path)?;
        writeln!(report, "DMC2 LINUXCNC SESSION FAILURE")?;
        writeln!(report, "identity: LINUXCNC_SESSION_EXITED_NONZERO")?;
        writeln!(report, "session-start-unix-ns: {session_started_ns}")?;
        writeln!(report, "exit-code: {}", optional_i32(status.code()))?;
        writeln!(report, "signal: {}", optional_i32(status.signal()))?;
        writeln!(report, "program: {}", program.to_string_lossy())?;
        write!(report, "arguments:")?;
        for argument in arguments {
            write!(report, " {:?}", argument.as_ref())?;
        }
        writeln!(report)?;
        write_outcome(&mut report, "stdout", &stdout)?;
        write_outcome(&mut report, "stderr", &stderr)?;
        copy_section(&mut report, "CAPTURED STDERR", &self.stderr_path)?;
        copy_section(&mut report, "CAPTURED STDOUT", &self.stdout_path)?;
        report.flush()?;
        report.sync_all()?;
        fs::rename(&temporary_path, &self.report_path)?;
        eprintln!(
            "DMC2 LINUXCNC SESSION FAILED: exit-code={} signal={}; full report: {}",
            optional_i32(status.code()),
            optional_i32(status.signal()),
            self.report_path.display()
        );
        Ok(Some(self.report_path))
    }
}

fn truncate_log(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn copy_stream(mut source: impl Read, mut log: File, mut forward: impl Write) -> StreamOutcome {
    let mut outcome = StreamOutcome::default();
    let mut buffer = [0_u8; 8192];
    loop {
        let count = match source.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) => {
                outcome.read_error = Some(error.to_string());
                break;
            }
        };
        if outcome.log_error.is_none() {
            if let Err(error) = log.write_all(&buffer[..count]) {
                outcome.log_error = Some(error.to_string());
            }
        }
        if outcome.forward_error.is_none() {
            if let Err(error) = forward.write_all(&buffer[..count]) {
                outcome.forward_error = Some(error.to_string());
            }
        }
    }
    if outcome.log_error.is_none() {
        if let Err(error) = log.flush().and_then(|()| log.sync_all()) {
            outcome.log_error = Some(error.to_string());
        }
    }
    if outcome.forward_error.is_none() {
        if let Err(error) = forward.flush() {
            outcome.forward_error = Some(error.to_string());
        }
    }
    outcome
}

fn join_stream(name: &str, thread: JoinHandle<StreamOutcome>) -> io::Result<StreamOutcome> {
    thread
        .join()
        .map_err(|_| io::Error::other(format!("{name} capture thread terminated unexpectedly")))
}

fn write_outcome(report: &mut File, name: &str, outcome: &StreamOutcome) -> io::Result<()> {
    writeln!(
        report,
        "{name}-capture-read-error: {}",
        outcome.read_error.as_deref().unwrap_or("NONE")
    )?;
    writeln!(
        report,
        "{name}-capture-log-error: {}",
        outcome.log_error.as_deref().unwrap_or("NONE")
    )?;
    writeln!(
        report,
        "{name}-capture-forward-error: {}",
        outcome.forward_error.as_deref().unwrap_or("NONE")
    )
}

fn copy_section(report: &mut File, title: &str, path: &Path) -> io::Result<()> {
    writeln!(report, "\n===== {title}: {} =====", path.display())?;
    let mut source = File::open(path)?;
    io::copy(&mut source, report)?;
    writeln!(report, "\n===== END {title} =====")
}

fn temporary_report_path(report_path: &Path) -> PathBuf {
    let mut path = report_path.as_os_str().to_os_string();
    path.push(format!(".partial-{}", std::process::id()));
    PathBuf::from(path)
}

fn optional_i32(value: Option<i32>) -> String {
    value.map_or_else(|| "NONE".to_owned(), |value| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    #[test]
    fn temporary_report_name_is_bound_to_the_supervisor_process() {
        let report = Path::new("/tmp/linuxcnc.report");
        assert_eq!(
            temporary_report_path(report),
            PathBuf::from(format!(
                "/tmp/linuxcnc.report.partial-{}",
                std::process::id()
            ))
        );
    }

    #[test]
    fn nonzero_status_is_not_treated_as_success() {
        assert!(!ExitStatus::from_raw(37 << 8).success());
    }
}
