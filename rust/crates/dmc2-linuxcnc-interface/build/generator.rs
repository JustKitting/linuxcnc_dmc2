use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use super::config::{
    ERROR_MESSAGE_CONTRACTS, EXPECTED_HEADER_FNV64, EXPECTED_LINUXCNC_COMMIT,
    EXPECTED_LINUXCNC_VERSION, STATUS_MESSAGE_CONTRACTS,
};
use super::domains::{self, Domain};
use super::parser::{integer_macro, interpreter_error_templates};
use super::probe::{self, Results};
use super::source::{self, Headers};

struct MachineLimits {
    joints: usize,
    axes: usize,
    spindles: usize,
    misc_errors: usize,
}

fn machine_limits(headers: &Headers) -> MachineLimits {
    MachineLimits {
        joints: integer_macro(&headers.emcmotcfg, "EMCMOT_MAX_JOINTS"),
        axes: integer_macro(&headers.emcmotcfg, "EMCMOT_MAX_AXIS"),
        spindles: integer_macro(&headers.emcmotcfg, "EMCMOT_MAX_SPINDLES"),
        misc_errors: integer_macro(&headers.emcmotcfg, "EMCMOT_MAX_MISC_ERROR"),
    }
}

fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn header_fingerprint(headers: &Headers) -> u64 {
    headers
        .all()
        .iter()
        .fold(0xcbf29ce484222325_u64, |hash, source| {
            fnv1a(hash, source.as_bytes())
        })
}

fn rust_identifier(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn preamble(fingerprint: u64, generated_code_count: usize, limits: &MachineLimits) -> String {
    format!(
        "pub const LINUXCNC_VERSION: &str = \"{EXPECTED_LINUXCNC_VERSION}\";\n\
         pub const LINUXCNC_SOURCE_COMMIT: &str = \"{EXPECTED_LINUXCNC_COMMIT}\";\n\
         pub const HEADER_SOURCE_FNV64: u64 = 0x{fingerprint:016x};\n\
         pub const GENERATED_CODE_COUNT: usize = {generated_code_count};\n\
         pub const STATUS_MESSAGE_CONTRACT_COUNT: usize = {};\n\
         pub const ERROR_MESSAGE_CONTRACT_COUNT: usize = {};\n\
         pub const EMCMOT_MAX_JOINTS: usize = {};\n\
         pub const EMCMOT_MAX_AXIS: usize = {};\n\
         pub const EMCMOT_MAX_SPINDLES: usize = {};\n\
         pub const EMCMOT_MAX_MISC_ERROR: usize = {};\n",
        STATUS_MESSAGE_CONTRACTS.len(),
        ERROR_MESSAGE_CONTRACTS.len(),
        limits.joints,
        limits.axes,
        limits.spindles,
        limits.misc_errors,
    )
}

fn append_interpreter_errors(generated: &mut String, templates: &[(String, String)]) {
    generated.push_str("pub static INTERPRETER_ERROR_TEMPLATES: &[MessageTemplate] = &[\n");
    for (name, template) in templates {
        generated.push_str(&format!(
            "MessageTemplate {{ name: {name:?}, template: {template:?} }},\n"
        ));
    }
    generated.push_str("];\n");
}

fn append_status_contracts(generated: &mut String, results: &Results) {
    generated.push_str("pub static STATUS_MESSAGE_CONTRACTS: &[StatusMessageContract] = &[\n");
    for (class_name, expected_message_type_name) in STATUS_MESSAGE_CONTRACTS {
        let contract = &results.status_contracts[*class_name];
        assert_eq!(
            contract.message_type_name, *expected_message_type_name,
            "status-message probe returned the wrong type name for {class_name}"
        );
        let message_type_name = &contract.message_type_name;
        let message_type = contract.message_type;
        let message_size = contract.message_size;
        generated.push_str(&format!(
            "StatusMessageContract {{ class_name: {class_name:?}, message_type_name: {message_type_name:?}, message_type: {message_type}, message_size: {message_size} }},\n"
        ));
    }
    generated.push_str("];\n");
}

fn append_error_contracts(generated: &mut String, results: &Results) {
    generated.push_str("pub static ERROR_MESSAGE_CONTRACTS: &[ErrorMessageContract] = &[\n");
    for expected in ERROR_MESSAGE_CONTRACTS {
        let contract = &results.error_contracts[expected.class_name];
        assert_eq!(contract.message_type_name, expected.message_type_name);
        assert_eq!(contract.payload_member, expected.payload_member);
        assert_eq!(contract.id_member.as_deref(), expected.id_member);
        generated.push_str(&format!(
            "ErrorMessageContract {{ class_name: {:?}, message_type_name: {:?}, message_type: {}, message_size: {}, payload_member: {:?}, payload_offset: {}, payload_size: {}, id_member: {:?}, id_offset: {:?}, id_size: {} }},\n",
            expected.class_name,
            contract.message_type_name,
            contract.message_type,
            contract.message_size,
            contract.payload_member,
            contract.payload_offset,
            contract.payload_size,
            contract.id_member.as_deref(),
            contract.id_offset,
            contract.id_size,
        ));
    }
    generated.push_str("];\n");
}

fn append_domains(generated: &mut String, domains: &[Domain], results: &Results) {
    for domain in domains {
        let identifier = rust_identifier(domain.name);
        generated.push_str(&format!(
            "pub static {identifier}: CodeDomain = CodeDomain {{ name: {:?}, codes: &[\n",
            domain.name
        ));
        let mut domain_values = BTreeSet::new();
        for symbol in &domain.symbols {
            let value = results.values[&(domain.name.to_owned(), symbol.to_owned())];
            assert!(
                domain_values.insert(value),
                "duplicate numeric value {value} in LinuxCNC code domain {}",
                domain.name
            );
            generated.push_str(&format!(
                "CodeName {{ code: {value}, name: {symbol:?} }},\n"
            ));
        }
        generated.push_str("] };\n");
    }
    generated.push_str("pub static DOMAINS: &[CodeDomain] = &[\n");
    for domain in domains {
        generated.push_str(&format!("{},\n", rust_identifier(domain.name)));
    }
    generated.push_str("];\n");
}

fn write_catalog(
    output_directory: &Path,
    headers: &Headers,
    domains: &[Domain],
    templates: &[(String, String)],
    results: &Results,
    limits: &MachineLimits,
) {
    let fingerprint = header_fingerprint(headers);
    assert_eq!(
        fingerprint, EXPECTED_HEADER_FNV64,
        "installed LinuxCNC 2.9.10 public headers differ from the audited source"
    );
    let generated_code_count = domains
        .iter()
        .map(|domain| domain.symbols.len())
        .sum::<usize>();
    let mut generated = preamble(fingerprint, generated_code_count, limits);
    append_interpreter_errors(&mut generated, templates);
    append_status_contracts(&mut generated, results);
    append_error_contracts(&mut generated, results);
    append_domains(&mut generated, domains, results);
    fs::write(output_directory.join("linuxcnc_code_catalog.rs"), generated)
        .expect("failed to write generated LinuxCNC code catalog");
}

pub(crate) fn generate() {
    let source_root = source::verify_checkout();
    let headers = source::load_headers(&source_root);
    let templates = interpreter_error_templates(&source::read_interpreter_errors(&source_root));
    let domains = domains::collect(&headers);
    let limits = machine_limits(&headers);
    let output_directory =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo did not provide OUT_DIR"));
    let results = probe::run(
        &output_directory,
        &domains,
        STATUS_MESSAGE_CONTRACTS,
        ERROR_MESSAGE_CONTRACTS,
    );
    write_catalog(
        &output_directory,
        &headers,
        &domains,
        &templates,
        &results,
        &limits,
    );
}
