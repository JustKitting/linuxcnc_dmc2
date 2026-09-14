//! Observation worker inside the standard task monitor. Saving never blocks
//! the NML/heartbeat loop. AXIS owns the mode/record HAL requests directly.
mod presentation;
mod recording;
mod window;

use dmc2_hal_sys::{
    self as hal,
    probe_stream::{flag, Stream, DEPTH},
};
use recording::Recording;
use std::{
    collections::VecDeque,
    io,
    os::unix::{fs::PermissionsExt, net::UnixDatagram},
    path::PathBuf,
    ptr, thread,
    time::Duration,
};

hal::userspace_hal_pin_catalog! {
    pub(super) struct Pins;
    error = RegistrationError;
    register = new_pin;
    pins {
        ready: bit out => "ready";
        retry: u32 in => "retry-save";
        dropped: u32 in => "dropped-samples";
    }
    groups {}
}
unsafe fn initialize(pins: &Pins) {
    unsafe {
        pins.initialize_zero();
    }
}
hal::userspace_hal_component! {
    error pub(super) RegistrationError;
    register pub(super) new_pin;
    create pub(super) create_hal;
    pins Pins;
    initialize initialize;
}

struct Worker {
    pins: *mut Pins,
    stream: Stream,
    socket: UnixDatagram,
    output: PathBuf,
}
// Moved once into its sole owning thread; NML never accesses these pins/stream.
unsafe impl Send for Worker {}

pub(super) fn start() {
    if let Err(error) = start_inner() {
        eprintln!("Probe recording unavailable: {error}. Action: turn Probe Mode off; correct the named startup cause and relaunch DMC2. Clear Fault and Pendant Mode remain independent.");
    }
}

fn start_inner() -> Result<(), String> {
    let (component, pins) =
        unsafe { create_hal("dmc2-probe-session") }.map_err(|e| e.to_string())?;
    let stream = unsafe { Stream::attach(component) }.map_err(|e| e.to_string())?;
    let runtime =
        PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR is missing")?);
    let socket_path = runtime.join("dmc2-probe.sock");
    // Standard launcher owns one session. This is a stale IPC endpoint only.
    match std::fs::remove_file(&socket_path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    let socket = UnixDatagram::bind(&socket_path).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    socket.set_nonblocking(true).map_err(|e| e.to_string())?;
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let root = executable
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .ok_or("standard native/bin path unavailable")?;
    if !root.join("live/dmc2.ini").is_file() {
        return Err("probe worker must run through the standard native/bin application".into());
    }
    let output = root.join("tmp/output/probe");
    let worker = Worker {
        pins,
        stream,
        socket,
        output,
    };
    thread::Builder::new()
        .name("probe-recording".into())
        .spawn(move || worker.run())
        .map_err(|e| e.to_string())?;
    Ok(())
}

type Saved = Option<(Recording, io::Result<PathBuf>)>;

impl Worker {
    fn run(mut self) {
        unsafe {
            ptr::write_volatile((*self.pins).ready, true);
        }
        let mut window = window::Window::default();
        let mut pending: VecDeque<Recording> = VecDeque::new();
        let mut writer: Option<thread::JoinHandle<Saved>> = None;
        let mut touch_id = 0u64;
        let mut notices: VecDeque<(u64, String)> = VecDeque::new();
        let mut latest = String::new();
        let mut error = String::new();
        let mut saved = String::new();
        let mut retry = 0;
        let mut dropped = 0;
        let mut save_failed = false;
        loop {
            let new_dropped = unsafe { ptr::read_volatile((*self.pins).dropped) };
            if new_dropped != dropped {
                let missing = new_dropped.wrapping_sub(dropped);

                dropped = new_dropped;
                error = format!("CAPTURE FAILED — KEEP THE SETUP IN PLACE. {missing} servo samples were lost. Turn Record off to save the retained partial recording; start a new recording to recapture.");
            }
            for _ in 0..DEPTH {
                let Some(frame) = self.stream.read() else {
                    break;
                };
                let previous = window.previous;
                let transition = window.observe(frame);
                if transition.started && !save_failed {
                    error.clear();
                }
                if transition.memory_full {
                    error = "Recording memory is full. Retained data is still in memory; newer samples are marked missing. Turn Record off to save, then start a new recording.".into();
                }
                if let Some(record) = transition.finished {
                    pending.push_back(record);
                }
                if frame.has(flag::TOUCH) {
                    touch_id += 1;
                    latest = presentation::touch(frame, previous);
                    if notices.len() == DEPTH {
                        notices.pop_front();
                        error = "The display fell behind the touch stream; an older bubble was dropped. Recorded samples are independent. Read the latest touch below.".into();
                    }
                    notices.push_back((touch_id, latest.clone()));
                }
            }
            if writer.as_ref().is_some_and(|w| w.is_finished()) {
                if let Some(w) = writer.take() {
                    match w.join() {
                        Ok(Some((record, Ok(path)))) => {
                            saved = path.display().to_string();
                            save_failed = false;
                            if record.missing == 0 {
                                error.clear();
                            }
                        }
                        Ok(Some((record, Err(e)))) => {
                            error = format!("Save failed: {e}. Recording {} remains in memory. Restore output access/space and press Retry Save.", record.id);
                            pending.push_front(record);
                            save_failed = true;
                        }
                        Ok(None) | Err(_) => {
                            error = "Recording writer terminated unexpectedly; its recording could not be recovered. Recapture in a new recording. Pendant Mode and Clear Fault remain independent.".into();
                            save_failed = true;
                        }
                    }
                }
            }
            let retry_now = unsafe { ptr::read_volatile((*self.pins).retry) };
            if retry_now != retry {
                retry = retry_now;
                save_failed = false;
            }
            if writer.is_none() && !save_failed {
                if let Some(record) = pending.pop_front() {
                    let output = self.output.clone();
                    // Transfer the buffer only after spawning succeeds.
                    let (tx, rx) = std::sync::mpsc::sync_channel::<Recording>(1);
                    match thread::Builder::new()
                        .name("probe-save".into())
                        .spawn(move || {
                            let record = rx.recv().ok()?;
                            let result = record.save(&output);
                            Some((record, result))
                        }) {
                        Ok(handle) => {
                            match tx.send(record) {
                                Ok(()) => writer = Some(handle),
                                Err(e) => {
                                    pending.push_front(e.0);
                                    save_failed = true;
                                    error = "Save worker disconnected. Buffer retained; press Retry Save.".into();
                                }
                            }
                        }
                        Err(e) => {
                            pending.push_front(record);
                            save_failed = true;
                            error = format!(
                                "Cannot start save worker: {e}. Buffer retained; press Retry Save."
                            );
                        }
                    }
                }
            }
            let mut request = [0u8; 128];
            match self.socket.recv_from(&mut request) {
                Ok((length, address)) => {
                    let cursor = std::str::from_utf8(&request[..length]).ok().and_then(|s| s.strip_prefix("STATUS ")).and_then(|s| s.trim().parse::<u64>().ok());
                    if let Some(cursor) = cursor {
                        while notices.front().is_some_and(|(id, _)| *id <= cursor) { notices.pop_front(); }
                        let (id, bubble) = notices.front().map(|(id, s)| (*id, s.as_str())).unwrap_or((cursor, ""));
                        let state = if window.active.is_some() { "recording" } else if save_failed { "save_failed" } else if writer.is_some() || !pending.is_empty() { "saving" } else { "idle" };
                        let reply = format!("{{\"state\":{},\"touch_id\":{},\"bubble\":{},\"latest\":{},\"error\":{},\"saved\":{},\"samples\":{},\"touches\":{}}}", presentation::json(state), id, presentation::json(bubble), presentation::json(&latest), presentation::json(&error), presentation::json(&saved), window.active.as_ref().map_or(0, |r| r.frames.len()), window.active.as_ref().map_or(0, |r| r.touches));
                        if let Err(e) = self.socket.send_to_addr(reply.as_bytes(), &address) {
                            if !matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound) {
                                error = format!("Probe information delivery failed: {e}. Reopen Probe Mode; recordings remain retained.");
                            }
                        }
                    }
                },
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {},
                Err(e) => error = format!("Probe information channel failed: {e}. Use Probe Mode off; restore the UI connection before capturing again."),
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}
