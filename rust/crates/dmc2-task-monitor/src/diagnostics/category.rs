//! Stable bit assignments published through the monitor's HAL interface.

pub const ABI: u64 = 1 << 0;
pub const TOP_RCS: u64 = 1 << 1;
pub const TASK_RCS: u64 = 1 << 2;
pub const MOTION_RCS: u64 = 1 << 3;
pub const TRAJECTORY_RCS: u64 = 1 << 4;
pub const JOINT_RCS: u64 = 1 << 5;
pub const AXIS_RCS: u64 = 1 << 6;
pub const SPINDLE_RCS: u64 = 1 << 7;
pub const IO_RCS: u64 = 1 << 8;
pub const TASK_EXEC: u64 = 1 << 9;
pub const INTERPRETER: u64 = 1 << 10;
pub const IO_FAULT: u64 = 1 << 11;
pub const JOINT_FAULT: u64 = 1 << 12;
pub const SPINDLE_ORIENT: u64 = 1 << 13;
pub const MISC_ERROR: u64 = 1 << 14;
pub const INPUT_TIMEOUT: u64 = 1 << 15;
pub const HARD_LIMIT: u64 = 1 << 16;
pub const SOFT_LIMIT: u64 = 1 << 17;
pub const INVALID_VALUE: u64 = 1 << 18;
pub const UNKNOWN_CODE: u64 = 1 << 19;
pub const TRANSPORT: u64 = 1 << 20;
pub const STATUS_MESSAGE: u64 = 1 << 21;
