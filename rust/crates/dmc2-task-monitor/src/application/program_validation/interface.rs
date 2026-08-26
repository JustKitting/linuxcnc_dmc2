use std::collections::BTreeSet;

use dmc2_linuxcnc_interface::{
    domain, error_message_contract, error_message_contract_by_type, public_integer_macro_names,
    public_integer_macro_names_i128, public_macro, status_message_contract, ErrorMessageContract,
    PublicHeaderContract, PublicMacroContract, PublicMacroKind, DOMAINS, EMC_NML_MESSAGE_TYPE,
    ENUM_CODE_COUNT, ENUM_DECLARATION_COUNT, ENUM_DOMAIN_CONTRACTS, ERROR_MESSAGE_CONTRACTS,
    ERROR_MESSAGE_CONTRACT_COUNT, GENERATED_CODE_COUNT, INTERPRETER_ERROR_TEMPLATES,
    LINUXCNC_SOURCE_COMMIT, LINUXCNC_VERSION, NML_OPERATOR_MESSAGE_TYPE, NON_ENUM_CODE_COUNT,
    PUBLIC_ENUM_HEADERS, PUBLIC_ENUM_HEADER_COUNT, PUBLIC_HEADERS, PUBLIC_HEADER_COUNT,
    PUBLIC_HEADER_SOURCE_BYTE_COUNT, PUBLIC_HEADER_SOURCE_FNV64, PUBLIC_INTEGER_MACRO_COUNT,
    PUBLIC_MACROS, PUBLIC_MACRO_DECLARATION_COUNT, PUBLIC_MACRO_NAME_COUNT,
    STATUS_MESSAGE_CONTRACTS, STATUS_MESSAGE_CONTRACT_COUNT,
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
const AUDITED_ERROR_MESSAGE_OBJECT_BYTES: usize = 1_656;
const AUDITED_ERROR_MESSAGE_FIELD_BYTES: usize = 1_629;
const AUDITED_ERROR_MESSAGE_PADDING_BYTES: usize = 27;
const AUDITED_PUBLIC_HEADER_COUNT: usize = 120;
const AUDITED_PUBLIC_HEADER_SOURCE_BYTE_COUNT: usize = 635_278;
const AUDITED_PUBLIC_HEADER_SOURCE_FNV64: u64 = 0x8f2986fcf6b52329;
const AUDITED_PUBLIC_MACRO_DECLARATION_COUNT: usize = 1_106;
const AUDITED_PUBLIC_MACRO_NAME_COUNT: usize = 1_029;
const AUDITED_PUBLIC_INTEGER_MACRO_COUNT: usize = 481;
const AUDITED_PUBLIC_MACRO_KIND_COUNTS: [usize; 6] = [86, 120, 126, 166, 315, 216];

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
    pub(super) error_message_object_bytes: usize,
    pub(super) error_message_field_bytes: usize,
    pub(super) error_message_padding_bytes: usize,
    pub(super) public_header_count: usize,
    pub(super) public_header_source_byte_count: usize,
    pub(super) public_header_source_fnv64: u64,
    pub(super) public_macro_declaration_count: usize,
    pub(super) public_macro_name_count: usize,
    pub(super) public_macro_kind_counts: [usize; 6],
    pub(super) public_integer_macro_count: usize,
    pub(super) handled_public_integer_macro_count: usize,
}

fn validate_public_macros(
    headers: &[PublicHeaderContract],
    macros: &[PublicMacroContract],
) -> Result<(usize, [usize; 6]), String> {
    if PUBLIC_HEADER_COUNT != AUDITED_PUBLIC_HEADER_COUNT
        || headers.len() != PUBLIC_HEADER_COUNT
        || PUBLIC_HEADER_SOURCE_BYTE_COUNT != AUDITED_PUBLIC_HEADER_SOURCE_BYTE_COUNT
        || PUBLIC_HEADER_SOURCE_FNV64 != AUDITED_PUBLIC_HEADER_SOURCE_FNV64
    {
        return Err(format!(
            "LinuxCNC public header inventory changed: headers={}/{} bytes={} fingerprint=0x{:016x}",
            headers.len(),
            PUBLIC_HEADER_COUNT,
            PUBLIC_HEADER_SOURCE_BYTE_COUNT,
            PUBLIC_HEADER_SOURCE_FNV64
        ));
    }
    let mut header_names = BTreeSet::new();
    let mut source_paths = BTreeSet::new();
    let mut source_byte_count = 0_usize;
    let mut declaration_count = 0_usize;
    let mut name_count = 0_usize;
    for header in headers {
        if header.header_name.is_empty()
            || !header.source_relative_path.starts_with("src/")
            || header.source_byte_count == 0
            || header.source_fnv64 == 0
            || !header_names.insert(header.header_name)
            || !source_paths.insert(header.source_relative_path)
        {
            return Err(format!(
                "invalid LinuxCNC public header contract: {}",
                header.header_name
            ));
        }
        source_byte_count = source_byte_count
            .checked_add(header.source_byte_count)
            .ok_or_else(|| "LinuxCNC public header byte total overflow".to_owned())?;
        declaration_count = declaration_count
            .checked_add(header.macro_declaration_count)
            .ok_or_else(|| "LinuxCNC public macro declaration total overflow".to_owned())?;
        name_count = name_count
            .checked_add(header.macro_name_count)
            .ok_or_else(|| "LinuxCNC public macro name total overflow".to_owned())?;
    }
    if source_byte_count != PUBLIC_HEADER_SOURCE_BYTE_COUNT
        || declaration_count != PUBLIC_MACRO_DECLARATION_COUNT
        || name_count != PUBLIC_MACRO_NAME_COUNT
        || PUBLIC_MACRO_DECLARATION_COUNT != AUDITED_PUBLIC_MACRO_DECLARATION_COUNT
        || PUBLIC_MACRO_NAME_COUNT != AUDITED_PUBLIC_MACRO_NAME_COUNT
        || macros.len() != PUBLIC_MACRO_NAME_COUNT
    {
        return Err(format!(
            "LinuxCNC public source totals changed: bytes={source_byte_count} declarations={declaration_count} names={name_count} contracts={}",
            macros.len()
        ));
    }

    let mut keys = BTreeSet::new();
    let mut kind_counts = [0_usize; 6];
    let mut handled_integer_count = 0_usize;
    for contract in macros {
        if !header_names.contains(contract.header_name)
            || contract.name.is_empty()
            || contract.declaration_count == 0
            || contract.declaration_count
                != contract.object_declaration_count + contract.function_declaration_count
            || !keys.insert((contract.header_name, contract.name))
            || public_macro(contract.header_name, contract.name) != Some(*contract)
        {
            return Err(format!(
                "invalid or undispatchable LinuxCNC public macro: {}::{}",
                contract.header_name, contract.name
            ));
        }
        let index = match contract.kind {
            PublicMacroKind::Inactive
                if contract.active_replacement.is_none() && contract.value.is_none() =>
            {
                0
            }
            PublicMacroKind::FunctionLike
                if contract.active_replacement.is_some() && contract.value.is_none() =>
            {
                1
            }
            PublicMacroKind::ObjectWithoutValue
                if contract.active_replacement == Some("") && contract.value.is_none() =>
            {
                2
            }
            PublicMacroKind::SignedInteger
                if matches!(
                    contract.value,
                    Some(dmc2_linuxcnc_interface::PublicInteger::Signed(_))
                ) =>
            {
                3
            }
            PublicMacroKind::UnsignedInteger
                if matches!(
                    contract.value,
                    Some(dmc2_linuxcnc_interface::PublicInteger::Unsigned(_))
                ) =>
            {
                4
            }
            PublicMacroKind::ObjectNotIntegerConstant
                if contract
                    .active_replacement
                    .is_some_and(|replacement| !replacement.is_empty())
                    && contract.value.is_none() =>
            {
                5
            }
            _ => {
                return Err(format!(
                    "inconsistent LinuxCNC public macro classification: {}::{}",
                    contract.header_name, contract.name
                ));
            }
        };
        kind_counts[index] += 1;
        if let Some(value) = contract.value {
            if !public_integer_macro_names(contract.header_name, value)
                .any(|name| name == contract.name)
            {
                return Err(format!(
                    "LinuxCNC integer macro dispatcher omitted {}::{}",
                    contract.header_name, contract.name
                ));
            }
            if let Some(value) = value.as_i128() {
                if !public_integer_macro_names_i128(contract.header_name, value)
                    .any(|name| name == contract.name)
                {
                    return Err(format!(
                        "LinuxCNC runtime numeric dispatcher omitted {}::{}",
                        contract.header_name, contract.name
                    ));
                }
            }
            handled_integer_count += 1;
        }
    }
    for header in headers {
        let header_macros = macros
            .iter()
            .filter(|contract| contract.header_name == header.header_name);
        if header_macros.clone().count() != header.macro_name_count
            || header_macros
                .map(|contract| contract.declaration_count)
                .sum::<usize>()
                != header.macro_declaration_count
        {
            return Err(format!(
                "LinuxCNC public header macro ownership changed: {}",
                header.header_name
            ));
        }
    }
    if kind_counts != AUDITED_PUBLIC_MACRO_KIND_COUNTS
        || PUBLIC_INTEGER_MACRO_COUNT != AUDITED_PUBLIC_INTEGER_MACRO_COUNT
        || handled_integer_count != PUBLIC_INTEGER_MACRO_COUNT
    {
        return Err(format!(
            "LinuxCNC public macro coverage changed: kinds={kind_counts:?} handled_integers={handled_integer_count} generated_integers={PUBLIC_INTEGER_MACRO_COUNT}"
        ));
    }
    if public_macro("not-a-header", "not-a-macro").is_some()
        || public_integer_macro_names_i128("not-a-header", i128::MAX)
            .next()
            .is_some()
    {
        return Err("LinuxCNC public macro dispatcher mislabeled an unknown value".to_owned());
    }
    Ok((handled_integer_count, kind_counts))
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

fn claim_message_bytes(
    owners: &mut [Option<&'static str>],
    class_name: &str,
    member_name: &'static str,
    offset: usize,
    size: usize,
) -> Result<(), String> {
    if size == 0 {
        return Err(format!(
            "LinuxCNC error message member is empty: {class_name}.{member_name}"
        ));
    }
    let end = offset.checked_add(size).ok_or_else(|| {
        format!("LinuxCNC error message member range overflow: {class_name}.{member_name}")
    })?;
    if end > owners.len() {
        return Err(format!(
            "LinuxCNC error message member is outside its object: {class_name}.{member_name}"
        ));
    }
    for owner in &mut owners[offset..end] {
        if let Some(previous) = owner {
            return Err(format!(
                "LinuxCNC error message members overlap: {class_name}.{previous} and {class_name}.{member_name}"
            ));
        }
        *owner = Some(member_name);
    }
    Ok(())
}

fn validate_message_contracts(
    error_contracts: &[ErrorMessageContract],
) -> Result<(usize, usize, usize), String> {
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

    if error_contracts.len() != ERROR_MESSAGE_CONTRACT_COUNT
        || ERROR_MESSAGE_CONTRACT_COUNT != AUDITED_ERROR_MESSAGE_COUNT
    {
        return Err(format!(
            "LinuxCNC error layout inventory has {} entries, expected {AUDITED_ERROR_MESSAGE_COUNT}",
            error_contracts.len()
        ));
    }
    let mut error_classes = BTreeSet::new();
    let mut error_types = BTreeSet::new();
    let mut object_bytes = 0_usize;
    let mut field_bytes = 0_usize;
    let mut padding_bytes = 0_usize;
    for contract in error_contracts {
        let is_nml_operator_message = contract.class_name.starts_with("NML_");
        let expected_name = if is_nml_operator_message {
            NML_OPERATOR_MESSAGE_TYPE.lookup(contract.message_type)
        } else {
            EMC_NML_MESSAGE_TYPE.lookup(contract.message_type)
        };
        let expected_optional_members = if is_nml_operator_message {
            (None, None)
        } else {
            (Some("serial_number"), Some("id"))
        };
        if expected_name != Some(contract.message_type_name)
            || !error_classes.insert(contract.class_name)
            || !error_types.insert(contract.message_type)
            || error_message_contract(contract.class_name) != Some(*contract)
            || error_message_contract_by_type(contract.message_type) != Some(*contract)
            || contract.serial_member != expected_optional_members.0
            || contract.id_member != expected_optional_members.1
            || contract.serial_member.is_some() != contract.serial_offset.is_some()
            || contract.id_member.is_some() != contract.id_offset.is_some()
            || (contract.serial_member.is_none() && contract.serial_size != 0)
            || (contract.id_member.is_none() && contract.id_size != 0)
        {
            return Err(format!(
                "invalid LinuxCNC error layout contract: {}",
                contract.class_name
            ));
        }

        let mut owners = vec![None; contract.message_size];
        claim_message_bytes(
            &mut owners,
            contract.class_name,
            "type",
            contract.type_offset,
            contract.type_size,
        )?;
        claim_message_bytes(
            &mut owners,
            contract.class_name,
            "size",
            contract.size_offset,
            contract.size_size,
        )?;
        if let Some(offset) = contract.serial_offset {
            claim_message_bytes(
                &mut owners,
                contract.class_name,
                "serial_number",
                offset,
                contract.serial_size,
            )?;
        }
        if let Some(offset) = contract.id_offset {
            claim_message_bytes(
                &mut owners,
                contract.class_name,
                "id",
                offset,
                contract.id_size,
            )?;
        }
        claim_message_bytes(
            &mut owners,
            contract.class_name,
            contract.payload_member,
            contract.payload_offset,
            contract.payload_size,
        )?;

        let contract_field_bytes = owners.iter().filter(|owner| owner.is_some()).count();
        let contract_padding_bytes = owners.len() - contract_field_bytes;
        object_bytes = object_bytes
            .checked_add(owners.len())
            .ok_or_else(|| "LinuxCNC error message object-byte total overflow".to_owned())?;
        field_bytes = field_bytes
            .checked_add(contract_field_bytes)
            .ok_or_else(|| "LinuxCNC error message field-byte total overflow".to_owned())?;
        padding_bytes = padding_bytes
            .checked_add(contract_padding_bytes)
            .ok_or_else(|| "LinuxCNC error message padding-byte total overflow".to_owned())?;
    }
    if error_message_contract_by_type(i64::MIN).is_some()
        || error_message_contract_by_type(i64::MAX).is_some()
        || object_bytes != AUDITED_ERROR_MESSAGE_OBJECT_BYTES
        || field_bytes != AUDITED_ERROR_MESSAGE_FIELD_BYTES
        || padding_bytes != AUDITED_ERROR_MESSAGE_PADDING_BYTES
        || field_bytes + padding_bytes != object_bytes
    {
        return Err(format!(
            "LinuxCNC error message byte coverage changed: objects={object_bytes} fields={field_bytes} padding={padding_bytes}"
        ));
    }
    Ok((object_bytes, field_bytes, padding_bytes))
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
        let (handled_public_integer_macro_count, public_macro_kind_counts) =
            validate_public_macros(PUBLIC_HEADERS, PUBLIC_MACROS)?;
        let handled_interpreter_error_count = validate_interpreter_errors()?;
        let (error_message_object_bytes, error_message_field_bytes, error_message_padding_bytes) =
            validate_message_contracts(ERROR_MESSAGE_CONTRACTS)?;
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
            error_message_object_bytes,
            error_message_field_bytes,
            error_message_padding_bytes,
            public_header_count: PUBLIC_HEADERS.len(),
            public_header_source_byte_count: PUBLIC_HEADER_SOURCE_BYTE_COUNT,
            public_header_source_fnv64: PUBLIC_HEADER_SOURCE_FNV64,
            public_macro_declaration_count: PUBLIC_MACRO_DECLARATION_COUNT,
            public_macro_name_count: PUBLIC_MACRO_NAME_COUNT,
            public_macro_kind_counts,
            public_integer_macro_count: PUBLIC_INTEGER_MACRO_COUNT,
            handled_public_integer_macro_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_public_macro_inventory_is_fully_dispatched() {
        let (handled, kinds) = validate_public_macros(PUBLIC_HEADERS, PUBLIC_MACROS).unwrap();
        assert_eq!(handled, 481);
        assert_eq!(kinds, [86, 120, 126, 166, 315, 216]);
    }

    #[test]
    fn missing_or_duplicate_public_macro_contract_is_rejected() {
        let mut missing = PUBLIC_MACROS.to_vec();
        missing.pop();
        assert!(validate_public_macros(PUBLIC_HEADERS, &missing).is_err());

        let mut duplicate = PUBLIC_MACROS.to_vec();
        duplicate[1] = duplicate[0];
        assert!(validate_public_macros(PUBLIC_HEADERS, &duplicate).is_err());
    }

    #[test]
    fn mutated_public_macro_classification_is_rejected() {
        let mut macros = PUBLIC_MACROS.to_vec();
        let contract = macros
            .iter_mut()
            .find(|contract| contract.header_name == "posemath.h" && contract.name == "PM_ERR")
            .unwrap();
        contract.kind = PublicMacroKind::FunctionLike;
        assert!(validate_public_macros(PUBLIC_HEADERS, &macros).is_err());
    }

    #[test]
    fn mutated_public_header_byte_ownership_is_rejected() {
        let mut headers = PUBLIC_HEADERS.to_vec();
        headers[0].source_byte_count += 1;
        assert!(validate_public_macros(&headers, PUBLIC_MACROS).is_err());
    }

    #[test]
    fn complete_error_message_objects_are_byte_accounted() {
        assert_eq!(
            validate_message_contracts(ERROR_MESSAGE_CONTRACTS).unwrap(),
            (1_656, 1_629, 27)
        );
    }

    #[test]
    fn missing_duplicate_or_mutated_error_message_contract_is_rejected() {
        let mut missing = ERROR_MESSAGE_CONTRACTS.to_vec();
        missing.pop();
        assert!(validate_message_contracts(&missing).is_err());

        let mut duplicate = ERROR_MESSAGE_CONTRACTS.to_vec();
        duplicate[1] = duplicate[0];
        assert!(validate_message_contracts(&duplicate).is_err());

        let mut mutated = ERROR_MESSAGE_CONTRACTS.to_vec();
        mutated[0].payload_offset = mutated[0].type_offset;
        assert!(validate_message_contracts(&mutated).is_err());
    }
}
