use super::*;

/// Interpolation rendering (§1/§22.2: non-string values print via
/// interpolation). The aggregate forms are the stable reference
/// renderings the execution corpus pins — the SINGLE implementation
/// both engines call (Rust `Display` parity is NOT assumed).
pub fn render(value: &Value) -> String {
    let mut out = String::new();
    let mut fuel = STRUCT_FUEL;
    let _ = render_into(value, &mut out, &mut fuel, 0);
    out
}

/// One pinned Topaz float-render golden vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatRenderGolden {
    pub name: &'static str,
    pub bits: u64,
    pub render: &'static str,
}

/// Canonical Topaz float-rendering goldens.
///
/// These literals define the public rendering contract independent of any
/// particular target runtime's default float printer. The rule is:
///
/// - `NaN` payloads and sign render as the single spelling `NaN`.
/// - infinities render as `inf` and `-inf`.
/// - finite integral floats with `abs(x) < 1e15` keep a `.0` suffix so common
///   int and float interpolation output do not collide.
/// - all other finite floats use Rust's shortest round-tripping decimal, expanded
///   to positional decimal notation rather than exponent notation.
pub const FLOAT_RENDER_GOLDENS: &[FloatRenderGolden] = &[
    FloatRenderGolden {
        name: "positive_zero",
        bits: 0x0000_0000_0000_0000,
        render: "0.0",
    },
    FloatRenderGolden {
        name: "negative_zero",
        bits: 0x8000_0000_0000_0000,
        render: "-0.0",
    },
    FloatRenderGolden {
        name: "one",
        bits: 0x3ff0_0000_0000_0000,
        render: "1.0",
    },
    FloatRenderGolden {
        name: "negative_one",
        bits: 0xbff0_0000_0000_0000,
        render: "-1.0",
    },
    FloatRenderGolden {
        name: "two",
        bits: 0x4000_0000_0000_0000,
        render: "2.0",
    },
    FloatRenderGolden {
        name: "below_integral_boundary",
        bits: 0x430c_6bf5_2633_fff8,
        render: "999999999999999.0",
    },
    FloatRenderGolden {
        name: "at_integral_boundary",
        bits: 0x430c_6bf5_2634_0000,
        render: "1000000000000000",
    },
    FloatRenderGolden {
        name: "above_integral_boundary",
        bits: 0x430c_6bf5_2634_0008,
        render: "1000000000000001",
    },
    FloatRenderGolden {
        name: "negative_integral_boundary",
        bits: 0xc30c_6bf5_2634_0000,
        render: "-1000000000000000",
    },
    FloatRenderGolden {
        name: "large_integral_positional",
        bits: 0x4341_c379_37e0_8000,
        render: "10000000000000000",
    },
    FloatRenderGolden {
        name: "two_pow_53",
        bits: 0x4340_0000_0000_0000,
        render: "9007199254740992",
    },
    FloatRenderGolden {
        name: "tiny_decimal",
        bits: 0x3ee4_f8b5_88e3_68f1,
        render: "0.00001",
    },
    FloatRenderGolden {
        name: "small_decimal",
        bits: 0x3f50_624d_d2f1_a9fc,
        render: "0.001",
    },
    FloatRenderGolden {
        name: "round_trip_tenth",
        bits: 0x3fb9_9999_9999_999a,
        render: "0.1",
    },
    FloatRenderGolden {
        name: "binary_sum",
        bits: 0x3fd3_3333_3333_3334,
        render: "0.30000000000000004",
    },
    FloatRenderGolden {
        name: "half",
        bits: 0x3fe0_0000_0000_0000,
        render: "0.5",
    },
    FloatRenderGolden {
        name: "max_finite",
        bits: 0x7fef_ffff_ffff_ffff,
        render: "179769313486231570000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    },
    FloatRenderGolden {
        name: "min_positive_normal",
        bits: 0x0010_0000_0000_0000,
        render: "0.000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000022250738585072014",
    },
    FloatRenderGolden {
        name: "min_positive_subnormal",
        bits: 0x0000_0000_0000_0001,
        render: "0.000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000005",
    },
    FloatRenderGolden {
        name: "positive_infinity",
        bits: 0x7ff0_0000_0000_0000,
        render: "inf",
    },
    FloatRenderGolden {
        name: "negative_infinity",
        bits: 0xfff0_0000_0000_0000,
        render: "-inf",
    },
    FloatRenderGolden {
        name: "quiet_nan",
        bits: 0x7ff8_0000_0000_0000,
        render: "NaN",
    },
    FloatRenderGolden {
        name: "quiet_nan_payload",
        bits: 0x7ff8_0000_0000_0001,
        render: "NaN",
    },
    FloatRenderGolden {
        name: "negative_quiet_nan",
        bits: 0xfff8_0000_0000_0000,
        render: "NaN",
    },
    FloatRenderGolden {
        name: "signaling_nan_roundtrip",
        bits: 0x7ff0_0000_0000_0001,
        render: "NaN",
    },
];

/// Stable float rendering used by Topaz interpolation.
pub fn render_float(x: f64) -> String {
    // Integral floats keep a `.0` below the public decimal boundary so
    // int and float renderings never collide in the common display path.
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{x:.1}")
    } else {
        x.to_string()
    }
}

/// Returns `false` when the budget ran out (exactly one `...` has
/// been emitted at the truncation point); callers stop immediately,
/// so truncation is deterministic and bounded.
pub(super) fn render_into(value: &Value, out: &mut String, fuel: &mut usize, depth: usize) -> bool {
    if *fuel == 0 || depth > STRUCT_DEPTH {
        out.push_str("...");
        return false;
    }
    *fuel -= 1;
    match value {
        Value::Int(x) => out.push_str(&x.to_string()),
        Value::Float(x) => out.push_str(&render_float(*x)),
        Value::Str(s) => out.push_str(s),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Unit => out.push_str("()"),
        Value::Null => out.push_str("null"),
        Value::Some(v) => {
            out.push_str("Some(");
            if !render_into(v, out, fuel, depth + 1) {
                return false;
            }
            out.push(')');
        }
        Value::None => out.push_str("None"),
        Value::Ok(v) => {
            out.push_str("Ok(");
            if !render_into(v, out, fuel, depth + 1) {
                return false;
            }
            out.push(')');
        }
        Value::Err(v) => {
            out.push_str("Err(");
            if !render_into(v, out, fuel, depth + 1) {
                return false;
            }
            out.push(')');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, v) in items.borrow().iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                if !render_into(v, out, fuel, depth + 1) {
                    return false;
                }
            }
            out.push(']');
        }
        Value::Record(fields) => {
            out.push_str("{ ");
            for (i, (name, v)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(name);
                out.push_str(": ");
                if !render_into(v, out, fuel, depth + 1) {
                    return false;
                }
            }
            out.push_str(" }");
        }
        Value::Map(map) => {
            out.push_str("Map{");
            for (i, (k, v)) in map.borrow().entries.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                if !render_into(&key_to_value(k), out, fuel, depth + 1) {
                    return false;
                }
                out.push_str(": ");
                if !render_into(v, out, fuel, depth + 1) {
                    return false;
                }
            }
            out.push('}');
        }
        Value::Set(set) => {
            out.push_str("Set{");
            for (i, k) in set.borrow().items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                if !render_into(&key_to_value(k), out, fuel, depth + 1) {
                    return false;
                }
            }
            out.push('}');
        }
        Value::Resource(_) => out.push_str("<file>"),
        Value::Composed(_) => out.push_str("<function>"),
        Value::Builtin { .. } => out.push_str("<function>"),
        Value::LispexApplicationOpaque(value) => {
            out.push('<');
            out.push_str(value.kind_name());
            out.push('>');
        }
        Value::Template(t) => t.render_into(out),
        Value::Namespace(n) => {
            out.push_str("<namespace ");
            out.push_str(n);
            out.push('>');
        }
        Value::Range {
            lo,
            hi,
            inclusive,
            step,
        } => {
            out.push_str(&format!(
                "{lo}{}{hi}",
                if *inclusive { ".." } else { "..<" }
            ));
            if *step != 1 {
                out.push_str(&format!(" by {step}"));
            }
        }
        Value::Closure(c) => {
            out.push_str("<function");
            if let Some(name) = c.name() {
                out.push(' ');
                out.push_str(name);
            }
            out.push('>');
        }
        // A parsed JSON tree renders AS its compact JSON text (so `print(parsed)`
        // and `JSON.stringify(parsed)` agree), keys sorted, number lexemes as stored.
        Value::Json(node) => write_json_node(out, node),
        Value::Enum {
            enum_id,
            variant,
            payloads,
            ..
        } => {
            out.push_str(enum_id);
            out.push('.');
            out.push_str(variant);
            if !payloads.is_empty() {
                out.push('(');
                for (i, p) in payloads.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    if !render_into(p, out, fuel, depth + 1) {
                        return false;
                    }
                }
                out.push(')');
            }
        }
        Value::NominalRecord {
            record_id, fields, ..
        } => {
            // `RecordId { name: v, … }` in declaration order (distinct from the
            // structural `{ name: v }` rendering, which has no leading id). A
            // zero-field record renders `RecordId {}` (no inner spaces).
            out.push_str(record_id);
            if fields.is_empty() {
                out.push_str(" {}");
            } else {
                out.push_str(" { ");
                for (i, (name, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(name);
                    out.push_str(": ");
                    if !render_into(v, out, fuel, depth + 1) {
                        return false;
                    }
                }
                out.push_str(" }");
            }
        }
        Value::Newtype {
            newtype_id, inner, ..
        } => {
            // `UserId(inner)` — the nominal id, then the wrapped value in parens.
            out.push_str(newtype_id);
            out.push('(');
            if !render_into(inner, out, fuel, depth + 1) {
                return false;
            }
            out.push(')');
        }
        // §8 (v5.4) a `Bytes` renders `Bytes(<lowercase-hex>)`: deterministic and
        // lossless (the hex is the SAME `bytes_to_hex` leaf `.toHex()` returns, so
        // `print(b)` and `b.toHex()` agree on the body). An EMPTY `Bytes` renders
        // `Bytes()`. Mirrors the `Newtype`/nominal `Id(...)` rendering style.
        Value::Bytes(b) => {
            out.push_str("Bytes(");
            bytes_to_hex_into(out, b);
            out.push(')');
        }
        Value::ByteBuffer(bytes) => {
            out.push_str("ByteBuffer(length: ");
            out.push_str(&bytes.borrow().len().to_string());
            out.push(')');
        }
        Value::Path(p) => {
            out.push_str("Path(");
            out.push_str(p);
            out.push(')');
        }
        Value::Regex(re) => {
            out.push_str("Regex(");
            out.push_str(re.as_str());
            out.push(')');
        }
        Value::RegexMatch(m) => {
            out.push_str("Match { start: ");
            out.push_str(&m.start.to_string());
            out.push_str(", end: ");
            out.push_str(&m.end.to_string());
            out.push_str(", text: ");
            out.push_str(&m.text);
            out.push_str(", groups: ");
            if !render_into(&regex_match_groups_value(m), out, fuel, depth + 1) {
                return false;
            }
            out.push_str(", named: ");
            if !render_into(&regex_match_named_value(m), out, fuel, depth + 1) {
                return false;
            }
            out.push_str(" }");
        }
        Value::Toml(t) => {
            out.push_str("TOML(");
            write_toml_inline(out, t);
            out.push(')');
        }
        Value::Url(u) => {
            out.push_str("URL(");
            out.push_str(&u.canonical);
            out.push(')');
        }
        Value::Date(d) => {
            out.push_str("Date(");
            out.push_str(&date_to_iso(*d));
            out.push(')');
        }
        Value::BigInt(n) => {
            out.push_str("BigInt(");
            out.push_str(&n.to_string_radix(10));
            out.push(')');
        }
        Value::Decimal(d) => {
            out.push_str("Decimal(");
            out.push_str(&d.to_string_canonical());
            out.push(')');
        }
    }
    true
}

/// Serialize a parsed JSON tree to compact, canonical JSON: object keys sorted
/// (the `BTreeMap` iterates in order), number lexemes emitted exactly as parsed,
/// strings RFC-8259-escaped. Total — a JSON tree cannot fail to serialize.
pub fn write_json_node(out: &mut String, node: &JsonValue) {
    match node {
        JsonValue::Null => out.push_str("null"),
        JsonValue::Bool(true) => out.push_str("true"),
        JsonValue::Bool(false) => out.push_str("false"),
        JsonValue::String(s) => push_json_escaped(out, s),
        JsonValue::Number(n) => out.push_str(&n.lexeme),
        JsonValue::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json_node(out, item);
            }
            out.push(']');
        }
        JsonValue::Object(entries) => {
            out.push('{');
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                push_json_escaped(out, k);
                out.push(':');
                write_json_node(out, v);
            }
            out.push('}');
        }
    }
}
