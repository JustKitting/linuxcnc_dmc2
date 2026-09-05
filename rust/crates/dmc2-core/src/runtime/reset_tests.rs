use super::tests::{inputs, reach_committed_runtime};
use super::*;

#[test]
fn ui_fault_clear_never_reenters_cold_start_machine_on() {
    let mut controller = RuntimeController::new();
    let (sequence, heartbeat) = reach_committed_runtime(&mut controller);
    controller.fail(FaultCode::AdapterFailure);
    for cycle in 1..200 {
        let mut frame = inputs(sequence + cycle, heartbeat + cycle);
        frame.machine.machine_on = false;
        frame.machine.estopped = cycle < 60;
        frame.linuxcnc_estop_reset_request = cycle == 1;
        let output = controller.update(1_000_000, frame);
        assert!(output.supervisor.fault.is_none());
        assert!(!output.supervisor.machine_on_request);
        assert!(!matches!(
            output.supervisor.command,
            Some(crate::supervisor::CommandEvent::JogIncrement(_))
        ));
    }
    assert!(!controller.supervisor().outputs().recovery_active);
}

fn request_decoder_reset(controller: &mut RuntimeController) -> (RuntimeInputs, u32) {
    let (sequence, heartbeat) = reach_committed_runtime(controller);
    controller.fail(FaultCode::QuadratureFailure);
    let mut frame = inputs(sequence + 1, heartbeat + 1);
    frame.machine.machine_on = false;
    frame.machine.estopped = true;
    frame.pendant_quadrature_fault = true;
    frame.linuxcnc_estop_reset_request = true;
    let output = controller.update(1_000_000, frame);
    assert!(output.supervisor.fault.is_some());
    assert_ne!(output.pendant_fault_reset_request, 0);
    frame.linuxcnc_estop_reset_request = false;
    (frame, output.pendant_fault_reset_request)
}

#[test]
fn same_ui_reset_clears_controller_only_after_matching_healthy_bridge_ack() {
    let mut controller = RuntimeController::new();
    let (mut frame, request) = request_decoder_reset(&mut controller);
    frame.pendant_sample.sequence += 1;
    frame.task_heartbeat += 1;
    frame.pendant_fault_reset_ack = request;
    frame.pendant_coherent = false;
    assert!(controller
        .update(1_000_000, frame)
        .supervisor
        .fault
        .is_some());
    frame.pendant_coherent = true;
    frame.pendant_quadrature_fault = false;
    let output = controller.update(1_000_000, frame);
    assert!(output.supervisor.fault.is_none());
    assert_eq!(output.pendant_fault_reset_request, 0);
    assert!(!output.supervisor.machine_on_request);
    assert!(output.supervisor.recovery_active);
}

#[test]
fn expired_reset_cannot_clear_controller_on_late_bridge_ack() {
    let mut controller = RuntimeController::new();
    let (mut frame, request) = request_decoder_reset(&mut controller);
    for _ in 0..101 {
        frame.pendant_sample.sequence += 1;
        frame.task_heartbeat += 1;
        controller.update(1_000_000, frame);
    }
    frame.pendant_sample.sequence += 1;
    frame.task_heartbeat += 1;
    frame.pendant_fault_reset_ack = request;
    frame.pendant_quadrature_fault = false;
    let output = controller.update(1_000_000, frame);
    assert!(output.supervisor.fault.is_some());
    assert_eq!(output.pendant_fault_reset_request, 0);
    assert!(!output.supervisor.external_enable);
}
