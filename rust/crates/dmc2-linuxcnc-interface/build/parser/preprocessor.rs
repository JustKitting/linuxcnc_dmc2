use std::borrow::ToOwned;
use std::string::String;
use std::vec::Vec;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum MacroForm {
    Object,
    Function,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct MacroDefinition {
    pub(crate) name: String,
    pub(crate) replacement: String,
    pub(crate) form: MacroForm,
}

pub(crate) fn logical_preprocessor_lines(source: &str) -> Vec<String> {
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

fn without_comments(line: &str, in_block_comment: &mut bool) -> String {
    let mut result = String::with_capacity(line.len());
    let mut characters = line.chars().peekable();
    let mut quote = None;
    while let Some(character) = characters.next() {
        if *in_block_comment {
            if character == '*' && characters.peek() == Some(&'/') {
                characters.next();
                *in_block_comment = false;
            }
            continue;
        }
        if let Some(delimiter) = quote {
            result.push(character);
            if character == '\\' {
                if let Some(escaped) = characters.next() {
                    result.push(escaped);
                }
            } else if character == delimiter {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' => {
                quote = Some(character);
                result.push(character);
            }
            '/' if characters.peek() == Some(&'/') => break,
            '/' if characters.peek() == Some(&'*') => {
                characters.next();
                *in_block_comment = true;
                result.push(' ');
            }
            _ => result.push(character),
        }
    }
    result
}

fn identifier_end(text: &str) -> usize {
    text.char_indices()
        .take_while(|(_, character)| character.is_ascii_alphanumeric() || *character == '_')
        .map(|(index, character)| index + character.len_utf8())
        .last()
        .unwrap_or(0)
}

fn function_replacement(remainder: &str) -> &str {
    let mut depth = 0_u32;
    for (index, character) in remainder.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                assert!(depth > 0, "unbalanced function-like macro parameters");
                depth -= 1;
                if depth == 0 {
                    return remainder[index + 1..].trim();
                }
            }
            _ => {}
        }
    }
    panic!("unterminated function-like macro parameters");
}

pub(crate) fn macro_definitions(source: &str) -> Vec<MacroDefinition> {
    let mut definitions = Vec::new();
    let mut in_block_comment = false;
    for raw_line in logical_preprocessor_lines(source) {
        let line = without_comments(&raw_line, &mut in_block_comment);
        let trimmed = line.trim_start();
        let Some(after_hash) = trimmed.strip_prefix('#') else {
            continue;
        };
        let after_hash = after_hash.trim_start();
        let Some(after_define) = after_hash.strip_prefix("define") else {
            continue;
        };
        if after_define
            .chars()
            .next()
            .is_some_and(|character| !character.is_ascii_whitespace())
        {
            continue;
        }
        let declaration = after_define.trim_start();
        let end = identifier_end(declaration);
        assert!(
            end > 0
                && declaration
                    .as_bytes()
                    .first()
                    .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_'),
            "#define omitted a C identifier: {line:?}"
        );
        let name = declaration[..end].to_owned();
        let remainder = &declaration[end..];
        let (form, replacement) = if remainder.starts_with('(') {
            (MacroForm::Function, function_replacement(remainder))
        } else {
            (MacroForm::Object, remainder.trim())
        };
        definitions.push(MacroDefinition {
            name,
            replacement: replacement.to_owned(),
            form,
        });
    }
    assert!(!in_block_comment, "unterminated block comment");
    definitions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_macro_form_comment_and_continuation_is_preserved() {
        let source = concat!(
            "/*\n#define DOCUMENTATION_ONLY 1\n*/\n",
            "#define EMPTY\n",
            "#define VALUE 7 /* explanation */\n",
            "#define CALL(x, y) ((x) + \\\n+ (y))\n",
            "# define ALT 9 // explanation\n",
            "#define URL \"https://linuxcnc.org/*literal*/\"\n",
            "#defineD NOT_A_DIRECTIVE\n",
        );
        assert_eq!(
            macro_definitions(source),
            std::vec![
                MacroDefinition {
                    name: "EMPTY".to_owned(),
                    replacement: String::new(),
                    form: MacroForm::Object,
                },
                MacroDefinition {
                    name: "VALUE".to_owned(),
                    replacement: "7".to_owned(),
                    form: MacroForm::Object,
                },
                MacroDefinition {
                    name: "CALL".to_owned(),
                    replacement: "((x) +  + (y))".to_owned(),
                    form: MacroForm::Function,
                },
                MacroDefinition {
                    name: "ALT".to_owned(),
                    replacement: "9".to_owned(),
                    form: MacroForm::Object,
                },
                MacroDefinition {
                    name: "URL".to_owned(),
                    replacement: "\"https://linuxcnc.org/*literal*/\"".to_owned(),
                    form: MacroForm::Object,
                },
            ]
        );
    }
}
