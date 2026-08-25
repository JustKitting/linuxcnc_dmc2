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
}
