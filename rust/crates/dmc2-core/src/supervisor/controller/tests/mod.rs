use super::*;
use crate::pendant::{AxisSelector, MultiplierSelector};

mod faults;
mod jog;
mod limits;

pub(super) fn sample(sequence: u32, detent: i32, deadman: bool) -> PendantSample {
    PendantSample {
        sequence,
        quadrature_errors: 0,
        latest_detent: detent,
        axis: AxisSelector::X,
        multiplier: MultiplierSelector::X1,
        deadman_held: deadman,
        estop_pressed: false,
        selector_valid: true,
    }
}

fn ready_machine() -> MachineSnapshot {
    MachineSnapshot {
        machine_on: true,
        estopped: false,
        manual_mode: true,
        joint_mode: false,
        teleop_mode: true,
        interp_idle: true,
        homed: [true; 3],
        homing: [false; 3],
        axis_stopped: [true; 3],
    }
}

pub(super) fn inputs(packet: Option<PendantSample>) -> SupervisorInputs {
    SupervisorInputs {
        link: LinkSnapshot {
            connected: true,
            serial_fault: false,
            quadrature_fault: false,
            estop_pressed: packet.is_some_and(|value| value.estop_pressed),
        },
        packet,
        machine: ready_machine(),
        counts_by_motor: [1_000, 2_000, 3_000],
        position_feedback_by_motor: [1.0, 2.0, 3.0],
        raw_limits: [false; 3],
        safety_limits: [false; 3],
        pendant_mode_enabled: true,
        command_channel_ready: true,
    }
}

pub(super) fn arm(supervisor: &mut LinuxCncPendantSupervisor) {
    supervisor.update(1_000_000, inputs(Some(sample(1, 0, false))));
    supervisor.update(20_000_000, inputs(Some(sample(2, 0, true))));
}
