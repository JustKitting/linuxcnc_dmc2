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
pub struct StatusMessageContract {
    pub class_name: &'static str,
    pub message_type_name: &'static str,
    pub message_type: i64,
    pub message_size: i64,
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
}

include!(concat!(env!("OUT_DIR"), "/linuxcnc_program_interface.rs"));

pub fn status_message_contract(class_name: &str) -> Option<StatusMessageContract> {
    STATUS_MESSAGE_CONTRACTS
        .iter()
        .copied()
        .find(|contract| contract.class_name == class_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_controller_input_domain_and_code_has_a_unique_name() {
        assert_eq!(DOMAINS.len(), 19);
        assert_eq!(
            DOMAINS
                .iter()
                .map(|domain| domain.codes.len())
                .sum::<usize>(),
            380
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
                assert!(domain.names(code.code).any(|name| name == code.name));
            }
            assert_eq!(domain.lookup(i64::MIN), None);
            assert_eq!(domain.lookup(i64::MAX), None);
        }
    }

    #[test]
    fn snapshot_extents_match_linuxcnc_2_9_10() {
        assert_eq!(EMCMOT_MAX_JOINTS, 16);
        assert_eq!(EMCMOT_MAX_AXIS, 9);
        assert_eq!(EMCMOT_MAX_SPINDLES, 8);
        assert_eq!(EMCMOT_MAX_MISC_ERROR, 64);
    }

    #[test]
    fn every_status_object_copied_by_the_program_has_an_exact_type() {
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
}
