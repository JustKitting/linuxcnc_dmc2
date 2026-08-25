//! Task-controller and interpreter status validation.

use dmc2_linuxcnc_interface::{
    CANON_UNITS, INTERPRETER_RETURN, RCS_STATUS, TASK_EXEC, TASK_INTERP, TASK_MODE, TASK_STATE,
};

use crate::snapshot::NativeSnapshot;

use super::super::catalog::{check_code, check_i32_set, domain_id, issue};
use super::super::category;
use super::super::report::{DiagnosticReport, Severity};
use super::super::validation::{
    account_open, check_bounded_c_bytes, check_finite, check_finite_f64_array, check_i32_range,
    check_pose, check_u32_flag,
};

pub(super) fn evaluate(snapshot: &NativeSnapshot, report: &mut DiagnosticReport) {
    account_open(report, "task.heartbeat", "monotonic_heartbeat");
    check_code(
        report,
        "task.mode",
        TASK_MODE,
        i64::from(snapshot.task.mode),
    );
    check_code(
        report,
        "task.state",
        TASK_STATE,
        i64::from(snapshot.task.state),
    );
    let exec_name = check_code(
        report,
        "task.exec_state",
        TASK_EXEC,
        i64::from(snapshot.task.exec_state),
    );
    if exec_name == Some("EMC_TASK_EXEC_ERROR") {
        issue(
            report,
            Severity::Error,
            category::TASK_EXEC,
            "task.exec_state",
            TASK_EXEC.name,
            domain_id(TASK_EXEC),
            i64::from(snapshot.task.exec_state),
            exec_name,
            "LinuxCNC task executor reported its explicit error state",
        );
    }
    check_code(
        report,
        "task.interp_state",
        TASK_INTERP,
        i64::from(snapshot.task.interp_state),
    );
    check_i32_range(
        report,
        "task.call_level",
        snapshot.task.call_level,
        0,
        i32::MAX,
        "nonnegative_count",
        "task interpreter call depth is negative",
    );
    for (source, _) in [
        ("task.motion_line", snapshot.task.motion_line),
        ("task.current_line", snapshot.task.current_line),
        ("task.read_line", snapshot.task.read_line),
    ] {
        account_open(report, source, "open_line_number");
    }
    check_u32_flag(
        report,
        "task.optional_stop_state",
        snapshot.task.optional_stop_state,
        "optional-stop state is neither false nor true",
    );
    check_u32_flag(
        report,
        "task.block_delete_state",
        snapshot.task.block_delete_state,
        "block-delete state is neither false nor true",
    );
    check_u32_flag(
        report,
        "task.input_timeout",
        snapshot.task.input_timeout,
        "input-timeout state is neither false nor true",
    );
    check_bounded_c_bytes(report, "task.file", &snapshot.task.file);
    check_bounded_c_bytes(report, "task.command", &snapshot.task.command);
    check_bounded_c_bytes(report, "task.ini_filename", &snapshot.task.ini_filename);
    check_pose(report, "task.g5x_offset", snapshot.task.g5x_offset);
    account_open(report, "task.g5x_index", "open_coordinate_system_index");
    check_pose(report, "task.g92_offset", snapshot.task.g92_offset);
    check_finite(report, "task.rotation_xy", snapshot.task.rotation_xy);
    check_pose(report, "task.tool_offset", snapshot.task.tool_offset);
    account_open(report, "task.active_g_codes", "open_interpreter_code_array");
    account_open(report, "task.active_m_codes", "open_interpreter_code_array");
    check_finite_f64_array(
        report,
        "task.active_settings",
        &snapshot.task.active_settings,
    );
    check_code(
        report,
        "task.program_units",
        CANON_UNITS,
        i64::from(snapshot.task.program_units),
    );
    let interpreter_name = check_code(
        report,
        "task.interpreter_errcode",
        INTERPRETER_RETURN,
        i64::from(snapshot.task.interpreter_errcode),
    );
    if matches!(
        interpreter_name,
        Some("INTERP_FILE_NOT_OPEN" | "INTERP_ERROR")
    ) && (exec_name == Some("EMC_TASK_EXEC_ERROR")
        || RCS_STATUS.lookup(i64::from(snapshot.task.rcs.status)) == Some("RCS_ERROR"))
    {
        issue(
            report,
            Severity::Error,
            category::INTERPRETER,
            "task.interpreter_errcode",
            INTERPRETER_RETURN.name,
            domain_id(INTERPRETER_RETURN),
            i64::from(snapshot.task.interpreter_errcode),
            interpreter_name,
            "LinuxCNC interpreter reported an error return",
        );
    }
    check_i32_set(
        report,
        "task.task_paused",
        snapshot.task.task_paused,
        &[0, 1],
        "task-paused state is neither false nor true",
    );
    check_finite(report, "task.delay_left", snapshot.task.delay_left);
    check_i32_range(
        report,
        "task.queued_mdi_commands",
        snapshot.task.queued_mdi_commands,
        0,
        i32::MAX,
        "nonnegative_count",
        "queued MDI command count is negative",
    );

    if snapshot.task.input_timeout == 1 {
        issue(
            report,
            Severity::Warning,
            category::INPUT_TIMEOUT,
            "task.input_timeout",
            "boolean_status",
            u32::MAX,
            i64::from(snapshot.task.input_timeout),
            None,
            "LinuxCNC reports an M66 input timeout",
        );
    }
}
