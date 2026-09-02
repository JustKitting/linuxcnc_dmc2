use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::failure::{Failure, FailureCode, Result};

const PACKET_PERIOD: Duration = Duration::from_millis(20);
const BOOT_MARKER: &[u8] = b"BOOT,P3,MYST1474-001,MONITOR_ONLY\n";

pub(crate) struct PendantStream {
    controls: Arc<Mutex<PendantControls>>,
    requested_detent: Arc<AtomicI32>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<()>>>,
}

#[derive(Clone, Copy)]
struct PendantControls {
    axis: AxisSelection,
    multiplier: MultiplierSelection,
    deadman_held: bool,
    estop_pressed: bool,
}

impl PendantStream {
    pub(crate) fn start(mut master: File) -> Self {
        let controls = Arc::new(Mutex::new(PendantControls {
            axis: AxisSelection::X,
            multiplier: MultiplierSelection::X1,
            deadman_held: true,
            estop_pressed: false,
        }));
        let requested_detent = Arc::new(AtomicI32::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_controls = Arc::clone(&controls);
        let worker_detent = Arc::clone(&requested_detent);
        let worker_stop = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            master.write_all(BOOT_MARKER).map_err(|error| {
                Failure::io(FailureCode::PendantTransport, "write P3 boot marker", error)
            })?;
            let started = Instant::now();
            let mut next_packet = Instant::now();
            let mut sequence = 0_u32;
            let mut detent_count = 0_i32;
            let mut transition_count = 0_i32;
            while !worker_stop.load(Ordering::Acquire) {
                sequence = sequence.wrapping_add(1);
                let latest_detent = worker_detent.swap(0, Ordering::AcqRel);
                if latest_detent != 0 {
                    detent_count = detent_count.wrapping_add(latest_detent);
                    transition_count = transition_count.wrapping_add(4);
                }
                let milliseconds = started.elapsed().as_millis().min(u32::MAX as u128) as u32;
                let controls = *worker_controls.lock().map_err(|_| {
                    Failure::new(
                        FailureCode::PendantTransport,
                        "pendant control-state lock was poisoned",
                    )
                })?;
                let axis = controls.axis.label();
                let multiplier = controls.multiplier.label();
                let deadman_held = i32::from(controls.deadman_held);
                let estop_pressed = i32::from(controls.estop_pressed);
                let selector_valid = i32::from(controls.axis != AxisSelection::Off);
                let packet = format!(
                    "P3,{sequence},{milliseconds},{detent_count},{transition_count},0,{latest_detent},{axis},{multiplier},{deadman_held},{estop_pressed},{selector_valid}\n"
                );
                master.write_all(packet.as_bytes()).map_err(|error| {
                    Failure::io(FailureCode::PendantTransport, "write live P3 packet", error)
                })?;
                next_packet += PACKET_PERIOD;
                let now = Instant::now();
                if next_packet > now {
                    thread::sleep(next_packet - now);
                } else {
                    next_packet = now;
                }
            }
            Ok(())
        });
        Self {
            controls,
            requested_detent,
            stop,
            worker: Some(worker),
        }
    }

    pub(crate) fn select(
        &self,
        axis: AxisSelection,
        multiplier: MultiplierSelection,
    ) -> Result<()> {
        let mut controls = self.controls()?;
        controls.axis = axis;
        controls.multiplier = multiplier;
        Ok(())
    }

    pub(crate) fn set_controls(
        &self,
        axis: AxisSelection,
        multiplier: MultiplierSelection,
        deadman_held: bool,
        estop_pressed: bool,
    ) -> Result<()> {
        *self.controls()? = PendantControls {
            axis,
            multiplier,
            deadman_held,
            estop_pressed,
        };
        Ok(())
    }

    fn controls(&self) -> Result<MutexGuard<'_, PendantControls>> {
        self.controls.lock().map_err(|_| {
            Failure::new(
                FailureCode::PendantTransport,
                "pendant control-state lock was poisoned",
            )
        })
    }

    pub(crate) fn request_detent(&self, direction: i32) -> Result<()> {
        if direction != -1 && direction != 1 {
            return Err(Failure::new(
                FailureCode::PendantTransport,
                format!("detent direction must be -1 or 1; observed={direction}"),
            ));
        }
        self.requested_detent
            .compare_exchange(0, direction, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|pending| {
                Failure::new(
                    FailureCode::PendantTransport,
                    format!("a pendant detent is already pending: {pending}"),
                )
            })
    }

    pub(crate) fn finish(mut self) -> Result<()> {
        self.stop.store(true, Ordering::Release);
        join_worker(self.worker.take())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub(crate) enum AxisSelection {
    Off = -1,
    X = 0,
    Y = 1,
    Z = 2,
}

impl AxisSelection {
    fn label(self) -> &'static str {
        match self {
            Self::Off => "N",
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
        }
    }

    pub(crate) fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
            Self::Off => panic!("OFF has no motion axis index"),
        }
    }

    pub(crate) const fn clockwise_sign(self) -> i32 {
        match self {
            Self::X => -1,
            Self::Y | Self::Z => 1,
            Self::Off => panic!("OFF has no motion direction"),
        }
    }

    pub(crate) fn motor_index(self) -> usize {
        match self {
            Self::X => 1,
            Self::Y => 0,
            Self::Z => 2,
            Self::Off => panic!("OFF has no motor index"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub(crate) enum MultiplierSelection {
    Off = 0,
    X1 = 1,
    X10 = 10,
    X100 = 100,
}

impl MultiplierSelection {
    fn label(self) -> &'static str {
        match self {
            Self::Off => "N",
            Self::X1 => "X1",
            Self::X10 => "X10",
            Self::X100 => "X100",
        }
    }

    pub(crate) const fn pulses(self) -> i32 {
        match self {
            Self::X1 => 10,
            Self::X10 => 100,
            Self::X100 => 1000,
            Self::Off => panic!("OFF has no jog increment"),
        }
    }
}

impl Drop for PendantStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = join_worker(self.worker.take());
    }
}

fn join_worker(worker: Option<JoinHandle<Result<()>>>) -> Result<()> {
    let Some(worker) = worker else {
        return Ok(());
    };
    worker
        .join()
        .map_err(|_| Failure::new(FailureCode::PendantTransport, "P3 packet thread panicked"))?
}
