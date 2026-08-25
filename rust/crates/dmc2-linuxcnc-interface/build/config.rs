pub(crate) const EXPECTED_LINUXCNC_VERSION: &str = "2.9.10";
pub(crate) const EXPECTED_LINUXCNC_COMMIT: &str = "86cdca76fa2a36274c432caa21952b23c267989a";
pub(crate) const SOURCE_ROOT_RELATIVE: &str = "../../../vendor/linuxcnc-2.9.10";
pub(crate) const EXPECTED_HEADER_FNV64: u64 = 0x5d196ecfe398141a;
pub(crate) const INCLUDE_ROOT: &str = "/usr/include/linuxcnc";
pub(crate) const EXPECTED_PUBLIC_HEADER_COUNT: usize = 120;
pub(crate) const EXPECTED_PUBLIC_HEADER_SOURCE_FNV64: u64 = 0x8f2986fcf6b52329;
pub(crate) const EXPECTED_PUBLIC_HEADER_SOURCE_BYTE_COUNT: usize = 635_278;
pub(crate) const EXPECTED_PUBLIC_MACRO_DECLARATION_COUNT: usize = 1_106;
pub(crate) const EXPECTED_PUBLIC_MACRO_NAME_COUNT: usize = 1_029;
pub(crate) const EXPECTED_PUBLIC_MACRO_INACTIVE_COUNT: usize = 86;
pub(crate) const EXPECTED_PUBLIC_MACRO_FUNCTION_COUNT: usize = 120;
pub(crate) const EXPECTED_PUBLIC_MACRO_EMPTY_OBJECT_COUNT: usize = 126;
pub(crate) const EXPECTED_PUBLIC_MACRO_SIGNED_INTEGER_COUNT: usize = 166;
pub(crate) const EXPECTED_PUBLIC_MACRO_UNSIGNED_INTEGER_COUNT: usize = 315;
pub(crate) const EXPECTED_PUBLIC_MACRO_NON_INTEGER_COUNT: usize = 216;
pub(crate) const EXPECTED_PUBLIC_ENUM_HEADER_COUNT: usize = 30;

pub(crate) const EXPECTED_DOMAIN_COUNTS: &[(&str, usize)] = &[
    ("emc_nml_message_type", 145),
    ("nml_operator_message_type", 3),
    ("task_mode", 3),
    ("task_state", 4),
    ("task_exec", 9),
    ("task_interp", 4),
    ("traj_mode", 3),
    ("io_abort_reason", 11),
    ("joint_type", 2),
    ("motion_command", 74),
    ("motion_command_status", 5),
    ("motion_state", 4),
    ("spindle_orient_state", 4),
    ("interpreter_return", 6),
    ("nml_error", 9),
    ("nml_channel_type", 6),
    ("rcs_status", 4),
    ("rcs_state", 53),
    ("canon_bool", 2),
    ("canon_plane", 6),
    ("canon_units", 3),
    ("canon_motion_mode", 3),
    ("canon_speed_feed_mode", 2),
    ("canon_direction", 3),
    ("canon_feed_reference", 2),
    ("canon_side", 3),
    ("canon_axis", 9),
    ("kinematics_type", 4),
    ("motion_type", 6),
    ("motion_flag", 5),
    ("motion_termination_condition", 3),
    ("spindle_feed_enable_flag", 4),
    ("aux_input_type", 2),
    ("aux_wait_mode", 5),
    ("debug_flag", 20),
    ("joint_flag", 8),
    ("motion_communication_result", 6),
    ("state_tag_flag", 25),
    ("state_tag_field", 9),
    ("state_tag_float_field", 6),
    ("cms_status", 23),
    ("cms_mode", 7),
    ("cms_internal_access", 11),
    ("cms_buffer_type", 4),
    ("cms_process_type", 4),
    ("cms_remote_port_type", 5),
    ("cms_encoding", 4),
    ("cms_connection_mode", 3),
    ("rcs_generic_command", 2),
    ("rcs_generic_message_type", 2),
];

pub(crate) const STATUS_MESSAGE_CONTRACTS: &[(&str, &str)] = &[
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

pub(crate) struct ErrorMessageSpec {
    pub(crate) class_name: &'static str,
    pub(crate) message_type_name: &'static str,
    pub(crate) payload_member: &'static str,
    pub(crate) id_member: Option<&'static str>,
}

pub(crate) const ERROR_MESSAGE_CONTRACTS: &[ErrorMessageSpec] = &[
    ErrorMessageSpec {
        class_name: "NML_ERROR",
        message_type_name: "NML_ERROR_TYPE",
        payload_member: "error",
        id_member: None,
    },
    ErrorMessageSpec {
        class_name: "NML_TEXT",
        message_type_name: "NML_TEXT_TYPE",
        payload_member: "text",
        id_member: None,
    },
    ErrorMessageSpec {
        class_name: "NML_DISPLAY",
        message_type_name: "NML_DISPLAY_TYPE",
        payload_member: "display",
        id_member: None,
    },
    ErrorMessageSpec {
        class_name: "EMC_OPERATOR_ERROR",
        message_type_name: "EMC_OPERATOR_ERROR_TYPE",
        payload_member: "error",
        id_member: Some("id"),
    },
    ErrorMessageSpec {
        class_name: "EMC_OPERATOR_TEXT",
        message_type_name: "EMC_OPERATOR_TEXT_TYPE",
        payload_member: "text",
        id_member: Some("id"),
    },
    ErrorMessageSpec {
        class_name: "EMC_OPERATOR_DISPLAY",
        message_type_name: "EMC_OPERATOR_DISPLAY_TYPE",
        payload_member: "display",
        id_member: Some("id"),
    },
];
