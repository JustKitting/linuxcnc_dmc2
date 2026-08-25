//! Source-backed classification of LinuxCNC status and error states.

use std::collections::BTreeSet;

use dmc2_linuxcnc_interface::{
    CodeDomain, CANON_UNITS, DEBUG_FLAG, DOMAINS, EMCMOT_MAX_AXIS, EMCMOT_MAX_JOINTS,
    EMCMOT_MAX_SPINDLES, EMC_NML_MESSAGE_TYPE, INTERPRETER_RETURN, JOINT_TYPE, KINEMATICS_TYPE,
    MOTION_COMMAND, MOTION_TYPE, NML_ERROR, RCS_GENERIC_COMMAND, RCS_STATE, RCS_STATUS,
    SPINDLE_ORIENT_STATE, STATE_TAG_FLAG, TASK_EXEC, TASK_INTERP, TASK_MODE, TASK_STATE, TRAJ_MODE,
};

use crate::snapshot::{NativeSnapshot, RcsStatusSnapshot};

pub mod category {
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
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Severity {
    Warning,
    Error,
}

impl Severity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Issue {
    pub severity: Severity,
    pub category: u64,
    pub source: String,
    pub domain: &'static str,
    pub domain_id: u32,
    pub value: i64,
    pub name: Option<&'static str>,
    pub detail: &'static str,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DiagnosticReport {
    pub active_error_mask: u64,
    pub active_warning_mask: u64,
    pub unknown_domain_mask: u64,
    pub issues: Vec<Issue>,
}

impl DiagnosticReport {
    pub fn error_active(&self) -> bool {
        self.active_error_mask != 0
    }

    pub fn warning_active(&self) -> bool {
        self.active_warning_mask != 0
    }

    pub fn unknown_code_active(&self) -> bool {
        self.unknown_domain_mask != 0
    }

    pub fn unknown_code_count(&self) -> u32 {
        self.issues
            .iter()
            .filter(|issue| issue.category == category::UNKNOWN_CODE)
            .count()
            .try_into()
            .unwrap_or(u32::MAX)
    }

    fn push(&mut self, issue: Issue) {
        match issue.severity {
            Severity::Warning => self.active_warning_mask |= issue.category,
            Severity::Error => self.active_error_mask |= issue.category,
        }
        if issue.category == category::UNKNOWN_CODE && issue.domain_id < 64 {
            self.unknown_domain_mask |= 1_u64 << issue.domain_id;
        }
        self.issues.push(issue);
    }
}

#[derive(Default)]
pub struct TransitionLogger {
    active: BTreeSet<Issue>,
}

#[derive(Clone, Debug, Default)]
pub struct TransitionUpdate {
    pub count: u32,
    pub latest: Option<Issue>,
    pub latest_action: i32,
}

impl TransitionLogger {
    pub fn update(&mut self, report: &DiagnosticReport) -> TransitionUpdate {
        let next = report.issues.iter().cloned().collect::<BTreeSet<_>>();
        let mut update = TransitionUpdate::default();
        for issue in next.difference(&self.active) {
            log_issue("assert", issue);
            update.count = update.count.saturating_add(1);
            update.latest = Some(issue.clone());
            update.latest_action = 1;
        }
        for issue in self.active.difference(&next) {
            log_issue("clear", issue);
            update.count = update.count.saturating_add(1);
            update.latest = Some(issue.clone());
            update.latest_action = -1;
        }
        self.active = next;
        update
    }
}

fn log_issue(action: &str, issue: &Issue) {
    eprintln!(
        "DMC2_LINUXCNC_DIAGNOSTIC action={} severity={} category=0x{:016x} source={} domain={} domain_id={} code={} name={} detail={:?}",
        action,
        issue.severity.as_str(),
        issue.category,
        issue.source,
        issue.domain,
        issue.domain_id,
        issue.value,
        issue.name.unwrap_or("UNKNOWN"),
        issue.detail,
    );
}

fn domain_id(domain: CodeDomain) -> u32 {
    DOMAINS
        .iter()
        .position(|candidate| candidate.name == domain.name)
        .expect("generated LinuxCNC domain was omitted from DOMAINS")
        .try_into()
        .expect("LinuxCNC domain index does not fit in u32")
}

fn issue(
    report: &mut DiagnosticReport,
    severity: Severity,
    category: u64,
    source: impl Into<String>,
    domain: &'static str,
    domain_id: u32,
    value: i64,
    name: Option<&'static str>,
    detail: &'static str,
) {
    report.push(Issue {
        severity,
        category,
        source: source.into(),
        domain,
        domain_id,
        value,
        name,
        detail,
    });
}

fn unknown_code(
    report: &mut DiagnosticReport,
    source: impl Into<String>,
    domain: CodeDomain,
    value: i64,
) {
    issue(
        report,
        Severity::Error,
        category::UNKNOWN_CODE,
        source,
        domain.name,
        domain_id(domain),
        value,
        None,
        "value is absent from the version-locked LinuxCNC 2.9.10 source catalog",
    );
}

fn check_code(
    report: &mut DiagnosticReport,
    source: impl Into<String>,
    domain: CodeDomain,
    value: i64,
) -> Option<&'static str> {
    let source = source.into();
    match domain.lookup(value) {
        Some(name) => Some(name),
        None => {
            unknown_code(report, source, domain, value);
            None
        }
    }
}

#[derive(Clone, Copy)]
enum CommandDomain {
    EmcNml,
    Motion,
}

impl CommandDomain {
    const fn catalog(self) -> CodeDomain {
        match self {
            Self::EmcNml => EMC_NML_MESSAGE_TYPE,
            Self::Motion => MOTION_COMMAND,
        }
    }
}

fn check_rcs(
    report: &mut DiagnosticReport,
    source: &str,
    status: RcsStatusSnapshot,
    commands: CommandDomain,
    fault_category: u64,
) {
    if status.command_type != -1
        && status.command_type != 0
        && commands.catalog().lookup(status.command_type).is_none()
        && RCS_GENERIC_COMMAND.lookup(status.command_type).is_none()
    {
        unknown_code(
            report,
            format!("{source}.command_type"),
            commands.catalog(),
            status.command_type,
        );
    }

    let rcs_name = check_code(
        report,
        format!("{source}.status"),
        RCS_STATUS,
        i64::from(status.status),
    );
    check_code(
        report,
        format!("{source}.state"),
        RCS_STATE,
        i64::from(status.state),
    );

    if rcs_name == Some("RCS_ERROR") {
        issue(
            report,
            Severity::Error,
            fault_category,
            format!("{source}.status"),
            RCS_STATUS.name,
            domain_id(RCS_STATUS),
            i64::from(status.status),
            rcs_name,
            "LinuxCNC subsystem reported RCS_ERROR",
        );
    }
    if status.reserved != 0 {
        issue(
            report,
            Severity::Error,
            category::ABI,
            format!("{source}.reserved"),
            "snapshot_abi",
            u32::MAX,
            i64::from(status.reserved),
            None,
            "reserved ABI field was modified",
        );
    }
}

fn check_i32_set(
    report: &mut DiagnosticReport,
    source: impl Into<String>,
    value: i32,
    allowed: &[i32],
    detail: &'static str,
) {
    if !allowed.contains(&value) {
        issue(
            report,
            Severity::Error,
            category::INVALID_VALUE,
            source,
            "constrained_integer",
            u32::MAX,
            i64::from(value),
            None,
            detail,
        );
    }
}

fn check_debug_mask(report: &mut DiagnosticReport, source: &str, raw: i32) {
    let allowed = DEBUG_FLAG
        .codes
        .iter()
        .fold(0_u32, |mask, code| mask | code.code as u32);
    let unknown = (raw as u32) & !allowed;
    if unknown != 0 {
        issue(
            report,
            Severity::Error,
            category::UNKNOWN_CODE,
            source,
            DEBUG_FLAG.name,
            domain_id(DEBUG_FLAG),
            i64::from(unknown),
            None,
            "debug mask contains bits absent from LinuxCNC 2.9.10",
        );
        report.unknown_domain_mask |= 1_u64 << domain_id(DEBUG_FLAG);
    }
}

pub fn disconnected(nml_error: i32) -> DiagnosticReport {
    let mut report = DiagnosticReport::default();
    let name = NML_ERROR.lookup(i64::from(nml_error));
    issue(
        &mut report,
        Severity::Error,
        category::TRANSPORT,
        "emcStatus",
        NML_ERROR.name,
        domain_id(NML_ERROR),
        i64::from(nml_error),
        name,
        "native LinuxCNC emcStatus channel is disconnected or unreadable",
    );
    if name.is_none() {
        unknown_code(
            &mut report,
            "emcStatus.error_type",
            NML_ERROR,
            i64::from(nml_error),
        );
    }
    report
}

pub fn evaluate(snapshot: &NativeSnapshot) -> DiagnosticReport {
    let mut report = DiagnosticReport::default();
    if !snapshot.valid_abi() {
        issue(
            &mut report,
            Severity::Error,
            category::ABI,
            "snapshot",
            "snapshot_abi",
            u32::MAX,
            i64::from(snapshot.abi_version),
            None,
            "native LinuxCNC status snapshot ABI does not match Rust",
        );
        return report;
    }

    check_rcs(
        &mut report,
        "top",
        snapshot.top_rcs,
        CommandDomain::EmcNml,
        category::TOP_RCS,
    );
    check_rcs(
        &mut report,
        "task",
        snapshot.task.rcs,
        CommandDomain::EmcNml,
        category::TASK_RCS,
    );
    check_rcs(
        &mut report,
        "motion",
        snapshot.motion_rcs,
        CommandDomain::Motion,
        category::MOTION_RCS,
    );
    check_rcs(
        &mut report,
        "trajectory",
        snapshot.trajectory.rcs,
        CommandDomain::Motion,
        category::TRAJECTORY_RCS,
    );

    check_code(
        &mut report,
        "task.mode",
        TASK_MODE,
        i64::from(snapshot.task.mode),
    );
    check_code(
        &mut report,
        "task.state",
        TASK_STATE,
        i64::from(snapshot.task.state),
    );
    let exec_name = check_code(
        &mut report,
        "task.exec_state",
        TASK_EXEC,
        i64::from(snapshot.task.exec_state),
    );
    if exec_name == Some("EMC_TASK_EXEC_ERROR") {
        issue(
            &mut report,
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
        &mut report,
        "task.interp_state",
        TASK_INTERP,
        i64::from(snapshot.task.interp_state),
    );
    check_code(
        &mut report,
        "task.program_units",
        CANON_UNITS,
        i64::from(snapshot.task.program_units),
    );
    let interpreter_name = check_code(
        &mut report,
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
            &mut report,
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
            &mut report,
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

    let joint_count = snapshot.trajectory.joints;
    if !(1..=EMCMOT_MAX_JOINTS as i32).contains(&joint_count) {
        issue(
            &mut report,
            Severity::Error,
            category::INVALID_VALUE,
            "trajectory.joints",
            "configured_count",
            u32::MAX,
            i64::from(joint_count),
            None,
            "configured joint count is outside LinuxCNC 2.9.10 bounds",
        );
    }
    let spindle_count = snapshot.trajectory.spindles;
    if !(0..=EMCMOT_MAX_SPINDLES as i32).contains(&spindle_count) {
        issue(
            &mut report,
            Severity::Error,
            category::INVALID_VALUE,
            "trajectory.spindles",
            "configured_count",
            u32::MAX,
            i64::from(spindle_count),
            None,
            "configured spindle count is outside LinuxCNC 2.9.10 bounds",
        );
    }
    let valid_axis_mask = (1_i32 << EMCMOT_MAX_AXIS) - 1;
    if snapshot.trajectory.axis_mask & !valid_axis_mask != 0 {
        issue(
            &mut report,
            Severity::Error,
            category::INVALID_VALUE,
            "trajectory.axis_mask",
            "axis_mask",
            u32::MAX,
            i64::from(snapshot.trajectory.axis_mask),
            None,
            "axis mask contains bits beyond LinuxCNC 2.9.10 maximum axes",
        );
    }
    check_code(
        &mut report,
        "trajectory.mode",
        TRAJ_MODE,
        i64::from(snapshot.trajectory.mode),
    );
    if snapshot.trajectory.kinematics_type != 0 {
        check_code(
            &mut report,
            "trajectory.kinematics_type",
            KINEMATICS_TYPE,
            i64::from(snapshot.trajectory.kinematics_type),
        );
    }
    if snapshot.trajectory.motion_type != 0 {
        check_code(
            &mut report,
            "trajectory.motion_type",
            MOTION_TYPE,
            i64::from(snapshot.trajectory.motion_type),
        );
    }
    check_i32_set(
        &mut report,
        "trajectory.probe_value",
        snapshot.trajectory.probe_value,
        &[0, 1],
        "probe input status is neither low nor high",
    );
    let state_tag_flag_count = STATE_TAG_FLAG
        .codes
        .iter()
        .find(|code| code.name == "GM_FLAG_MAX_FLAGS")
        .expect("generated state-tag catalog omitted GM_FLAG_MAX_FLAGS")
        .code as u32;
    let valid_state_tag_flags = (1_u64 << state_tag_flag_count) - 1;
    if snapshot.trajectory.state_tag_flags & !valid_state_tag_flags != 0 {
        issue(
            &mut report,
            Severity::Error,
            category::UNKNOWN_CODE,
            "trajectory.state_tag_flags",
            STATE_TAG_FLAG.name,
            domain_id(STATE_TAG_FLAG),
            (snapshot.trajectory.state_tag_flags & !valid_state_tag_flags) as i64,
            None,
            "state tag contains flag bits absent from LinuxCNC 2.9.10",
        );
        report.unknown_domain_mask |= 1_u64 << domain_id(STATE_TAG_FLAG);
    }

    for (index, joint) in snapshot
        .joints
        .iter()
        .take(joint_count.clamp(0, EMCMOT_MAX_JOINTS as i32) as usize)
        .enumerate()
    {
        check_rcs(
            &mut report,
            &format!("joint[{index}]"),
            joint.rcs,
            CommandDomain::Motion,
            category::JOINT_RCS,
        );
        check_code(
            &mut report,
            format!("joint[{index}].joint_type"),
            JOINT_TYPE,
            i64::from(joint.joint_type),
        );
        if joint.fault != 0 {
            issue(
                &mut report,
                Severity::Error,
                category::JOINT_FAULT,
                format!("joint[{index}].fault"),
                "boolean_status",
                u32::MAX,
                i64::from(joint.fault),
                None,
                "LinuxCNC reports a joint amplifier/following fault",
            );
        }
        if joint.min_hard_limit != 0 || joint.max_hard_limit != 0 {
            issue(
                &mut report,
                Severity::Warning,
                category::HARD_LIMIT,
                format!("joint[{index}].hard_limit"),
                "limit_status",
                u32::MAX,
                i64::from((joint.min_hard_limit != 0) as i32)
                    - i64::from((joint.max_hard_limit != 0) as i32),
                None,
                "joint hard-limit input is active",
            );
        }
        if joint.min_soft_limit != 0 || joint.max_soft_limit != 0 {
            issue(
                &mut report,
                Severity::Warning,
                category::SOFT_LIMIT,
                format!("joint[{index}].soft_limit"),
                "limit_status",
                u32::MAX,
                i64::from((joint.min_soft_limit != 0) as i32)
                    - i64::from((joint.max_soft_limit != 0) as i32),
                None,
                "joint soft-limit status is active",
            );
        }
    }

    for index in 0..EMCMOT_MAX_AXIS {
        if snapshot.trajectory.axis_mask & (1_i32 << index) == 0 {
            continue;
        }
        check_rcs(
            &mut report,
            &format!("axis[{index}]"),
            snapshot.axes[index].rcs,
            CommandDomain::Motion,
            category::AXIS_RCS,
        );
    }

    for (index, spindle) in snapshot
        .spindles
        .iter()
        .take(spindle_count.clamp(0, EMCMOT_MAX_SPINDLES as i32) as usize)
        .enumerate()
    {
        check_rcs(
            &mut report,
            &format!("spindle[{index}]"),
            spindle.rcs,
            CommandDomain::Motion,
            category::SPINDLE_RCS,
        );
        check_i32_set(
            &mut report,
            format!("spindle[{index}].direction"),
            spindle.direction,
            &[-1, 0, 1],
            "spindle direction is not reverse, stopped, or forward",
        );
        check_i32_set(
            &mut report,
            format!("spindle[{index}].brake"),
            spindle.brake,
            &[0, 1],
            "spindle brake state is neither released nor engaged",
        );
        check_i32_set(
            &mut report,
            format!("spindle[{index}].enabled"),
            spindle.enabled,
            &[0, 1],
            "spindle enabled state is neither false nor true",
        );
        let orient_name = check_code(
            &mut report,
            format!("spindle[{index}].orient_state"),
            SPINDLE_ORIENT_STATE,
            i64::from(spindle.orient_state),
        );
        if orient_name == Some("EMCMOT_ORIENT_FAULTED") {
            issue(
                &mut report,
                Severity::Error,
                category::SPINDLE_ORIENT,
                format!("spindle[{index}].orient_fault"),
                SPINDLE_ORIENT_STATE.name,
                domain_id(SPINDLE_ORIENT_STATE),
                i64::from(spindle.orient_fault),
                orient_name,
                "LinuxCNC spindle orientation reported a fault",
            );
        }
    }

    for (index, value) in snapshot.misc_error.iter().copied().enumerate() {
        if value != 0 {
            issue(
                &mut report,
                Severity::Error,
                category::MISC_ERROR,
                format!("motion.misc_error[{index}]"),
                "misc_error",
                u32::MAX,
                i64::from(value),
                None,
                "LinuxCNC miscellaneous error input is active",
            );
        }
    }
    check_rcs(
        &mut report,
        "io",
        snapshot.io.rcs,
        CommandDomain::EmcNml,
        category::IO_RCS,
    );
    for (source, status) in [
        ("io.tool", snapshot.io.tool_rcs),
        ("io.aux", snapshot.io.aux_rcs),
        ("io.coolant", snapshot.io.coolant_rcs),
        ("io.lube", snapshot.io.lube_rcs),
    ] {
        check_rcs(
            &mut report,
            source,
            status,
            CommandDomain::EmcNml,
            category::IO_RCS,
        );
    }
    if snapshot.io.fault != 0 {
        issue(
            &mut report,
            if snapshot.io.reason > 0 {
                Severity::Warning
            } else {
                Severity::Error
            },
            category::IO_FAULT,
            "io.fault",
            "io_fault_payload",
            u32::MAX,
            i64::from(snapshot.io.reason),
            None,
            "LinuxCNC IO/toolchanger fault is active; value is its reason payload",
        );
    }
    for (source, value) in [
        ("io.estop", snapshot.io.estop),
        ("io.coolant_mist", snapshot.io.coolant_mist),
        ("io.coolant_flood", snapshot.io.coolant_flood),
        ("io.lube_on", snapshot.io.lube_on),
        ("io.lube_level", snapshot.io.lube_level),
    ] {
        check_i32_set(
            &mut report,
            source,
            value,
            &[0, 1],
            "LinuxCNC boolean status is neither false nor true",
        );
    }

    check_debug_mask(&mut report, "top.debug", snapshot.top_debug);
    check_debug_mask(&mut report, "motion.debug", snapshot.motion_debug);
    check_debug_mask(&mut report, "io.debug", snapshot.io.debug);
    report
}

#[cfg(test)]
mod tests;
