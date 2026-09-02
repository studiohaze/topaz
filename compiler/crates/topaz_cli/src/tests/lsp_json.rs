use super::*;

#[test]
fn json_string_decoder_handles_surrogate_pairs() {
    let raw = r#""\ud83d\ude00x""#;
    let (decoded, end) = parse_json_string(raw, 0).expect("valid JSON string");
    assert_eq!(decoded, "😀x");
    assert_eq!(end, raw.len());
}
