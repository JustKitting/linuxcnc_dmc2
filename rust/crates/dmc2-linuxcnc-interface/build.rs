use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const EXPECTED_LINUXCNC_VERSION: &str = "2.9.10";
const EXPECTED_LINUXCNC_COMMIT: &str = "86cdca76fa2a36274c432caa21952b23c267989a";
const SOURCE_ROOT_RELATIVE: &str = "../../../vendor/linuxcnc-2.9.10";
const EXPECTED_HEADER_FNV64: u64 = 0x5d196ecfe398141a;
const INCLUDE_ROOT: &str = "/usr/include/linuxcnc";

const EXPECTED_DOMAIN_COUNTS: &[(&str, usize)] = &[
    ("emc_nml_message_type", 145),
    ("nml_operator_message_type", 3),
    ("task_mode", 3),
    ("task_state", 4),
    ("task_exec", 9),
    ("task_interp", 4),
    ("traj_mode", 3),
    ("io_abort_reason", 11),
    ("joint_type", 2),
    ("motion_command", 74),
    ("motion_command_status", 5),
    ("motion_state", 4),
    ("spindle_orient_state", 4),
    ("interpreter_return", 6),
    ("nml_error", 9),
    ("nml_channel_type", 6),
    ("rcs_status", 4),
    ("rcs_state", 53),
    ("canon_bool", 2),
    ("canon_plane", 6),
    ("canon_units", 3),
    ("canon_motion_mode", 3),
    ("canon_speed_feed_mode", 2),
    ("canon_direction", 3),
    ("canon_feed_reference", 2),
    ("canon_side", 3),
    ("canon_axis", 9),
    ("kinematics_type", 4),
    ("motion_type", 6),
    ("motion_flag", 5),
    ("motion_termination_condition", 3),
    ("spindle_feed_enable_flag", 4),
    ("aux_input_type", 2),
    ("aux_wait_mode", 5),
    ("debug_flag", 20),
    ("joint_flag", 8),
    ("motion_communication_result", 6),
    ("state_tag_flag", 25),
    ("state_tag_field", 9),
    ("state_tag_float_field", 6),
    ("cms_status", 23),
    ("cms_mode", 7),
    ("cms_internal_access", 11),
    ("cms_buffer_type", 4),
    ("cms_process_type", 4),
    ("cms_remote_port_type", 5),
    ("cms_encoding", 4),
    ("cms_connection_mode", 3),
    ("rcs_generic_command", 2),
    ("rcs_generic_message_type", 2),
];

fn command_output(program: &str, arguments: &[&str]) -> String {
    let output = Command::new(program)
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("failed to execute {program}: {error}"));
    assert!(
        output.status.success(),
        "{program} failed with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|error| panic!("{program} produced non-UTF-8 output: {error}"))
}

fn source_root() -> PathBuf {
    PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"))
        .join(SOURCE_ROOT_RELATIVE)
}

fn source_header_path(name: &str) -> PathBuf {
    let relative = match name {
        "emc.hh" | "interp_return.hh" | "canon.hh" | "motion_types.h" | "debugflags.h" => {
            format!("src/emc/nml_intf/{name}")
        }
        "emc_nml.hh" => "src/emc/nml_intf/emc_nml.hh".to_owned(),
        "motion.h" => "src/emc/motion/motion.h".to_owned(),
        "emcmotcfg.h" => "src/emc/motion/emcmotcfg.h".to_owned(),
        "kinematics.h" => "src/emc/kinematics/kinematics.h".to_owned(),
        "nml.hh" | "nml_oi.hh" => format!("src/libnml/nml/{name}"),
        "rcs.hh" => "src/libnml/rcs/rcs.hh".to_owned(),
        "stat_msg.hh" => "src/libnml/nml/stat_msg.hh".to_owned(),
        "cmd_msg.hh" => "src/libnml/nml/cmd_msg.hh".to_owned(),
        "cms.hh" => "src/libnml/cms/cms.hh".to_owned(),
        "state_tag.h" => "src/emc/motion/state_tag.h".to_owned(),
        "usrmotintf.h" => "src/emc/motion/usrmotintf.h".to_owned(),
        _ => panic!("no audited LinuxCNC source mapping for {name}"),
    };
    source_root().join(relative)
}

fn read_header(name: &str) -> String {
    let installed_path = Path::new(INCLUDE_ROOT).join(name);
    let source_path = source_header_path(name);
    println!("cargo:rerun-if-changed={}", installed_path.display());
    println!("cargo:rerun-if-changed={}", source_path.display());
    let installed = fs::read_to_string(&installed_path).unwrap_or_else(|error| {
        panic!(
            "failed to read installed header {}: {error}",
            installed_path.display()
        )
    });
    let source = fs::read_to_string(&source_path).unwrap_or_else(|error| {
        panic!(
            "failed to read pulled source {}: {error}",
            source_path.display()
        )
    });
    assert_eq!(
        source, installed,
        "installed {name} does not exactly match pulled LinuxCNC v2.9.10 source"
    );
    source
}

fn logical_preprocessor_lines(source: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    for line in source.lines() {
        let trimmed = line.trim_end();
        if let Some(prefix) = trimmed.strip_suffix('\\') {
            current.push_str(prefix);
            current.push(' ');
        } else if current.is_empty() {
            result.push(trimmed.to_owned());
        } else {
            current.push_str(trimmed);
            result.push(std::mem::take(&mut current));
        }
    }
    assert!(current.is_empty(), "unterminated preprocessor continuation");
    result
}

fn c_string_literals(expression: &str) -> String {
    let bytes = expression.as_bytes();
    let mut output = String::new();
    let mut index = 0;
    let mut found = false;
    while index < bytes.len() {
        if bytes[index] != b'"' {
            index += 1;
            continue;
        }
        found = true;
        index += 1;
        while index < bytes.len() && bytes[index] != b'"' {
            if bytes[index] == b'\\' {
                index += 1;
                assert!(
                    index < bytes.len(),
                    "unterminated C string escape in {expression:?}"
                );
                match bytes[index] {
                    b'n' => output.push('\n'),
                    b'r' => output.push('\r'),
                    b't' => output.push('\t'),
                    b'\\' => output.push('\\'),
                    b'"' => output.push('"'),
                    other => {
                        output.push('\\');
                        output.push(other as char);
                    }
                }
            } else {
                output.push(bytes[index] as char);
            }
            index += 1;
        }
        assert!(
            index < bytes.len(),
            "unterminated C string in {expression:?}"
        );
        index += 1;
    }
    assert!(
        found && !output.is_empty(),
        "NCE macro has no message text: {expression:?}"
    );
    output
}

fn interpreter_error_templates(source: &str) -> Vec<(String, String)> {
    let mut templates = Vec::new();
    for line in logical_preprocessor_lines(source) {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("#define NCE_") {
            continue;
        }
        let mut parts = trimmed.splitn(3, char::is_whitespace);
        assert_eq!(parts.next(), Some("#define"));
        let name = parts.next().expect("NCE macro omitted its name").to_owned();
        let expression = parts.next().expect("NCE macro omitted its message");
        templates.push((name, c_string_literals(expression)));
    }
    assert_eq!(
        templates.len(),
        198,
        "LinuxCNC v2.9.10 NCE template count changed"
    );
    templates
}

fn strip_comments(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut index = 0;
    let mut in_block = false;
    let mut in_line = false;
    while index < bytes.len() {
        if in_block {
            if index + 1 < bytes.len() && bytes[index] == b'*' && bytes[index + 1] == b'/' {
                in_block = false;
                index += 2;
            } else {
                if bytes[index] == b'\n' {
                    output.push('\n');
                }
                index += 1;
            }
            continue;
        }
        if in_line {
            if bytes[index] == b'\n' {
                in_line = false;
                output.push('\n');
            }
            index += 1;
            continue;
        }
        if index + 1 < bytes.len() && bytes[index] == b'/' && bytes[index + 1] == b'*' {
            in_block = true;
            index += 2;
            continue;
        }
        if index + 1 < bytes.len() && bytes[index] == b'/' && bytes[index + 1] == b'/' {
            in_line = true;
            index += 2;
            continue;
        }
        output.push(bytes[index] as char);
        index += 1;
    }
    assert!(
        !in_block,
        "unterminated block comment while parsing LinuxCNC headers"
    );
    output
}

fn enum_body_after(source: &str, marker: &str) -> String {
    let cleaned = strip_comments(source);
    let marker_index = cleaned
        .find(marker)
        .unwrap_or_else(|| panic!("LinuxCNC header omitted enum marker {marker:?}"));
    let open_relative = cleaned[marker_index..]
        .find('{')
        .unwrap_or_else(|| panic!("enum marker {marker:?} has no opening brace"));
    let open = marker_index + open_relative;
    let mut depth = 0_u32;
    for (relative, character) in cleaned[open..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return cleaned[open + 1..open + relative].to_owned();
                }
            }
            _ => {}
        }
    }
    panic!("enum marker {marker:?} has no closing brace");
}

fn typedef_enum_body(source: &str, alias: &str) -> String {
    let cleaned = strip_comments(source);
    let end_marker = format!("}} {alias}");
    let end = cleaned
        .find(&end_marker)
        .unwrap_or_else(|| panic!("LinuxCNC header omitted typedef enum alias {alias:?}"));
    let start = cleaned[..end]
        .rfind("typedef enum")
        .unwrap_or_else(|| panic!("typedef enum alias {alias:?} has no declaration"));
    enum_body_after(&cleaned[start..], "typedef enum")
}

fn parse_enum_names(body: &str) -> Vec<String> {
    let mut names = Vec::new();
    for raw_item in body.split(',') {
        let item = raw_item
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join(" ");
        let declaration = item.split('=').next().unwrap_or_default().trim();
        if declaration.is_empty() {
            continue;
        }
        let name = declaration
            .split_whitespace()
            .last()
            .unwrap_or_default()
            .trim_matches(|character: char| !character.is_ascii_alphanumeric() && character != '_');
        assert!(
            !name.is_empty()
                && name
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_'),
            "could not parse LinuxCNC enum item {raw_item:?}"
        );
        names.push(name.to_owned());
    }
    assert!(!names.is_empty(), "parsed an empty LinuxCNC enum");
    names
}

fn named_enum(source: &str, marker: &str) -> Vec<String> {
    parse_enum_names(&enum_body_after(source, marker))
}

fn typedef_enum(source: &str, alias: &str) -> Vec<String> {
    parse_enum_names(&typedef_enum_body(source, alias))
}

fn macro_names<F>(source: &str, mut accept: F) -> Vec<String>
where
    F: FnMut(&str, &str) -> bool,
{
    let cleaned = strip_comments(source);
    let mut names = Vec::new();
    for line in cleaned.lines() {
        let mut tokens = line.split_whitespace();
        if tokens.next() != Some("#define") {
            continue;
        }
        let Some(name) = tokens.next() else {
            continue;
        };
        let remainder = tokens.collect::<Vec<_>>().join(" ");
        if accept(name, &remainder) {
            names.push(name.to_owned());
        }
    }
    assert!(!names.is_empty(), "parsed an empty LinuxCNC macro domain");
    names
}

fn integer_macro(source: &str, expected_name: &str) -> usize {
    let cleaned = strip_comments(source);
    let mut matches = cleaned.lines().filter_map(|line| {
        let mut tokens = line.split_whitespace();
        if tokens.next() != Some("#define") || tokens.next() != Some(expected_name) {
            return None;
        }
        Some(
            tokens
                .next()
                .unwrap_or_else(|| panic!("{expected_name} has no value"))
                .parse::<usize>()
                .unwrap_or_else(|error| {
                    panic!("{expected_name} is not a decimal integer: {error}")
                }),
        )
    });
    let value = matches
        .next()
        .unwrap_or_else(|| panic!("LinuxCNC header omitted {expected_name}"));
    assert!(
        matches.next().is_none(),
        "LinuxCNC header defines {expected_name} more than once"
    );
    value
}

fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
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

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let source_root = source_root();
    let source_root_text = source_root
        .to_str()
        .expect("LinuxCNC source path is not valid UTF-8");
    let installed_version = command_output("linuxcnc_var", &["LINUXCNCVERSION"]);
    assert_eq!(
        installed_version.trim(),
        EXPECTED_LINUXCNC_VERSION,
        "refusing to generate an interface catalog for an unaudited LinuxCNC version"
    );
    let source_commit = command_output("git", &["-C", source_root_text, "rev-parse", "HEAD"]);
    assert_eq!(
        source_commit.trim(),
        EXPECTED_LINUXCNC_COMMIT,
        "pulled LinuxCNC source is not the audited v2.9.10 commit"
    );
    let source_changes = command_output("git", &["-C", source_root_text, "status", "--porcelain"]);
    assert!(
        source_changes.trim().is_empty(),
        "pulled LinuxCNC v2.9.10 source has local modifications"
    );

    let emc = read_header("emc.hh");
    let emc_nml = read_header("emc_nml.hh");
    let motion = read_header("motion.h");
    let emcmotcfg = read_header("emcmotcfg.h");
    let interp_return = read_header("interp_return.hh");
    let nml = read_header("nml.hh");
    let nml_oi = read_header("nml_oi.hh");
    let rcs = read_header("rcs.hh");
    let stat_msg = read_header("stat_msg.hh");
    let canon = read_header("canon.hh");
    let kinematics = read_header("kinematics.h");
    let motion_types = read_header("motion_types.h");
    let debug_flags = read_header("debugflags.h");
    let state_tag = read_header("state_tag.h");
    let usrmotintf = read_header("usrmotintf.h");
    let cms = read_header("cms.hh");
    let cmd_msg = read_header("cmd_msg.hh");
    let nce_path = source_root.join("src/emc/rs274ngc/rs274ngc_return.hh");
    println!("cargo:rerun-if-changed={}", nce_path.display());
    let nce_source = fs::read_to_string(&nce_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", nce_path.display()));
    let nce_templates = interpreter_error_templates(&nce_source);
    let status_message_contracts = [
        ("EMC_STAT", "EMC_STAT_TYPE"),
        ("EMC_TASK_STAT", "EMC_TASK_STAT_TYPE"),
        ("EMC_MOTION_STAT", "EMC_MOTION_STAT_TYPE"),
        ("EMC_TRAJ_STAT", "EMC_TRAJ_STAT_TYPE"),
        ("EMC_JOINT_STAT", "EMC_JOINT_STAT_TYPE"),
        ("EMC_AXIS_STAT", "EMC_AXIS_STAT_TYPE"),
        ("EMC_SPINDLE_STAT", "EMC_SPINDLE_STAT_TYPE"),
        ("EMC_IO_STAT", "EMC_IO_STAT_TYPE"),
        ("EMC_TOOL_STAT", "EMC_TOOL_STAT_TYPE"),
        ("EMC_AUX_STAT", "EMC_AUX_STAT_TYPE"),
        ("EMC_COOLANT_STAT", "EMC_COOLANT_STAT_TYPE"),
        ("EMC_LUBE_STAT", "EMC_LUBE_STAT_TYPE"),
    ];
    let emcmot_max_joints = integer_macro(&emcmotcfg, "EMCMOT_MAX_JOINTS");
    let emcmot_max_axis = integer_macro(&emcmotcfg, "EMCMOT_MAX_AXIS");
    let emcmot_max_spindles = integer_macro(&emcmotcfg, "EMCMOT_MAX_SPINDLES");
    let emcmot_max_misc_error = integer_macro(&emcmotcfg, "EMCMOT_MAX_MISC_ERROR");

    let mut domains: Vec<(&str, Vec<String>)> = vec![
        (
            "emc_nml_message_type",
            macro_names(&emc, |name, value| {
                name.starts_with("EMC_") && name.ends_with("_TYPE") && value.contains("NMLTYPE")
            }),
        ),
        (
            "nml_operator_message_type",
            macro_names(&nml_oi, |name, value| {
                matches!(
                    name,
                    "NML_ERROR_TYPE" | "NML_TEXT_TYPE" | "NML_DISPLAY_TYPE"
                ) && value.contains("NMLTYPE")
            }),
        ),
        ("task_mode", named_enum(&emc, "enum EMC_TASK_MODE_ENUM")),
        ("task_state", named_enum(&emc, "enum EMC_TASK_STATE_ENUM")),
        ("task_exec", named_enum(&emc, "enum EMC_TASK_EXEC_ENUM")),
        ("task_interp", named_enum(&emc, "enum EMC_TASK_INTERP_ENUM")),
        ("traj_mode", named_enum(&emc, "enum EMC_TRAJ_MODE_ENUM")),
        (
            "io_abort_reason",
            named_enum(&emc, "enum EMC_IO_ABORT_REASON_ENUM"),
        ),
        ("joint_type", named_enum(&emc, "enum EmcJointType")),
        ("motion_command", typedef_enum(&motion, "cmd_code_t")),
        (
            "motion_command_status",
            typedef_enum(&motion, "cmd_status_t"),
        ),
        ("motion_state", typedef_enum(&motion, "motion_state_t")),
        (
            "spindle_orient_state",
            typedef_enum(&motion, "orient_state_t"),
        ),
        (
            "interpreter_return",
            named_enum(&interp_return, "enum InterpReturn"),
        ),
        ("nml_error", named_enum(&nml, "enum NML_ERROR_TYPE")),
        (
            "nml_channel_type",
            named_enum(&nml, "enum NML_CHANNEL_TYPE"),
        ),
        ("rcs_status", named_enum(&rcs, "enum RCS_STATUS")),
        ("rcs_state", named_enum(&stat_msg, "enum RCS_STATE")),
        ("canon_bool", named_enum(&canon, "enum CanonBool")),
        ("canon_plane", named_enum(&canon, "enum CANON_PLANE")),
        ("canon_units", named_enum(&canon, "enum CANON_UNITS")),
        (
            "canon_motion_mode",
            named_enum(&canon, "enum CANON_MOTION_MODE"),
        ),
        (
            "canon_speed_feed_mode",
            named_enum(&canon, "enum CANON_SPEED_FEED_MODE"),
        ),
        (
            "canon_direction",
            named_enum(&canon, "enum CANON_DIRECTION"),
        ),
        (
            "canon_feed_reference",
            named_enum(&canon, "enum CANON_FEED_REFERENCE"),
        ),
        ("canon_side", named_enum(&canon, "enum CANON_SIDE")),
        ("canon_axis", named_enum(&canon, "enum CANON_AXIS")),
        (
            "kinematics_type",
            typedef_enum(&kinematics, "KINEMATICS_TYPE"),
        ),
        (
            "motion_type",
            macro_names(&motion_types, |name, _| {
                name.starts_with("EMC_MOTION_TYPE_")
            }),
        ),
        (
            "motion_flag",
            macro_names(&motion, |name, _| {
                name.starts_with("EMCMOT_MOTION_") && name.ends_with("_BIT")
            }),
        ),
        (
            "motion_termination_condition",
            macro_names(&motion, |name, _| name.starts_with("EMCMOT_TERM_COND_")),
        ),
        (
            "spindle_feed_enable_flag",
            macro_names(&motion, |name, _| {
                matches!(
                    name,
                    "SS_ENABLED" | "FS_ENABLED" | "AF_ENABLED" | "FH_ENABLED"
                )
            }),
        ),
        (
            "aux_input_type",
            macro_names(&canon, |name, _| {
                matches!(name, "DIGITAL_INPUT" | "ANALOG_INPUT")
            }),
        ),
        (
            "aux_wait_mode",
            macro_names(&canon, |name, _| name.starts_with("WAIT_MODE_")),
        ),
        (
            "debug_flag",
            macro_names(&debug_flags, |name, _| name.starts_with("EMC_DEBUG_")),
        ),
        (
            "joint_flag",
            macro_names(&motion, |name, _| {
                name.starts_with("EMCMOT_JOINT_") && name.ends_with("_BIT")
            }),
        ),
        (
            "motion_communication_result",
            macro_names(&usrmotintf, |name, _| name.starts_with("EMCMOT_COMM_")),
        ),
        ("state_tag_flag", typedef_enum(&state_tag, "StateFlag")),
        ("state_tag_field", typedef_enum(&state_tag, "StateField")),
        (
            "state_tag_float_field",
            typedef_enum(&state_tag, "StateFieldFloat"),
        ),
        ("cms_status", named_enum(&cms, "enum CMS_STATUS")),
        ("cms_mode", named_enum(&cms, "enum CMSMODE")),
        (
            "cms_internal_access",
            named_enum(&cms, "enum CMS_INTERNAL_ACCESS_TYPE"),
        ),
        ("cms_buffer_type", named_enum(&cms, "enum CMS_BUFFERTYPE")),
        ("cms_process_type", named_enum(&cms, "enum CMS_PROCESSTYPE")),
        (
            "cms_remote_port_type",
            named_enum(&cms, "enum CMS_REMOTE_PORT_TYPE"),
        ),
        (
            "cms_encoding",
            named_enum(&cms, "enum CMS_NEUTRAL_ENCODING_METHOD"),
        ),
        (
            "cms_connection_mode",
            named_enum(&cms, "enum CMS_CONNECTION_MODE"),
        ),
        (
            "rcs_generic_command",
            named_enum(&cmd_msg, "enum RCS_GENERIC_CMD_ID"),
        ),
        (
            "rcs_generic_message_type",
            [
                macro_names(&cmd_msg, |name, value| {
                    name == "RCS_GENERIC_CMD_TYPE" && value.contains("NMLTYPE")
                }),
                macro_names(&stat_msg, |name, value| {
                    name == "RCS_GENERIC_STATUS_TYPE" && value.contains("NMLTYPE")
                }),
            ]
            .concat(),
        ),
    ];

    let mut seen_domain_names = BTreeSet::new();
    for (domain, names) in &domains {
        assert!(
            seen_domain_names.insert(*domain),
            "duplicate catalog domain {domain}"
        );
        let unique = names.iter().collect::<BTreeSet<_>>();
        assert_eq!(
            unique.len(),
            names.len(),
            "duplicate source name in {domain}"
        );
    }
    let actual_domain_counts = domains
        .iter()
        .map(|(name, codes)| (*name, codes.len()))
        .collect::<Vec<_>>();
    assert_eq!(
        actual_domain_counts, EXPECTED_DOMAIN_COUNTS,
        "LinuxCNC 2.9.10 public code domains changed or the source parser omitted a code"
    );

    let output_directory =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo did not provide OUT_DIR"));
    let probe_source = output_directory.join("linuxcnc_code_probe.cc");
    let probe_binary = output_directory.join("linuxcnc_code_probe");
    let mut probe = String::from(
        "#include <cstdio>\n#include \"emc.hh\"\n#include \"emc_nml.hh\"\n#include \"motion.h\"\n#include \"interp_return.hh\"\n#include \"nml.hh\"\n#include \"nml_oi.hh\"\n#include \"rcs.hh\"\n#include \"stat_msg.hh\"\n#include \"cmd_msg.hh\"\n#include \"cms.hh\"\n#include \"canon.hh\"\n#include \"kinematics.h\"\n#include \"motion_types.h\"\n#include \"debugflags.h\"\n#include \"state_tag.h\"\n#include \"usrmotintf.h\"\nint main() {\n",
    );
    for (domain, names) in &domains {
        for name in names {
            probe.push_str(&format!(
                "std::printf(\"{domain}\\t{name}\\t%lld\\n\", static_cast<long long>({name}));\n"
            ));
        }
    }
    for (class_name, message_type_name) in status_message_contracts {
        probe.push_str(&format!(
            "std::printf(\"__status_message_contract__\\t{class_name}\\t{message_type_name}\\t%lld\\t%zu\\n\", static_cast<long long>({message_type_name}), sizeof({class_name}));\n"
        ));
    }
    probe.push_str("return 0;\n}\n");
    fs::write(&probe_source, probe).expect("failed to write LinuxCNC code probe");

    let probe_source_text = probe_source.to_str().expect("non-UTF-8 probe source path");
    let probe_binary_text = probe_binary.to_str().expect("non-UTF-8 probe binary path");
    let compile = Command::new("g++")
        .args([
            "-std=c++17",
            "-O2",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-isystem",
            INCLUDE_ROOT,
            probe_source_text,
            "-o",
            probe_binary_text,
        ])
        .output()
        .expect("failed to execute g++ for LinuxCNC code probe");
    assert!(
        compile.status.success(),
        "LinuxCNC code probe failed to compile: {}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let probe_output = Command::new(&probe_binary)
        .output()
        .expect("failed to execute LinuxCNC code probe");
    assert!(
        probe_output.status.success(),
        "LinuxCNC code probe failed: {}",
        String::from_utf8_lossy(&probe_output.stderr)
    );

    let mut values = BTreeMap::new();
    let mut status_contract_values = BTreeMap::new();
    for line in String::from_utf8(probe_output.stdout)
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
                status_contract_values
                    .insert(
                        fields[1].to_owned(),
                        (fields[2].to_owned(), message_type, message_size),
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

    let requested_count = domains.iter().map(|(_, names)| names.len()).sum::<usize>();
    assert_eq!(
        values.len(),
        requested_count,
        "LinuxCNC code probe omitted a source code"
    );
    assert_eq!(
        status_contract_values.len(),
        status_message_contracts.len(),
        "LinuxCNC code probe omitted a public status-message contract"
    );

    let mut fingerprint = 0xcbf29ce484222325_u64;
    for source in [
        &emc,
        &emc_nml,
        &motion,
        &emcmotcfg,
        &interp_return,
        &nml,
        &nml_oi,
        &rcs,
        &stat_msg,
        &canon,
        &kinematics,
        &motion_types,
        &debug_flags,
        &state_tag,
        &usrmotintf,
        &cms,
        &cmd_msg,
    ] {
        fingerprint = fnv1a(fingerprint, source.as_bytes());
    }
    assert_eq!(
        fingerprint, EXPECTED_HEADER_FNV64,
        "installed LinuxCNC 2.9.10 public headers differ from the audited source"
    );

    let mut generated = format!(
        "pub const LINUXCNC_VERSION: &str = \"{EXPECTED_LINUXCNC_VERSION}\";\n\
         pub const LINUXCNC_SOURCE_COMMIT: &str = \"{EXPECTED_LINUXCNC_COMMIT}\";\n\
         pub const HEADER_SOURCE_FNV64: u64 = 0x{fingerprint:016x};\n\
         pub const GENERATED_CODE_COUNT: usize = {requested_count};\n\
         pub const STATUS_MESSAGE_CONTRACT_COUNT: usize = {};\n\
         pub const EMCMOT_MAX_JOINTS: usize = {emcmot_max_joints};\n\
         pub const EMCMOT_MAX_AXIS: usize = {emcmot_max_axis};\n\
         pub const EMCMOT_MAX_SPINDLES: usize = {emcmot_max_spindles};\n\
         pub const EMCMOT_MAX_MISC_ERROR: usize = {emcmot_max_misc_error};\n",
        status_message_contracts.len(),
    );
    generated.push_str("pub static INTERPRETER_ERROR_TEMPLATES: &[MessageTemplate] = &[\n");
    for (name, template) in &nce_templates {
        generated.push_str(&format!(
            "MessageTemplate {{ name: {name:?}, template: {template:?} }},\n"
        ));
    }
    generated.push_str("];\n");
    generated.push_str("pub static STATUS_MESSAGE_CONTRACTS: &[StatusMessageContract] = &[\n");
    for (class_name, expected_message_type_name) in status_message_contracts {
        let (message_type_name, message_type, message_size) = &status_contract_values[class_name];
        assert_eq!(
            message_type_name, expected_message_type_name,
            "status-message probe returned the wrong type name for {class_name}"
        );
        generated.push_str(&format!(
            "StatusMessageContract {{ class_name: {class_name:?}, message_type_name: {message_type_name:?}, message_type: {message_type}, message_size: {message_size} }},\n"
        ));
    }
    generated.push_str("];\n");
    for (domain, names) in &domains {
        let identifier = rust_identifier(domain);
        generated.push_str(&format!(
            "pub static {identifier}: CodeDomain = CodeDomain {{ name: \"{domain}\", codes: &[\n"
        ));
        let mut domain_values = BTreeSet::new();
        for name in names {
            let value = values[&(domain.to_string(), name.to_string())];
            assert!(
                domain_values.insert(value),
                "duplicate numeric value {value} in LinuxCNC code domain {domain}"
            );
            generated.push_str(&format!(
                "CodeName {{ code: {value}, name: \"{name}\" }},\n"
            ));
        }
        generated.push_str("] };\n");
    }
    generated.push_str("pub static DOMAINS: &[CodeDomain] = &[\n");
    for (domain, _) in &domains {
        generated.push_str(&format!("{},\n", rust_identifier(domain)));
    }
    generated.push_str("];\n");
    fs::write(output_directory.join("linuxcnc_code_catalog.rs"), generated)
        .expect("failed to write generated LinuxCNC code catalog");

    domains.clear();
}
