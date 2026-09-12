use dmc2_hal_sys::probe_stream::{flag, Frame, DEPTH};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const MEMORY_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_FRAMES: usize = MEMORY_BYTES / std::mem::size_of::<Frame>();

pub struct Recording {
    pub id: String,
    pub frames: Vec<Frame>,
    pub missing: u64,
    pub touches: u64,
    pub start_unix: f64,
    pub start_servo: f64,
}

impl Recording {
    pub fn new(first: Frame, sequence: u64) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            id: format!("probe-{}-{}-{sequence}", now.as_nanos(), std::process::id()),
            frames: Vec::new(),
            missing: 0,
            touches: 0,
            start_unix: now.as_secs_f64(),
            start_servo: first.seconds,
        }
    }
    pub fn push(&mut self, frame: Frame) -> bool {
        self.touches += u64::from(frame.has(flag::TOUCH));
        if self.frames.len() >= MAX_FRAMES
            || (self.frames.len() == self.frames.capacity()
                && self
                    .frames
                    .try_reserve_exact(DEPTH.min(MAX_FRAMES - self.frames.len()))
                    .is_err())
        {
            self.missing += 1;
            return false;
        }
        self.frames.push(frame);
        true
    }
    fn headers(&self) -> Vec<String> {
        vec![
            "# DMC2_PROBE_RECORDING 1".into(),
            "# units=mm,mm/s; frame=LinuxCNC machine XYZ (trivkins joint.N.pos-fb); input=Mesa IN1 rising edge".into(),
            "# position_source=LinuxCNC reported stepgen feedback; radius_and_tool_offsets=NOT_APPLIED; X_positive=physical LEFT / LinuxCNC +X; X_negative=physical RIGHT / LinuxCNC -X".into(),
            format!("# unix_time_at_first_read={}; first_servo_seconds={}; wall_clock_is_reader_anchor_not_hardware_timestamp=true", self.start_unix, self.start_servo),
            "# servo_seconds=sum_of_nominal_servo_periods; feedback_velocity=consecutive_position_difference/nominal_period; input_and_position_are_servo_samples_not_a_hardware_trigger_latch".into(),
            format!("# frames={}; touches_observed={}; missing_samples_minimum={}; incomplete={}", self.frames.len(), self.touches, self.missing, self.missing != 0),
            "# flags: mode=1 record=2 contact=4 touch=8 deadman=16 enabled=32 manual=64 teleop=128 coord=256 idle=512 transport_bad=1024 controller_fault=2048 homing=4096 selected=8192 gap=16384 feedback_velocity_valid=32768 pendant=65536 all_homed=131072".into(),
            "event,servo_seconds,cycle,period_ns,flags,homed_mask,controller_phase,pendant_axis,pendant_multiplier,pendant_detents,x_mm,y_mm,z_mm,command_x_mm,command_y_mm,command_z_mm,command_vx_mm_s,command_vy_mm_s,command_vz_mm_s,feedback_vx_mm_s,feedback_vy_mm_s,feedback_vz_mm_s,position_valid,command_direction,feedback_direction".into(),
        ]
    }
    fn read_back(&self, path: &Path, headers: &[String]) -> io::Result<()> {
        let mut lines = BufReader::new(File::open(path)?).lines();
        for expected in headers
            .iter()
            .cloned()
            .chain(self.frames.iter().map(frame_csv))
        {
            if lines.next().transpose()?.as_ref() != Some(&expected) {
                return Err(io::Error::other("saved data differs from the exact retained recording; buffer retained for Retry Save"));
            }
        }
        if lines.next().transpose()?.is_some() {
            return Err(io::Error::other(
                "unexpected extra data in recording; existing file preserved",
            ));
        }
        Ok(())
    }
    /// Only the Record-off transition queues this work. No file is opened
    /// by push/new. Failures retain this exact buffer for visible Retry Save.
    pub fn save(&self, directory: &Path) -> io::Result<PathBuf> {
        fs::create_dir_all(directory)?;
        let path = directory.join(format!("{}.csv", self.id));
        let partial = directory.join(format!("{}.partial", self.id));
        let headers = self.headers();
        if !path.try_exists()? {
            let file = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&partial)?;
            let mut out = BufWriter::new(file);
            for line in headers
                .iter()
                .cloned()
                .chain(self.frames.iter().map(frame_csv))
            {
                writeln!(out, "{line}")?;
            }
            out.flush()?;
            out.get_ref().sync_all()?;
            drop(out);
            self.read_back(&partial, &headers)?;
            fs::hard_link(&partial, &path)?;
        }
        // A retry after publication/sync failure must not truncate either link.
        self.read_back(&path, &headers)?;
        match fs::remove_file(&partial) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        File::open(directory)?.sync_all()?;
        Ok(path)
    }
}

fn frame_csv(f: &Frame) -> String {
    let mut row = format!(
        "{},{},{},{},{},{},{},{},{},{},{}",
        if f.has(flag::TOUCH) {
            "touch"
        } else {
            "sample"
        },
        f.seconds,
        f.cycle,
        f.period_ns,
        f.flags,
        f.homed,
        f.phase,
        f.axis,
        f.multiplier,
        f.detents,
        f.position[0]
    );
    // Rust's shortest round-trip formatting retains each f64 exactly; fixed
    // decimal rounding is confined to the UI, never applied to saved samples.
    for value in f.position[1..]
        .iter()
        .chain(f.command_position.iter())
        .chain(f.command_velocity.iter())
        .chain(f.feedback_velocity.iter())
    {
        row.push(',');
        row.push_str(&value.to_string());
    }
    row.push(',');
    row.push_str(if f.valid_position() { "true" } else { "false" });
    row.push(',');
    row.push_str(&direction_csv(f.command_velocity));
    row.push(',');
    let feedback_direction = if f.has(flag::VELOCITY_VALID) {
        direction_csv(f.feedback_velocity)
    } else {
        "unavailable".into()
    };
    row.push_str(&feedback_direction);
    row
}

fn direction_csv(velocity: [f64; 3]) -> String {
    if velocity.iter().any(|v| !v.is_finite()) {
        return "unavailable".into();
    }
    let mut direction = String::new();
    for (axis, value) in ["X", "Y", "Z"].iter().zip(velocity) {
        if value == 0.0 {
            continue;
        }
        if !direction.is_empty() {
            direction.push(';');
        }
        direction.push_str(axis);
        direction.push(if value > 0.0 { '+' } else { '-' });
    }
    if direction.is_empty() {
        "stationary".into()
    } else {
        direction
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(sequence: u64) -> Recording {
        let frame = Frame {
            flags: flag::MODE | flag::RECORD | flag::TOUCH | flag::ALL_HOMED,
            homed: 0b111,
            position: [299.1234567890123, 160.0, 120.0],
            ..Frame::default()
        };
        let mut record = Recording::new(frame, sequence);
        record.push(frame);
        record
    }
    #[test]
    fn no_early_output_then_exact_roundtrip_and_idempotent_retry() {
        let r = record(1);
        let directory = std::env::temp_dir().join(&r.id);
        assert!(!directory.exists());
        let path = r.save(&directory).unwrap();
        let before = fs::read(&path).unwrap();
        assert_eq!(r.save(&directory).unwrap(), path);
        assert_eq!(fs::read(&path).unwrap(), before);
        let data = String::from_utf8(before).unwrap();
        let row = data.lines().find(|s| s.starts_with("touch,")).unwrap();
        let x: f64 = row.split(',').nth(10).unwrap().parse().unwrap();
        assert_eq!(x.to_bits(), r.frames[0].position[0].to_bits());
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn failed_save_retains_buffer_and_can_retry_after_output_is_restored() {
        let r = record(2);
        let directory = std::env::temp_dir().join(&r.id);
        fs::write(&directory, b"not a directory").unwrap();
        let original = r.frames.clone();
        assert!(r.save(&directory).is_err());
        assert_eq!(r.frames, original);
        fs::remove_file(&directory).unwrap();
        assert!(r.save(&directory).unwrap().is_file());
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn conflicting_finished_record_is_never_overwritten() {
        let r = record(3);
        let directory = std::env::temp_dir().join(&r.id);
        fs::create_dir(&directory).unwrap();
        let path = directory.join(format!("{}.csv", r.id));
        fs::write(&path, b"existing record").unwrap();
        assert!(r.save(&directory).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"existing record");
        fs::remove_dir_all(directory).unwrap();
    }
}
