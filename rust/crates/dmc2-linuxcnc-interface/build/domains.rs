use std::collections::BTreeSet;

use super::config::EXPECTED_DOMAIN_COUNTS;
use super::parser::{macro_names, named_enum, typedef_enum};
use super::source::Headers;

pub(crate) struct Domain {
    pub(crate) name: String,
    pub(crate) symbols: Vec<String>,
}

fn domain(name: &'static str, symbols: Vec<String>) -> Domain {
    Domain {
        name: name.to_owned(),
        symbols,
    }
}

fn named_domain(name: &'static str, source: &str, declaration_name: &'static str) -> Domain {
    domain(
        name,
        named_enum(source, &format!("enum {declaration_name}")),
    )
}

fn typedef_domain(name: &'static str, source: &str, declaration_name: &'static str) -> Domain {
    domain(name, typedef_enum(source, declaration_name))
}

pub(crate) fn collect(headers: &Headers) -> Vec<Domain> {
    let domains = vec![
        domain(
            "emc_nml_message_type",
            macro_names(&headers.emc, |name, value| {
                name.starts_with("EMC_") && name.ends_with("_TYPE") && value.contains("NMLTYPE")
            }),
        ),
        named_domain("task_mode", &headers.emc, "EMC_TASK_MODE_ENUM"),
        named_domain("task_state", &headers.emc, "EMC_TASK_STATE_ENUM"),
        named_domain("task_exec", &headers.emc, "EMC_TASK_EXEC_ENUM"),
        named_domain("task_interp", &headers.emc, "EMC_TASK_INTERP_ENUM"),
        named_domain("traj_mode", &headers.emc, "EMC_TRAJ_MODE_ENUM"),
        named_domain("joint_type", &headers.emc, "EmcJointType"),
        typedef_domain("motion_command", &headers.motion, "cmd_code_t"),
        typedef_domain("spindle_orient_state", &headers.motion, "orient_state_t"),
        named_domain("interpreter_return", &headers.interp_return, "InterpReturn"),
        named_domain("nml_error", &headers.nml, "NML_ERROR_TYPE"),
        named_domain("rcs_status", &headers.rcs, "RCS_STATUS"),
        named_domain("rcs_state", &headers.stat_msg, "RCS_STATE"),
        named_domain("canon_units", &headers.canon, "CANON_UNITS"),
        typedef_domain("kinematics_type", &headers.kinematics, "KINEMATICS_TYPE"),
        domain(
            "motion_type",
            macro_names(&headers.motion_types, |name, _| {
                name.starts_with("EMC_MOTION_TYPE_")
            }),
        ),
        domain(
            "debug_flag",
            macro_names(&headers.debug_flags, |name, _| {
                name.starts_with("EMC_DEBUG_")
            }),
        ),
        typedef_domain("state_tag_flag", &headers.state_tag, "StateFlag"),
        named_domain(
            "rcs_generic_command",
            &headers.cmd_msg,
            "RCS_GENERIC_CMD_ID",
        ),
    ];
    validate(&domains);
    domains
}

fn validate(domains: &[Domain]) {
    assert_eq!(
        domains.len(),
        EXPECTED_DOMAIN_COUNTS.len(),
        "the controller's LinuxCNC value-domain inventory changed"
    );
    let mut seen_domain_names = BTreeSet::new();
    for domain in domains {
        assert!(
            seen_domain_names.insert(domain.name.as_str()),
            "duplicate controller interface domain {}",
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
    let actual = domains
        .iter()
        .map(|domain| (domain.name.as_str(), domain.symbols.len()))
        .collect::<Vec<_>>();
    assert_eq!(
        actual, EXPECTED_DOMAIN_COUNTS,
        "LinuxCNC 2.9.10 changed a value domain consumed by the controller"
    );
}
