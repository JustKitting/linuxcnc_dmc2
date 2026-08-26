use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use super::config::{
    ERROR_MESSAGE_CONTRACTS, EXPECTED_HEADER_FNV64, EXPECTED_LINUXCNC_COMMIT,
    EXPECTED_LINUXCNC_VERSION, EXPECTED_PUBLIC_ENUM_HEADER_COUNT, STATUS_MESSAGE_CONTRACTS,
};
use super::domains::{self, Domain};
use super::macro_inventory::{self, Inventory, MacroClassification};
use super::parser::{enum_declarations, integer_macro, interpreter_error_templates};
use super::probe::{self, Results};
use super::source::{self, Headers, PublicEnumHeader};

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

fn preamble(
    fingerprint: u64,
    generated_code_count: usize,
    enum_code_count: usize,
    enum_declaration_count: usize,
    macro_inventory: &Inventory,
    limits: &MachineLimits,
) -> String {
    let macro_declaration_count = macro_inventory
        .macros
        .iter()
        .map(|contract| contract.declaration_count)
        .sum::<usize>();
    let integer_macro_count = macro_inventory
        .macros
        .iter()
        .filter(|contract| {
            matches!(
                contract.classification,
                MacroClassification::SignedInteger(_) | MacroClassification::UnsignedInteger(_)
            )
        })
        .count();
    format!(
        "pub const LINUXCNC_VERSION: &str = \"{EXPECTED_LINUXCNC_VERSION}\";\n\
         pub const LINUXCNC_SOURCE_COMMIT: &str = \"{EXPECTED_LINUXCNC_COMMIT}\";\n\
         pub const HEADER_SOURCE_FNV64: u64 = 0x{fingerprint:016x};\n\
         pub const GENERATED_CODE_COUNT: usize = {generated_code_count};\n\
         pub const ENUM_CODE_COUNT: usize = {enum_code_count};\n\
         pub const NON_ENUM_CODE_COUNT: usize = {};\n\
         pub const PUBLIC_ENUM_HEADER_COUNT: usize = {EXPECTED_PUBLIC_ENUM_HEADER_COUNT};\n\
         pub const PUBLIC_HEADER_COUNT: usize = {};\n\
         pub const PUBLIC_HEADER_SOURCE_FNV64: u64 = 0x{:016x};\n\
         pub const PUBLIC_HEADER_SOURCE_BYTE_COUNT: usize = {};\n\
         pub const PUBLIC_MACRO_DECLARATION_COUNT: usize = {macro_declaration_count};\n\
         pub const PUBLIC_MACRO_NAME_COUNT: usize = {};\n\
         pub const PUBLIC_INTEGER_MACRO_COUNT: usize = {integer_macro_count};\n\
         pub const ENUM_DECLARATION_COUNT: usize = {enum_declaration_count};\n\
         pub const STATUS_MESSAGE_CONTRACT_COUNT: usize = {};\n\
         pub const ERROR_MESSAGE_CONTRACT_COUNT: usize = {};\n\
         pub const EMCMOT_MAX_JOINTS: usize = {};\n\
         pub const EMCMOT_MAX_AXIS: usize = {};\n\
         pub const EMCMOT_MAX_SPINDLES: usize = {};\n\
         pub const EMCMOT_MAX_MISC_ERROR: usize = {};\n",
        generated_code_count - enum_code_count,
        macro_inventory.headers.len(),
        macro_inventory.source_fnv64,
        macro_inventory
            .headers
            .iter()
            .map(|contract| contract.source_byte_count)
            .sum::<usize>(),
        macro_inventory.macros.len(),
        STATUS_MESSAGE_CONTRACTS.len(),
        ERROR_MESSAGE_CONTRACTS.len(),
        limits.joints,
        limits.axes,
        limits.spindles,
        limits.misc_errors,
    )
}

fn append_public_headers(generated: &mut String, inventory: &Inventory) {
    generated.push_str("pub static PUBLIC_HEADERS: &[PublicHeaderContract] = &[\n");
    for header in &inventory.headers {
        generated.push_str(&format!(
            "PublicHeaderContract {{ header_name: {:?}, source_relative_path: {:?}, source_byte_count: {}, source_fnv64: 0x{:016x}, macro_declaration_count: {}, macro_name_count: {} }},\n",
            header.header_name,
            header.source_relative_path,
            header.source_byte_count,
            header.source_fnv64,
            header.macro_declaration_count,
            header.macro_name_count,
        ));
    }
    generated.push_str("];\n");
}

fn append_public_macros(generated: &mut String, inventory: &Inventory) {
    generated.push_str("pub static PUBLIC_MACROS: &[PublicMacroContract] = &[\n");
    for contract in &inventory.macros {
        let replacement = match &contract.active_replacement {
            Some(value) => format!("Some({value:?})"),
            None => "None".to_owned(),
        };
        let (kind, value) = match contract.classification {
            MacroClassification::Inactive => ("Inactive", "None".to_owned()),
            MacroClassification::FunctionLike => ("FunctionLike", "None".to_owned()),
            MacroClassification::ObjectWithoutValue => ("ObjectWithoutValue", "None".to_owned()),
            MacroClassification::SignedInteger(value) => (
                "SignedInteger",
                format!("Some(PublicInteger::Signed({value}))"),
            ),
            MacroClassification::UnsignedInteger(value) => (
                "UnsignedInteger",
                format!("Some(PublicInteger::Unsigned({value}))"),
            ),
            MacroClassification::ObjectNotIntegerConstant => {
                ("ObjectNotIntegerConstant", "None".to_owned())
            }
        };
        generated.push_str(&format!(
            "PublicMacroContract {{ header_name: {:?}, name: {:?}, declaration_count: {}, object_declaration_count: {}, function_declaration_count: {}, active_replacement: {replacement}, kind: PublicMacroKind::{kind}, value: {value} }},\n",
            contract.header_name,
            contract.name,
            contract.declaration_count,
            contract.object_declaration_count,
            contract.function_declaration_count,
        ));
    }
    generated.push_str("];\n");
}

fn append_enum_domain_contracts(generated: &mut String, domains: &[Domain]) {
    generated.push_str("pub static ENUM_DOMAIN_CONTRACTS: &[EnumDomainContract] = &[\n");
    for domain in domains {
        let Some(origin) = domain.enum_origin.as_ref() else {
            continue;
        };
        generated.push_str(&format!(
            "EnumDomainContract {{ header_name: {:?}, declaration_kind: {:?}, declaration_name: {:?}, domain_name: {:?} }},\n",
            origin.header_name,
            origin.kind.name(),
            origin.declaration_name,
            domain.name,
        ));
    }
    generated.push_str("];\n");
}

fn append_public_enum_headers(generated: &mut String, headers: &[PublicEnumHeader]) {
    generated.push_str("pub static PUBLIC_ENUM_HEADERS: &[PublicEnumHeaderContract] = &[\n");
    for header in headers {
        let declaration_count = enum_declarations(&header.source).len();
        generated.push_str(&format!(
            "PublicEnumHeaderContract {{ header_name: {:?}, declaration_count: {declaration_count} }},\n",
            header.name,
        ));
    }
    generated.push_str("];\n");
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
        assert_eq!(contract.serial_member.as_deref(), expected.serial_member);
        assert_eq!(contract.payload_member, expected.payload_member);
        assert_eq!(contract.id_member.as_deref(), expected.id_member);
        generated.push_str(&format!(
            "ErrorMessageContract {{ class_name: {:?}, message_type_name: {:?}, message_type: {}, message_size: {}, type_offset: {}, type_size: {}, size_offset: {}, size_size: {}, serial_member: {:?}, serial_offset: {:?}, serial_size: {}, payload_member: {:?}, payload_offset: {}, payload_size: {}, id_member: {:?}, id_offset: {:?}, id_size: {} }},\n",
            expected.class_name,
            contract.message_type_name,
            contract.message_type,
            contract.message_size,
            contract.type_offset,
            contract.type_size,
            contract.size_offset,
            contract.size_size,
            contract.serial_member.as_deref(),
            contract.serial_offset,
            contract.serial_size,
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
        let identifier = rust_identifier(&domain.name);
        generated.push_str(&format!(
            "pub static {identifier}: CodeDomain = CodeDomain {{ name: {:?}, codes: &[\n",
            domain.name
        ));
        for symbol in &domain.symbols {
            let value = results.values[&(domain.name.to_owned(), symbol.to_owned())];
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

struct CatalogInputs<'a> {
    output_directory: &'a Path,
    headers: &'a Headers,
    public_enum_headers: &'a [PublicEnumHeader],
    domains: &'a [Domain],
    templates: &'a [(String, String)],
    results: &'a Results,
    macro_inventory: &'a Inventory,
    limits: &'a MachineLimits,
}

fn write_catalog(inputs: CatalogInputs<'_>) {
    let CatalogInputs {
        output_directory,
        headers,
        public_enum_headers,
        domains,
        templates,
        results,
        macro_inventory,
        limits,
    } = inputs;
    let fingerprint = header_fingerprint(headers);
    assert_eq!(
        fingerprint, EXPECTED_HEADER_FNV64,
        "installed LinuxCNC 2.9.10 public headers differ from the audited source"
    );
    let generated_code_count = domains
        .iter()
        .map(|domain| domain.symbols.len())
        .sum::<usize>();
    let enum_declaration_count = domains
        .iter()
        .filter(|domain| domain.enum_origin.is_some())
        .count();
    let enum_code_count = domains
        .iter()
        .filter(|domain| domain.enum_origin.is_some())
        .map(|domain| domain.symbols.len())
        .sum();
    let mut generated = preamble(
        fingerprint,
        generated_code_count,
        enum_code_count,
        enum_declaration_count,
        macro_inventory,
        limits,
    );
    append_public_headers(&mut generated, macro_inventory);
    append_public_macros(&mut generated, macro_inventory);
    append_interpreter_errors(&mut generated, templates);
    append_status_contracts(&mut generated, results);
    append_error_contracts(&mut generated, results);
    append_public_enum_headers(&mut generated, public_enum_headers);
    append_enum_domain_contracts(&mut generated, domains);
    append_domains(&mut generated, domains, results);
    fs::write(output_directory.join("linuxcnc_code_catalog.rs"), generated)
        .expect("failed to write generated LinuxCNC code catalog");
}

pub(crate) fn generate() {
    let source_root = source::verify_checkout();
    let headers = source::load_headers(&source_root);
    let public_headers = source::load_public_headers(&source_root);
    let public_enum_headers = source::load_public_enum_headers(&public_headers);
    let templates = interpreter_error_templates(&source::read_interpreter_errors(&source_root));
    let domains = domains::collect(&headers, &public_enum_headers);
    let limits = machine_limits(&headers);
    let output_directory =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo did not provide OUT_DIR"));
    let macro_inventory =
        macro_inventory::collect(&public_headers, &source_root, &output_directory);
    let results = probe::run(
        &output_directory,
        &domains,
        STATUS_MESSAGE_CONTRACTS,
        ERROR_MESSAGE_CONTRACTS,
    );
    write_catalog(CatalogInputs {
        output_directory: &output_directory,
        headers: &headers,
        public_enum_headers: &public_enum_headers,
        domains: &domains,
        templates: &templates,
        results: &results,
        macro_inventory: &macro_inventory,
        limits: &limits,
    });
}
