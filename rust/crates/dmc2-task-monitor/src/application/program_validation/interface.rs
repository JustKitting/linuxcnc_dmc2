use std::collections::BTreeSet;

use dmc2_linuxcnc_interface::{
    domain, error_message_contract, status_message_contract, DOMAINS, EMC_NML_MESSAGE_TYPE,
    ENUM_CODE_COUNT, ENUM_DECLARATION_COUNT, ENUM_DOMAIN_CONTRACTS, ERROR_MESSAGE_CONTRACTS,
    ERROR_MESSAGE_CONTRACT_COUNT, GENERATED_CODE_COUNT, INTERPRETER_ERROR_TEMPLATES,
    LINUXCNC_SOURCE_COMMIT, LINUXCNC_VERSION, NML_OPERATOR_MESSAGE_TYPE, NON_ENUM_CODE_COUNT,
    PUBLIC_ENUM_HEADERS, PUBLIC_ENUM_HEADER_COUNT, STATUS_MESSAGE_CONTRACTS,
    STATUS_MESSAGE_CONTRACT_COUNT,
};

const AUDITED_DOMAIN_COUNT: usize = 91;
const AUDITED_CODE_COUNT: usize = 920;
const AUDITED_ENUM_CODE_COUNT: usize = 711;
const AUDITED_NON_ENUM_CODE_COUNT: usize = 209;
const AUDITED_ENUM_HEADER_COUNT: usize = 30;
const AUDITED_ENUM_DECLARATION_COUNT: usize = 79;
const AUDITED_INTERPRETER_ERROR_COUNT: usize = 198;
const AUDITED_STATUS_MESSAGE_COUNT: usize = 12;
const AUDITED_ERROR_MESSAGE_COUNT: usize = 6;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct InterfaceCoverage {
    pub(super) domain_count: usize,
    pub(super) code_count: usize,
    pub(super) handled_code_count: usize,
    pub(super) enum_code_count: usize,
    pub(super) non_enum_code_count: usize,
    pub(super) enum_header_count: usize,
    pub(super) enum_declaration_count: usize,
    pub(super) interpreter_error_count: usize,
    pub(super) handled_interpreter_error_count: usize,
    pub(super) status_message_count: usize,
    pub(super) error_message_count: usize,
}

fn validate_codes() -> Result<usize, String> {
    if DOMAINS.len() != AUDITED_DOMAIN_COUNT {
        return Err(format!(
            "LinuxCNC domain inventory has {} entries, expected {AUDITED_DOMAIN_COUNT}",
            DOMAINS.len()
        ));
    }
    let mut domain_names = BTreeSet::new();
    let mut handled = 0;
    for code_domain in DOMAINS {
        if code_domain.name.is_empty() || code_domain.codes.is_empty() {
            return Err(format!(
                "empty LinuxCNC code domain: {:?}",
                code_domain.name
            ));
        }
        if !domain_names.insert(code_domain.name) {
            return Err(format!(
                "duplicate LinuxCNC code domain: {}",
                code_domain.name
            ));
        }
        if domain(code_domain.name).map(|entry| entry.name) != Some(code_domain.name) {
            return Err(format!(
                "LinuxCNC domain dispatcher omitted {}",
                code_domain.name
            ));
        }
        let mut code_names = BTreeSet::new();
        for code in code_domain.codes {
            if code.name.is_empty() || !code_names.insert(code.name) {
                return Err(format!(
                    "empty or duplicate code name in {}: {:?}",
                    code_domain.name, code.name
                ));
            }
            if !code_domain
                .names(code.code)
                .any(|candidate| candidate == code.name)
            {
                return Err(format!(
                    "LinuxCNC code dispatcher omitted {}::{}={}",
                    code_domain.name, code.name, code.code
                ));
            }
            handled += 1;
        }
        if code_domain.lookup(i64::MIN).is_some() || code_domain.lookup(i64::MAX).is_some() {
            return Err(format!(
                "LinuxCNC domain {} mislabeled an unknown boundary value",
                code_domain.name
            ));
        }
    }
    if handled != GENERATED_CODE_COUNT || handled != AUDITED_CODE_COUNT {
        return Err(format!(
            "LinuxCNC code dispatcher handled {handled} entries, generated {GENERATED_CODE_COUNT}, expected {AUDITED_CODE_COUNT}"
        ));
    }
    Ok(handled)
}

fn validate_enum_inventory() -> Result<(), String> {
    if PUBLIC_ENUM_HEADERS.len() != PUBLIC_ENUM_HEADER_COUNT
        || PUBLIC_ENUM_HEADER_COUNT != AUDITED_ENUM_HEADER_COUNT
    {
        return Err(format!(
            "LinuxCNC public enum header inventory has {} entries, generated {PUBLIC_ENUM_HEADER_COUNT}, expected {AUDITED_ENUM_HEADER_COUNT}",
            PUBLIC_ENUM_HEADERS.len()
        ));
    }
    let declaration_total = PUBLIC_ENUM_HEADERS
        .iter()
        .map(|header| header.declaration_count)
        .sum::<usize>();
    if declaration_total != ENUM_DECLARATION_COUNT
        || ENUM_DECLARATION_COUNT != AUDITED_ENUM_DECLARATION_COUNT
        || ENUM_DOMAIN_CONTRACTS.len() != ENUM_DECLARATION_COUNT
    {
        return Err(format!(
            "LinuxCNC enum inventory has {declaration_total} declarations, {} contracts, generated {ENUM_DECLARATION_COUNT}, expected {AUDITED_ENUM_DECLARATION_COUNT}",
            ENUM_DOMAIN_CONTRACTS.len()
        ));
    }
    let mut origins = BTreeSet::new();
    for contract in ENUM_DOMAIN_CONTRACTS {
        let origin = (
            contract.header_name,
            contract.declaration_kind,
            contract.declaration_name,
        );
        if contract.header_name.is_empty()
            || contract.declaration_name.is_empty()
            || !matches!(contract.declaration_kind, "anonymous" | "named" | "typedef")
            || !origins.insert(origin)
            || domain(contract.domain_name).is_none()
        {
            return Err(format!("invalid LinuxCNC enum contract: {origin:?}"));
        }
    }
    if ENUM_CODE_COUNT != AUDITED_ENUM_CODE_COUNT
        || NON_ENUM_CODE_COUNT != AUDITED_NON_ENUM_CODE_COUNT
        || ENUM_CODE_COUNT + NON_ENUM_CODE_COUNT != GENERATED_CODE_COUNT
    {
        return Err(format!(
            "LinuxCNC enum/non-enum code totals are {ENUM_CODE_COUNT}/{NON_ENUM_CODE_COUNT}, expected {AUDITED_ENUM_CODE_COUNT}/{AUDITED_NON_ENUM_CODE_COUNT}"
        ));
    }
    Ok(())
}

fn validate_interpreter_errors() -> Result<usize, String> {
    if INTERPRETER_ERROR_TEMPLATES.len() != AUDITED_INTERPRETER_ERROR_COUNT {
        return Err(format!(
            "LinuxCNC interpreter error inventory has {} entries, expected {AUDITED_INTERPRETER_ERROR_COUNT}",
            INTERPRETER_ERROR_TEMPLATES.len()
        ));
    }
    let mut names = BTreeSet::new();
    for entry in INTERPRETER_ERROR_TEMPLATES {
        if !entry.name.starts_with("NCE_") || entry.template.is_empty() || !names.insert(entry.name)
        {
            return Err(format!(
                "invalid LinuxCNC interpreter error contract: {}",
                entry.name
            ));
        }
    }
    Ok(names.len())
}

fn validate_message_contracts() -> Result<(), String> {
    if STATUS_MESSAGE_CONTRACTS.len() != STATUS_MESSAGE_CONTRACT_COUNT
        || STATUS_MESSAGE_CONTRACT_COUNT != AUDITED_STATUS_MESSAGE_COUNT
    {
        return Err(format!(
            "LinuxCNC status layout inventory has {} entries, expected {AUDITED_STATUS_MESSAGE_COUNT}",
            STATUS_MESSAGE_CONTRACTS.len()
        ));
    }
    let mut status_classes = BTreeSet::new();
    let mut status_types = BTreeSet::new();
    for contract in STATUS_MESSAGE_CONTRACTS {
        if contract.message_size <= 0
            || !status_classes.insert(contract.class_name)
            || !status_types.insert(contract.message_type)
            || EMC_NML_MESSAGE_TYPE.lookup(contract.message_type)
                != Some(contract.message_type_name)
            || status_message_contract(contract.class_name) != Some(*contract)
        {
            return Err(format!(
                "invalid LinuxCNC status layout contract: {}",
                contract.class_name
            ));
        }
    }

    if ERROR_MESSAGE_CONTRACTS.len() != ERROR_MESSAGE_CONTRACT_COUNT
        || ERROR_MESSAGE_CONTRACT_COUNT != AUDITED_ERROR_MESSAGE_COUNT
    {
        return Err(format!(
            "LinuxCNC error layout inventory has {} entries, expected {AUDITED_ERROR_MESSAGE_COUNT}",
            ERROR_MESSAGE_CONTRACTS.len()
        ));
    }
    let mut error_classes = BTreeSet::new();
    let mut error_types = BTreeSet::new();
    for contract in ERROR_MESSAGE_CONTRACTS {
        let expected_name = if contract.class_name.starts_with("NML_") {
            NML_OPERATOR_MESSAGE_TYPE.lookup(contract.message_type)
        } else {
            EMC_NML_MESSAGE_TYPE.lookup(contract.message_type)
        };
        let payload_end = contract
            .payload_offset
            .checked_add(contract.payload_size)
            .ok_or_else(|| format!("error payload range overflow: {}", contract.class_name))?;
        if contract.message_size < payload_end
            || expected_name != Some(contract.message_type_name)
            || !error_classes.insert(contract.class_name)
            || !error_types.insert(contract.message_type)
            || error_message_contract(contract.class_name) != Some(*contract)
        {
            return Err(format!(
                "invalid LinuxCNC error layout contract: {}",
                contract.class_name
            ));
        }
    }
    Ok(())
}

impl InterfaceCoverage {
    pub(super) fn collect() -> Result<Self, String> {
        if LINUXCNC_VERSION != "2.9.10"
            || LINUXCNC_SOURCE_COMMIT != "86cdca76fa2a36274c432caa21952b23c267989a"
        {
            return Err(format!(
                "unaudited LinuxCNC source: version={LINUXCNC_VERSION} commit={LINUXCNC_SOURCE_COMMIT}"
            ));
        }
        let handled_code_count = validate_codes()?;
        validate_enum_inventory()?;
        let handled_interpreter_error_count = validate_interpreter_errors()?;
        validate_message_contracts()?;
        Ok(Self {
            domain_count: DOMAINS.len(),
            code_count: GENERATED_CODE_COUNT,
            handled_code_count,
            enum_code_count: ENUM_CODE_COUNT,
            non_enum_code_count: NON_ENUM_CODE_COUNT,
            enum_header_count: PUBLIC_ENUM_HEADER_COUNT,
            enum_declaration_count: ENUM_DECLARATION_COUNT,
            interpreter_error_count: INTERPRETER_ERROR_TEMPLATES.len(),
            handled_interpreter_error_count,
            status_message_count: STATUS_MESSAGE_CONTRACTS.len(),
            error_message_count: ERROR_MESSAGE_CONTRACTS.len(),
        })
    }
}
