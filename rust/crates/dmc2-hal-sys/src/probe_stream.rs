//! Servo-synchronous probe observations over LinuxCNC 2.9.10's SPSC HAL stream.
//! No file I/O, allocation, or machine command is performed by this module.
use core::{fmt, ptr};

use crate::{hal_stream_data as Data, hal_stream_t as RawStream};

// ASCII "DMCp": a dedicated key, distinct from LinuxCNC sampler/streamer keys.
const KEY: i32 = i32::from_be_bytes(*b"DMCp");
// 4095 usable frames, roughly four seconds at the configured 1 ms servo period.
pub const DEPTH: usize = 4096;
const TYPES: &[u8] = b"FUUUSSSSUFFFFFFFFFFFF\0";
const FIELDS: usize = TYPES.len() - 1;

pub mod flag {
    pub const MODE: u32 = 1 << 0;
    pub const RECORD: u32 = 1 << 1;
    pub const CONTACT: u32 = 1 << 2;
    pub const TOUCH: u32 = 1 << 3;
    pub const DEADMAN: u32 = 1 << 4;
    pub const ENABLED: u32 = 1 << 5;
    pub const MANUAL: u32 = 1 << 6;
    pub const TELEOP: u32 = 1 << 7;
    pub const COORD: u32 = 1 << 8;
    pub const IDLE: u32 = 1 << 9;
    pub const TRANSPORT_BAD: u32 = 1 << 10;
    pub const CONTROLLER_FAULT: u32 = 1 << 11;
    pub const HOMING: u32 = 1 << 12;
    pub const SELECTED: u32 = 1 << 13;
    pub const GAP: u32 = 1 << 14;
    pub const VELOCITY_VALID: u32 = 1 << 15;
    pub const PENDANT: u32 = 1 << 16;
    pub const ALL_HOMED: u32 = 1 << 17;
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    pub seconds: f64,
    pub cycle: u32,
    pub period_ns: u32,
    pub flags: u32,
    pub phase: i32,
    pub axis: i32,
    pub multiplier: i32,
    pub detents: i32,
    pub homed: u32,
    /// Homed joint XYZ from joint.N.pos-fb, equivalent to machine XYZ for trivkins.
    pub position: [f64; 3],
    pub command_position: [f64; 3],
    pub command_velocity: [f64; 3],
    /// Finite difference of consecutive feedback samples; never encoder accuracy.
    pub feedback_velocity: [f64; 3],
}

impl Frame {
    pub fn has(self, flag: u32) -> bool {
        self.flags & flag != 0
    }

    pub fn valid_position(self) -> bool {
        self.homed == 0b111
            && self.has(flag::ALL_HOMED)
            && !self.has(flag::TRANSPORT_BAD | flag::CONTROLLER_FAULT | flag::HOMING | flag::GAP)
            && self.position.iter().all(|v| v.is_finite())
    }

    fn encode(self) -> [Data; FIELDS] {
        let mut data = [Data { u: 0 }; FIELDS];
        data[0].f = self.seconds;
        data[1].u = self.cycle;
        data[2].u = self.period_ns;
        data[3].u = self.flags;
        data[4].s = self.phase;
        data[5].s = self.axis;
        data[6].s = self.multiplier;
        data[7].s = self.detents;
        data[8].u = self.homed;
        for (group, values) in [
            self.position,
            self.command_position,
            self.command_velocity,
            self.feedback_velocity,
        ]
        .iter()
        .enumerate()
        {
            for (axis, value) in values.iter().enumerate() {
                data[9 + group * 3 + axis].f = *value;
            }
        }
        data
    }

    unsafe fn decode(data: [Data; FIELDS]) -> Self {
        unsafe {
            Self {
                seconds: data[0].f,
                cycle: data[1].u,
                period_ns: data[2].u,
                flags: data[3].u,
                phase: data[4].s,
                axis: data[5].s,
                multiplier: data[6].s,
                detents: data[7].s,
                homed: data[8].u,
                position: [data[9].f, data[10].f, data[11].f],
                command_position: [data[12].f, data[13].f, data[14].f],
                command_velocity: [data[15].f, data[16].f, data[17].f],
                feedback_velocity: [data[18].f, data[19].f, data[20].f],
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamError {
    pub operation: &'static str,
    pub raw: i32,
}
impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "probe sample stream {} failed (LinuxCNC return {}); action: turn Probe Mode off, inspect the named stream failure, then relaunch the matched DMC2 application", self.operation, self.raw)
    }
}

pub struct Stream {
    raw: RawStream,
}
impl Stream {
    pub unsafe fn create(component: i32) -> Result<Self, StreamError> {
        let mut raw: RawStream = unsafe { core::mem::zeroed() };
        let result = unsafe {
            crate::hal_stream_create(
                &mut raw,
                component,
                KEY,
                DEPTH as i32,
                TYPES.as_ptr().cast(),
            )
        };
        if result < 0 {
            return Err(StreamError {
                operation: "create",
                raw: result,
            });
        }
        Ok(Self { raw })
    }
    pub unsafe fn attach(component: i32) -> Result<Self, StreamError> {
        let mut raw: RawStream = unsafe { core::mem::zeroed() };
        let result =
            unsafe { crate::hal_stream_attach(&mut raw, component, KEY, TYPES.as_ptr().cast()) };
        if result < 0 {
            return Err(StreamError {
                operation: "attach",
                raw: result,
            });
        }
        let count = unsafe { crate::hal_stream_element_count(&mut raw) };
        if count != FIELDS as i32 {
            unsafe {
                crate::hal_stream_detach(&mut raw);
            }
            return Err(StreamError {
                operation: "validate field count",
                raw: count,
            });
        }
        Ok(Self { raw })
    }
    pub fn write(&mut self, frame: Frame) -> bool {
        let mut data = frame.encode();
        unsafe { crate::hal_stream_write(&mut self.raw, data.as_mut_ptr()) == 0 }
    }
    pub fn read(&mut self) -> Option<Frame> {
        if !unsafe { crate::hal_stream_readable(&mut self.raw) } {
            return None;
        }
        let mut data = [Data { u: 0 }; FIELDS];
        let result =
            unsafe { crate::hal_stream_read(&mut self.raw, data.as_mut_ptr(), ptr::null_mut()) };
        if result < 0 {
            return None;
        } // The only negative result is empty (hal_lib.c).
        Some(unsafe { Frame::decode(data) })
    }
    pub unsafe fn detach(&mut self) -> Result<(), StreamError> {
        let result = unsafe { crate::hal_stream_detach(&mut self.raw) };
        if result < 0 {
            Err(StreamError {
                operation: "detach",
                raw: result,
            })
        } else {
            Ok(())
        }
    }
}
