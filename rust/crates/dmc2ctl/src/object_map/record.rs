//! Small, versioned UTF-8 metadata records; binary payloads are retained verbatim.
use super::Error;
use std::collections::BTreeMap;
use std::fmt::Write;

pub fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                write!(out, "\\u{:04x}", c as u32).unwrap();
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn encode(schema: &str, fields: &[(&str, &str)], payload: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = format!("{schema}\n");
    for (key, value) in fields {
        if value.is_empty() || value.chars().any(char::is_control) {
            return Err(Error::Input(format!(
                "{key} must be nonempty text without control characters."
            )));
        }
        writeln!(out, "{key}={value}").unwrap();
    }
    out.push('\n');
    let mut out = out.into_bytes();
    out.extend_from_slice(payload);
    Ok(out)
}

pub fn decode<'a>(
    bytes: &'a [u8],
    schema: &str,
    keys: &[&str],
) -> Result<(BTreeMap<String, String>, &'a [u8]), Error> {
    let boundary = bytes
        .windows(2)
        .position(|v| v == b"\n\n")
        .ok_or_else(|| Error::Data("Object record has no complete metadata boundary.".into()))?;
    let header = std::str::from_utf8(&bytes[..boundary])
        .map_err(|e| Error::Data(format!("Object metadata is not UTF-8: {e}")))?;
    let mut lines = header.lines();
    if lines.next() != Some(schema) {
        return Err(Error::Data(format!("Expected {schema} object record.")));
    }
    let mut fields = BTreeMap::new();
    for line in lines {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| Error::Data("Malformed object metadata field.".into()))?;
        if !keys.contains(&key)
            || value.is_empty()
            || value.chars().any(char::is_control)
            || fields.insert(key.into(), value.into()).is_some()
        {
            return Err(Error::Data(format!(
                "Invalid or duplicate object field {key:?}."
            )));
        }
    }
    if fields.len() != keys.len() {
        let missing = keys
            .iter()
            .copied()
            .filter(|key| !fields.contains_key(*key))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(Error::Data(format!(
            "{schema} is missing required fields: {missing}. Restore these fields from the original record or fill the prepared request, then retry."
        )));
    }
    Ok((fields, &bytes[boundary + 2..]))
}
