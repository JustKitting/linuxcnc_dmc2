use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::config::INCLUDE_ROOT;
use super::domains::Domain;

pub(crate) struct StatusContractValue {
    pub(crate) message_type_name: String,
    pub(crate) message_type: i64,
    pub(crate) message_size: i64,
}

pub(crate) struct Results {
    pub(crate) values: BTreeMap<(String, String), i64>,
    pub(crate) status_contracts: BTreeMap<String, StatusContractValue>,
}

fn paths(output_directory: &Path) -> (PathBuf, PathBuf) {
    (
        output_directory.join("linuxcnc_code_probe.cc"),
        output_directory.join("linuxcnc_code_probe"),
    )
}

fn source(domains: &[Domain], status_contracts: &[(&str, &str)]) -> String {
    let mut probe = String::from(
        "#include <cstdio>\n#include \"emc.hh\"\n#include \"emc_nml.hh\"\n#include \"motion.h\"\n#include \"interp_return.hh\"\n#include \"nml.hh\"\n#include \"nml_oi.hh\"\n#include \"rcs.hh\"\n#include \"stat_msg.hh\"\n#include \"cmd_msg.hh\"\n#include \"cms.hh\"\n#include \"canon.hh\"\n#include \"kinematics.h\"\n#include \"motion_types.h\"\n#include \"debugflags.h\"\n#include \"state_tag.h\"\n#include \"usrmotintf.h\"\nint main() {\n",
    );
    for domain in domains {
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
    for line in String::from_utf8(stdout)
        .expect("LinuxCNC code probe produced non-UTF-8 output")
        .lines()
    {
        let fields = line.split('\t').collect::<Vec<_>>();
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
    }
}

pub(crate) fn run(
    output_directory: &Path,
    domains: &[Domain],
    status_contracts: &[(&str, &str)],
) -> Results {
    let (probe_source, probe_binary) = paths(output_directory);
    fs::write(&probe_source, source(domains, status_contracts))
        .expect("failed to write LinuxCNC code probe");
    compile(&probe_source, &probe_binary);
    let result = Command::new(&probe_binary)
        .output()
        .expect("failed to execute LinuxCNC code probe");
    assert!(
        result.status.success(),
        "LinuxCNC code probe failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let parsed = parse(result.stdout);
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
    parsed
}
