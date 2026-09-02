use super::super::*;

/// Canonical ABI value envelope for host-callable exports.
///
/// This is NOT `JSON.stringify`: it is a lossless transport shape for the Web
/// Target / worker boundary. Integers are decimal strings (never JSON numbers),
/// constructors stay tagged (`some`/`none`/`ok`/`err`), and unsupported values
/// fail closed with a deterministic message instead of falling back to `render`.
pub fn canonical_abi_encode(value: &Value) -> Result<String, String> {
    let mut out = String::new();
    encode_abi_value(value, &mut out, "$", 0)?;
    Ok(out)
}

pub fn canonical_abi_args_encode(args: &[Value]) -> Result<String, String> {
    let mut out = String::from("[");
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        encode_abi_value(arg, &mut out, &format!("$[{i}]"), 0)?;
    }
    out.push(']');
    Ok(out)
}

pub fn canonical_abi_decode(input: &str) -> Result<Value, String> {
    let tree = json_parse(input).map_err(|e| {
        format!(
            "ABI_JSON: invalid JSON at line {}, column {}: {}",
            e.line, e.column, e.message
        )
    })?;
    decode_abi_value(&tree, "$", 0)
}

pub fn canonical_abi_decode_args(input: &str) -> Result<Vec<Value>, String> {
    let tree = json_parse(input).map_err(|e| {
        format!(
            "ABI_JSON: invalid JSON at line {}, column {}: {}",
            e.line, e.column, e.message
        )
    })?;
    let JsonValue::Array(items) = tree else {
        return Err("ABI_ARGS: expected an array of ABI values".to_string());
    };
    items
        .iter()
        .enumerate()
        .map(|(i, item)| decode_abi_value(item, &format!("$[{i}]"), 0))
        .collect()
}

pub fn canonical_abi_completed(value: &Value) -> String {
    let mut out = String::from("{\"status\":\"ok\",\"value\":");
    match encode_abi_value(value, &mut out, "$.value", 0) {
        Ok(()) => out.push('}'),
        Err(e) => return canonical_abi_error(&e),
    }
    out
}

pub fn canonical_abi_faulted(error: &RtError) -> String {
    let mut out = String::from("{\"status\":\"fault\",\"code\":");
    push_json_escaped(&mut out, error.code);
    out.push_str(",\"message\":");
    push_json_escaped(&mut out, &error.message);
    out.push_str(",\"span\":{\"file\":");
    out.push_str(&error.span.file.0.to_string());
    out.push_str(",\"lo\":");
    out.push_str(&error.span.lo.to_string());
    out.push_str(",\"hi\":");
    out.push_str(&error.span.hi.to_string());
    out.push_str("}}");
    out
}

pub fn canonical_abi_error(message: &str) -> String {
    let mut out = String::from("{\"status\":\"error\",\"message\":");
    push_json_escaped(&mut out, message);
    out.push('}');
    out
}

pub(in crate::value) fn encode_abi_value(
    value: &Value,
    out: &mut String,
    path: &str,
    depth: u32,
) -> Result<(), String> {
    if depth > JSON_MAX_DEPTH {
        return Err(format!(
            "ABI_LIMIT: {path}: structure exceeds the ABI value depth limit"
        ));
    }
    match value {
        Value::Int(n) => {
            out.push_str("{\"$\":\"int\",\"value\":");
            push_json_escaped(out, &n.to_string());
            out.push('}');
        }
        Value::Bool(b) => {
            out.push_str("{\"$\":\"bool\",\"value\":");
            out.push_str(if *b { "true" } else { "false" });
            out.push('}');
        }
        Value::Str(s) => {
            out.push_str("{\"$\":\"string\",\"value\":");
            push_json_escaped(out, s);
            out.push('}');
        }
        Value::Unit => out.push_str("{\"$\":\"unit\"}"),
        Value::Null => out.push_str("{\"$\":\"null\"}"),
        Value::None => out.push_str("{\"$\":\"none\"}"),
        Value::Some(inner) => {
            out.push_str("{\"$\":\"some\",\"value\":");
            encode_abi_value(inner, out, &format!("{path}.value"), depth + 1)?;
            out.push('}');
        }
        Value::Ok(inner) => {
            out.push_str("{\"$\":\"ok\",\"value\":");
            encode_abi_value(inner, out, &format!("{path}.value"), depth + 1)?;
            out.push('}');
        }
        Value::Err(inner) => {
            out.push_str("{\"$\":\"err\",\"value\":");
            encode_abi_value(inner, out, &format!("{path}.value"), depth + 1)?;
            out.push('}');
        }
        Value::Array(items) => {
            let snapshot = items.borrow().clone();
            out.push_str("{\"$\":\"array\",\"items\":[");
            for (i, item) in snapshot.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                encode_abi_value(item, out, &format!("{path}.items[{i}]"), depth + 1)?;
            }
            out.push_str("]}");
        }
        Value::Record(fields) => {
            out.push_str("{\"$\":\"record\",\"fields\":{");
            for (i, (name, field)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                push_json_escaped(out, name);
                out.push(':');
                encode_abi_value(field, out, &format!("{path}.fields.{name}"), depth + 1)?;
            }
            out.push_str("}}");
        }
        Value::NominalRecord {
            record_id, fields, ..
        } => {
            out.push_str("{\"$\":\"nominal-record\",\"id\":");
            push_json_escaped(out, record_id);
            out.push_str(",\"fields\":[");
            for (i, (name, field)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str("{\"name\":");
                push_json_escaped(out, name);
                out.push_str(",\"value\":");
                encode_abi_value(field, out, &format!("{path}.fields[{i}].value"), depth + 1)?;
                out.push('}');
            }
            out.push_str("]}");
        }
        Value::Enum {
            enum_id,
            variant,
            variant_index,
            payloads,
            ..
        } => {
            out.push_str("{\"$\":\"enum\",\"id\":");
            push_json_escaped(out, enum_id);
            out.push_str(",\"variant\":");
            push_json_escaped(out, variant);
            out.push_str(",\"index\":");
            push_json_escaped(out, &variant_index.to_string());
            out.push_str(",\"payloads\":[");
            for (i, payload) in payloads.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                encode_abi_value(payload, out, &format!("{path}.payloads[{i}]"), depth + 1)?;
            }
            out.push_str("]}");
        }
        Value::Newtype {
            newtype_id, inner, ..
        } => {
            out.push_str("{\"$\":\"newtype\",\"id\":");
            push_json_escaped(out, newtype_id);
            out.push_str(",\"value\":");
            encode_abi_value(inner, out, &format!("{path}.value"), depth + 1)?;
            out.push('}');
        }
        Value::Json(node) => {
            out.push_str("{\"$\":\"json\",\"value\":");
            write_json_node(out, node);
            out.push('}');
        }
        Value::Bytes(bytes) => {
            out.push_str("{\"$\":\"bytes\",\"hex\":");
            let mut hex = String::new();
            bytes_to_hex_into(&mut hex, bytes);
            push_json_escaped(out, &hex);
            out.push('}');
        }
        other => {
            return Err(format!(
                "ABI_UNSUPPORTED: {path}: `{}` is not ABI-encodable",
                other.kind()
            ));
        }
    }
    Ok(())
}

pub(in crate::value) fn decode_abi_value(
    node: &JsonValue,
    path: &str,
    depth: u32,
) -> Result<Value, String> {
    if depth > JSON_MAX_DEPTH {
        return Err(format!(
            "ABI_LIMIT: {path}: structure exceeds the ABI value depth limit"
        ));
    }
    let obj = abi_object(node, path)?;
    let tag = abi_string_field(obj, "$", path)?;
    match tag.as_ref() {
        "int" => {
            abi_exact_fields(obj, &["$", "value"], path)?;
            let raw = abi_string_field(obj, "value", path)?;
            let n = raw
                .parse::<i64>()
                .map_err(|_| format!("ABI_INT: {path}.value is not an i64 decimal string"))?;
            if n.to_string() != raw.as_ref() {
                return Err(format!("ABI_INT: {path}.value is not canonical decimal"));
            }
            Ok(Value::Int(n))
        }
        "bool" => {
            abi_exact_fields(obj, &["$", "value"], path)?;
            let JsonValue::Bool(b) = abi_field(obj, "value", path)? else {
                return Err(format!("ABI_BOOL: {path}.value must be boolean"));
            };
            Ok(Value::Bool(*b))
        }
        "string" => {
            abi_exact_fields(obj, &["$", "value"], path)?;
            Ok(Value::Str(abi_string_field(obj, "value", path)?))
        }
        "unit" => {
            abi_exact_fields(obj, &["$"], path)?;
            Ok(Value::Unit)
        }
        "null" => {
            abi_exact_fields(obj, &["$"], path)?;
            Ok(Value::Null)
        }
        "none" => {
            abi_exact_fields(obj, &["$"], path)?;
            Ok(Value::None)
        }
        "some" => {
            abi_exact_fields(obj, &["$", "value"], path)?;
            Ok(Value::Some(Rc::new(decode_abi_value(
                abi_field(obj, "value", path)?,
                &format!("{path}.value"),
                depth + 1,
            )?)))
        }
        "ok" => {
            abi_exact_fields(obj, &["$", "value"], path)?;
            Ok(Value::Ok(Rc::new(decode_abi_value(
                abi_field(obj, "value", path)?,
                &format!("{path}.value"),
                depth + 1,
            )?)))
        }
        "err" => {
            abi_exact_fields(obj, &["$", "value"], path)?;
            Ok(Value::Err(Rc::new(decode_abi_value(
                abi_field(obj, "value", path)?,
                &format!("{path}.value"),
                depth + 1,
            )?)))
        }
        "array" => {
            abi_exact_fields(obj, &["$", "items"], path)?;
            let JsonValue::Array(items) = abi_field(obj, "items", path)? else {
                return Err(format!("ABI_ARRAY: {path}.items must be an array"));
            };
            let mut out = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                out.push(decode_abi_value(
                    item,
                    &format!("{path}.items[{i}]"),
                    depth + 1,
                )?);
            }
            Ok(Value::array(out))
        }
        "record" => {
            abi_exact_fields(obj, &["$", "fields"], path)?;
            let JsonValue::Object(fields) = abi_field(obj, "fields", path)? else {
                return Err(format!("ABI_RECORD: {path}.fields must be an object"));
            };
            let mut out = BTreeMap::new();
            for (name, field) in fields.iter() {
                out.insert(
                    name.to_string(),
                    decode_abi_value(field, &format!("{path}.fields.{name}"), depth + 1)?,
                );
            }
            Ok(Value::Record(Rc::new(out)))
        }
        "nominal-record" => {
            abi_exact_fields(obj, &["$", "fields", "id"], path)?;
            let record_id = abi_string_field(obj, "id", path)?;
            let JsonValue::Array(fields) = abi_field(obj, "fields", path)? else {
                return Err(format!(
                    "ABI_NOMINAL_RECORD: {path}.fields must be an array"
                ));
            };
            let mut seen = std::collections::BTreeSet::new();
            let mut out = Vec::with_capacity(fields.len());
            for (i, field) in fields.iter().enumerate() {
                let field_path = format!("{path}.fields[{i}]");
                let field_obj = abi_object(field, &field_path)?;
                abi_exact_fields(field_obj, &["name", "value"], &field_path)?;
                let name = abi_string_field(field_obj, "name", &field_path)?;
                if !seen.insert(name.clone()) {
                    return Err(format!(
                        "ABI_NOMINAL_RECORD: {field_path}.name duplicates `{name}`"
                    ));
                }
                let value = decode_abi_value(
                    abi_field(field_obj, "value", &field_path)?,
                    &format!("{field_path}.value"),
                    depth + 1,
                )?;
                out.push((name, value));
            }
            Ok(Value::NominalRecord {
                record_id,
                declaration_identity: None,
                method_identity: None,
                fields: Rc::from(out.into_boxed_slice()),
            })
        }
        "enum" => {
            abi_exact_fields(obj, &["$", "id", "index", "payloads", "variant"], path)?;
            let enum_id = abi_string_field(obj, "id", path)?;
            let variant = abi_string_field(obj, "variant", path)?;
            let index = abi_string_field(obj, "index", path)?;
            let variant_index = parse_abi_u32_string(&index, &format!("{path}.index"))?;
            let JsonValue::Array(payloads) = abi_field(obj, "payloads", path)? else {
                return Err(format!("ABI_ENUM: {path}.payloads must be an array"));
            };
            let mut out = Vec::with_capacity(payloads.len());
            for (i, payload) in payloads.iter().enumerate() {
                out.push(decode_abi_value(
                    payload,
                    &format!("{path}.payloads[{i}]"),
                    depth + 1,
                )?);
            }
            Ok(Value::Enum {
                enum_id,
                declaration_identity: None,
                method_identity: None,
                variant,
                variant_index,
                payloads: Rc::from(out.into_boxed_slice()),
            })
        }
        "newtype" => {
            abi_exact_fields(obj, &["$", "id", "value"], path)?;
            let newtype_id = abi_string_field(obj, "id", path)?;
            Ok(Value::Newtype {
                newtype_id,
                declaration_identity: None,
                method_identity: None,
                inner: Rc::new(decode_abi_value(
                    abi_field(obj, "value", path)?,
                    &format!("{path}.value"),
                    depth + 1,
                )?),
            })
        }
        "json" => {
            abi_exact_fields(obj, &["$", "value"], path)?;
            Ok(Value::Json(Rc::new(abi_field(obj, "value", path)?.clone())))
        }
        "bytes" => {
            abi_exact_fields(obj, &["$", "hex"], path)?;
            let raw = abi_string_field(obj, "hex", path)?;
            Ok(Value::Bytes(Rc::from(
                decode_lower_hex(&raw, &format!("{path}.hex"))?.into_boxed_slice(),
            )))
        }
        other => Err(format!("ABI_TAG: {path} has unsupported tag `{other}`")),
    }
}

pub(in crate::value) fn abi_object<'a>(
    node: &'a JsonValue,
    path: &str,
) -> Result<&'a BTreeMap<Rc<str>, JsonValue>, String> {
    match node {
        JsonValue::Object(obj) => Ok(obj),
        other => Err(format!(
            "ABI_SHAPE: {path} must be an object, found `{}`",
            json_kind_name(other)
        )),
    }
}

pub(in crate::value) fn abi_field<'a>(
    obj: &'a BTreeMap<Rc<str>, JsonValue>,
    key: &str,
    path: &str,
) -> Result<&'a JsonValue, String> {
    obj.get(key)
        .ok_or_else(|| format!("ABI_SHAPE: {path} missing `{key}`"))
}

pub(in crate::value) fn abi_string_field(
    obj: &BTreeMap<Rc<str>, JsonValue>,
    key: &str,
    path: &str,
) -> Result<Rc<str>, String> {
    match abi_field(obj, key, path)? {
        JsonValue::String(s) => Ok(s.clone()),
        other => Err(format!(
            "ABI_SHAPE: {path}.{key} must be a string, found `{}`",
            json_kind_name(other)
        )),
    }
}

pub(in crate::value) fn abi_exact_fields(
    obj: &BTreeMap<Rc<str>, JsonValue>,
    allowed: &[&str],
    path: &str,
) -> Result<(), String> {
    if obj.len() != allowed.len() || allowed.iter().any(|k| !obj.contains_key(*k)) {
        return Err(format!(
            "ABI_SHAPE: {path} must contain exactly `{}`",
            allowed.join("`, `")
        ));
    }
    Ok(())
}

pub(in crate::value) fn decode_lower_hex(raw: &str, path: &str) -> Result<Vec<u8>, String> {
    let bytes = raw.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return Err(format!("ABI_BYTES: {path} has odd length"));
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let hi = lower_hex_nibble(pair[0])
            .ok_or_else(|| format!("ABI_BYTES: {path} contains a non-lowercase-hex digit"))?;
        let lo = lower_hex_nibble(pair[1])
            .ok_or_else(|| format!("ABI_BYTES: {path} contains a non-lowercase-hex digit"))?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

pub(in crate::value) fn parse_abi_u32_string(raw: &str, path: &str) -> Result<u32, String> {
    let n = raw
        .parse::<u32>()
        .map_err(|_| format!("ABI_U32: {path} is not a u32 decimal string"))?;
    if n.to_string() != raw {
        return Err(format!("ABI_U32: {path} is not canonical decimal"));
    }
    Ok(n)
}

pub(in crate::value) fn lower_hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(10 + b - b'a'),
        _ => None,
    }
}
