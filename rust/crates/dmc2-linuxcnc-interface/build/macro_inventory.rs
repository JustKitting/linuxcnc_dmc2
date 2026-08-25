use std::collections::BTreeMap;
use std::path::Path;

use super::config::{
    EXPECTED_PUBLIC_HEADER_COUNT, EXPECTED_PUBLIC_HEADER_SOURCE_BYTE_COUNT,
    EXPECTED_PUBLIC_HEADER_SOURCE_FNV64, EXPECTED_PUBLIC_MACRO_DECLARATION_COUNT,
    EXPECTED_PUBLIC_MACRO_EMPTY_OBJECT_COUNT, EXPECTED_PUBLIC_MACRO_FUNCTION_COUNT,
    EXPECTED_PUBLIC_MACRO_INACTIVE_COUNT, EXPECTED_PUBLIC_MACRO_NAME_COUNT,
    EXPECTED_PUBLIC_MACRO_NON_INTEGER_COUNT, EXPECTED_PUBLIC_MACRO_SIGNED_INTEGER_COUNT,
    EXPECTED_PUBLIC_MACRO_UNSIGNED_INTEGER_COUNT,
};
use super::macro_probe::{self, BindgenClassification, IntegerValue};
use super::parser::{macro_definitions, MacroForm};
use super::source::PublicHeader;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum MacroClassification {
    Inactive,
    FunctionLike,
    ObjectWithoutValue,
    SignedInteger(i128),
    UnsignedInteger(u128),
    ObjectNotIntegerConstant,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MacroContract {
    pub(crate) header_name: String,
    pub(crate) name: String,
    pub(crate) declaration_count: usize,
    pub(crate) object_declaration_count: usize,
    pub(crate) function_declaration_count: usize,
    pub(crate) active_replacement: Option<String>,
    pub(crate) classification: MacroClassification,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HeaderContract {
    pub(crate) header_name: String,
    pub(crate) source_relative_path: String,
    pub(crate) source_byte_count: usize,
    pub(crate) source_fnv64: u64,
    pub(crate) macro_declaration_count: usize,
    pub(crate) macro_name_count: usize,
}

pub(crate) struct Inventory {
    pub(crate) headers: Vec<HeaderContract>,
    pub(crate) macros: Vec<MacroContract>,
    pub(crate) source_fnv64: u64,
}

fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn source_fingerprint(headers: &[PublicHeader], source_root: &Path) -> u64 {
    headers.iter().fold(0xcbf29ce484222325, |hash, header| {
        let relative = header
            .source_path
            .strip_prefix(source_root)
            .unwrap_or_else(|error| {
                panic!(
                    "public header source {} is outside {}: {error}",
                    header.source_path.display(),
                    source_root.display()
                )
            });
        let hash = fnv1a(hash, header.name.as_bytes());
        let hash = fnv1a(hash, &[0]);
        let hash = fnv1a(hash, relative.as_os_str().as_encoded_bytes());
        let hash = fnv1a(hash, &[0]);
        fnv1a(hash, header.source.as_bytes())
    })
}

fn classify_integer(value: IntegerValue) -> MacroClassification {
    match value {
        IntegerValue::Signed(value) => MacroClassification::SignedInteger(value),
        IntegerValue::Unsigned(value) => MacroClassification::UnsignedInteger(value),
    }
}

pub(crate) fn collect(
    headers: &[PublicHeader],
    source_root: &Path,
    output_directory: &Path,
) -> Inventory {
    macro_probe::verify_tool();
    let mut header_contracts = Vec::with_capacity(headers.len());
    let mut macro_contracts = Vec::new();
    let mut fallback_sequence = 0;

    for header in headers {
        let definitions = macro_definitions(&header.source);
        let mut declarations = BTreeMap::<String, Vec<_>>::new();
        for definition in definitions {
            declarations
                .entry(definition.name.clone())
                .or_default()
                .push(definition);
        }
        let active = macro_probe::active_definitions(header, source_root);
        let object_value_names = declarations
            .keys()
            .filter(|name| {
                active.get(*name).is_some_and(|definition| {
                    definition.form == MacroForm::Object && !definition.replacement.is_empty()
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        let bindgen =
            macro_probe::bindgen_classifications(header, source_root, &object_value_names);

        let macro_declaration_count = declarations.values().map(Vec::len).sum();
        let macro_name_count = declarations.len();
        let relative = header
            .source_path
            .strip_prefix(source_root)
            .unwrap_or_else(|error| {
                panic!(
                    "public header source {} is outside {}: {error}",
                    header.source_path.display(),
                    source_root.display()
                )
            });
        header_contracts.push(HeaderContract {
            header_name: header.name.clone(),
            source_relative_path: relative
                .to_str()
                .expect("public header source path is not UTF-8")
                .to_owned(),
            source_byte_count: header.source.len(),
            source_fnv64: fnv1a(0xcbf29ce484222325, header.source.as_bytes()),
            macro_declaration_count,
            macro_name_count,
        });

        for (name, raw_declarations) in declarations {
            let object_declaration_count = raw_declarations
                .iter()
                .filter(|definition| definition.form == MacroForm::Object)
                .count();
            let function_declaration_count = raw_declarations.len() - object_declaration_count;
            let (active_replacement, classification) = match active.get(&name) {
                None => (None, MacroClassification::Inactive),
                Some(definition) if definition.form == MacroForm::Function => (
                    Some(definition.replacement.clone()),
                    MacroClassification::FunctionLike,
                ),
                Some(definition) if definition.replacement.is_empty() => {
                    (Some(String::new()), MacroClassification::ObjectWithoutValue)
                }
                Some(definition) => {
                    let classification = match bindgen.get(&name) {
                        Some(BindgenClassification::Integer(value)) => classify_integer(*value),
                        Some(BindgenClassification::NonInteger) => {
                            MacroClassification::ObjectNotIntegerConstant
                        }
                        None => {
                            fallback_sequence += 1;
                            macro_probe::integral_fallback(
                                header,
                                source_root,
                                output_directory,
                                fallback_sequence,
                                &name,
                            )
                            .map(classify_integer)
                            .unwrap_or(MacroClassification::ObjectNotIntegerConstant)
                        }
                    };
                    (Some(definition.replacement.clone()), classification)
                }
            };
            macro_contracts.push(MacroContract {
                header_name: header.name.clone(),
                name,
                declaration_count: raw_declarations.len(),
                object_declaration_count,
                function_declaration_count,
                active_replacement,
                classification,
            });
        }
    }

    let parsed_declarations = macro_contracts
        .iter()
        .map(|contract| contract.declaration_count)
        .sum::<usize>();
    let header_declarations = header_contracts
        .iter()
        .map(|contract| contract.macro_declaration_count)
        .sum::<usize>();
    assert_eq!(
        parsed_declarations, header_declarations,
        "one or more public macro declarations were not assigned a contract"
    );
    assert_eq!(
        macro_contracts.len(),
        header_contracts
            .iter()
            .map(|contract| contract.macro_name_count)
            .sum::<usize>(),
        "one or more public macro names were not assigned a contract"
    );

    let source_fnv64 = source_fingerprint(headers, source_root);
    let source_byte_count = header_contracts
        .iter()
        .map(|contract| contract.source_byte_count)
        .sum::<usize>();
    let mut classification_counts = [0_usize; 6];
    for contract in &macro_contracts {
        assert!(contract.declaration_count > 0);
        assert_eq!(
            contract.declaration_count,
            contract.object_declaration_count + contract.function_declaration_count,
            "raw macro form counts do not add up for {}::{}",
            contract.header_name,
            contract.name
        );
        let index = match contract.classification {
            MacroClassification::Inactive => {
                assert!(contract.active_replacement.is_none());
                0
            }
            MacroClassification::FunctionLike => {
                assert!(contract.active_replacement.is_some());
                1
            }
            MacroClassification::ObjectWithoutValue => {
                assert_eq!(contract.active_replacement.as_deref(), Some(""));
                2
            }
            MacroClassification::SignedInteger(_) => {
                assert!(contract
                    .active_replacement
                    .as_ref()
                    .is_some_and(|value| !value.is_empty()));
                3
            }
            MacroClassification::UnsignedInteger(_) => {
                assert!(contract
                    .active_replacement
                    .as_ref()
                    .is_some_and(|value| !value.is_empty()));
                4
            }
            MacroClassification::ObjectNotIntegerConstant => {
                assert!(contract
                    .active_replacement
                    .as_ref()
                    .is_some_and(|value| !value.is_empty()));
                5
            }
        };
        classification_counts[index] += 1;
    }
    assert_eq!(headers.len(), EXPECTED_PUBLIC_HEADER_COUNT);
    assert_eq!(source_fnv64, EXPECTED_PUBLIC_HEADER_SOURCE_FNV64);
    assert_eq!(source_byte_count, EXPECTED_PUBLIC_HEADER_SOURCE_BYTE_COUNT);
    assert_eq!(parsed_declarations, EXPECTED_PUBLIC_MACRO_DECLARATION_COUNT);
    assert_eq!(macro_contracts.len(), EXPECTED_PUBLIC_MACRO_NAME_COUNT);
    assert_eq!(
        classification_counts,
        [
            EXPECTED_PUBLIC_MACRO_INACTIVE_COUNT,
            EXPECTED_PUBLIC_MACRO_FUNCTION_COUNT,
            EXPECTED_PUBLIC_MACRO_EMPTY_OBJECT_COUNT,
            EXPECTED_PUBLIC_MACRO_SIGNED_INTEGER_COUNT,
            EXPECTED_PUBLIC_MACRO_UNSIGNED_INTEGER_COUNT,
            EXPECTED_PUBLIC_MACRO_NON_INTEGER_COUNT,
        ],
        "the compiler-backed LinuxCNC public macro classification changed"
    );

    Inventory {
        headers: header_contracts,
        macros: macro_contracts,
        source_fnv64,
    }
}
