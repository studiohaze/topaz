use std::collections::BTreeMap;
use std::rc::Rc;

use topaz_value::{JsonNumber, JsonValue, json_parse, write_json_node};

pub(crate) const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

pub(crate) fn string(value: impl Into<String>) -> JsonValue {
    JsonValue::String(Rc::from(value.into()))
}

pub(crate) fn boolean(value: bool) -> JsonValue {
    JsonValue::Bool(value)
}

pub(crate) fn unsigned(value: u64) -> JsonValue {
    assert!(
        value <= MAX_SAFE_INTEGER,
        "canonical integer exceeds JSON bound"
    );
    JsonValue::Number(JsonNumber {
        lexeme: Rc::from(value.to_string()),
        int: i64::try_from(value).ok(),
    })
}

pub(crate) fn signed(value: i64) -> JsonValue {
    assert!(
        value.unsigned_abs() <= MAX_SAFE_INTEGER,
        "canonical integer exceeds JSON bound"
    );
    JsonValue::Number(JsonNumber {
        lexeme: Rc::from(value.to_string()),
        int: Some(value),
    })
}

pub(crate) fn array(values: impl IntoIterator<Item = JsonValue>) -> JsonValue {
    JsonValue::Array(values.into_iter().collect::<Vec<_>>().into())
}

pub(crate) fn object(
    fields: impl IntoIterator<Item = (impl Into<String>, JsonValue)>,
) -> JsonValue {
    let fields = fields
        .into_iter()
        .map(|(key, value)| (Rc::<str>::from(key.into()), value))
        .collect::<BTreeMap<_, _>>();
    JsonValue::Object(Rc::new(fields))
}

pub(crate) fn encode(value: &JsonValue) -> Vec<u8> {
    let mut output = String::new();
    write_json_node(&mut output, value);
    output.push('\n');
    output.into_bytes()
}

pub(crate) fn encode_jsonl<'a>(rows: impl IntoIterator<Item = &'a JsonValue>) -> Vec<u8> {
    let mut output = String::new();
    for row in rows {
        write_json_node(&mut output, row);
        output.push('\n');
    }
    output.into_bytes()
}

pub(crate) fn validate(bytes: &[u8], jsonl: bool) -> Result<Vec<JsonValue>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "canonical JSON is not UTF-8")?;
    if text.starts_with('\u{feff}') {
        return Err("canonical JSON must not start with a BOM".to_string());
    }
    if text.contains('\r') || !text.ends_with('\n') {
        return Err("canonical JSON requires LF lines and one final LF".to_string());
    }
    let lines = if jsonl {
        text.strip_suffix('\n')
            .expect("checked")
            .split('\n')
            .collect::<Vec<_>>()
    } else {
        vec![text.strip_suffix('\n').expect("checked")]
    };
    if lines.iter().any(|line| line.is_empty()) {
        return Err("canonical JSON contains an empty row".to_string());
    }
    let mut values = Vec::with_capacity(lines.len());
    for line in lines {
        let value = json_parse(line).map_err(|error| {
            format!(
                "invalid JSON at line {}, column {}: {}",
                error.line, error.column, error.message
            )
        })?;
        validate_numbers(&value)?;
        let canonical = encode(&value);
        if canonical.as_slice() != format!("{line}\n").as_bytes() {
            return Err("JSON bytes are not canonical".to_string());
        }
        values.push(value);
    }
    Ok(values)
}

fn validate_numbers(value: &JsonValue) -> Result<(), String> {
    match value {
        JsonValue::Number(number) => {
            let bytes = number.lexeme.as_bytes();
            let valid = bytes == b"0"
                || (bytes.first() == Some(&b'-')
                    && bytes.get(1).is_some_and(u8::is_ascii_digit)
                    && bytes.get(1) != Some(&b'0')
                    && bytes[2..].iter().all(u8::is_ascii_digit))
                || (bytes
                    .first()
                    .is_some_and(|byte| byte.is_ascii_digit() && *byte != b'0')
                    && bytes[1..].iter().all(u8::is_ascii_digit));
            if !valid {
                return Err("canonical JSON numbers must be shortest integers".to_string());
            }
        }
        JsonValue::Array(items) => {
            for item in items.iter() {
                validate_numbers(item)?;
            }
        }
        JsonValue::Object(fields) => {
            for item in fields.values() {
                validate_numbers(item)?;
            }
        }
        JsonValue::Null | JsonValue::Bool(_) | JsonValue::String(_) => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_writer_orders_keys_and_preserves_unicode() {
        let bytes = encode(&object([("z", unsigned(2)), ("a", string("Топаз 토파즈"))]));
        assert_eq!(
            std::str::from_utf8(&bytes).expect("UTF-8"),
            "{\"a\":\"Топаз 토파즈\",\"z\":2}\n"
        );
        assert!(validate(&bytes, false).is_ok());
    }

    #[test]
    fn validator_rejects_noncanonical_numbers_and_spacing() {
        assert!(validate(b"{\"n\":1.0}\n", false).is_err());
        assert!(validate(b"{ \"n\":1}\\n", false).is_err());
        assert!(validate(b"{\"n\":01}\n", false).is_err());
    }
}
