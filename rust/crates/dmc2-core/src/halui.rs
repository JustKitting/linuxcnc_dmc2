use crate::supervisor::{CommandEvent, JogCommand};

pub const HALUI_EDGE_HOLD_NS: u64 = 40_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum PulsePhase {
    Idle,
    Assert,
    Release,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HaluiCommandOutputs {
    pub axis_increment_plus: [bool; 3],
    pub axis_increment_minus: [bool; 3],
    pub joint_increment_plus: [bool; 3],
    pub joint_increment_minus: [bool; 3],
    pub axis_increment: [f64; 3],
    pub joint_increment: [f64; 3],
    pub axis_jog_speed: f64,
    pub joint_jog_speed: f64,
    pub jog_stop: bool,
    pub jog_stop_immediate: bool,
    pub phase: PulsePhase,
}

impl HaluiCommandOutputs {
    const fn safe(phase: PulsePhase) -> Self {
        Self {
            axis_increment_plus: [false; 3],
            axis_increment_minus: [false; 3],
            joint_increment_plus: [false; 3],
            joint_increment_minus: [false; 3],
            axis_increment: [0.0; 3],
            joint_increment: [0.0; 3],
            axis_jog_speed: 0.0,
            joint_jog_speed: 0.0,
            jog_stop: false,
            jog_stop_immediate: false,
            phase,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HaluiCommandSequencer {
    phase: PulsePhase,
    elapsed_ns: u64,
    command: Option<JogCommand>,
    jog_stop: bool,
    jog_stop_immediate: bool,
}

impl HaluiCommandSequencer {
    pub const fn new() -> Self {
        Self {
            phase: PulsePhase::Idle,
            elapsed_ns: 0,
            command: None,
            jog_stop: false,
            jog_stop_immediate: false,
        }
    }

    pub const fn ready(&self) -> bool {
        matches!(self.phase, PulsePhase::Idle) && !self.jog_stop && !self.jog_stop_immediate
    }

    pub fn advance(&mut self, period_ns: u64) {
        self.jog_stop = false;
        self.jog_stop_immediate = false;
        match self.phase {
            PulsePhase::Idle => {}
            PulsePhase::Assert | PulsePhase::Release => {
                self.elapsed_ns = self.elapsed_ns.saturating_add(period_ns);
                if self.elapsed_ns >= HALUI_EDGE_HOLD_NS {
                    self.elapsed_ns = 0;
                    self.phase = match self.phase {
                        PulsePhase::Assert => PulsePhase::Release,
                        PulsePhase::Release => {
                            self.command = None;
                            PulsePhase::Idle
                        }
                        PulsePhase::Idle => PulsePhase::Idle,
                    };
                }
            }
        }
    }

    pub fn accept(&mut self, event: CommandEvent) -> bool {
        match event {
            CommandEvent::JogIncrement(command) => {
                if !self.ready() {
                    return false;
                }
                self.command = Some(command);
                self.phase = PulsePhase::Assert;
                self.elapsed_ns = 0;
                true
            }
            CommandEvent::JogStop => {
                self.cancel_increment();
                self.jog_stop = true;
                true
            }
            CommandEvent::JogStopImmediate => {
                self.force_stop_immediate();
                true
            }
        }
    }

    pub fn force_stop_immediate(&mut self) {
        self.cancel_increment();
        self.jog_stop = true;
        self.jog_stop_immediate = true;
    }

    fn cancel_increment(&mut self) {
        self.phase = PulsePhase::Idle;
        self.elapsed_ns = 0;
        self.command = None;
    }

    pub fn outputs(&self, pulses_per_mm: i32) -> HaluiCommandOutputs {
        let mut outputs = HaluiCommandOutputs::safe(self.phase);
        outputs.jog_stop = self.jog_stop;
        outputs.jog_stop_immediate = self.jog_stop_immediate;
        let Some(command) = self.command else {
            return outputs;
        };
        let index = command.axis.index();
        let increment = command.signed_delta_pulses.abs() / pulses_per_mm as f64;
        if command.joint_jog {
            outputs.joint_increment[index] = increment;
            outputs.joint_jog_speed = command.speed_mm_per_minute as f64;
            if self.phase == PulsePhase::Assert {
                outputs.joint_increment_plus[index] = command.signed_delta_pulses > 0.0;
                outputs.joint_increment_minus[index] = command.signed_delta_pulses < 0.0;
            }
        } else {
            outputs.axis_increment[index] = increment;
            outputs.axis_jog_speed = command.speed_mm_per_minute as f64;
            if self.phase == PulsePhase::Assert {
                outputs.axis_increment_plus[index] = command.signed_delta_pulses > 0.0;
                outputs.axis_increment_minus[index] = command.signed_delta_pulses < 0.0;
            }
        }
        outputs
    }
}

impl Default for HaluiCommandSequencer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supervisor::JogCommand;
    use crate::Axis;

    fn increment(joint_jog: bool) -> CommandEvent {
        CommandEvent::JogIncrement(JogCommand {
            axis: Axis::X,
            joint_jog,
            signed_delta_pulses: -10.0,
            speed_mm_per_minute: 300,
        })
    }

    #[test]
    fn one_increment_has_one_bounded_rising_edge_and_a_full_low_interval() {
        let mut sequencer = HaluiCommandSequencer::new();
        assert!(sequencer.accept(increment(false)));
        let high = sequencer.outputs(1_000);
        assert!(high.axis_increment_minus[0]);
        assert_eq!(high.axis_increment[0], 0.01);
        assert_eq!(high.axis_jog_speed, 300.0);

        sequencer.advance(HALUI_EDGE_HOLD_NS - 1);
        assert!(sequencer.outputs(1_000).axis_increment_minus[0]);
        sequencer.advance(1);
        let low = sequencer.outputs(1_000);
        assert!(!low.axis_increment_minus[0]);
        assert!(!sequencer.ready());

        sequencer.advance(HALUI_EDGE_HOLD_NS);
        assert!(sequencer.ready());
        assert_eq!(sequencer.outputs(1_000).axis_increment[0], 0.0);
    }

    #[test]
    fn joint_and_axis_commands_can_never_assert_together() {
        let mut sequencer = HaluiCommandSequencer::new();
        assert!(sequencer.accept(increment(true)));
        let outputs = sequencer.outputs(1_000);
        assert!(outputs.joint_increment_minus[0]);
        assert_eq!(outputs.axis_increment_minus, [false; 3]);
    }

    #[test]
    fn stop_preempts_an_increment_and_is_emitted_for_one_update_cycle() {
        let mut sequencer = HaluiCommandSequencer::new();
        assert!(sequencer.accept(increment(false)));
        assert!(sequencer.accept(CommandEvent::JogStopImmediate));
        let stopped = sequencer.outputs(1_000);
        assert!(stopped.jog_stop);
        assert!(stopped.jog_stop_immediate);
        assert_eq!(stopped.axis_increment_minus, [false; 3]);

        sequencer.advance(1_000_000);
        let cleared = sequencer.outputs(1_000);
        assert!(!cleared.jog_stop);
        assert!(!cleared.jog_stop_immediate);
        assert!(sequencer.ready());
    }

    #[test]
    fn forced_immediate_stop_cannot_be_rejected_while_busy() {
        let mut sequencer = HaluiCommandSequencer::new();
        assert!(sequencer.accept(increment(false)));

        sequencer.force_stop_immediate();

        let stopped = sequencer.outputs(1_000);
        assert!(stopped.jog_stop);
        assert!(stopped.jog_stop_immediate);
        assert_eq!(stopped.axis_increment_minus, [false; 3]);
    }

    #[test]
    fn busy_channel_rejects_a_second_increment() {
        let mut sequencer = HaluiCommandSequencer::new();
        assert!(sequencer.accept(increment(false)));
        assert!(!sequencer.accept(increment(false)));
    }
}
