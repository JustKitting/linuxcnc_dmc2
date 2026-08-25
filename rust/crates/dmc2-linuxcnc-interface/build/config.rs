pub(crate) const EXPECTED_LINUXCNC_VERSION: &str = "2.9.10";
pub(crate) const EXPECTED_LINUXCNC_COMMIT: &str = "86cdca76fa2a36274c432caa21952b23c267989a";
pub(crate) const SOURCE_ROOT_RELATIVE: &str = "../../../vendor/linuxcnc-2.9.10";
pub(crate) const INCLUDE_ROOT: &str = "/usr/include/linuxcnc";

// Only value domains read by dmc2-task-monitor belong here. This is a
// controller interface, not an inventory of LinuxCNC's public headers.
pub(crate) const EXPECTED_DOMAIN_COUNTS: &[(&str, usize)] = &[
    ("emc_nml_message_type", 145),
    ("task_mode", 3),
    ("task_state", 4),
    ("task_exec", 9),
    ("task_interp", 4),
    ("traj_mode", 3),
    ("joint_type", 2),
    ("motion_command", 74),
    ("spindle_orient_state", 4),
    ("interpreter_return", 6),
    ("nml_error", 9),
    ("rcs_status", 4),
    ("rcs_state", 53),
    ("canon_units", 3),
    ("kinematics_type", 4),
    ("motion_type", 6),
    ("debug_flag", 20),
    ("state_tag_flag", 25),
    ("rcs_generic_command", 2),
];

// These are the RCS status objects copied into the task-monitor snapshot.
pub(crate) const STATUS_MESSAGE_TYPES: &[(&str, &str)] = &[
    ("EMC_STAT", "EMC_STAT_TYPE"),
    ("EMC_TASK_STAT", "EMC_TASK_STAT_TYPE"),
    ("EMC_MOTION_STAT", "EMC_MOTION_STAT_TYPE"),
    ("EMC_TRAJ_STAT", "EMC_TRAJ_STAT_TYPE"),
    ("EMC_JOINT_STAT", "EMC_JOINT_STAT_TYPE"),
    ("EMC_AXIS_STAT", "EMC_AXIS_STAT_TYPE"),
    ("EMC_SPINDLE_STAT", "EMC_SPINDLE_STAT_TYPE"),
    ("EMC_IO_STAT", "EMC_IO_STAT_TYPE"),
    ("EMC_TOOL_STAT", "EMC_TOOL_STAT_TYPE"),
    ("EMC_AUX_STAT", "EMC_AUX_STAT_TYPE"),
    ("EMC_COOLANT_STAT", "EMC_COOLANT_STAT_TYPE"),
    ("EMC_LUBE_STAT", "EMC_LUBE_STAT_TYPE"),
];
