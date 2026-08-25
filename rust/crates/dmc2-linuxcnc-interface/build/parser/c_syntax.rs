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
