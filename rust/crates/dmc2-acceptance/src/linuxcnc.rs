use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::failure::{Failure, FailureCode, Result};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(4);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ManualJogMode {
    AxisTeleop,
    JointFree,
}

impl ManualJogMode {
    const fn teleop_command(self) -> &'static str {
        match self {
            Self::AxisTeleop => "set teleop_enable on",
            Self::JointFree => "set teleop_enable off",
        }
    }
}

pub(crate) fn reserve_loopback_port() -> Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| {
        Failure::io(
            FailureCode::LinuxCncLaunch,
            "reserve linuxcncrsh loopback port",
            error,
        )
    })?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|error| {
            Failure::io(
                FailureCode::LinuxCncLaunch,
                "read reserved linuxcncrsh loopback port",
                error,
            )
        })
}

pub(crate) struct LinuxCncSession {
    child: Child,
    port: u16,
    log_path: PathBuf,
    rsh: Option<RshClient>,
}

impl LinuxCncSession {
    pub(crate) fn start(ini_path: &Path, run_directory: &Path, port: u16) -> Result<Self> {
        let log_path = run_directory.join("linuxcnc.log");
        let stdout = create_log(&log_path)?;
        let stderr = stdout.try_clone().map_err(|error| {
            Failure::io(
                FailureCode::LinuxCncLaunch,
                "clone LinuxCNC log descriptor",
                error,
            )
        })?;
        let child = Command::new("linuxcnc")
            .arg("-r")
            .arg(ini_path)
            .current_dir(run_directory)
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .map_err(|error| {
                Failure::io(
                    FailureCode::LinuxCncLaunch,
                    "launch isolated LinuxCNC acceptance machine",
                    error,
                )
            })?;
        Ok(Self {
            child,
            port,
            log_path,
            rsh: None,
        })
    }

    pub(crate) fn wait_until_ready(&mut self) -> Result<()> {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().map_err(|error| {
                Failure::io(
                    FailureCode::LinuxCncLaunch,
                    "poll LinuxCNC startup process",
                    error,
                )
            })? {
                return Err(Failure::new(
                    FailureCode::LinuxCncLaunch,
                    format!(
                        "LinuxCNC exited during startup with {status}; log-tail={:?}",
                        self.log_tail()
                    ),
                ));
            }
            match TcpStream::connect(("127.0.0.1", self.port)) {
                Ok(stream) => {
                    self.rsh = Some(RshClient::new(stream)?);
                    return Ok(());
                }
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
                Err(error) => {
                    return Err(Failure::new(
                        FailureCode::Timeout,
                        format!(
                            "linuxcncrsh did not accept connections on port {} within {:?}: {}; log-tail={:?}",
                            self.port,
                            STARTUP_TIMEOUT,
                            error,
                            self.log_tail()
                        ),
                    ))
                }
            }
        }
    }

    pub(crate) fn configure_manual_mode(&mut self, mode: ManualJogMode) -> Result<()> {
        let rsh = self.rsh.as_mut().ok_or_else(|| {
            Failure::new(
                FailureCode::LinuxCncProtocol,
                "linuxcncrsh connection was not established",
            )
        })?;
        rsh.command("hello EMC dmc2-motion-acceptance 1.0")?;
        rsh.command("set verbose on")?;
        rsh.command("set enable EMCTOO")?;
        rsh.command("set set_wait done")?;
        rsh.command("set estop off")?;
        rsh.command("set machine on")?;
        rsh.command("set mode manual")?;
        rsh.command(mode.teleop_command())?;
        Ok(())
    }

    pub(crate) fn select_manual_jog_mode(&mut self, mode: ManualJogMode) -> Result<()> {
        let rsh = self.rsh.as_mut().ok_or_else(|| {
            Failure::new(
                FailureCode::LinuxCncProtocol,
                "linuxcncrsh connection was not established",
            )
        })?;
        rsh.command("set mode manual")?;
        rsh.command(mode.teleop_command())?;
        Ok(())
    }

    pub(crate) fn home_joint(&mut self, joint: usize) -> Result<()> {
        let rsh = self.rsh.as_mut().ok_or_else(|| {
            Failure::new(
                FailureCode::LinuxCncProtocol,
                "linuxcncrsh connection was not established",
            )
        })?;
        rsh.command("set set_wait received")?;
        let home_result = rsh.command(&format!("set home {joint}"));
        let restore_result = rsh.command("set set_wait done");
        home_result.and(restore_result)
    }

    pub(crate) fn shutdown(&mut self) -> Result<()> {
        if let Some(rsh) = self.rsh.as_mut() {
            rsh.send_shutdown()?;
        }
        self.rsh = None;
        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().map_err(|error| {
                Failure::io(FailureCode::LinuxCncLaunch, "poll LinuxCNC shutdown", error)
            })? {
                if status.success() {
                    return Ok(());
                }
                return Err(Failure::new(
                    FailureCode::LinuxCncLaunch,
                    format!(
                        "LinuxCNC exited with {status}; log-tail={:?}",
                        self.log_tail()
                    ),
                ));
            }
            if Instant::now() >= deadline {
                return Err(Failure::new(
                    FailureCode::Timeout,
                    format!(
                        "LinuxCNC did not shut down within {:?}; log-tail={:?}",
                        SHUTDOWN_TIMEOUT,
                        self.log_tail()
                    ),
                ));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    pub(crate) fn log_tail(&self) -> String {
        let Ok(mut file) = File::open(&self.log_path) else {
            return "<log unavailable>".to_owned();
        };
        let mut text = String::new();
        if file.read_to_string(&mut text).is_err() {
            return "<log unreadable>".to_owned();
        }
        let lines = text.lines().collect::<Vec<_>>();
        lines[lines.len().saturating_sub(40)..].join("\n")
    }
}

impl Drop for LinuxCncSession {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            if let Some(rsh) = self.rsh.as_mut() {
                let _ = rsh.send_shutdown();
            }
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline {
                if self.child.try_wait().ok().flatten().is_some() {
                    return;
                }
                thread::sleep(Duration::from_millis(50));
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn create_log(path: &Path) -> Result<File> {
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| {
            Failure::io(
                FailureCode::LinuxCncLaunch,
                "create LinuxCNC acceptance log",
                error,
            )
        })
}

struct RshClient {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
}

impl RshClient {
    fn new(stream: TcpStream) -> Result<Self> {
        stream
            .set_write_timeout(Some(COMMAND_TIMEOUT))
            .map_err(|error| {
                Failure::io(
                    FailureCode::LinuxCncProtocol,
                    "set linuxcncrsh write timeout",
                    error,
                )
            })?;
        stream
            .set_read_timeout(Some(COMMAND_TIMEOUT))
            .map_err(|error| {
                Failure::io(
                    FailureCode::LinuxCncProtocol,
                    "set linuxcncrsh read timeout",
                    error,
                )
            })?;
        let reader_stream = stream.try_clone().map_err(|error| {
            Failure::io(
                FailureCode::LinuxCncProtocol,
                "clone linuxcncrsh stream",
                error,
            )
        })?;
        Ok(Self {
            writer: stream,
            reader: BufReader::new(reader_stream),
        })
    }

    fn command(&mut self, command: &str) -> Result<()> {
        self.writer
            .write_all(format!("{command}\n").as_bytes())
            .and_then(|_| self.writer.flush())
            .map_err(|error| {
                Failure::io(
                    FailureCode::LinuxCncProtocol,
                    "write linuxcncrsh command",
                    error,
                )
            })?;

        let mut responses = Vec::new();
        loop {
            let mut line = String::new();
            self.reader.read_line(&mut line).map_err(|error| {
                Failure::new(
                    FailureCode::LinuxCncProtocol,
                    format!(
                        "operation=read linuxcncrsh response; command={command:?}; responses={responses:?}; error={error}"
                    ),
                )
            })?;
            if line.is_empty() {
                return Err(Failure::new(
                    FailureCode::LinuxCncProtocol,
                    format!("connection closed while awaiting response to {command:?}"),
                ));
            }
            let normalized = line.trim().to_ascii_uppercase();
            responses.push(line.trim().to_owned());
            if normalized.contains("NAK") {
                return Err(Failure::new(
                    FailureCode::LinuxCncProtocol,
                    format!("command={command:?}; responses={responses:?}"),
                ));
            }
            if normalized.contains("ACK") {
                return Ok(());
            }
        }
    }

    fn send_shutdown(&mut self) -> Result<()> {
        self.writer
            .write_all(b"shutdown\n")
            .and_then(|_| self.writer.flush())
            .map_err(|error| {
                Failure::io(
                    FailureCode::LinuxCncProtocol,
                    "send linuxcncrsh shutdown",
                    error,
                )
            })
    }
}

pub(crate) fn hal_bool(pin: &str) -> Result<bool> {
    match hal_value(pin)?.to_ascii_lowercase().as_str() {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        value => Err(Failure::new(
            FailureCode::HalCommand,
            format!("pin={pin}; expected=bit; observed={value:?}"),
        )),
    }
}

pub(crate) fn hal_i32(pin: &str) -> Result<i32> {
    let value = hal_value(pin)?;
    value.parse::<i32>().map_err(|error| {
        Failure::new(
            FailureCode::HalCommand,
            format!("pin={pin}; expected=s32; observed={value:?}; error={error}"),
        )
    })
}

pub(crate) fn hal_u32(pin: &str) -> Result<u32> {
    let value = hal_value(pin)?;
    value.parse::<u32>().map_err(|error| {
        Failure::new(
            FailureCode::HalCommand,
            format!("pin={pin}; expected=u32; observed={value:?}; error={error}"),
        )
    })
}

pub(crate) fn hal_f64(pin: &str) -> Result<f64> {
    let value = hal_value(pin)?;
    value.parse::<f64>().map_err(|error| {
        Failure::new(
            FailureCode::HalCommand,
            format!("pin={pin}; expected=float; observed={value:?}; error={error}"),
        )
    })
}

pub(crate) fn set_hal_signal_bool(signal: &str, value: bool) -> Result<()> {
    let encoded = if value { "true" } else { "false" };
    let output = Command::new("halcmd")
        .args(["-s", "sets", signal, encoded])
        .output()
        .map_err(|error| Failure::io(FailureCode::HalCommand, "execute halcmd sets", error))?;
    if !output.status.success() {
        return Err(Failure::new(
            FailureCode::HalCommand,
            format!(
                "signal={signal}; requested={encoded}; exit={}; stdout={:?}; stderr={:?}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }
    Ok(())
}

fn hal_value(pin: &str) -> Result<String> {
    let output = Command::new("halcmd")
        .args(["-s", "getp", pin])
        .output()
        .map_err(|error| Failure::io(FailureCode::HalCommand, "execute halcmd getp", error))?;
    if !output.status.success() {
        return Err(Failure::new(
            FailureCode::HalCommand,
            format!(
                "pin={pin}; exit={}; stdout={:?}; stderr={:?}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub(crate) fn wait_for_bool(pin: &str, expected: bool, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let observed = hal_bool(pin)?;
        if observed == expected {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(Failure::new(
                FailureCode::Timeout,
                format!("pin={pin}; expected={expected}; observed={observed}; timeout={timeout:?}"),
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

pub(crate) fn wait_for_i32(pin: &str, expected: i32, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let observed = hal_i32(pin)?;
        if observed == expected {
            return Ok(());
        }
        if pin != "dmc2-pendant-control.fault-code" && hal_bool("dmc2-pendant-control.fault")? {
            return Err(Failure::new(
                FailureCode::MotionNotObserved,
                format!(
                    "pin={pin}; expected={expected}; observed={observed}; controller_fault={}",
                    controller_fault_evidence()?
                ),
            ));
        }
        if Instant::now() >= deadline {
            return Err(Failure::new(
                FailureCode::Timeout,
                format!("pin={pin}; expected={expected}; observed={observed}; timeout={timeout:?}"),
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn wait_for_u32_advance(
    pin: &str,
    start: u32,
    minimum_delta: u32,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let observed = hal_u32(pin)?;
        let delta = observed.wrapping_sub(start);
        if delta >= minimum_delta && delta <= i32::MAX as u32 {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(Failure::new(
                FailureCode::Timeout,
                format!(
                    "pin={pin}; start={start}; last={observed}; required_delta={minimum_delta}; timeout={timeout:?}"
                ),
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn wait_for_position_change(
    pin: &str,
    start: f64,
    minimum_change: f64,
    timeout: Duration,
) -> Result<f64> {
    let deadline = Instant::now() + timeout;
    loop {
        let observed = hal_f64(pin)?;
        if (observed - start).abs() >= minimum_change {
            return Ok(observed);
        }
        if hal_bool("dmc2-pendant-control.fault")? {
            return Err(Failure::new(
                FailureCode::MotionNotObserved,
                format!(
                    "pin={pin}; start={start:.9}; last={observed:.9}; controller_fault={}",
                    controller_fault_evidence()?
                ),
            ));
        }
        if Instant::now() >= deadline {
            return Err(Failure::new(
                FailureCode::MotionNotObserved,
                format!(
                    "pin={pin}; start={start:.9}; last={observed:.9}; required_change={minimum_change:.9}; timeout={timeout:?}"
                ),
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn controller_fault_evidence() -> Result<String> {
    const PINS: &[&str] = &[
        "dmc2-pendant-control.fault-code",
        "dmc2-pendant-control.fault-data-s00",
        "dmc2-pendant-control.fault-data-s01",
        "dmc2-pendant-control.fault-data-s02",
        "dmc2-pendant-control.fault-data-s03",
        "dmc2-pendant-control.fault-data-f00",
        "dmc2-pendant-control.fault-data-f01",
        "dmc2-pendant-control.fault-data-f02",
        "dmc2-pendant-control.fault-data-f03",
        "dmc2-pendant-control.fault-data-f04",
        "dmc2-pendant-control.fault-data-u06",
        "dmc2-pendant-control.fault-data-u07",
        "dmc2-pendant-control.fault-data-b35",
        "dmc2-pendant-control.fault-data-b36",
        "joint.0.motor-pos-cmd",
        "joint.0.motor-pos-fb",
        "joint.0.wheel-jog-active",
        "joint.0.in-position",
        "stepgen.1.counts",
    ];
    let mut evidence = Vec::with_capacity(PINS.len());
    for pin in PINS {
        evidence.push(format!("{pin}={}", hal_value(pin)?));
    }
    Ok(evidence.join(","))
}
