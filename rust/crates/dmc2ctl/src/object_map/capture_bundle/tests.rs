use super::*;
#[test]
fn bundle_preserves_absent_empty_and_non_utf8_companions_without_repair() {
    let id = Id::parse("fixture").unwrap();
    let context = Context {
        parts: [Some(Vec::new()), Some(vec![0, 255, 10]), None],
    };
    let raw = b"original\nledger\n";
    let bytes = encode(&id, "/synthetic/source", raw, &context).unwrap();
    let decoded = decode(&bytes, &id).unwrap();
    assert_eq!(decoded.raw, raw);
    assert_eq!(decoded.context.parts, context.parts);
    assert!(decoded.context.text(Kind::Feeds).is_err());
    assert!(decoded.context.text(Kind::Outline).is_err());
    assert!(decode(&bytes[..bytes.len() - 1], &id).is_err());
    let mut excess = bytes;
    excess.push(1);
    assert!(decode(&excess, &id).is_err());
}
#[test]
fn legacy_snapshot_has_explicitly_missing_context_even_if_source_still_exists() {
    let id = Id::parse("fixture").unwrap();
    let bytes = record::encode(
        V1,
        &[("id", id.as_str()), ("source_path", "/synthetic/source")],
        b"original\n",
    )
    .unwrap();
    let decoded = decode(&bytes, &id).unwrap();
    assert_eq!(decoded.raw, b"original\n");
    assert!(decoded.context.parts.iter().all(Option::is_none));
    assert!(decode(&bytes, &Id::parse("other").unwrap()).is_err());
}
