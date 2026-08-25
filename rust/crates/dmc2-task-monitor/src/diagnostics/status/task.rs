//! Task-controller and interpreter status validation.

use dmc2_linuxcnc_interface::{
    CANON_UNITS, INTERPRETER_RETURN, RCS_STATUS, TASK_EXEC, TASK_INTERP, TASK_MODE, TASK_STATE,
};

use crate::snapshot::NativeSnapshot;

use super::super::catalog::{check_code, domain_id, issue};
use super::super::category;
use super::super::report::{DiagnosticReport, Severity};

pub(super) fn evaluate(snapshot: &NativeSnapshot, report: &mut DiagnosticReport) {
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
    if snapshot.task.input_timeout != 0 {
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
