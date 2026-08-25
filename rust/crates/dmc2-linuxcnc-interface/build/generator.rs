use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use super::config::STATUS_MESSAGE_TYPES;
use super::domains::{self, Domain};
use super::parser::integer_macro;
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

fn preamble(limits: &MachineLimits) -> String {
    format!(
        "pub const EMCMOT_MAX_JOINTS: usize = {};\n\
         pub const EMCMOT_MAX_AXIS: usize = {};\n\
         pub const EMCMOT_MAX_SPINDLES: usize = {};\n\
         pub const EMCMOT_MAX_MISC_ERROR: usize = {};\n",
        limits.joints, limits.axes, limits.spindles, limits.misc_errors,
    )
}

fn append_status_contracts(generated: &mut String, results: &Results) {
    generated.push_str("pub static STATUS_MESSAGE_CONTRACTS: &[StatusMessageContract] = &[\n");
    for (class_name, message_type_name) in STATUS_MESSAGE_TYPES {
        let message_type = results.values[&(
            "emc_nml_message_type".to_owned(),
            (*message_type_name).to_owned(),
        )];
        let message_size = results.status_sizes[*class_name];
        generated.push_str(&format!(
            "StatusMessageContract {{ class_name: {class_name:?}, message_type_name: {message_type_name:?}, message_type: {message_type}, message_size: {message_size} }},\n"
        ));
    }
    generated.push_str("];\n");
}

fn append_domains(generated: &mut String, domains: &[Domain], results: &Results) {
    for domain in domains {
        let identifier = rust_identifier(&domain.name);
        generated.push_str(&format!(
            "pub static {identifier}: CodeDomain = CodeDomain {{ name: {:?}, codes: &[\n",
            domain.name
        ));
        for symbol in &domain.symbols {
            let value = results.values[&(domain.name.clone(), symbol.clone())];
            generated.push_str(&format!(
                "CodeName {{ code: {value}, name: {symbol:?} }},\n"
            ));
        }
        generated.push_str("] };\n");
    }
    generated.push_str("pub static DOMAINS: &[CodeDomain] = &[\n");
    for domain in domains {
        generated.push_str(&format!("{},\n", rust_identifier(&domain.name)));
    }
    generated.push_str("];\n");
}

fn write_interface(
    output_directory: &Path,
    domains: &[Domain],
    results: &Results,
    limits: &MachineLimits,
) {
    let mut generated = preamble(limits);
    append_status_contracts(&mut generated, results);
    append_domains(&mut generated, domains, results);
    fs::write(
        output_directory.join("linuxcnc_program_interface.rs"),
        generated,
    )
    .expect("failed to write the controller's LinuxCNC interface values");
}

pub(crate) fn generate() {
    let source_root = source::verify_checkout();
    let headers = source::load_headers(&source_root);
    let domains = domains::collect(&headers);
    let limits = machine_limits(&headers);
    let output_directory =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo did not provide OUT_DIR"));
    let results = probe::run(&output_directory, &domains);
    write_interface(&output_directory, &domains, &results, &limits);
}
