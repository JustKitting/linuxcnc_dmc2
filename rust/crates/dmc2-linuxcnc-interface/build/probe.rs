use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::config::{ErrorMessageSpec, INCLUDE_ROOT};
use super::domains::Domain;

pub(crate) struct StatusContractValue {
    pub(crate) message_type_name: String,
    pub(crate) message_type: i64,
    pub(crate) message_size: i64,
}

pub(crate) struct ErrorContractValue {
    pub(crate) message_type_name: String,
    pub(crate) message_type: i64,
    pub(crate) message_size: usize,
    pub(crate) payload_member: String,
    pub(crate) payload_offset: usize,
    pub(crate) payload_size: usize,
    pub(crate) id_member: Option<String>,
    pub(crate) id_offset: Option<usize>,
    pub(crate) id_size: usize,
}

pub(crate) struct Results {
    pub(crate) values: BTreeMap<(String, String), i64>,
    pub(crate) status_contracts: BTreeMap<String, StatusContractValue>,
    pub(crate) error_contracts: BTreeMap<String, ErrorContractValue>,
}

fn paths(output_directory: &Path) -> (PathBuf, PathBuf) {
    (
        output_directory.join("linuxcnc_code_probe.cc"),
        output_directory.join("linuxcnc_code_probe"),
    )
}

fn enum_paths(output_directory: &Path) -> (PathBuf, PathBuf) {
    (
        output_directory.join("linuxcnc_public_enum_probe.cc"),
        output_directory.join("linuxcnc_public_enum_probe"),
    )
}

fn source(
    domains: &[Domain],
    status_contracts: &[(&str, &str)],
    error_contracts: &[ErrorMessageSpec],
) -> String {
    let mut probe = String::from(
        "#include <cstddef>\n#include <cstdio>\n#include \"emc.hh\"\n#include \"emc_nml.hh\"\n#include \"motion.h\"\n#include \"interp_return.hh\"\n#include \"nml.hh\"\n#include \"nml_oi.hh\"\n#include \"rcs.hh\"\n#include \"stat_msg.hh\"\n#include \"cmd_msg.hh\"\n#include \"cms.hh\"\n#include \"canon.hh\"\n#include \"kinematics.h\"\n#include \"motion_types.h\"\n#include \"debugflags.h\"\n#include \"state_tag.h\"\n#include \"usrmotintf.h\"\nint main() {\n",
    );
    for domain in domains {
        if domain.enum_probe_body.is_some() {
            continue;
        }
        for symbol in &domain.symbols {
            probe.push_str(&format!(
                "std::printf(\"{}\\t{symbol}\\t%lld\\n\", static_cast<long long>({symbol}));\n",
                domain.name
            ));
        }
    }
    for (class_name, message_type_name) in status_contracts {
        probe.push_str(&format!(
            "std::printf(\"__status_message_contract__\\t{class_name}\\t{message_type_name}\\t%lld\\t%zu\\n\", static_cast<long long>({message_type_name}), sizeof({class_name}));\n"
        ));
    }
    for contract in error_contracts {
        let class_name = contract.class_name;
        let message_type_name = contract.message_type_name;
        let payload_member = contract.payload_member;
        let (id_member, id_offset, id_size) = match contract.id_member {
            Some(id_member) => (
                id_member,
                format!("static_cast<long long>(offsetof({class_name}, {id_member}))"),
                format!("sizeof((({class_name} *)nullptr)->{id_member})"),
            ),
            None => ("-", "-1LL".to_owned(), "static_cast<size_t>(0)".to_owned()),
        };
        probe.push_str(&format!(
            "std::printf(\"__error_message_contract__\\t{class_name}\\t{message_type_name}\\t%lld\\t%zu\\t{payload_member}\\t%zu\\t%zu\\t{id_member}\\t%lld\\t%zu\\n\", static_cast<long long>({message_type_name}), sizeof({class_name}), offsetof({class_name}, {payload_member}), sizeof((({class_name} *)nullptr)->{payload_member}), {id_offset}, {id_size});\n"
        ));
    }
    probe.push_str("return 0;\n}\n");
    probe
}

fn extracted_enum_source(domains: &[Domain]) -> String {
    let mut probe = String::from("extern \"C\" int printf(const char *, ...);\n");
    for (index, domain) in domains.iter().enumerate() {
        let Some(body) = domain.enum_probe_body.as_deref() else {
            continue;
        };
        probe.push_str(&format!(
            "namespace dmc2_public_enum_{index} {{ enum Values {{\n{body}\n}}; }}\n"
        ));
    }
    probe.push_str("int main() {\n");
    for (index, domain) in domains.iter().enumerate() {
        if domain.enum_probe_body.is_none() {
            continue;
        }
        for symbol in &domain.symbols {
            probe.push_str(&format!(
                "printf(\"{}\\t{symbol}\\t%lld\\n\", static_cast<long long>(dmc2_public_enum_{index}::{symbol}));\n",
                domain.name
            ));
        }
    }
    probe.push_str("return 0;\n}\n");
    probe
}

fn compile(source: &Path, binary: &Path) {
    let result = Command::new("g++")
        .args([
            "-std=c++17",
            "-O2",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-Wno-invalid-offsetof",
            "-isystem",
            INCLUDE_ROOT,
            source.to_str().expect("non-UTF-8 probe source path"),
            "-o",
            binary.to_str().expect("non-UTF-8 probe binary path"),
        ])
        .output()
        .expect("failed to execute g++ for LinuxCNC code probe");
    assert!(
        result.status.success(),
        "LinuxCNC code probe failed to compile: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn parse(stdout: Vec<u8>) -> Results {
    let mut values = BTreeMap::new();
    let mut status_contracts = BTreeMap::new();
    let mut error_contracts = BTreeMap::new();
    for line in String::from_utf8(stdout)
        .expect("LinuxCNC code probe produced non-UTF-8 output")
        .lines()
    {
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.first() == Some(&"__error_message_contract__") {
            assert_eq!(
                fields.len(),
                11,
                "malformed LinuxCNC error-message contract line: {line}"
            );
            let parse_usize = |index: usize, label: &str| {
                fields[index]
                    .parse::<usize>()
                    .unwrap_or_else(|error| panic!("invalid {label} in {line:?}: {error}"))
            };
            let message_type = fields[3]
                .parse::<i64>()
                .unwrap_or_else(|error| panic!("invalid error-message type in {line:?}: {error}"));
            let id_offset_raw = fields[9]
                .parse::<i64>()
                .unwrap_or_else(|error| panic!("invalid id offset in {line:?}: {error}"));
            let (id_member, id_offset) = if fields[8] == "-" {
                assert_eq!(id_offset_raw, -1, "missing id has an offset in {line:?}");
                (None, None)
            } else {
                assert!(id_offset_raw >= 0, "present id has no offset in {line:?}");
                (Some(fields[8].to_owned()), Some(id_offset_raw as usize))
            };
            assert!(
                error_contracts
                    .insert(
                        fields[1].to_owned(),
                        ErrorContractValue {
                            message_type_name: fields[2].to_owned(),
                            message_type,
                            message_size: parse_usize(4, "error-message size"),
                            payload_member: fields[5].to_owned(),
                            payload_offset: parse_usize(6, "payload offset"),
                            payload_size: parse_usize(7, "payload size"),
                            id_member,
                            id_offset,
                            id_size: parse_usize(10, "id size"),
                        },
                    )
                    .is_none(),
                "duplicate LinuxCNC error-message contract: {line}"
            );
            continue;
        }
        if fields.first() == Some(&"__status_message_contract__") {
            assert_eq!(
                fields.len(),
                5,
                "malformed LinuxCNC status-message contract line: {line}"
            );
            let message_type = fields[3]
                .parse::<i64>()
                .unwrap_or_else(|error| panic!("invalid status-message type in {line:?}: {error}"));
            let message_size = fields[4]
                .parse::<i64>()
                .unwrap_or_else(|error| panic!("invalid status-message size in {line:?}: {error}"));
            assert!(
                message_size > 0,
                "non-positive status-message size in {line:?}"
            );
            assert!(
                status_contracts
                    .insert(
                        fields[1].to_owned(),
                        StatusContractValue {
                            message_type_name: fields[2].to_owned(),
                            message_type,
                            message_size,
                        },
                    )
                    .is_none(),
                "duplicate LinuxCNC status-message contract: {line}"
            );
            continue;
        }
        assert_eq!(
            fields.len(),
            3,
            "malformed LinuxCNC code probe line: {line}"
        );
        let value = fields[2]
            .parse::<i64>()
            .unwrap_or_else(|error| panic!("invalid code value in {line:?}: {error}"));
        assert!(
            values
                .insert((fields[0].to_owned(), fields[1].to_owned()), value)
                .is_none(),
            "duplicate LinuxCNC code probe result: {line}"
        );
    }
    Results {
        values,
        status_contracts,
        error_contracts,
    }
}

fn execute(binary: &Path) -> Results {
    let result = Command::new(binary)
        .output()
        .expect("failed to execute LinuxCNC code probe");
    assert!(
        result.status.success(),
        "LinuxCNC code probe failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    parse(result.stdout)
}

pub(crate) fn run(
    output_directory: &Path,
    domains: &[Domain],
    status_contracts: &[(&str, &str)],
    error_contracts: &[ErrorMessageSpec],
) -> Results {
    let (probe_source, probe_binary) = paths(output_directory);
    fs::write(
        &probe_source,
        source(domains, status_contracts, error_contracts),
    )
    .expect("failed to write LinuxCNC code probe");
    compile(&probe_source, &probe_binary);
    let mut parsed = execute(&probe_binary);
    let extracted_count = domains
        .iter()
        .filter(|domain| domain.enum_probe_body.is_some())
        .map(|domain| domain.symbols.len())
        .sum::<usize>();
    if extracted_count > 0 {
        let (enum_source, enum_binary) = enum_paths(output_directory);
        fs::write(&enum_source, extracted_enum_source(domains))
            .expect("failed to write LinuxCNC public-enum probe");
        compile(&enum_source, &enum_binary);
        let extracted = execute(&enum_binary);
        assert_eq!(
            extracted.values.len(),
            extracted_count,
            "LinuxCNC public-enum probe omitted a source code"
        );
        assert!(extracted.status_contracts.is_empty());
        assert!(extracted.error_contracts.is_empty());
        for (key, value) in extracted.values {
            assert!(
                parsed.values.insert(key.clone(), value).is_none(),
                "duplicate result across LinuxCNC code probes: {key:?}"
            );
        }
    }
    let requested_count = domains
        .iter()
        .map(|domain| domain.symbols.len())
        .sum::<usize>();
    assert_eq!(
        parsed.values.len(),
        requested_count,
        "LinuxCNC code probe omitted a source code"
    );
    assert_eq!(
        parsed.status_contracts.len(),
        status_contracts.len(),
        "LinuxCNC code probe omitted a public status-message contract"
    );
    assert_eq!(
        parsed.error_contracts.len(),
        error_contracts.len(),
        "LinuxCNC code probe omitted a public error-message contract"
    );
    parsed
}
