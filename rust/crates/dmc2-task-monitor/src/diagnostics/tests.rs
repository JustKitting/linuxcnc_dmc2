use std::collections::BTreeSet;

use super::*;
use crate::snapshot::{
    NativeSnapshot, RcsStatusSnapshot, SNAPSHOT_ABI_VERSION, SNAPSHOT_FIELDS,
    SNAPSHOT_LOGICAL_FIELD_COUNT,
};
use dmc2_linuxcnc_interface::{
    status_message_contract, CodeDomain, CANON_UNITS, CMS_STATUS, EMC_NML_MESSAGE_TYPE,
    INTERPRETER_RETURN, JOINT_TYPE, KINEMATICS_TYPE, MOTION_COMMAND, NML_ERROR, RCS_STATE,
    RCS_STATUS, SPINDLE_ORIENT_STATE, TASK_EXEC, TASK_INTERP, TASK_MODE, TASK_STATE, TRAJ_MODE,
};

fn code(domain: CodeDomain, name: &str) -> i32 {
    domain
        .codes
        .iter()
        .find(|entry| entry.name == name)
        .unwrap_or_else(|| panic!("missing generated code {name}"))
        .code
        .try_into()
        .expect("test code did not fit i32")
}

fn normal_snapshot() -> NativeSnapshot {
    let mut snapshot = NativeSnapshot::safe();
    snapshot.abi_version = SNAPSHOT_ABI_VERSION;
    snapshot.struct_size = core::mem::size_of::<NativeSnapshot>() as u32;
    let done = code(RCS_STATUS, "RCS_DONE");
    let state = code(RCS_STATE, "S0");
    let set_rcs = |rcs: &mut RcsStatusSnapshot, class_name: &str| {
        let contract = status_message_contract(class_name)
            .unwrap_or_else(|| panic!("missing status contract for {class_name}"));
        rcs.message_type = contract.message_type.try_into().unwrap();
        rcs.message_size = contract.message_size;
        rcs.command_type = -1;
        rcs.status = done;
        rcs.state = state;
    };
    set_rcs(&mut snapshot.top_rcs, "EMC_STAT");
    set_rcs(&mut snapshot.task.rcs, "EMC_TASK_STAT");
    set_rcs(&mut snapshot.motion_rcs, "EMC_MOTION_STAT");
    set_rcs(&mut snapshot.trajectory.rcs, "EMC_TRAJ_STAT");
    set_rcs(&mut snapshot.io.rcs, "EMC_IO_STAT");
    set_rcs(&mut snapshot.io.tool.rcs, "EMC_TOOL_STAT");
    set_rcs(&mut snapshot.io.aux.rcs, "EMC_AUX_STAT");
    set_rcs(&mut snapshot.io.coolant.rcs, "EMC_COOLANT_STAT");
    set_rcs(&mut snapshot.io.lube.rcs, "EMC_LUBE_STAT");
    snapshot.task.mode = code(TASK_MODE, "EMC_TASK_MODE_MANUAL");
    snapshot.task.state = code(TASK_STATE, "EMC_TASK_STATE_ESTOP");
    snapshot.task.exec_state = code(TASK_EXEC, "EMC_TASK_EXEC_DONE");
    snapshot.task.interp_state = code(TASK_INTERP, "EMC_TASK_INTERP_IDLE");
    snapshot.task.program_units = code(CANON_UNITS, "CANON_UNITS_MM");
    snapshot.task.interpreter_errcode = code(INTERPRETER_RETURN, "INTERP_OK");
    snapshot.trajectory.joints = 3;
    snapshot.trajectory.spindles = 1;
    snapshot.trajectory.axis_mask = 0b111;
    snapshot.trajectory.mode = code(TRAJ_MODE, "EMC_TRAJ_MODE_FREE");
    snapshot.trajectory.kinematics_type = code(KINEMATICS_TYPE, "KINEMATICS_IDENTITY");
    for index in 0..snapshot.joints.len() {
        set_rcs(&mut snapshot.joints[index].rcs, "EMC_JOINT_STAT");
        snapshot.joints[index].joint_type = code(JOINT_TYPE, "EMC_LINEAR");
    }
    for index in 0..snapshot.axes.len() {
        set_rcs(&mut snapshot.axes[index].rcs, "EMC_AXIS_STAT");
    }
    for index in 0..snapshot.spindles.len() {
        set_rcs(&mut snapshot.spindles[index].rcs, "EMC_SPINDLE_STAT");
        snapshot.spindles[index].orient_state = code(SPINDLE_ORIENT_STATE, "EMCMOT_ORIENT_NONE");
    }
    snapshot.io.aux.estop = 1;
    snapshot.io.lube.level = 1;
    snapshot
}

#[test]
fn known_normal_snapshot_has_no_diagnostics() {
    let report = evaluate(&normal_snapshot());
    assert_eq!(report, DiagnosticReport::default());
}

#[test]
fn every_snapshot_field_has_exactly_one_executed_diagnostic_policy() {
    let report = evaluate(&normal_snapshot());
    let expected = SNAPSHOT_FIELDS
        .iter()
        .map(|field| field.path)
        .collect::<BTreeSet<_>>();
    let actual = report
        .covered_fields()
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let missing = expected.difference(&actual).copied().collect::<Vec<_>>();
    let extra = actual.difference(&expected).copied().collect::<Vec<_>>();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "snapshot policy mismatch; missing={missing:?}; extra={extra:?}"
    );
    assert_eq!(expected.len(), SNAPSHOT_LOGICAL_FIELD_COUNT);
    assert_eq!(actual.len(), SNAPSHOT_LOGICAL_FIELD_COUNT);
    assert!(
        report
            .covered_fields()
            .values()
            .all(|policy| !policy.is_empty()),
        "snapshot field has an empty diagnostic policy"
    );
}

#[test]
fn every_rcs_error_source_is_classified() {
    type RcsSource = fn(&mut NativeSnapshot) -> &mut RcsStatusSnapshot;
    let sources: &[(RcsSource, u64)] = &[
        (|s| &mut s.top_rcs, category::TOP_RCS),
        (|s| &mut s.task.rcs, category::TASK_RCS),
        (|s| &mut s.motion_rcs, category::MOTION_RCS),
        (|s| &mut s.trajectory.rcs, category::TRAJECTORY_RCS),
        (|s| &mut s.joints[0].rcs, category::JOINT_RCS),
        (|s| &mut s.axes[0].rcs, category::AXIS_RCS),
        (|s| &mut s.spindles[0].rcs, category::SPINDLE_RCS),
        (|s| &mut s.io.rcs, category::IO_RCS),
        (|s| &mut s.io.tool.rcs, category::IO_RCS),
        (|s| &mut s.io.aux.rcs, category::IO_RCS),
        (|s| &mut s.io.coolant.rcs, category::IO_RCS),
        (|s| &mut s.io.lube.rcs, category::IO_RCS),
    ];
    for (select, expected) in sources {
        let mut snapshot = normal_snapshot();
        select(&mut snapshot).status = code(RCS_STATUS, "RCS_ERROR");
        let report = evaluate(&snapshot);
        assert_ne!(report.active_error_mask & expected, 0);
    }
}

#[test]
fn every_rcs_source_enforces_its_exact_message_type_and_size() {
    let sources: &[fn(&mut NativeSnapshot) -> &mut RcsStatusSnapshot] = &[
        |s| &mut s.top_rcs,
        |s| &mut s.task.rcs,
        |s| &mut s.motion_rcs,
        |s| &mut s.trajectory.rcs,
        |s| &mut s.joints[0].rcs,
        |s| &mut s.axes[0].rcs,
        |s| &mut s.spindles[0].rcs,
        |s| &mut s.io.rcs,
        |s| &mut s.io.tool.rcs,
        |s| &mut s.io.aux.rcs,
        |s| &mut s.io.coolant.rcs,
        |s| &mut s.io.lube.rcs,
    ];
    for select in sources {
        let mut wrong_type = normal_snapshot();
        select(&mut wrong_type).message_type = i32::MAX;
        assert_ne!(
            evaluate(&wrong_type).active_error_mask & category::STATUS_MESSAGE,
            0
        );

        let mut wrong_size = normal_snapshot();
        select(&mut wrong_size).message_size = i64::MAX;
        assert_ne!(
            evaluate(&wrong_size).active_error_mask & category::STATUS_MESSAGE,
            0
        );
    }
}

#[test]
fn every_unknown_checked_enum_is_reported_without_guessing() {
    let mut snapshot = normal_snapshot();
    snapshot.task.mode = i32::MAX;
    snapshot.task.state = i32::MAX;
    snapshot.task.exec_state = i32::MAX;
    snapshot.task.interp_state = i32::MAX;
    snapshot.task.program_units = i32::MAX;
    snapshot.task.interpreter_errcode = i32::MAX;
    snapshot.trajectory.mode = i32::MAX;
    snapshot.trajectory.kinematics_type = i32::MAX;
    snapshot.trajectory.motion_type = i32::MAX;
    snapshot.joints[0].joint_type = i32::MAX;
    snapshot.spindles[0].orient_state = i32::MAX;
    let report = evaluate(&snapshot);
    assert_eq!(report.unknown_code_count(), 11);
    assert!(report.unknown_code_active());
    assert_ne!(report.active_error_mask & category::UNKNOWN_CODE, 0);
    assert!(report
        .issues
        .iter()
        .all(|issue| { issue.category != category::UNKNOWN_CODE || issue.name.is_none() }));
}

#[test]
fn operational_fault_fields_are_all_covered() {
    let mut snapshot = normal_snapshot();
    snapshot.task.exec_state = code(TASK_EXEC, "EMC_TASK_EXEC_ERROR");
    snapshot.task.interpreter_errcode = code(INTERPRETER_RETURN, "INTERP_ERROR");
    snapshot.io.fault = 1;
    snapshot.io.reason = -22;
    snapshot.joints[0].fault = 1;
    snapshot.spindles[0].orient_state = code(SPINDLE_ORIENT_STATE, "EMCMOT_ORIENT_FAULTED");
    snapshot.spindles[0].orient_fault = 7;
    snapshot.misc_error[63] = 1;
    let report = evaluate(&snapshot);
    let expected = category::TASK_EXEC
        | category::INTERPRETER
        | category::IO_FAULT
        | category::JOINT_FAULT
        | category::SPINDLE_ORIENT
        | category::MISC_ERROR;
    assert_eq!(report.active_error_mask & expected, expected);
}

#[test]
fn expected_limit_and_timeout_states_are_warnings_not_errors() {
    let mut snapshot = normal_snapshot();
    snapshot.task.input_timeout = 1;
    snapshot.joints[0].min_hard_limit = 1;
    snapshot.joints[0].min_soft_limit = 1;
    let report = evaluate(&snapshot);
    let expected = category::INPUT_TIMEOUT | category::HARD_LIMIT | category::SOFT_LIMIT;
    assert_eq!(report.active_warning_mask & expected, expected);
    assert_eq!(report.active_error_mask & expected, 0);
}

#[test]
fn stale_error_payloads_are_not_treated_as_active_faults() {
    let mut snapshot = normal_snapshot();
    snapshot.task.interpreter_errcode = code(INTERPRETER_RETURN, "INTERP_ERROR");
    snapshot.spindles[0].orient_fault = 73;
    let report = evaluate(&snapshot);
    assert_eq!(report.active_error_mask & category::INTERPRETER, 0);
    assert_eq!(report.active_error_mask & category::SPINDLE_ORIENT, 0);
}

#[test]
fn spindle_orient_fault_is_preserved_as_an_open_signed_payload() {
    for payload in [i32::MIN, -73, 0, 73, i32::MAX] {
        let mut snapshot = normal_snapshot();
        snapshot.spindles[0].orient_state = code(SPINDLE_ORIENT_STATE, "EMCMOT_ORIENT_FAULTED");
        snapshot.spindles[0].orient_fault = payload;
        let report = evaluate(&snapshot);
        let issue = report
            .issues
            .iter()
            .find(|issue| issue.category == category::SPINDLE_ORIENT)
            .expect("faulted orient state omitted its payload");
        assert_eq!(issue.domain, "spindle_orient_fault_payload");
        assert_eq!(issue.value, i64::from(payload));
        assert_eq!(issue.name, None);
        assert!(!report.unknown_code_active());
    }
}

#[test]
fn unknown_nml_and_motion_command_echoes_are_both_reported() {
    let mut snapshot = normal_snapshot();
    snapshot.top_rcs.command_type = i32::MAX;
    snapshot.motion_rcs.command_type = i32::MAX;
    let report = evaluate(&snapshot);
    assert_eq!(report.unknown_code_count(), 2);
    assert!(report
        .issues
        .iter()
        .any(|issue| issue.domain == EMC_NML_MESSAGE_TYPE.name));
    assert!(report
        .issues
        .iter()
        .any(|issue| issue.domain == MOTION_COMMAND.name));
}

#[test]
fn disconnected_status_is_an_explicit_transport_error() {
    let report = disconnected(
        code(NML_ERROR, "NML_TIMED_OUT"),
        code(CMS_STATUS, "CMS_STATUS_NOT_SET"),
    );
    assert!(report.error_active());
    assert_ne!(report.active_error_mask & category::TRANSPORT, 0);
    assert!(!report.unknown_code_active());
}

#[test]
fn transition_logger_reports_assertions_and_clears_once() {
    let mut logger = TransitionLogger::default();
    let fault = disconnected(
        code(NML_ERROR, "NML_TIMED_OUT"),
        code(CMS_STATUS, "CMS_STATUS_NOT_SET"),
    );
    let first = logger.update(&fault);
    assert_eq!(first.count, 1);
    assert_eq!(first.latest_action, 1);
    assert_eq!(logger.update(&fault).count, 0);
    let cleared = logger.update(&DiagnosticReport::default());
    assert_eq!(cleared.count, 1);
    assert_eq!(cleared.latest_action, -1);
    assert_eq!(logger.update(&DiagnosticReport::default()).count, 0);
}

#[test]
fn every_nml_transport_error_code_has_its_exact_source_name() {
    for entry in NML_ERROR.codes {
        let report = disconnected(entry.code as i32, code(CMS_STATUS, "CMS_STATUS_NOT_SET"));
        let transport = report
            .issues
            .iter()
            .find(|issue| issue.category == category::TRANSPORT && issue.domain == NML_ERROR.name)
            .expect("transport issue was omitted");
        assert_eq!(transport.name, Some(entry.name));
        assert_eq!(transport.value, entry.code);
    }
}

#[test]
fn unknown_nml_transport_code_is_never_mislabeled() {
    let report = disconnected(i32::MAX, code(CMS_STATUS, "CMS_STATUS_NOT_SET"));
    assert!(report.unknown_code_active());
    assert!(report
        .issues
        .iter()
        .any(|issue| issue.category == category::UNKNOWN_CODE && issue.name.is_none()));
}

#[test]
fn every_cms_status_code_has_its_exact_source_backed_policy() {
    let snapshot = normal_snapshot();
    let nml_no_error = code(NML_ERROR, "NML_NO_ERROR");
    for entry in CMS_STATUS.codes {
        let report = evaluate_with_transport(&snapshot, nml_no_error, entry.code as i32);
        let cms_issues = report
            .issues
            .iter()
            .filter(|issue| issue.domain == CMS_STATUS.name)
            .collect::<Vec<_>>();
        if matches!(entry.name, "CMS_READ_OLD" | "CMS_READ_OK") {
            assert!(cms_issues.is_empty(), "{}", entry.name);
        } else {
            assert_eq!(cms_issues.len(), 1, "{}", entry.name);
            assert_eq!(cms_issues[0].category, category::TRANSPORT);
            assert_eq!(cms_issues[0].name, Some(entry.name));
            assert_eq!(cms_issues[0].value, entry.code);
        }
    }
}

#[test]
fn unopened_cms_status_is_only_accepted_while_disconnected() {
    let status_not_set = code(CMS_STATUS, "CMS_STATUS_NOT_SET");
    let nml_no_error = code(NML_ERROR, "NML_NO_ERROR");

    let live = evaluate_with_transport(&normal_snapshot(), nml_no_error, status_not_set);
    assert!(live.issues.iter().any(|issue| {
        issue.category == category::TRANSPORT
            && issue.domain == CMS_STATUS.name
            && issue.name == Some("CMS_STATUS_NOT_SET")
    }));

    let opening = disconnected(nml_no_error, status_not_set);
    assert!(!opening
        .issues
        .iter()
        .any(|issue| issue.domain == CMS_STATUS.name));
    assert!(opening
        .issues
        .iter()
        .any(|issue| { issue.category == category::TRANSPORT && issue.domain == NML_ERROR.name }));
}

#[test]
fn unknown_cms_transport_code_is_never_mislabeled() {
    let report = evaluate_with_transport(
        &normal_snapshot(),
        code(NML_ERROR, "NML_NO_ERROR"),
        i32::MAX,
    );
    let unknown = report
        .issues
        .iter()
        .find(|issue| issue.category == category::UNKNOWN_CODE && issue.domain == CMS_STATUS.name)
        .expect("unknown CMS status was omitted");
    assert_eq!(unknown.name, None);
    assert_eq!(unknown.value, i64::from(i32::MAX));
}
