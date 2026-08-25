use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::config::{INCLUDE_ROOT, STATUS_MESSAGE_TYPES};
use super::domains::Domain;

pub(crate) struct Results {
    pub(crate) values: BTreeMap<(String, String), i64>,
    pub(crate) status_sizes: BTreeMap<String, i64>,
}

fn paths(output_directory: &Path) -> (PathBuf, PathBuf) {
    (
        output_directory.join("dmc2_linuxcnc_value_probe.cc"),
        output_directory.join("dmc2_linuxcnc_value_probe"),
    )
}

fn source(domains: &[Domain]) -> String {
    let mut probe = String::from(
        "#include <cstdio>\n#include \"emc.hh\"\n#include \"emc_nml.hh\"\n#include \"motion.h\"\n#include \"interp_return.hh\"\n#include \"nml.hh\"\n#include \"rcs.hh\"\n#include \"stat_msg.hh\"\n#include \"cmd_msg.hh\"\n#include \"canon.hh\"\n#include \"kinematics.h\"\n#include \"motion_types.h\"\n#include \"debugflags.h\"\n#include \"state_tag.h\"\nint main() {\n",
    );
    for domain in domains {
        for symbol in &domain.symbols {
            probe.push_str(&format!(
                "std::printf(\"{}\\t{symbol}\\t%lld\\n\", static_cast<long long>({symbol}));\n",
                domain.name
            ));
        }
    }
    for (class_name, _) in STATUS_MESSAGE_TYPES {
        probe.push_str(&format!(
            "std::printf(\"__dmc2_status_size__\\t{class_name}\\t%zu\\n\", sizeof({class_name}));\n"
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
        .expect("failed to execute g++ for the controller's LinuxCNC value probe");
    assert!(
        result.status.success(),
        "controller value probe failed to compile: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn execute(binary: &Path) -> Results {
    let result = Command::new(binary)
        .output()
        .expect("failed to execute the controller's LinuxCNC value probe");
    assert!(
        result.status.success(),
        "controller value probe failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let mut values = BTreeMap::new();
    let mut status_sizes = BTreeMap::new();
    for line in String::from_utf8(result.stdout)
        .expect("controller value probe produced non-UTF-8 output")
        .lines()
    {
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.first() == Some(&"__dmc2_status_size__") {
            assert_eq!(fields.len(), 3, "malformed controller status size: {line}");
            let size = fields[2]
                .parse::<i64>()
                .unwrap_or_else(|error| panic!("invalid status size in {line:?}: {error}"));
            assert!(size > 0, "non-positive status size in {line:?}");
            assert!(
                status_sizes.insert(fields[1].to_owned(), size).is_none(),
                "duplicate controller status size: {line}"
            );
            continue;
        }
        assert_eq!(fields.len(), 3, "malformed controller value line: {line}");
        let value = fields[2]
            .parse::<i64>()
            .unwrap_or_else(|error| panic!("invalid controller value in {line:?}: {error}"));
        assert!(
            values
                .insert((fields[0].to_owned(), fields[1].to_owned()), value)
                .is_none(),
            "duplicate controller value result: {line}"
        );
    }
    Results {
        values,
        status_sizes,
    }
}

pub(crate) fn run(output_directory: &Path, domains: &[Domain]) -> Results {
    let (probe_source, probe_binary) = paths(output_directory);
    fs::write(&probe_source, source(domains))
        .expect("failed to write the controller's LinuxCNC value probe");
    compile(&probe_source, &probe_binary);
    let results = execute(&probe_binary);
    let requested = domains
        .iter()
        .map(|domain| domain.symbols.len())
        .sum::<usize>();
    assert_eq!(
        results.values.len(),
        requested,
        "controller value probe omitted a required LinuxCNC value"
    );
    assert_eq!(
        results.status_sizes.len(),
        STATUS_MESSAGE_TYPES.len(),
        "controller value probe omitted a copied status-object size"
    );
    results
}
