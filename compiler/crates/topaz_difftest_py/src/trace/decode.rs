use crate::*;

use super::json::*;
use super::model::*;

pub(crate) fn parse_trace_v1(input: &str) -> Result<PyTrace, String> {
    let root = JsonParser::new(input).parse()?;
    let obj = object(&root, "trace")?;
    reject_unknown_keys(
        obj,
        &[
            "v",
            "status",
            "stdout",
            "files",
            "defer_errors",
            "fault",
            "value",
        ],
        "trace",
    )?;
    let version = required_u64(obj, "v", "trace")?;
    if version != 1 {
        return Err(format!("unsupported trace version {version}"));
    }
    let status = required_string(obj, "status", "trace")?.to_string();
    if status != "ok" && status != "fault" {
        return Err(format!("unsupported trace status {status:?}"));
    }
    let stdout = required_string_array(obj, "stdout", "trace")?;
    let files = parse_trace_files(required(obj, "files", "trace")?)?;
    let defer_errors = parse_trace_defer_errors(required(obj, "defer_errors", "trace")?)?;
    let fault = match required(obj, "fault", "trace")? {
        JsonValue::Null => None,
        value => Some(parse_trace_fault(value, "fault")?),
    };
    let value = if let Some(value) = obj.get("value") {
        Some(parse_trace_value(value, "value")?)
    } else {
        None
    };
    Ok(PyTrace {
        version,
        status,
        stdout,
        files,
        defer_errors,
        fault,
        value,
    })
}

fn parse_trace_files(value: &JsonValue) -> Result<Vec<TraceFile>, String> {
    let entries = array(value, "files")?;
    let mut out = Vec::with_capacity(entries.len());
    for (idx, entry) in entries.iter().enumerate() {
        let context = format!("files[{idx}]");
        let obj = object(entry, &context)?;
        reject_unknown_keys(obj, &["path", "content"], &context)?;
        let path = required_string(obj, "path", &context)?.to_string();
        let content = parse_trace_value(
            required(obj, "content", &context)?,
            &format!("{context}.content"),
        )?;
        out.push(TraceFile { path, content });
    }
    Ok(out)
}

fn parse_trace_defer_errors(value: &JsonValue) -> Result<Vec<TraceDeferError>, String> {
    let entries = array(value, "defer_errors")?;
    let mut out = Vec::with_capacity(entries.len());
    for (idx, entry) in entries.iter().enumerate() {
        let context = format!("defer_errors[{idx}]");
        let obj = object(entry, &context)?;
        reject_unknown_keys(obj, &["rendered", "fault"], &context)?;
        let rendered = required_string(obj, "rendered", &context)?.to_string();
        let fault = match obj.get("fault") {
            Some(JsonValue::Null) => {
                return Err(format!(
                    "{context}.fault must be a structured fault, not null"
                ));
            }
            Some(value) => Some(parse_trace_fault(value, &format!("{context}.fault"))?),
            None => None,
        };
        out.push(TraceDeferError { rendered, fault });
    }
    Ok(out)
}

fn parse_trace_fault(value: &JsonValue, context: &str) -> Result<TraceFault, String> {
    let obj = object(value, context)?;
    reject_unknown_keys(obj, &["code", "message", "span"], context)?;
    let span_value = required(obj, "span", context)?;
    let span_obj = object(span_value, &format!("{context}.span"))?;
    reject_unknown_keys(span_obj, &["file", "lo", "hi"], &format!("{context}.span"))?;
    Ok(TraceFault {
        code: required_string(obj, "code", context)?.to_string(),
        message: required_string(obj, "message", context)?.to_string(),
        span: TraceSpan {
            file: required_i64(span_obj, "file", &format!("{context}.span"))?,
            lo: required_i64(span_obj, "lo", &format!("{context}.span"))?,
            hi: required_i64(span_obj, "hi", &format!("{context}.span"))?,
        },
    })
}

fn parse_trace_value(value: &JsonValue, context: &str) -> Result<TraceValue, String> {
    let obj = object(value, context)?;
    if obj.len() != 1 {
        return Err(format!("{context}: tagged value must have exactly one tag"));
    }
    let (tag, payload) = obj.iter().next().expect("tag exists");
    let parsed = match tag.as_str() {
        "int" => TraceValue::Int(json_i64(payload, context)?),
        "bool" => TraceValue::Bool(bool_value(payload, context)?),
        "null" => {
            if !matches!(payload, JsonValue::Null) {
                return Err(format!("{context}.null must be null"));
            }
            TraceValue::Null
        }
        "str" => TraceValue::Str(string_value(payload, context)?.to_string()),
        "list" => {
            let mut values = Vec::new();
            for (idx, item) in array(payload, context)?.iter().enumerate() {
                values.push(parse_trace_value(item, &format!("{context}.list[{idx}]"))?);
            }
            TraceValue::List(values)
        }
        "some" => TraceValue::Some(Box::new(parse_trace_value(
            payload,
            &format!("{context}.some"),
        )?)),
        "result" => parse_result_value(payload, context)?,
        "f64" => TraceValue::F64(parse_fixed_hex_u64(
            string_value(payload, context)?,
            context,
        )?),
        "bytes" => {
            let hex = string_value(payload, context)?;
            if hex.len() % 2 != 0 {
                return Err(format!("{context}.bytes must have an even hex length"));
            }
            validate_fixed_or_empty_hex(hex, context)?;
            TraceValue::Bytes(hex.to_string())
        }
        "record" => {
            let fields = object(payload, context)?;
            let mut out = BTreeMap::new();
            for (field, value) in fields {
                out.insert(
                    field.clone(),
                    parse_trace_value(value, &format!("{context}.record.{field}"))?,
                );
            }
            TraceValue::Record(out)
        }
        "map" => {
            let mut out = Vec::new();
            for (idx, entry) in array(payload, context)?.iter().enumerate() {
                let entry_context = format!("{context}.map[{idx}]");
                let entry_obj = object(entry, &entry_context)?;
                reject_unknown_keys(entry_obj, &["key", "value"], &entry_context)?;
                let key = parse_trace_value(
                    required(entry_obj, "key", &entry_context)?,
                    &format!("{entry_context}.key"),
                )?;
                let value = parse_trace_value(
                    required(entry_obj, "value", &entry_context)?,
                    &format!("{entry_context}.value"),
                )?;
                out.push((key, value));
            }
            TraceValue::Map(out)
        }
        "set" => {
            let mut out = Vec::new();
            for (idx, item) in array(payload, context)?.iter().enumerate() {
                out.push(parse_trace_value(item, &format!("{context}.set[{idx}]"))?);
            }
            TraceValue::Set(out)
        }
        "enum" => {
            let enum_obj = object(payload, context)?;
            reject_unknown_keys(enum_obj, &["id", "variant", "index", "payloads"], context)?;
            let payload_values = array(
                required(enum_obj, "payloads", context)?,
                &format!("{context}.enum.payloads"),
            )?;
            let mut payloads = Vec::new();
            for (idx, item) in payload_values.iter().enumerate() {
                payloads.push(parse_trace_value(
                    item,
                    &format!("{context}.enum.payloads[{idx}]"),
                )?);
            }
            TraceValue::Enum {
                id: required_string(enum_obj, "id", context)?.to_string(),
                variant: required_string(enum_obj, "variant", context)?.to_string(),
                index: required_u64(enum_obj, "index", context)?,
                payloads,
            }
        }
        "range" => {
            let range = object(payload, context)?;
            reject_unknown_keys(range, &["lo", "hi", "inclusive", "step"], context)?;
            TraceValue::Range {
                lo: json_i64(
                    required(range, "lo", context)?,
                    &format!("{context}.range.lo"),
                )?,
                hi: json_i64(
                    required(range, "hi", context)?,
                    &format!("{context}.range.hi"),
                )?,
                inclusive: bool_value(
                    required(range, "inclusive", context)?,
                    &format!("{context}.range.inclusive"),
                )?,
                step: json_i64(
                    required(range, "step", context)?,
                    &format!("{context}.range.step"),
                )?,
            }
        }
        _ => return Err(format!("{context}: unknown tagged value {tag:?}")),
    };
    Ok(parsed)
}

fn parse_result_value(value: &JsonValue, context: &str) -> Result<TraceValue, String> {
    let obj = object(value, context)?;
    match (obj.get("ok"), obj.get("err"), obj.len()) {
        (Some(ok), None, 1) => Ok(TraceValue::ResultOk(Box::new(parse_trace_value(
            ok,
            &format!("{context}.result.ok"),
        )?))),
        (None, Some(err), 1) => Ok(TraceValue::ResultErr(Box::new(parse_trace_value(
            err,
            &format!("{context}.result.err"),
        )?))),
        _ => Err(format!(
            "{context}.result must contain exactly one of ok/err"
        )),
    }
}

fn validate_fixed_hex(value: &str, len: usize, context: &str) -> Result<(), String> {
    if value.len() != len {
        return Err(format!("{context}: expected {len} lower-case hex digits"));
    }
    validate_fixed_or_empty_hex(value, context)
}

fn parse_fixed_hex_u64(value: &str, context: &str) -> Result<u64, String> {
    validate_fixed_hex(value, 16, context)?;
    u64::from_str_radix(value, 16).map_err(|e| format!("{context}: parse f64 bits: {e}"))
}

fn validate_fixed_or_empty_hex(value: &str, context: &str) -> Result<(), String> {
    if value
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(())
    } else {
        Err(format!(
            "{context}: hex must be lower-case without a prefix"
        ))
    }
}

fn required<'a>(
    obj: &'a BTreeMap<String, JsonValue>,
    key: &str,
    context: &str,
) -> Result<&'a JsonValue, String> {
    obj.get(key)
        .ok_or_else(|| format!("{context}: missing required key {key:?}"))
}

fn required_string<'a>(
    obj: &'a BTreeMap<String, JsonValue>,
    key: &str,
    context: &str,
) -> Result<&'a str, String> {
    string_value(required(obj, key, context)?, &format!("{context}.{key}"))
}

fn required_string_array(
    obj: &BTreeMap<String, JsonValue>,
    key: &str,
    context: &str,
) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for (idx, item) in array(required(obj, key, context)?, &format!("{context}.{key}"))?
        .iter()
        .enumerate()
    {
        out.push(string_value(item, &format!("{context}.{key}[{idx}]"))?.to_string());
    }
    Ok(out)
}

fn required_u64(
    obj: &BTreeMap<String, JsonValue>,
    key: &str,
    context: &str,
) -> Result<u64, String> {
    json_u64(required(obj, key, context)?, &format!("{context}.{key}"))
}

fn required_i64(
    obj: &BTreeMap<String, JsonValue>,
    key: &str,
    context: &str,
) -> Result<i64, String> {
    json_i64(required(obj, key, context)?, &format!("{context}.{key}"))
}

fn reject_unknown_keys(
    obj: &BTreeMap<String, JsonValue>,
    allowed: &[&str],
    context: &str,
) -> Result<(), String> {
    for key in obj.keys() {
        if !allowed.iter().any(|allowed| allowed == key) {
            return Err(format!("{context}: unknown key {key:?}"));
        }
    }
    Ok(())
}

fn object<'a>(
    value: &'a JsonValue,
    context: &str,
) -> Result<&'a BTreeMap<String, JsonValue>, String> {
    match value {
        JsonValue::Object(obj) => Ok(obj),
        other => Err(format!("{context}: expected object, got {other:?}")),
    }
}

fn array<'a>(value: &'a JsonValue, context: &str) -> Result<&'a [JsonValue], String> {
    match value {
        JsonValue::Array(values) => Ok(values),
        other => Err(format!("{context}: expected array, got {other:?}")),
    }
}

fn string_value<'a>(value: &'a JsonValue, context: &str) -> Result<&'a str, String> {
    match value {
        JsonValue::String(s) => Ok(s),
        other => Err(format!("{context}: expected string, got {other:?}")),
    }
}

fn bool_value(value: &JsonValue, context: &str) -> Result<bool, String> {
    match value {
        JsonValue::Bool(v) => Ok(*v),
        other => Err(format!("{context}: expected bool, got {other:?}")),
    }
}

fn json_u64(value: &JsonValue, context: &str) -> Result<u64, String> {
    let raw = number_value(value, context)?;
    if raw.contains(['.', 'e', 'E']) || raw.starts_with('-') {
        return Err(format!("{context}: expected unsigned integer, got {raw:?}"));
    }
    raw.parse::<u64>()
        .map_err(|e| format!("{context}: parse unsigned integer: {e}"))
}

fn json_i64(value: &JsonValue, context: &str) -> Result<i64, String> {
    let raw = number_value(value, context)?;
    if raw.contains(['.', 'e', 'E']) {
        return Err(format!("{context}: expected integer, got {raw:?}"));
    }
    raw.parse::<i64>()
        .map_err(|e| format!("{context}: parse integer: {e}"))
}

fn number_value<'a>(value: &'a JsonValue, context: &str) -> Result<&'a str, String> {
    match value {
        JsonValue::Number(raw) => Ok(raw),
        other => Err(format!("{context}: expected number, got {other:?}")),
    }
}
