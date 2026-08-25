use super::*;

#[test]
fn explicit_task_timeout_fault_cannot_leave_stale_status_accepted() {
    let mut supervisor = LinuxCncPendantSupervisor::new();
    arm(&mut supervisor);
    supervisor.fail(FaultCode::TaskHeartbeatTimeout);
    let output = supervisor.update(1_000_000, inputs(None));
    assert_eq!(output.fault, Some(FaultCode::TaskHeartbeatTimeout));
    assert!(!output.external_enable);
    assert!(!output.control_ready);
}
