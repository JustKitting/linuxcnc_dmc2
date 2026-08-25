use std::collections::BTreeSet;

use super::config::EXPECTED_DOMAIN_COUNTS;
use super::parser::{enum_declarations, macro_names, named_enum, typedef_enum, EnumKind};
use super::source::{Headers, PublicEnumHeader};

#[derive(Clone)]
pub(crate) struct EnumOrigin {
    pub(crate) header_name: String,
    pub(crate) kind: EnumKind,
    pub(crate) declaration_name: String,
}

pub(crate) struct Domain {
    pub(crate) name: String,
    pub(crate) symbols: Vec<String>,
    pub(crate) enum_origin: Option<EnumOrigin>,
    pub(crate) enum_probe_body: Option<String>,
}

fn domain(name: &'static str, symbols: Vec<String>) -> Domain {
    Domain {
        name: name.to_owned(),
        symbols,
        enum_origin: None,
        enum_probe_body: None,
    }
}

fn named_domain(
    name: &'static str,
    header_name: &'static str,
    source: &str,
    declaration_name: &'static str,
) -> Domain {
    Domain {
        name: name.to_owned(),
        symbols: named_enum(source, &format!("enum {declaration_name}")),
        enum_origin: Some(EnumOrigin {
            header_name: header_name.to_owned(),
            kind: EnumKind::Named,
            declaration_name: declaration_name.to_owned(),
        }),
        enum_probe_body: None,
    }
}

fn typedef_domain(
    name: &'static str,
    header_name: &'static str,
    source: &str,
    declaration_name: &'static str,
) -> Domain {
    Domain {
        name: name.to_owned(),
        symbols: typedef_enum(source, declaration_name),
        enum_origin: Some(EnumOrigin {
            header_name: header_name.to_owned(),
            kind: EnumKind::Typedef,
            declaration_name: declaration_name.to_owned(),
        }),
        enum_probe_body: None,
    }
}

fn automatic_enum_domain_name(header: &str, kind: EnumKind, declaration: &str) -> String {
    format!("public_enum/{header}/{}/{declaration}", kind.name())
}

pub(crate) fn collect(headers: &Headers, public_headers: &[PublicEnumHeader]) -> Vec<Domain> {
    let mut domains = vec![
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
        named_domain("task_mode", "emc.hh", &headers.emc, "EMC_TASK_MODE_ENUM"),
        named_domain("task_state", "emc.hh", &headers.emc, "EMC_TASK_STATE_ENUM"),
        named_domain("task_exec", "emc.hh", &headers.emc, "EMC_TASK_EXEC_ENUM"),
        named_domain(
            "task_interp",
            "emc.hh",
            &headers.emc,
            "EMC_TASK_INTERP_ENUM",
        ),
        named_domain("traj_mode", "emc.hh", &headers.emc, "EMC_TRAJ_MODE_ENUM"),
        named_domain(
            "io_abort_reason",
            "emc.hh",
            &headers.emc,
            "EMC_IO_ABORT_REASON_ENUM",
        ),
        named_domain("joint_type", "emc.hh", &headers.emc, "EmcJointType"),
        typedef_domain("motion_command", "motion.h", &headers.motion, "cmd_code_t"),
        typedef_domain(
            "motion_command_status",
            "motion.h",
            &headers.motion,
            "cmd_status_t",
        ),
        typedef_domain(
            "motion_state",
            "motion.h",
            &headers.motion,
            "motion_state_t",
        ),
        typedef_domain(
            "spindle_orient_state",
            "motion.h",
            &headers.motion,
            "orient_state_t",
        ),
        named_domain(
            "interpreter_return",
            "interp_return.hh",
            &headers.interp_return,
            "InterpReturn",
        ),
        named_domain("nml_error", "nml.hh", &headers.nml, "NML_ERROR_TYPE"),
        named_domain(
            "nml_channel_type",
            "nml.hh",
            &headers.nml,
            "NML_CHANNEL_TYPE",
        ),
        named_domain("rcs_status", "rcs.hh", &headers.rcs, "RCS_STATUS"),
        named_domain("rcs_state", "stat_msg.hh", &headers.stat_msg, "RCS_STATE"),
        named_domain("canon_bool", "canon.hh", &headers.canon, "CanonBool"),
        named_domain("canon_plane", "canon.hh", &headers.canon, "CANON_PLANE"),
        named_domain("canon_units", "canon.hh", &headers.canon, "CANON_UNITS"),
        named_domain(
            "canon_motion_mode",
            "canon.hh",
            &headers.canon,
            "CANON_MOTION_MODE",
        ),
        named_domain(
            "canon_speed_feed_mode",
            "canon.hh",
            &headers.canon,
            "CANON_SPEED_FEED_MODE",
        ),
        named_domain(
            "canon_direction",
            "canon.hh",
            &headers.canon,
            "CANON_DIRECTION",
        ),
        named_domain(
            "canon_feed_reference",
            "canon.hh",
            &headers.canon,
            "CANON_FEED_REFERENCE",
        ),
        named_domain("canon_side", "canon.hh", &headers.canon, "CANON_SIDE"),
        named_domain("canon_axis", "canon.hh", &headers.canon, "CANON_AXIS"),
        typedef_domain(
            "kinematics_type",
            "kinematics.h",
            &headers.kinematics,
            "KINEMATICS_TYPE",
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
        typedef_domain(
            "state_tag_flag",
            "state_tag.h",
            &headers.state_tag,
            "StateFlag",
        ),
        typedef_domain(
            "state_tag_field",
            "state_tag.h",
            &headers.state_tag,
            "StateField",
        ),
        typedef_domain(
            "state_tag_float_field",
            "state_tag.h",
            &headers.state_tag,
            "StateFieldFloat",
        ),
        named_domain("cms_status", "cms.hh", &headers.cms, "CMS_STATUS"),
        named_domain("cms_mode", "cms.hh", &headers.cms, "CMSMODE"),
        named_domain(
            "cms_internal_access",
            "cms.hh",
            &headers.cms,
            "CMS_INTERNAL_ACCESS_TYPE",
        ),
        named_domain("cms_buffer_type", "cms.hh", &headers.cms, "CMS_BUFFERTYPE"),
        named_domain(
            "cms_process_type",
            "cms.hh",
            &headers.cms,
            "CMS_PROCESSTYPE",
        ),
        named_domain(
            "cms_remote_port_type",
            "cms.hh",
            &headers.cms,
            "CMS_REMOTE_PORT_TYPE",
        ),
        named_domain(
            "cms_encoding",
            "cms.hh",
            &headers.cms,
            "CMS_NEUTRAL_ENCODING_METHOD",
        ),
        named_domain(
            "cms_connection_mode",
            "cms.hh",
            &headers.cms,
            "CMS_CONNECTION_MODE",
        ),
        named_domain(
            "rcs_generic_command",
            "cmd_msg.hh",
            &headers.cmd_msg,
            "RCS_GENERIC_CMD_ID",
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
    validate_curated(&domains);
    let curated_origins = domains
        .iter()
        .filter_map(|domain| domain.enum_origin.as_ref())
        .map(|origin| {
            (
                origin.header_name.clone(),
                origin.kind,
                origin.declaration_name.clone(),
            )
        })
        .collect::<BTreeSet<_>>();
    for header in public_headers {
        for declaration in enum_declarations(&header.source) {
            let origin_key = (
                header.name.clone(),
                declaration.kind,
                declaration.name.clone(),
            );
            if curated_origins.contains(&origin_key) {
                continue;
            }
            domains.push(Domain {
                name: automatic_enum_domain_name(&header.name, declaration.kind, &declaration.name),
                symbols: declaration.symbols,
                enum_origin: Some(EnumOrigin {
                    header_name: header.name.clone(),
                    kind: declaration.kind,
                    declaration_name: declaration.name,
                }),
                enum_probe_body: Some(declaration.body),
            });
        }
    }
    validate_inventory(public_headers, &domains);
    validate_core_enum_subset(headers, &domains);
    domains
}

fn validate_curated(domains: &[Domain]) {
    assert_eq!(
        domains.len(),
        EXPECTED_DOMAIN_COUNTS.len(),
        "the curated LinuxCNC code-domain inventory changed"
    );
    let mut seen_domain_names = BTreeSet::new();
    for domain in domains {
        assert!(
            seen_domain_names.insert(domain.name.as_str()),
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
        .map(|domain| (domain.name.as_str(), domain.symbols.len()))
        .collect::<Vec<_>>();
    assert_eq!(
        actual_domain_counts, EXPECTED_DOMAIN_COUNTS,
        "LinuxCNC 2.9.10 public code domains changed or the source parser omitted a code"
    );
}

fn validate_inventory(public_headers: &[PublicEnumHeader], domains: &[Domain]) {
    let declared_enum_list = public_headers
        .iter()
        .flat_map(|header| {
            enum_declarations(&header.source)
                .into_iter()
                .map(move |declaration| (header.name.clone(), declaration.kind, declaration.name))
        })
        .collect::<Vec<_>>();
    let declared_enums = declared_enum_list.iter().cloned().collect::<BTreeSet<_>>();
    assert_eq!(
        declared_enums.len(),
        declared_enum_list.len(),
        "a LinuxCNC public header declares the same enum identity twice"
    );
    let mapped_enum_list = domains
        .iter()
        .filter_map(|domain| domain.enum_origin.as_ref())
        .map(|origin| {
            (
                origin.header_name.clone(),
                origin.kind,
                origin.declaration_name.clone(),
            )
        })
        .collect::<Vec<_>>();
    let mapped_enums = mapped_enum_list.iter().cloned().collect::<BTreeSet<_>>();
    assert_eq!(
        mapped_enums.len(),
        mapped_enum_list.len(),
        "a LinuxCNC public enum declaration was mapped more than once"
    );
    assert_eq!(
        mapped_enums, declared_enums,
        "a LinuxCNC public enum declaration is missing from the generated code catalog"
    );
    let duplicate_probe_names = domains
        .iter()
        .map(|domain| domain.name.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        duplicate_probe_names.len(),
        domains.len(),
        "two LinuxCNC public enum declarations generated the same domain name"
    );
}

/*
    The 17 core headers remain available separately for ABI fingerprints and
    message-layout probes. Their enum declarations are a strict subset of the
    source-discovered public-header inventory above.
*/
fn validate_core_enum_subset(headers: &Headers, domains: &[Domain]) {
    let declared_enums = headers
        .named()
        .into_iter()
        .flat_map(|(header_name, source)| {
            enum_declarations(source)
                .into_iter()
                .map(move |declaration| (header_name, declaration.kind, declaration.name))
        })
        .collect::<BTreeSet<_>>();
    let mapped_enums = domains
        .iter()
        .filter_map(|domain| domain.enum_origin.as_ref())
        .map(|origin| {
            (
                origin.header_name.as_str(),
                origin.kind,
                origin.declaration_name.clone(),
            )
        })
        .collect::<BTreeSet<_>>();
    assert!(
        declared_enums.is_subset(&mapped_enums),
        "a core LinuxCNC enum declaration is missing from the public enum inventory"
    );
}
