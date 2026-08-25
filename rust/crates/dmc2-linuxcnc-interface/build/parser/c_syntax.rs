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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum EnumKind {
    Named,
    Typedef,
}

impl EnumKind {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Named => "named",
            Self::Typedef => "typedef",
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct EnumDeclaration {
    pub(crate) kind: EnumKind,
    pub(crate) name: String,
}

fn c_tokens(source: &str) -> Vec<String> {
    let cleaned = strip_comments(source);
    let bytes = cleaned.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'\'' || byte == b'"' {
            let quote = byte;
            index += 1;
            let mut escaped = false;
            while index < bytes.len() {
                let current = bytes[index];
                index += 1;
                if escaped {
                    escaped = false;
                } else if current == b'\\' {
                    escaped = true;
                } else if current == quote {
                    break;
                }
            }
            continue;
        }
        if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            tokens.push(cleaned[start..index].to_owned());
            continue;
        }
        if matches!(byte, b'{' | b'}' | b';') {
            tokens.push((byte as char).to_string());
        }
        index += 1;
    }
    tokens
}

pub(crate) fn enum_declarations(source: &str) -> Vec<EnumDeclaration> {
    let tokens = c_tokens(source);
    let mut declarations = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if tokens[index] != "enum" {
            index += 1;
            continue;
        }
        let is_typedef = index > 0 && tokens[index - 1] == "typedef";
        let mut cursor = index + 1;
        let tag = if tokens.get(cursor).is_some_and(|token| token != "{") {
            let value = tokens[cursor].clone();
            cursor += 1;
            Some(value)
        } else {
            None
        };
        if tokens.get(cursor).map(String::as_str) != Some("{") {
            index += 1;
            continue;
        }
        let mut depth = 1_u32;
        cursor += 1;
        while cursor < tokens.len() && depth > 0 {
            match tokens[cursor].as_str() {
                "{" => depth += 1,
                "}" => depth -= 1,
                _ => {}
            }
            cursor += 1;
        }
        assert_eq!(depth, 0, "unterminated enum declaration in LinuxCNC header");
        let (kind, name) = if is_typedef {
            let alias = tokens
                .get(cursor)
                .filter(|token| token.as_str() != ";")
                .unwrap_or_else(|| panic!("typedef enum has no alias in LinuxCNC header"));
            (EnumKind::Typedef, alias.clone())
        } else {
            (
                EnumKind::Named,
                tag.unwrap_or_else(|| panic!("non-typedef enum has no tag in LinuxCNC header")),
            )
        };
        declarations.push(EnumDeclaration { kind, name });
        index = cursor;
    }
    declarations
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

pub(crate) fn named_enum(source: &str, marker: &str) -> Vec<String> {
    parse_enum_names(&enum_body_after(source, marker))
}

pub(crate) fn typedef_enum(source: &str, alias: &str) -> Vec<String> {
    parse_enum_names(&typedef_enum_body(source, alias))
}

pub(crate) fn macro_names<F>(source: &str, mut accept: F) -> Vec<String>
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

pub(crate) fn integer_macro(source: &str, expected_name: &str) -> usize {
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
