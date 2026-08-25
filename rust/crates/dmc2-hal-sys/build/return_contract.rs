//! Exact return-expression audit for every numeric HAL function we bind.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::PathBuf;

use super::config::{SOURCE_HAL_LIBRARY_RELATIVE, SOURCE_ROOT_RELATIVE};

struct Contract {
    function: &'static str,
    returns: &'static [&'static str],
}

const CONTRACTS: &[Contract] = &[
    Contract {
        function: "hal_init",
        returns: &["-EINVAL", "-ENOMEM", "comp_id"],
    },
    Contract {
        function: "hal_exit",
        returns: &["-EINVAL", "0"],
    },
    Contract {
        function: "hal_ready",
        returns: &["-EINVAL", "0"],
    },
    Contract {
        function: "hal_pin_new",
        returns: &["-EINVAL", "-ENOMEM", "-EPERM", "0"],
    },
    Contract {
        function: "hal_param_new",
        returns: &["-EINVAL", "-ENOMEM", "-EPERM", "0"],
    },
    Contract {
        function: "hal_export_funct",
        returns: &["-EINVAL", "-ENOMEM", "-EPERM", "0"],
    },
    Contract {
        function: "hal_pin_bit_new",
        returns: &["hal_pin_new(name,HAL_BIT,dir,(void**)data_ptr_addr,comp_id)"],
    },
    Contract {
        function: "hal_pin_float_new",
        returns: &["hal_pin_new(name,HAL_FLOAT,dir,(void**)data_ptr_addr,comp_id)"],
    },
    Contract {
        function: "hal_pin_s32_new",
        returns: &["hal_pin_new(name,HAL_S32,dir,(void**)data_ptr_addr,comp_id)"],
    },
    Contract {
        function: "hal_pin_u32_new",
        returns: &["hal_pin_new(name,HAL_U32,dir,(void**)data_ptr_addr,comp_id)"],
    },
    Contract {
        function: "hal_param_float_new",
        returns: &["hal_param_new(name,HAL_FLOAT,dir,(void*)data_addr,comp_id)"],
    },
    Contract {
        function: "hal_param_u32_new",
        returns: &["hal_param_new(name,HAL_U32,dir,(void*)data_addr,comp_id)"],
    },
];

fn mask_non_code(source: &str) -> String {
    #[derive(Clone, Copy)]
    enum State {
        Code,
        LineComment,
        BlockComment,
        String,
        Character,
    }

    let bytes = source.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut state = State::Code;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        let next = bytes.get(index + 1).copied();
        match state {
            State::Code if byte == b'/' && next == Some(b'/') => {
                output.extend_from_slice(b"  ");
                state = State::LineComment;
                index += 2;
            }
            State::Code if byte == b'/' && next == Some(b'*') => {
                output.extend_from_slice(b"  ");
                state = State::BlockComment;
                index += 2;
            }
            State::Code if byte == b'\"' => {
                output.push(b' ');
                state = State::String;
                index += 1;
            }
            State::Code if byte == b'\'' => {
                output.push(b' ');
                state = State::Character;
                index += 1;
            }
            State::Code => {
                output.push(byte);
                index += 1;
            }
            State::LineComment if byte == b'\n' => {
                output.push(byte);
                state = State::Code;
                index += 1;
            }
            State::LineComment => {
                output.push(b' ');
                index += 1;
            }
            State::BlockComment if byte == b'*' && next == Some(b'/') => {
                output.extend_from_slice(b"  ");
                state = State::Code;
                index += 2;
            }
            State::BlockComment => {
                output.push(if byte == b'\n' { b'\n' } else { b' ' });
                index += 1;
            }
            State::String | State::Character if byte == b'\\' && next.is_some() => {
                output.extend_from_slice(b"  ");
                index += 2;
            }
            State::String if byte == b'\"' => {
                output.push(b' ');
                state = State::Code;
                index += 1;
            }
            State::Character if byte == b'\'' => {
                output.push(b' ');
                state = State::Code;
                index += 1;
            }
            State::String | State::Character => {
                output.push(if byte == b'\n' { b'\n' } else { b' ' });
                index += 1;
            }
        }
    }
    String::from_utf8(output).expect("masked C source remained UTF-8")
}

fn function_body<'a>(source: &'a str, function: &str) -> &'a str {
    let signature = format!("int {function}(");
    let signature_start = source
        .find(&signature)
        .unwrap_or_else(|| panic!("LinuxCNC source omitted {function}"));
    let open = source[signature_start..]
        .find('{')
        .map(|offset| signature_start + offset)
        .unwrap_or_else(|| panic!("LinuxCNC function {function} omitted its body"));
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    for (offset, byte) in bytes[open..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth
                    .checked_sub(1)
                    .unwrap_or_else(|| panic!("unbalanced body for {function}"));
                if depth == 0 {
                    return &source[open + 1..open + offset];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated body for {function}")
}

fn is_identifier(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn return_expressions(body: &str) -> BTreeSet<String> {
    let bytes = body.as_bytes();
    let mut expressions = BTreeSet::new();
    let mut index = 0;
    while index + 6 <= bytes.len() {
        if &bytes[index..index + 6] != b"return"
            || (index > 0 && is_identifier(bytes[index - 1]))
            || (index + 6 < bytes.len() && is_identifier(bytes[index + 6]))
        {
            index += 1;
            continue;
        }
        let expression_start = index + 6;
        let end = bytes[expression_start..]
            .iter()
            .position(|byte| *byte == b';')
            .map(|offset| expression_start + offset)
            .expect("return expression omitted its semicolon");
        let expression = body[expression_start..end]
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>();
        assert!(
            !expression.is_empty(),
            "void return is outside the HAL contract"
        );
        expressions.insert(expression);
        index = end + 1;
    }
    expressions
}

pub(crate) fn verify_audited_returns() {
    let manifest =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let source = manifest
        .join(SOURCE_ROOT_RELATIVE)
        .join(SOURCE_HAL_LIBRARY_RELATIVE);
    let masked = mask_non_code(
        &fs::read_to_string(&source)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", source.display())),
    );
    for contract in CONTRACTS {
        let actual = return_expressions(function_body(&masked, contract.function));
        let expected = contract
            .returns
            .iter()
            .map(|expression| (*expression).to_owned())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            actual, expected,
            "LinuxCNC 2.9.10 return contract changed for {}",
            contract.function
        );
    }
}
