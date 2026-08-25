use std::collections::BTreeSet;

use super::config::EXPECTED_DOMAIN_COUNTS;
use super::parser::{macro_names, named_enum, typedef_enum};
use super::source::Headers;

pub(crate) struct Domain {
    pub(crate) name: &'static str,
    pub(crate) symbols: Vec<String>,
}

fn domain(name: &'static str, symbols: Vec<String>) -> Domain {
    Domain { name, symbols }
}

pub(crate) fn collect(headers: &Headers) -> Vec<Domain> {
    let domains = vec![
        domain(
            "emc_nml_message_type",
            macro_names(&headers.emc, |name, value| {
                name.starts_with("EMC_") && name.ends_with("_TYPE") && value.contains("NMLTYPE")
            }),
        ),
        domain(
            "nml_operator_message_type",
            macro_names(&headers.nml_oi, |name, value| {
                matches!(
                    name,
                    "NML_ERROR_TYPE" | "NML_TEXT_TYPE" | "NML_DISPLAY_TYPE"
                ) && value.contains("NMLTYPE")
            }),
        ),
        domain(
            "task_mode",
            named_enum(&headers.emc, "enum EMC_TASK_MODE_ENUM"),
        ),
        domain(
            "task_state",
            named_enum(&headers.emc, "enum EMC_TASK_STATE_ENUM"),
        ),
        domain(
            "task_exec",
            named_enum(&headers.emc, "enum EMC_TASK_EXEC_ENUM"),
        ),
        domain(
            "task_interp",
            named_enum(&headers.emc, "enum EMC_TASK_INTERP_ENUM"),
        ),
        domain(
            "traj_mode",
            named_enum(&headers.emc, "enum EMC_TRAJ_MODE_ENUM"),
        ),
        domain(
            "io_abort_reason",
            named_enum(&headers.emc, "enum EMC_IO_ABORT_REASON_ENUM"),
        ),
        domain("joint_type", named_enum(&headers.emc, "enum EmcJointType")),
        domain(
            "motion_command",
            typedef_enum(&headers.motion, "cmd_code_t"),
        ),
        domain(
            "motion_command_status",
            typedef_enum(&headers.motion, "cmd_status_t"),
        ),
        domain(
            "motion_state",
            typedef_enum(&headers.motion, "motion_state_t"),
        ),
        domain(
            "spindle_orient_state",
            typedef_enum(&headers.motion, "orient_state_t"),
        ),
        domain(
            "interpreter_return",
            named_enum(&headers.interp_return, "enum InterpReturn"),
        ),
        domain("nml_error", named_enum(&headers.nml, "enum NML_ERROR_TYPE")),
        domain(
            "nml_channel_type",
            named_enum(&headers.nml, "enum NML_CHANNEL_TYPE"),
        ),
        domain("rcs_status", named_enum(&headers.rcs, "enum RCS_STATUS")),
        domain("rcs_state", named_enum(&headers.stat_msg, "enum RCS_STATE")),
        domain("canon_bool", named_enum(&headers.canon, "enum CanonBool")),
        domain(
            "canon_plane",
            named_enum(&headers.canon, "enum CANON_PLANE"),
        ),
        domain(
            "canon_units",
            named_enum(&headers.canon, "enum CANON_UNITS"),
        ),
        domain(
            "canon_motion_mode",
            named_enum(&headers.canon, "enum CANON_MOTION_MODE"),
        ),
        domain(
            "canon_speed_feed_mode",
            named_enum(&headers.canon, "enum CANON_SPEED_FEED_MODE"),
        ),
        domain(
            "canon_direction",
            named_enum(&headers.canon, "enum CANON_DIRECTION"),
        ),
        domain(
            "canon_feed_reference",
            named_enum(&headers.canon, "enum CANON_FEED_REFERENCE"),
        ),
        domain("canon_side", named_enum(&headers.canon, "enum CANON_SIDE")),
        domain("canon_axis", named_enum(&headers.canon, "enum CANON_AXIS")),
        domain(
            "kinematics_type",
            typedef_enum(&headers.kinematics, "KINEMATICS_TYPE"),
        ),
        domain(
            "motion_type",
            macro_names(&headers.motion_types, |name, _| {
                name.starts_with("EMC_MOTION_TYPE_")
            }),
        ),
        domain(
            "motion_flag",
            macro_names(&headers.motion, |name, _| {
                name.starts_with("EMCMOT_MOTION_") && name.ends_with("_BIT")
            }),
        ),
        domain(
            "motion_termination_condition",
            macro_names(&headers.motion, |name, _| {
                name.starts_with("EMCMOT_TERM_COND_")
            }),
        ),
        domain(
            "spindle_feed_enable_flag",
            macro_names(&headers.motion, |name, _| {
                matches!(
                    name,
                    "SS_ENABLED" | "FS_ENABLED" | "AF_ENABLED" | "FH_ENABLED"
                )
            }),
        ),
        domain(
            "aux_input_type",
            macro_names(&headers.canon, |name, _| {
                matches!(name, "DIGITAL_INPUT" | "ANALOG_INPUT")
            }),
        ),
        domain(
            "aux_wait_mode",
            macro_names(&headers.canon, |name, _| name.starts_with("WAIT_MODE_")),
        ),
        domain(
            "debug_flag",
            macro_names(&headers.debug_flags, |name, _| {
                name.starts_with("EMC_DEBUG_")
            }),
        ),
        domain(
            "joint_flag",
            macro_names(&headers.motion, |name, _| {
                name.starts_with("EMCMOT_JOINT_") && name.ends_with("_BIT")
            }),
        ),
        domain(
            "motion_communication_result",
            macro_names(&headers.usrmotintf, |name, _| {
                name.starts_with("EMCMOT_COMM_")
            }),
        ),
        domain(
            "state_tag_flag",
            typedef_enum(&headers.state_tag, "StateFlag"),
        ),
        domain(
            "state_tag_field",
            typedef_enum(&headers.state_tag, "StateField"),
        ),
        domain(
            "state_tag_float_field",
            typedef_enum(&headers.state_tag, "StateFieldFloat"),
        ),
        domain("cms_status", named_enum(&headers.cms, "enum CMS_STATUS")),
        domain("cms_mode", named_enum(&headers.cms, "enum CMSMODE")),
        domain(
            "cms_internal_access",
            named_enum(&headers.cms, "enum CMS_INTERNAL_ACCESS_TYPE"),
        ),
        domain(
            "cms_buffer_type",
            named_enum(&headers.cms, "enum CMS_BUFFERTYPE"),
        ),
        domain(
            "cms_process_type",
            named_enum(&headers.cms, "enum CMS_PROCESSTYPE"),
        ),
        domain(
            "cms_remote_port_type",
            named_enum(&headers.cms, "enum CMS_REMOTE_PORT_TYPE"),
        ),
        domain(
            "cms_encoding",
            named_enum(&headers.cms, "enum CMS_NEUTRAL_ENCODING_METHOD"),
        ),
        domain(
            "cms_connection_mode",
            named_enum(&headers.cms, "enum CMS_CONNECTION_MODE"),
        ),
        domain(
            "rcs_generic_command",
            named_enum(&headers.cmd_msg, "enum RCS_GENERIC_CMD_ID"),
        ),
        domain(
            "rcs_generic_message_type",
            [
                macro_names(&headers.cmd_msg, |name, value| {
                    name == "RCS_GENERIC_CMD_TYPE" && value.contains("NMLTYPE")
                }),
                macro_names(&headers.stat_msg, |name, value| {
                    name == "RCS_GENERIC_STATUS_TYPE" && value.contains("NMLTYPE")
                }),
            ]
            .concat(),
        ),
    ];
    validate(&domains);
    domains
}

fn validate(domains: &[Domain]) {
    let mut seen_domain_names = BTreeSet::new();
    for domain in domains {
        assert!(
            seen_domain_names.insert(domain.name),
            "duplicate catalog domain {}",
            domain.name
        );
        let unique = domain.symbols.iter().collect::<BTreeSet<_>>();
        assert_eq!(
            unique.len(),
            domain.symbols.len(),
            "duplicate source name in {}",
            domain.name
        );
    }
    let actual_domain_counts = domains
        .iter()
        .map(|domain| (domain.name, domain.symbols.len()))
        .collect::<Vec<_>>();
    assert_eq!(
        actual_domain_counts, EXPECTED_DOMAIN_COUNTS,
        "LinuxCNC 2.9.10 public code domains changed or the source parser omitted a code"
    );
}
