//! Exactly one armed recording interval. It never opens an output file.
use super::recording::Recording;
use dmc2_hal_sys::probe_stream::{flag, Frame};

#[derive(Default)]
pub struct Window {
    pub previous: Option<Frame>,
    pub active: Option<Recording>,
    sequence: u64,
}

pub struct Transition {
    pub started: bool,
    pub memory_full: bool,
    pub finished: Option<Recording>,
}

impl Window {
    pub fn observe(&mut self, frame: Frame) -> Transition {
        let armed = frame.has(flag::MODE) && frame.has(flag::RECORD);
        let started = armed && self.active.is_none();
        if started {
            self.sequence += 1;
            self.active = Some(Recording::new(frame, self.sequence));
        }
        let mut memory_full = false;
        if let Some(r) = self.active.as_mut() {
            r.missing += self
                .previous
                .filter(|p| p.has(flag::RECORD) && p.has(flag::MODE))
                .map_or(u64::from(frame.has(flag::GAP)), |p| {
                    u64::from(frame.cycle.wrapping_sub(p.cycle).saturating_sub(1))
                });
            if armed {
                memory_full = !r.push(frame);
            }
        }
        self.previous = Some(frame);
        Transition {
            started,
            memory_full,
            finished: if armed { None } else { self.active.take() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(cycle: u32, flags: u32) -> Frame {
        Frame {
            cycle,
            flags,
            position: [299.0, 160.0, 120.0],
            homed: 0b111,
            ..Frame::default()
        }
    }

    #[test]
    fn bubbles_do_not_create_a_recording() {
        let mut window = Window::default();
        assert!(window
            .observe(sample(1, flag::MODE | flag::TOUCH))
            .finished
            .is_none());
        assert!(window.active.is_none());
        assert!(window.observe(sample(2, 0)).finished.is_none());
    }

    #[test]
    fn retains_every_armed_sample_and_excludes_touches_outside_the_window() {
        let mut window = Window::default();
        window.observe(sample(1, flag::MODE | flag::TOUCH));
        assert!(window.observe(sample(2, flag::MODE | flag::RECORD)).started);
        window.observe(sample(3, flag::MODE | flag::RECORD | flag::TOUCH));
        let record = window
            .observe(sample(4, flag::MODE | flag::TOUCH))
            .finished
            .unwrap();
        assert_eq!(
            record.frames.iter().map(|f| f.cycle).collect::<Vec<_>>(),
            [2, 3]
        );
        assert_eq!(record.touches, 1);
        assert_eq!(record.missing, 0);
        assert_eq!(record.frames[1].position, [299.0, 160.0, 120.0]);
    }

    #[test]
    fn mode_off_finishes_recording_even_if_record_request_remains_high() {
        let mut window = Window::default();
        window.observe(sample(1, flag::MODE | flag::RECORD));
        assert!(window.observe(sample(2, flag::RECORD)).finished.is_some());
        assert!(window.active.is_none());
        assert!(window.observe(sample(3, flag::RECORD)).finished.is_none());
    }

    #[test]
    fn missing_cycles_are_retained_on_stop_instead_of_claiming_a_complete_record() {
        let mut window = Window::default();
        window.observe(sample(1, flag::MODE | flag::RECORD));
        window.observe(sample(4, flag::MODE | flag::RECORD | flag::GAP));
        let record = window.observe(sample(7, 0)).finished.unwrap();
        assert_eq!(record.missing, 4);
    }
}
