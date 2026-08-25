#![no_std]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodeName {
    pub code: i64,
    pub name: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct CodeDomain {
    pub name: &'static str,
    pub codes: &'static [CodeName],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MessageTemplate {
    pub name: &'static str,
    pub template: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatusMessageContract {
    pub class_name: &'static str,
    pub message_type_name: &'static str,
    pub message_type: i64,
    pub message_size: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ErrorMessageContract {
    pub class_name: &'static str,
    pub message_type_name: &'static str,
    pub message_type: i64,
    pub message_size: usize,
    pub payload_member: &'static str,
    pub payload_offset: usize,
    pub payload_size: usize,
    pub id_member: Option<&'static str>,
    pub id_offset: Option<usize>,
    pub id_size: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnumDomainContract {
    pub header_name: &'static str,
    pub declaration_kind: &'static str,
    pub declaration_name: &'static str,
    pub domain_name: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublicEnumHeaderContract {
    pub header_name: &'static str,
    pub declaration_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PublicInteger {
    Signed(i128),
    Unsigned(u128),
}

impl PublicInteger {
    pub const fn as_i128(self) -> Option<i128> {
        match self {
            Self::Signed(value) => Some(value),
            Self::Unsigned(value) if value <= i128::MAX as u128 => Some(value as i128),
            Self::Unsigned(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicMacroKind {
    Inactive,
    FunctionLike,
    ObjectWithoutValue,
    SignedInteger,
    UnsignedInteger,
    ObjectNotIntegerConstant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublicMacroContract {
    pub header_name: &'static str,
    pub name: &'static str,
    pub declaration_count: usize,
    pub object_declaration_count: usize,
    pub function_declaration_count: usize,
    pub active_replacement: Option<&'static str>,
    pub kind: PublicMacroKind,
    pub value: Option<PublicInteger>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublicHeaderContract {
    pub header_name: &'static str,
    pub source_relative_path: &'static str,
    pub source_byte_count: usize,
    pub source_fnv64: u64,
    pub macro_declaration_count: usize,
    pub macro_name_count: usize,
}

impl CodeDomain {
    pub fn names(self, code: i64) -> impl Iterator<Item = &'static str> {
        self.codes
            .iter()
            .filter(move |entry| entry.code == code)
            .map(|entry| entry.name)
    }

    pub fn lookup(self, code: i64) -> Option<&'static str> {
        self.names(code).next()
    }

    pub fn contains(self, code: i64) -> bool {
        self.lookup(code).is_some()
    }
}

include!(concat!(env!("OUT_DIR"), "/linuxcnc_code_catalog.rs"));

pub fn domain(name: &str) -> Option<CodeDomain> {
    DOMAINS.iter().copied().find(|domain| domain.name == name)
}

pub fn status_message_contract(class_name: &str) -> Option<StatusMessageContract> {
    STATUS_MESSAGE_CONTRACTS
        .iter()
        .copied()
        .find(|contract| contract.class_name == class_name)
}

pub fn error_message_contract(class_name: &str) -> Option<ErrorMessageContract> {
    ERROR_MESSAGE_CONTRACTS
        .iter()
        .copied()
        .find(|contract| contract.class_name == class_name)
}

pub fn public_macro(header_name: &str, name: &str) -> Option<PublicMacroContract> {
    PUBLIC_MACROS
        .iter()
        .copied()
        .find(|contract| contract.header_name == header_name && contract.name == name)
}

pub fn public_integer_macro_names(
    header_name: &str,
    value: PublicInteger,
) -> impl Iterator<Item = &'static str> + '_ {
    PUBLIC_MACROS
        .iter()
        .filter(move |contract| {
            contract.header_name == header_name && contract.value == Some(value)
        })
        .map(|contract| contract.name)
}

pub fn public_integer_macro_names_i128(
    header_name: &str,
    value: i128,
) -> impl Iterator<Item = &'static str> + '_ {
    PUBLIC_MACROS
        .iter()
        .filter(move |contract| {
            contract.header_name == header_name
                && contract
                    .value
                    .and_then(PublicInteger::as_i128)
                    .is_some_and(|candidate| candidate == value)
        })
        .map(|contract| contract.name)
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
#[path = "../build/parser/preprocessor.rs"]
mod preprocessor_build_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_generated_domain_and_code_has_a_unique_name() {
        assert!(!DOMAINS.is_empty());
        assert_eq!(DOMAINS.len(), 91);
        assert_eq!(
            DOMAINS
                .iter()
                .map(|domain| domain.codes.len())
                .sum::<usize>(),
            GENERATED_CODE_COUNT
        );
        for (domain_index, domain) in DOMAINS.iter().enumerate() {
            assert!(!domain.name.is_empty());
            assert!(!domain.codes.is_empty(), "empty domain: {}", domain.name);
            for other in DOMAINS.iter().skip(domain_index + 1) {
                assert_ne!(domain.name, other.name, "duplicate domain name");
            }
            for (code_index, code) in domain.codes.iter().enumerate() {
                assert!(!code.name.is_empty());
                for other in domain.codes.iter().skip(code_index + 1) {
                    assert_ne!(code.name, other.name, "duplicate name in {}", domain.name);
                }
                assert!(domain.lookup(code.code).is_some());
                assert!(domain.names(code.code).any(|name| name == code.name));
            }
        }
    }

    #[test]
    fn every_enum_declaration_in_every_public_header_has_one_domain() {
        assert_eq!(PUBLIC_ENUM_HEADERS.len(), PUBLIC_ENUM_HEADER_COUNT);
        assert_eq!(
            PUBLIC_ENUM_HEADERS
                .iter()
                .map(|header| header.declaration_count)
                .sum::<usize>(),
            ENUM_DECLARATION_COUNT
        );
        for (index, header) in PUBLIC_ENUM_HEADERS.iter().enumerate() {
            assert!(!header.header_name.is_empty());
            for other in PUBLIC_ENUM_HEADERS.iter().skip(index + 1) {
                assert_ne!(header.header_name, other.header_name);
            }
        }
        assert_eq!(ENUM_DECLARATION_COUNT, 79);
        assert_eq!(ENUM_DOMAIN_CONTRACTS.len(), ENUM_DECLARATION_COUNT);
        for (index, contract) in ENUM_DOMAIN_CONTRACTS.iter().enumerate() {
            assert!(matches!(
                contract.declaration_kind,
                "anonymous" | "named" | "typedef"
            ));
            assert!(!contract.header_name.is_empty());
            assert!(!contract.declaration_name.is_empty());
            assert!(domain(contract.domain_name).is_some());
            for other in ENUM_DOMAIN_CONTRACTS.iter().skip(index + 1) {
                assert_ne!(
                    (
                        contract.header_name,
                        contract.declaration_kind,
                        contract.declaration_name,
                    ),
                    (
                        other.header_name,
                        other.declaration_kind,
                        other.declaration_name,
                    ),
                    "one LinuxCNC enum declaration was mapped more than once"
                );
            }
        }
    }

    #[test]
    fn source_aliases_are_preserved_without_ambiguity_or_data_loss() {
        let pose_errors = domain("public_enum/emcpose.h/typedef/EmcPoseErr").unwrap();
        assert_eq!(pose_errors.lookup(-2), Some("EMCPOSE_ERR_INPUT_MISSING"));
        let mut aliases = pose_errors.names(-2);
        assert_eq!(aliases.next(), Some("EMCPOSE_ERR_INPUT_MISSING"));
        assert_eq!(aliases.next(), Some("EMCPOSE_ERR_ALL"));
        assert_eq!(aliases.next(), None);
    }

    #[test]
    fn unknown_values_are_never_mislabeled() {
        for domain in DOMAINS {
            assert_eq!(domain.lookup(i64::MIN), None);
            assert_eq!(domain.lookup(i64::MAX), None);
        }
    }

    #[test]
    fn catalog_is_compiled_only_for_linuxcnc_2_9_10() {
        assert_eq!(LINUXCNC_VERSION, "2.9.10");
        assert_eq!(
            LINUXCNC_SOURCE_COMMIT,
            "86cdca76fa2a36274c432caa21952b23c267989a"
        );
        assert_eq!(GENERATED_CODE_COUNT, 920);
        assert_eq!(ENUM_CODE_COUNT, 711);
        assert_eq!(NON_ENUM_CODE_COUNT, 209);
        assert_eq!(ENUM_CODE_COUNT + NON_ENUM_CODE_COUNT, GENERATED_CODE_COUNT);
        assert_eq!(PUBLIC_ENUM_HEADER_COUNT, 30);
        assert_eq!(LINUXCNC_SOURCE_COMMIT.len(), 40);
        assert_ne!(HEADER_SOURCE_FNV64, 0);
        assert_eq!(EMCMOT_MAX_JOINTS, 16);
        assert_eq!(EMCMOT_MAX_AXIS, 9);
        assert_eq!(EMCMOT_MAX_SPINDLES, 8);
        assert_eq!(EMCMOT_MAX_MISC_ERROR, 64);
    }

    #[test]
    fn every_public_header_byte_and_macro_declaration_is_accounted_for() {
        assert_eq!(PUBLIC_HEADER_COUNT, 120);
        assert_eq!(PUBLIC_HEADERS.len(), PUBLIC_HEADER_COUNT);
        assert_eq!(PUBLIC_HEADER_SOURCE_FNV64, 0x8f2986fcf6b52329);
        assert_eq!(PUBLIC_HEADER_SOURCE_BYTE_COUNT, 635_278);
        assert_eq!(PUBLIC_MACRO_DECLARATION_COUNT, 1_106);
        assert_eq!(PUBLIC_MACRO_NAME_COUNT, 1_029);
        assert_eq!(PUBLIC_INTEGER_MACRO_COUNT, 481);
        assert_eq!(PUBLIC_MACROS.len(), PUBLIC_MACRO_NAME_COUNT);

        let mut header_names = BTreeSet::new();
        let mut source_paths = BTreeSet::new();
        for header in PUBLIC_HEADERS {
            assert!(!header.header_name.is_empty());
            assert!(header.source_relative_path.starts_with("src/"));
            assert!(header.source_byte_count > 0);
            assert_ne!(header.source_fnv64, 0);
            assert!(header_names.insert(header.header_name));
            assert!(source_paths.insert(header.source_relative_path));
            assert_eq!(
                PUBLIC_MACROS
                    .iter()
                    .filter(|contract| contract.header_name == header.header_name)
                    .count(),
                header.macro_name_count
            );
            assert_eq!(
                PUBLIC_MACROS
                    .iter()
                    .filter(|contract| contract.header_name == header.header_name)
                    .map(|contract| contract.declaration_count)
                    .sum::<usize>(),
                header.macro_declaration_count
            );
        }
        assert_eq!(
            PUBLIC_HEADERS
                .iter()
                .map(|header| header.source_byte_count)
                .sum::<usize>(),
            PUBLIC_HEADER_SOURCE_BYTE_COUNT
        );
        assert_eq!(
            PUBLIC_HEADERS
                .iter()
                .map(|header| header.macro_declaration_count)
                .sum::<usize>(),
            PUBLIC_MACRO_DECLARATION_COUNT
        );
    }

    #[test]
    fn every_public_macro_has_one_total_classification_and_dispatch_path() {
        let mut keys = BTreeSet::new();
        let mut kind_counts = [0_usize; 6];
        let mut integer_count = 0;
        for contract in PUBLIC_MACROS {
            assert!(keys.insert((contract.header_name, contract.name)));
            assert!(contract.declaration_count > 0);
            assert_eq!(
                contract.declaration_count,
                contract.object_declaration_count + contract.function_declaration_count
            );
            assert_eq!(
                public_macro(contract.header_name, contract.name),
                Some(*contract)
            );
            let index = match contract.kind {
                PublicMacroKind::Inactive => {
                    assert_eq!(contract.active_replacement, None);
                    assert_eq!(contract.value, None);
                    0
                }
                PublicMacroKind::FunctionLike => {
                    assert!(contract.active_replacement.is_some());
                    assert_eq!(contract.value, None);
                    1
                }
                PublicMacroKind::ObjectWithoutValue => {
                    assert_eq!(contract.active_replacement, Some(""));
                    assert_eq!(contract.value, None);
                    2
                }
                PublicMacroKind::SignedInteger => {
                    assert!(matches!(contract.value, Some(PublicInteger::Signed(_))));
                    3
                }
                PublicMacroKind::UnsignedInteger => {
                    assert!(matches!(contract.value, Some(PublicInteger::Unsigned(_))));
                    4
                }
                PublicMacroKind::ObjectNotIntegerConstant => {
                    assert!(contract
                        .active_replacement
                        .is_some_and(|replacement| !replacement.is_empty()));
                    assert_eq!(contract.value, None);
                    5
                }
            };
            kind_counts[index] += 1;
            if let Some(value) = contract.value {
                integer_count += 1;
                assert!(public_integer_macro_names(contract.header_name, value)
                    .any(|name| name == contract.name));
                if let Some(value) = value.as_i128() {
                    assert!(public_integer_macro_names_i128(contract.header_name, value)
                        .any(|name| name == contract.name));
                }
            }
        }
        assert_eq!(kind_counts, [86, 120, 126, 166, 315, 216]);
        assert_eq!(integer_count, PUBLIC_INTEGER_MACRO_COUNT);
        assert_eq!(public_macro("not-a-header", "not-a-macro"), None);
        assert_eq!(
            public_integer_macro_names_i128("posemath.h", i128::MAX).next(),
            None
        );
    }

    #[test]
    fn previously_uncatalogued_numeric_error_and_result_macros_are_dispatchable() {
        let expected = [
            ("PM_OK", 0),
            ("PM_ERR", -1),
            ("PM_IMPL_ERR", -2),
            ("PM_NORM_ERR", -3),
            ("PM_DIV_ERR", -4),
        ];
        for (name, value) in expected {
            let contract = public_macro("posemath.h", name).unwrap();
            assert!(contract.value.is_some());
            assert!(public_integer_macro_names_i128("posemath.h", value)
                .any(|candidate| candidate == name));
        }
        assert_eq!(
            public_macro("emc.hh", "EMC_LOG_TYPE_IO_CMD").unwrap().value,
            Some(PublicInteger::Unsigned(21))
        );
        assert_eq!(
            public_macro("emc.hh", "EMC_LOG_TYPE_TASK_CMD")
                .unwrap()
                .value,
            Some(PublicInteger::Unsigned(51))
        );
    }

    #[test]
    fn every_interpreter_error_template_from_source_is_cataloged() {
        assert_eq!(INTERPRETER_ERROR_TEMPLATES.len(), 198);
        for (index, entry) in INTERPRETER_ERROR_TEMPLATES.iter().enumerate() {
            assert!(entry.name.starts_with("NCE_"));
            assert!(!entry.template.is_empty());
            for other in INTERPRETER_ERROR_TEMPLATES.iter().skip(index + 1) {
                assert_ne!(entry.name, other.name);
            }
        }
    }

    #[test]
    fn every_public_status_message_has_an_exact_type_and_size_contract() {
        assert_eq!(STATUS_MESSAGE_CONTRACTS.len(), 12);
        for (index, contract) in STATUS_MESSAGE_CONTRACTS.iter().enumerate() {
            assert!(contract.class_name.starts_with("EMC_"));
            assert!(contract.class_name.ends_with("_STAT"));
            assert!(contract.message_type_name.starts_with("EMC_"));
            assert!(contract.message_type_name.ends_with("_STAT_TYPE"));
            assert_eq!(
                EMC_NML_MESSAGE_TYPE.lookup(contract.message_type),
                Some(contract.message_type_name)
            );
            assert!(contract.message_size > 0);
            assert_eq!(
                status_message_contract(contract.class_name),
                Some(*contract)
            );
            for other in STATUS_MESSAGE_CONTRACTS.iter().skip(index + 1) {
                assert_ne!(contract.class_name, other.class_name);
                assert_ne!(contract.message_type, other.message_type);
            }
        }
    }

    #[test]
    fn every_error_channel_message_has_an_exact_payload_layout() {
        assert_eq!(ERROR_MESSAGE_CONTRACT_COUNT, 6);
        assert_eq!(ERROR_MESSAGE_CONTRACTS.len(), 6);
        let expected = [
            ("NML_ERROR", "NML_ERROR_TYPE", 1, "error"),
            ("NML_TEXT", "NML_TEXT_TYPE", 2, "text"),
            ("NML_DISPLAY", "NML_DISPLAY_TYPE", 3, "display"),
            ("EMC_OPERATOR_ERROR", "EMC_OPERATOR_ERROR_TYPE", 11, "error"),
            ("EMC_OPERATOR_TEXT", "EMC_OPERATOR_TEXT_TYPE", 12, "text"),
            (
                "EMC_OPERATOR_DISPLAY",
                "EMC_OPERATOR_DISPLAY_TYPE",
                13,
                "display",
            ),
        ];
        for (index, contract) in ERROR_MESSAGE_CONTRACTS.iter().enumerate() {
            let (class_name, type_name, message_type, payload_member) = expected[index];
            assert_eq!(contract.class_name, class_name);
            assert_eq!(contract.message_type_name, type_name);
            assert_eq!(contract.message_type, message_type);
            assert_eq!(contract.payload_member, payload_member);
            assert!(contract.message_size >= contract.payload_offset + contract.payload_size);
            assert_eq!(error_message_contract(class_name), Some(*contract));
            if class_name.starts_with("NML_") {
                assert_eq!(contract.payload_size, 256);
                assert_eq!(contract.id_member, None);
                assert_eq!(contract.id_offset, None);
                assert_eq!(contract.id_size, 0);
                assert_eq!(
                    NML_OPERATOR_MESSAGE_TYPE.lookup(message_type),
                    Some(type_name)
                );
            } else {
                assert_eq!(contract.payload_size, 255);
                assert_eq!(contract.id_member, Some("id"));
                assert_eq!(contract.id_size, core::mem::size_of::<i32>());
                assert!(contract.id_offset.is_some());
                assert_eq!(EMC_NML_MESSAGE_TYPE.lookup(message_type), Some(type_name));
            }
        }
    }
}
