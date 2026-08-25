use super::preprocessor::logical_preprocessor_lines;

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

pub(crate) fn interpreter_error_templates(source: &str) -> Vec<(String, String)> {
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
