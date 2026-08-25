use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::config::INCLUDE_ROOT;
use super::parser::{macro_definitions, MacroDefinition};
use super::source::PublicHeader;

const EXPECTED_BINDGEN_VERSION: &str = "bindgen 0.71.1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IntegerValue {
    Signed(i128),
    Unsigned(u128),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BindgenClassification {
    Integer(IntegerValue),
    NonInteger,
}

struct HeaderContext {
    input: PathBuf,
    forced_includes: Vec<&'static str>,
    include_directories: Vec<PathBuf>,
}

fn context(header: &PublicHeader, source_root: &Path) -> HeaderContext {
    let manifest =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let forced_includes = match header.name.as_str() {
        "hostmot2-serial.h" => vec!["rtapi_stdint.h"],
        "homing.h" => vec!["motion.h"],
        "interpl.hh" => vec!["nmlmsg.hh"],
        "rem_msg.hh" | "tcp_srv.hh" => vec!["cms.hh"],
        "rtapi_bitops.h" => vec!["stddef.h"],
        _ => Vec::new(),
    };
    if header.name == "interp_internal.hh" {
        HeaderContext {
            input: manifest.join("build/wrappers/interp_internal.hh"),
            forced_includes,
            include_directories: vec![
                source_root.join("src/emc/rs274ngc"),
                source_root.join("src/emc/tooldata"),
            ],
        }
    } else {
        let include_directories = if header.name == "cms_xup.hh" {
            vec![manifest.join("build/wrappers/include")]
        } else {
            Vec::new()
        };
        HeaderContext {
            input: header.installed_path.clone(),
            forced_includes,
            include_directories,
        }
    }
}

fn compiler_arguments(context: &HeaderContext) -> Vec<OsString> {
    let mut arguments = vec![
        OsString::from("-x"),
        OsString::from("c++"),
        OsString::from("-std=c++17"),
        OsString::from("-DULAPI"),
        OsString::from("-I"),
        OsString::from(INCLUDE_ROOT),
    ];
    for directory in &context.include_directories {
        arguments.push(OsString::from("-I"));
        arguments.push(directory.as_os_str().to_owned());
    }
    for include in &context.forced_includes {
        arguments.push(OsString::from("-include"));
        arguments.push(OsString::from(include));
    }
    arguments
}

fn checked_output(mut command: Command, purpose: &str) -> Output {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("failed to execute {purpose}: {error}"));
    assert!(
        output.status.success(),
        "{purpose} failed with {}:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

pub(crate) fn verify_tool() {
    let mut command = Command::new("bindgen");
    command.arg("--version");
    let output = checked_output(command, "bindgen version check");
    let version = String::from_utf8(output.stdout).expect("bindgen version is not UTF-8");
    assert_eq!(
        version.trim(),
        EXPECTED_BINDGEN_VERSION,
        "refusing to classify LinuxCNC public macros with an unaudited bindgen version"
    );
}

pub(crate) fn active_definitions(
    header: &PublicHeader,
    source_root: &Path,
) -> BTreeMap<String, MacroDefinition> {
    let context = context(header, source_root);
    let mut command = Command::new("g++");
    command.args(compiler_arguments(&context));
    command.args(["-dM", "-E"]);
    command.arg(&context.input);
    let output = checked_output(
        command,
        &format!("active macro preprocessing for {}", header.name),
    );
    let source = String::from_utf8(output.stdout).unwrap_or_else(|error| {
        panic!(
            "preprocessor output for {} is not UTF-8: {error}",
            header.name
        )
    });
    let mut definitions = BTreeMap::new();
    for definition in macro_definitions(&source) {
        assert!(
            definitions
                .insert(definition.name.clone(), definition)
                .is_none(),
            "the active preprocessor inventory for {} contains a duplicate macro",
            header.name
        );
    }
    definitions
}

fn allowlist(names: &[String]) -> String {
    assert!(!names.is_empty());
    format!("^({})$", names.join("|"))
}

fn parse_integer(value: &str, signed: bool) -> Option<IntegerValue> {
    let value = value.trim().replace('_', "");
    if signed {
        value.parse::<i128>().ok().map(IntegerValue::Signed)
    } else {
        match value.as_str() {
            "false" => Some(IntegerValue::Unsigned(0)),
            "true" => Some(IntegerValue::Unsigned(1)),
            _ => value.parse::<u128>().ok().map(IntegerValue::Unsigned),
        }
    }
}

fn parse_bindgen_constants(
    source: &str,
    requested: &[String],
) -> BTreeMap<String, BindgenClassification> {
    let mut result = BTreeMap::new();
    for line in source.lines() {
        let Some(declaration) = line.trim().strip_prefix("pub const ") else {
            continue;
        };
        let Some((name, remainder)) = declaration.split_once(": ") else {
            continue;
        };
        if requested
            .binary_search_by(|candidate| candidate.as_str().cmp(name))
            .is_err()
        {
            continue;
        }
        let Some((kind, value)) = remainder
            .strip_suffix(';')
            .and_then(|text| text.split_once(" = "))
        else {
            continue;
        };
        let classification = match kind {
            "i8" | "i16" | "i32" | "i64" | "i128" | "isize" => {
                parse_integer(value, true).map(BindgenClassification::Integer)
            }
            "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "bool" => {
                parse_integer(value, false).map(BindgenClassification::Integer)
            }
            "f32" | "f64" => Some(BindgenClassification::NonInteger),
            _ if kind.starts_with('&') || kind.starts_with('*') || kind.starts_with('[') => {
                Some(BindgenClassification::NonInteger)
            }
            _ => None,
        };
        if let Some(classification) = classification {
            assert!(
                result.insert(name.to_owned(), classification).is_none(),
                "bindgen emitted {name} more than once"
            );
        }
    }
    result
}

pub(crate) fn bindgen_classifications(
    header: &PublicHeader,
    source_root: &Path,
    names: &[String],
) -> BTreeMap<String, BindgenClassification> {
    if names.is_empty() {
        return BTreeMap::new();
    }
    let context = context(header, source_root);
    let mut requested = names.to_vec();
    requested.sort();
    requested.dedup();
    assert_eq!(
        requested.len(),
        names.len(),
        "duplicate bindgen allowlist name"
    );
    let mut command = Command::new("bindgen");
    command.arg(&context.input);
    command.args([
        "--allowlist-var",
        &allowlist(&requested),
        "--no-layout-tests",
        "--",
    ]);
    command.args(compiler_arguments(&context));
    let output = checked_output(
        command,
        &format!("bindgen macro classification for {}", header.name),
    );
    let bindings = String::from_utf8(output.stdout)
        .unwrap_or_else(|error| panic!("bindgen output for {} is not UTF-8: {error}", header.name));
    parse_bindgen_constants(&bindings, &requested)
}

fn include_source(context: &HeaderContext) -> String {
    let mut source = String::from("#include <cstdio>\n#include <type_traits>\n");
    for include in &context.forced_includes {
        source.push_str(&format!("#include <{include}>\n"));
    }
    source.push_str(&format!(
        "#include {:?}\n",
        context.input.to_str().expect("non-UTF-8 macro probe input")
    ));
    source
}

pub(crate) fn integral_fallback(
    header: &PublicHeader,
    source_root: &Path,
    output_directory: &Path,
    sequence: usize,
    name: &str,
) -> Option<IntegerValue> {
    let context = context(header, source_root);
    let source_path = output_directory.join(format!("linuxcnc_macro_{sequence}.cc"));
    let binary_path = output_directory.join(format!("linuxcnc_macro_{sequence}"));
    let mut source = include_source(&context);
    source.push_str(&format!(
        "using Dmc2Raw = std::remove_cv_t<std::remove_reference_t<decltype(({name}))>>;\n\
         template <typename T, bool IsEnum = std::is_enum_v<T>> struct Dmc2Base {{ using Type = T; }};\n\
         template <typename T> struct Dmc2Base<T, true> {{ using Type = std::underlying_type_t<T>; }};\n\
         using Dmc2Value = typename Dmc2Base<Dmc2Raw>::Type;\n\
         static_assert(std::is_integral_v<Dmc2Value>);\n\
         static_assert(sizeof(Dmc2Value) <= sizeof(unsigned long long));\n\
         constexpr Dmc2Value dmc2_value = static_cast<Dmc2Value>(({name}));\n\
         int main() {{\n\
             if constexpr (std::is_signed_v<Dmc2Value>) {{\n\
                 std::printf(\"signed\\t%lld\\n\", static_cast<long long>(dmc2_value));\n\
             }} else {{\n\
                 std::printf(\"unsigned\\t%llu\\n\", static_cast<unsigned long long>(dmc2_value));\n\
             }}\n\
             return 0;\n\
         }}\n"
    ));
    fs::write(&source_path, source).expect("failed to write LinuxCNC macro fallback probe");
    let mut command = Command::new("g++");
    command.args(compiler_arguments(&context));
    command.args(["-O2", "-Wall", "-Wextra"]);
    command.arg(&source_path);
    command.arg("-o");
    command.arg(&binary_path);
    let compiled = command.output().unwrap_or_else(|error| {
        panic!(
            "failed to compile fallback probe for {}::{name}: {error}",
            header.name
        )
    });
    if !compiled.status.success() {
        return None;
    }
    let execute = Command::new(&binary_path);
    let output = checked_output(
        execute,
        &format!("integral macro probe for {}::{name}", header.name),
    );
    let text = String::from_utf8(output.stdout).unwrap_or_else(|error| {
        panic!(
            "macro probe output for {}::{name} is not UTF-8: {error}",
            header.name
        )
    });
    let (kind, value) = text.trim().split_once('\t').unwrap_or_else(|| {
        panic!(
            "malformed macro probe output for {}::{name}: {text:?}",
            header.name
        )
    });
    match kind {
        "signed" => Some(IntegerValue::Signed(value.parse().unwrap_or_else(
            |error| {
                panic!(
                    "invalid signed macro value for {}::{name}: {error}",
                    header.name
                )
            },
        ))),
        "unsigned" => Some(IntegerValue::Unsigned(value.parse().unwrap_or_else(
            |error| {
                panic!(
                    "invalid unsigned macro value for {}::{name}: {error}",
                    header.name
                )
            },
        ))),
        _ => panic!(
            "unknown macro probe kind for {}::{name}: {kind}",
            header.name
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindgen_integer_and_non_integer_constants_are_distinguished() {
        let requested = vec![
            "NEGATIVE".to_owned(),
            "POSITIVE".to_owned(),
            "REAL".to_owned(),
            "TEXT".to_owned(),
        ];
        let source = "pub const NEGATIVE: i32 = -4;\npub const POSITIVE: u64 = 42;\npub const REAL: f64 = 1.5;\npub const TEXT: &[u8; 2] = b\"x\\0\";\n";
        let parsed = parse_bindgen_constants(source, &requested);
        assert_eq!(
            parsed["NEGATIVE"],
            BindgenClassification::Integer(IntegerValue::Signed(-4))
        );
        assert_eq!(
            parsed["POSITIVE"],
            BindgenClassification::Integer(IntegerValue::Unsigned(42))
        );
        assert_eq!(parsed["REAL"], BindgenClassification::NonInteger);
        assert_eq!(parsed["TEXT"], BindgenClassification::NonInteger);
    }
}
