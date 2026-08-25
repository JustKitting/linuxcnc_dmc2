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

impl CodeDomain {
    pub fn lookup(self, code: i64) -> Option<&'static str> {
        self.codes
            .iter()
            .find(|entry| entry.code == code)
            .map(|entry| entry.name)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_generated_domain_and_code_has_a_unique_name() {
        assert!(!DOMAINS.is_empty());
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
                    assert_ne!(code.code, other.code, "duplicate value in {}", domain.name);
                }
                assert_eq!(domain.lookup(code.code), Some(code.name));
            }
        }
    }

    #[test]
    fn every_enum_declaration_in_the_audited_headers_has_one_domain() {
        assert_eq!(ENUM_DECLARATION_COUNT, 38);
        assert_eq!(ENUM_DOMAIN_CONTRACTS.len(), ENUM_DECLARATION_COUNT);
        for (index, contract) in ENUM_DOMAIN_CONTRACTS.iter().enumerate() {
            assert!(matches!(contract.declaration_kind, "named" | "typedef"));
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
        assert_eq!(GENERATED_CODE_COUNT, 550);
        assert_eq!(LINUXCNC_SOURCE_COMMIT.len(), 40);
        assert_ne!(HEADER_SOURCE_FNV64, 0);
        assert_eq!(EMCMOT_MAX_JOINTS, 16);
        assert_eq!(EMCMOT_MAX_AXIS, 9);
        assert_eq!(EMCMOT_MAX_SPINDLES, 8);
        assert_eq!(EMCMOT_MAX_MISC_ERROR, 64);
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
