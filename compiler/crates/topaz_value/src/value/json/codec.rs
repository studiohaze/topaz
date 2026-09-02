use super::super::*;

/// Structural depth bound for [`json_stringify`] — a safe backstop for any
/// (untypeable but constructible) cyclic aggregate, so encoding errors with
/// `JSON_LIMIT` instead of overflowing the stack or double-borrowing.
pub(in crate::value) const JSON_MAX_DEPTH: u32 = 128;

/// §22 `JSON.stringify` shared leaf (interpreter ≡ emitted Rust): encode a value
/// as CANONICAL JSON — object keys sorted (records are already field-sorted; Map
/// keys are sorted here), no whitespace — or `Err(message)` for a non-encodable
/// value. PURE: no host/IO/clock/locale/serde. NOMINAL types encode STRUCTURALLY
/// (v5.4 §4): a nominal record → a sorted-key object (same shape as a structural
/// record), an enum → a tagged object `{"tag":…[,"values":[…]]}`, a newtype →
/// transparently as its inner value. v1 rejects `float` (a canonicalization
/// hazard, deferred), `Result`, `Set`, a `Map` with any non-string key,
/// resources, functions, templates, ranges, namespaces — and a nominal whose
/// fields/payloads/base are non-encodable (the CHECK gate rejects those before
/// run; this leaf still guards them for `--unchecked`). The error carries a
/// deterministic `$` / `.field` / `[i]` path so both engines report identically.
pub fn json_stringify(value: &Value, _canonical: bool) -> Result<String, String> {
    let mut out = String::new();
    encode_json(value, &mut out, "$", 0)?;
    Ok(out)
}

pub(in crate::value) fn encode_json(
    value: &Value,
    out: &mut String,
    path: &str,
    depth: u32,
) -> Result<(), String> {
    if depth > JSON_MAX_DEPTH {
        return Err(format!(
            "JSON_LIMIT: {path}: structure exceeds the JSON.stringify depth limit"
        ));
    }
    match value {
        Value::Int(n) => out.push_str(&n.to_string()),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Unit | Value::None | Value::Null => out.push_str("null"),
        Value::Str(s) => push_json_escaped(out, s),
        // §22.1 `Some(v)` encodes as `v` (`None` is `null`, handled above).
        Value::Some(v) => encode_json(v, out, path, depth)?,
        Value::Array(items) => {
            // Snapshot first so recursion can't double-borrow a self-referential
            // aggregate (it bottoms out at the depth limit instead).
            let snapshot = items.borrow().clone();
            out.push('[');
            for (i, item) in snapshot.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                encode_json(item, out, &format!("{path}[{i}]"), depth + 1)?;
            }
            out.push(']');
        }
        // A record is BTreeMap-backed → its field names are already canonical (sorted).
        Value::Record(fields) => {
            out.push('{');
            for (i, (k, v)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                push_json_escaped(out, k);
                out.push(':');
                encode_json(v, out, &format!("{path}.{k}"), depth + 1)?;
            }
            out.push('}');
        }
        // A Map encodes as an object: STRING keys only, SORTED for canonical
        // output (JSON does not inherit the Map's insertion order).
        Value::Map(map) => {
            let mut pairs: Vec<(Rc<str>, Value)> = Vec::new();
            for (k, v) in map.borrow().pairs() {
                match k {
                    Value::Str(ks) => pairs.push((ks, v)),
                    other => {
                        return Err(format!(
                            "JSON_NON_STRING_KEY: {path}: a Map key has type `{}`; only string keys encode to JSON",
                            other.kind()
                        ));
                    }
                }
            }
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            out.push('{');
            for (i, (k, v)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                push_json_escaped(out, k);
                out.push(':');
                encode_json(v, out, &format!("{path}.{k}"), depth + 1)?;
            }
            out.push('}');
        }
        // §3 (v5.4) a NOMINAL record encodes as a JSON object — STRUCTURALLY, like
        // the structural `Value::Record` arm above (any nominal whose fields are all
        // JSON-encodable is encodable; `derives(JSON)` records checker metadata but
        // is not a runtime gate. KEY ORDERING: keys are SORTED here, the
        // SAME canonical form the structural `Record` arm produces (it is BTreeMap-
        // backed, already sorted), so the two record kinds serialize CONSISTENTLY and a
        // record and a same-shaped nominal stringify identically. (The nominal stores
        // fields in DECLARATION order — for render/Order — so we sort a snapshot here.)
        // The `record_id` (nominal identity) is NOT emitted: JSON has no nominal tag for
        // records, and emitting it would diverge from the structural form.
        Value::NominalRecord { fields, .. } => {
            let mut pairs: Vec<(&Rc<str>, &Value)> = fields.iter().map(|(k, v)| (k, v)).collect();
            pairs.sort_by(|a, b| a.0.cmp(b.0));
            out.push('{');
            for (i, (k, v)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                push_json_escaped(out, k);
                out.push(':');
                encode_json(v, out, &format!("{path}.{k}"), depth + 1)?;
            }
            out.push('}');
        }
        // §3 (v5.4) an enum encodes as a tagged object: the variant name is stored under
        // `"tag"`, plus its payload. The language currently has positional tuple
        // payloads only, so named payload fields cannot be constructed. Positional
        // payloads therefore encode under a
        // `"values"` ARRAY (used uniformly for every payloadful arity, so the shape does
        // not depend on payload count). A PAYLOAD-LESS variant is `{"tag":"Name"}` — the
        // canonical payload-free form. Encodes structurally (any enum whose payloads are
        // all encodable is encodable; no `derives(JSON)` gate).
        Value::Enum {
            enum_id,
            variant,
            payloads,
            ..
        } => {
            if enum_id.as_ref() == "RoundingMode" {
                return Err(format!(
                    "JSON_UNSUPPORTED: {path}: `RoundingMode` is not JSON-encodable"
                ));
            }
            out.push_str("{\"tag\":");
            push_json_escaped(out, variant);
            if !payloads.is_empty() {
                out.push_str(",\"values\":[");
                for (i, p) in payloads.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    encode_json(p, out, &format!("{path}.values[{i}]"), depth + 1)?;
                }
                out.push(']');
            }
            out.push('}');
        }
        // §3 (v5.4) a NEWTYPE encodes TRANSPARENTLY as its inner base value — the wrapper
        // identity is erased in JSON (`UserId(5)` → `5`). Encodes
        // STRUCTURALLY: encodable iff the base is. Same depth (a transparent unwrap, like
        // `Some(v)` above — no nesting level is added).
        Value::Newtype { inner, .. } => encode_json(inner, out, path, depth)?,
        // A parsed JSON tree (`JSON.parse` result) re-serializes via its own
        // canonical writer — sorted keys, number lexemes exactly as parsed — so
        // `stringify(parse(t))` round-trips to canonical JSON.
        Value::Json(node) => write_json_node(out, node),
        // §11 (v5.4) `Match` encodes like its documented record surface. Keep keys
        // sorted to match canonical record JSON ordering.
        Value::RegexMatch(m) => {
            out.push('{');
            out.push_str("\"end\":");
            encode_json(&Value::Int(m.end), out, &format!("{path}.end"), depth + 1)?;
            out.push_str(",\"groups\":");
            encode_json(
                &regex_match_groups_value(m),
                out,
                &format!("{path}.groups"),
                depth + 1,
            )?;
            out.push_str(",\"named\":");
            encode_json(
                &regex_match_named_value(m),
                out,
                &format!("{path}.named"),
                depth + 1,
            )?;
            out.push_str(",\"start\":");
            encode_json(
                &Value::Int(m.start),
                out,
                &format!("{path}.start"),
                depth + 1,
            )?;
            out.push_str(",\"text\":");
            encode_json(
                &Value::Str(m.text.clone()),
                out,
                &format!("{path}.text"),
                depth + 1,
            )?;
            out.push('}');
        }
        Value::Float(_) => {
            return Err(format!(
                "JSON_UNSUPPORTED: {path}: float is not supported by JSON.stringify v1"
            ));
        }
        other => {
            return Err(format!(
                "JSON_UNSUPPORTED: {path}: `{}` is not JSON-encodable",
                other.kind()
            ));
        }
    }
    Ok(())
}

/// Append `raw` as a JSON string literal (RFC 8259; control chars < U+0020 as
/// lowercase `\uXXXX`). Deterministic.
pub(in crate::value) fn push_json_escaped(out: &mut String, raw: &str) {
    out.push('"');
    for c in raw.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// §22 `JSON.stringify(value)` builtin: wrap the leaf into a Topaz
/// `Result<string, string>` (`Ok` on success, `Err(message)` for a
/// non-encodable value). The SHARED entry both engines call.
pub fn builtin_json_stringify(value: Value) -> Value {
    match json_stringify(&value, true) {
        Ok(s) => Value::Ok(Rc::new(Value::str(s))),
        Err(e) => Value::Err(Rc::new(Value::str(e))),
    }
}

/// §4 (v5.4) the DERIVED builtin-protocol dispatch leaf — `Show.show(x)` →
/// `render`, `Eq.equals(a, b)` → `values_equal`, `Order.compare(a, b)` →
/// `values_compare`. BOTH engines call this single leaf for a `derives`d
/// conformance, so the result is byte-identical run≡build (incl. the GUARD_COMPARE
/// fault a non-comparable inner raises under `--unchecked`, mapped via `cmp_guard`).
/// The semantics match the corresponding operators (`==`/`<`) exactly because they
/// route through the SAME `values_equal`/`values_compare` leaves. A MANUAL `impl`
/// conformance never reaches here — the call site dispatches to the registered user
/// method first. `protocol`/`method` are the spelled names; an unrecognized pair is
/// an internal error (the checker only admits the builtin protocol methods on a
/// derived conformance), faulted as GUARD_TYPE for safety on the `--unchecked` lane.
pub fn builtin_protocol_dispatch(
    protocol: &str,
    method: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RtError> {
    match (protocol, method) {
        // Show.show(value) -> string : the deterministic debug render (the same leaf
        // string interpolation `{x}` uses).
        ("Show", "show") => {
            let v = args.first().cloned().unwrap_or(Value::Unit);
            Ok(Value::str(render(&v)))
        }
        // Eq.equals(a, b) -> bool : structural equality (the `==` leaf).
        ("Eq", "equals") => {
            let a = args.first().cloned().unwrap_or(Value::Unit);
            let b = args.get(1).cloned().unwrap_or(Value::Unit);
            Ok(Value::Bool(
                values_equal(&a, &b).map_err(|e| cmp_guard(e, span))?,
            ))
        }
        // Order.compare(a, b) -> int : -1 / 0 / +1 from the `<` leaf's ordering. A
        // canonical small-int verdict (NOT the raw `Ordering` discriminant) so the
        // result is a stable, documented `int`.
        ("Order", "compare") => {
            let a = args.first().cloned().unwrap_or(Value::Unit);
            let b = args.get(1).cloned().unwrap_or(Value::Unit);
            let ord = values_compare(&a, &b).map_err(|e| cmp_guard(e, span))?;
            use std::cmp::Ordering;
            Ok(Value::Int(match ord {
                Ordering::Less => -1,
                Ordering::Equal => 0,
                Ordering::Greater => 1,
            }))
        }
        _ => Err(fault(
            codes::GUARD_TYPE,
            format!("no derived protocol method `{protocol}.{method}`"),
            span,
        )),
    }
}
