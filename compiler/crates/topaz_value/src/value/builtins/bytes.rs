use super::super::*;

// ----------------------------------------------------------------------------
// §8 (v5.4) the `Bytes` builtin namespace — the immutable byte-array + encoding
// stdlib (UTF-8 / hex / base64), implemented PURELY (no crypto/encoding crate, no
// host). Every codec routes through ONE shared leaf that the interpreter AND the
// emitted Rust BOTH call by name, so a conversion is byte-identical run≡build by
// construction (and, on a native decline → boxed, across all three columns). The
// hex + base64 alphabets are RFC 4648 §8/§4 STANDARD (lowercase hex out; base64
// with `+`/`/` and `=` padding). Decisions pinned here (and in the unit tests):
//   * `decodeUtf8`/`fromHex`/`fromBase64` are FALLIBLE: a value-level `Err(string)`
//     (NEVER a fault) so a program can recover from bad input. `encodeUtf8`/`toHex`/
//     `toBase64`/`length` are total; `slice` CLAMPS (never faults, like `arr.slice`).
//   * `fromHex` accepts BOTH cases, rejects an odd length / any non-hex digit.
//   * `fromBase64` is STRICT RFC 4648: requires correct `=` padding to a multiple of
//     4, rejects a non-alphabet char and non-zero padding bits.

/// Pull the `Rc<[u8]>` out of a `Value::Bytes`, else a GUARD_TYPE fault. The checker
/// proves the receiver is a `Bytes`, so this fault is the `--unchecked` backstop —
/// identical on both engines because both call this one leaf.
pub(in crate::value) fn bytes_arg(
    arg: &Value,
    name: &str,
    span: Span,
) -> Result<Rc<[u8]>, RtError> {
    match arg {
        Value::Bytes(b) => Ok(b.clone()),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`Bytes.{name}` takes a `Bytes`, found `{}`", other.kind()),
            span,
        )),
    }
}

/// Pull the `Rc<str>` out of a `Value::Str`, else a GUARD_TYPE fault (the
/// `--unchecked` backstop for a static `Bytes.x(s)` string argument).
pub(in crate::value) fn bytes_str_arg(
    arg: &Value,
    name: &str,
    span: Span,
) -> Result<Rc<str>, RtError> {
    match arg {
        Value::Str(s) => Ok(s.clone()),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`Bytes.{name}` takes a `string`, found `{}`", other.kind()),
            span,
        )),
    }
}

/// Append the lowercase RFC 4648 §8 hex of `b` to `out` (the SHARED hex writer
/// `Bytes.toHex` and the `Bytes(...)` render both use, so they agree). Each byte →
/// two lowercase hex digits, high nibble first; an empty slice appends nothing.
pub fn bytes_to_hex_into(out: &mut String, b: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.reserve(b.len() * 2);
    for &byte in b {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
}

/// §8 `Bytes.encodeUtf8(s) -> Bytes` — the string's UTF-8 bytes (Topaz strings ARE
/// UTF-8, so this is total: it copies the string's byte representation).
pub fn builtin_bytes_empty(_span: Span) -> Result<Value, RtError> {
    Ok(Value::Bytes(Rc::from(Vec::new().as_slice())))
}

/// Build bytes from an array of integer byte values. Values outside 0..255 are
/// recoverable `Err`s because this is a data conversion API.
pub fn builtin_bytes_from_array(arg: Value, span: Span) -> Result<Value, RtError> {
    let items = match arg {
        Value::Array(items) => items.borrow().clone(),
        other => {
            return Err(fault(
                codes::GUARD_TYPE,
                format!(
                    "`Bytes.fromArray` takes an `Array<int>`, found `{}`",
                    other.kind()
                ),
                span,
            ));
        }
    };
    let mut out = Vec::with_capacity(items.len());
    for (idx, item) in items.iter().enumerate() {
        match item {
            Value::Int(n) if (0..=255).contains(n) => out.push(*n as u8),
            Value::Int(_) => {
                return Ok(Value::Err(Rc::new(Value::str(format!(
                    "Bytes.fromArray: value at index {idx} is outside 0..255"
                )))));
            }
            other => {
                return Ok(Value::Err(Rc::new(Value::str(format!(
                    "Bytes.fromArray: value at index {idx} is `{}`, expected `int`",
                    other.kind()
                )))));
            }
        }
    }
    Ok(Value::Ok(Rc::new(Value::Bytes(Rc::from(out.as_slice())))))
}

pub fn builtin_bytes_encode_utf8(arg: Value, span: Span) -> Result<Value, RtError> {
    let s = bytes_str_arg(&arg, "encodeUtf8", span)?;
    Ok(Value::Bytes(Rc::from(s.as_bytes())))
}

/// §8 `b.decodeUtf8() -> Result<string, string>` — decode the bytes as UTF-8. Valid
/// UTF-8 → `Ok(string)`; INVALID UTF-8 → `Err(message)` (never a fault).
pub fn builtin_bytes_decode_utf8(recv: Value, span: Span) -> Result<Value, RtError> {
    let b = bytes_arg(&recv, "decodeUtf8", span)?;
    Ok(match std::str::from_utf8(&b) {
        Ok(s) => Value::Ok(Rc::new(Value::str(s))),
        Err(_) => Value::Err(Rc::new(Value::str("Bytes.decodeUtf8: invalid UTF-8"))),
    })
}

/// §8 `b.toHex() -> string` — lowercase RFC 4648 §8 hex (total).
pub fn builtin_bytes_to_hex(recv: Value, span: Span) -> Result<Value, RtError> {
    let b = bytes_arg(&recv, "toHex", span)?;
    let mut out = String::with_capacity(b.len() * 2);
    bytes_to_hex_into(&mut out, &b);
    Ok(Value::str(out))
}

/// §8 `Bytes.fromHex(s) -> Result<Bytes, string>` — decode hex, accepting BOTH
/// lowercase and uppercase digits. An ODD length or any NON-hex character → `Err`.
pub fn builtin_bytes_from_hex(arg: Value, span: Span) -> Result<Value, RtError> {
    let s = bytes_str_arg(&arg, "fromHex", span)?;
    let raw = s.as_bytes();
    let err = |m: &str| Ok(Value::Err(Rc::new(Value::str(m))));
    if raw.len() % 2 != 0 {
        return err("Bytes.fromHex: odd-length hex string");
    }
    let nibble = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };
    let mut out = Vec::with_capacity(raw.len() / 2);
    for pair in raw.chunks_exact(2) {
        match (nibble(pair[0]), nibble(pair[1])) {
            (Some(hi), Some(lo)) => out.push((hi << 4) | lo),
            _ => return err("Bytes.fromHex: invalid hex digit"),
        }
    }
    Ok(Value::Ok(Rc::new(Value::Bytes(Rc::from(out.as_slice())))))
}

/// The RFC 4648 §4 STANDARD base64 alphabet (index → output char).
pub(in crate::value) const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// §8 `b.toBase64() -> string` — RFC 4648 §4 STANDARD base64, padded with `=` to a
/// multiple of 4 (total). Three input bytes → four output chars; the final 1/2-byte
/// group is padded (`==` / `=`). An empty input → `""`.
pub fn builtin_bytes_to_base64(recv: Value, span: Span) -> Result<Value, RtError> {
    let b = bytes_arg(&recv, "toBase64", span)?;
    let a = BASE64_ALPHABET;
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for chunk in b.chunks(3) {
        // Pack up to 3 bytes into a 24-bit big-endian group, then emit 4 sextets.
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(a[((n >> 18) & 0x3f) as usize] as char);
        out.push(a[((n >> 12) & 0x3f) as usize] as char);
        // The 3rd/4th chars become `=` when the source group is short.
        out.push(if chunk.len() > 1 {
            a[((n >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            a[(n & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    Ok(Value::str(out))
}

/// §8 `Bytes.fromBase64(s) -> Result<Bytes, string>` — STRICT RFC 4648 §4 STANDARD
/// base64 WITH padding: the input length must be a multiple of 4, `=` may appear
/// only as the last one/two chars, a non-alphabet char → `Err`, and the unused
/// padding bits must be zero (canonical). Any violation → `Err` (never a fault).
pub fn builtin_bytes_from_base64(arg: Value, span: Span) -> Result<Value, RtError> {
    let s = bytes_str_arg(&arg, "fromBase64", span)?;
    let raw = s.as_bytes();
    let err = |m: &str| Ok(Value::Err(Rc::new(Value::str(m))));
    if raw.is_empty() {
        return Ok(Value::Ok(Rc::new(Value::Bytes(Rc::from(
            Vec::new().as_slice(),
        )))));
    }
    if raw.len() % 4 != 0 {
        return err("Bytes.fromBase64: length is not a multiple of 4");
    }
    // Reverse-map a char to its 0..=63 sextet (`=` and others are non-values).
    let sextet = |c: u8| -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    // Padding `=` is permitted ONLY as the final one or two chars.
    let pad = raw.iter().filter(|&&c| c == b'=').count();
    if pad > 2 || raw[..raw.len() - pad].contains(&b'=') {
        return err("Bytes.fromBase64: misplaced padding");
    }
    let mut out = Vec::with_capacity(raw.len() / 4 * 3);
    for group in raw.chunks_exact(4) {
        let pads = group.iter().filter(|&&c| c == b'=').count();
        let mut sextets = [0u32; 4];
        for (i, &c) in group.iter().enumerate() {
            if c == b'=' {
                continue;
            }
            match sextet(c) {
                Some(v) => sextets[i] = v as u32,
                None => return err("Bytes.fromBase64: invalid base64 character"),
            }
        }
        let n = (sextets[0] << 18) | (sextets[1] << 12) | (sextets[2] << 6) | sextets[3];
        // `pads` (0/1/2) drops the trailing 1/2 bytes of this group. Canonical
        // encodings leave the dropped low bits zero — reject a non-zero remainder.
        match pads {
            0 => {
                out.push((n >> 16) as u8);
                out.push((n >> 8) as u8);
                out.push(n as u8);
            }
            1 => {
                if n & 0xff != 0 {
                    return err("Bytes.fromBase64: non-canonical padding bits");
                }
                out.push((n >> 16) as u8);
                out.push((n >> 8) as u8);
            }
            2 => {
                if n & 0xffff != 0 {
                    return err("Bytes.fromBase64: non-canonical padding bits");
                }
                out.push((n >> 16) as u8);
            }
            _ => unreachable!("pad count is 0..=2 by the guard above"),
        }
    }
    Ok(Value::Ok(Rc::new(Value::Bytes(Rc::from(out.as_slice())))))
}

/// §8 `b.length() -> int` — the byte count (total).
pub fn builtin_bytes_length(recv: Value, span: Span) -> Result<Value, RtError> {
    Ok(Value::Int(builtin_bytes_length_i64(&recv, span)?))
}

/// Direct checked `Bytes.length` leaf. The handle remains a `Value`; only the
/// integer result avoids a tagged round trip.
pub fn builtin_bytes_length_i64(recv: &Value, span: Span) -> Result<i64, RtError> {
    Ok(bytes_arg(recv, "length", span)?.len() as i64)
}

pub fn builtin_bytes_is_empty(recv: Value, span: Span) -> Result<Value, RtError> {
    let b = bytes_arg(&recv, "isEmpty", span)?;
    Ok(Value::Bool(b.is_empty()))
}

pub fn builtin_bytes_get(recv: Value, index: Value, span: Span) -> Result<Value, RtError> {
    let _ = bytes_arg(&recv, "get", span)?;
    let Value::Int(index) = index else {
        return Err(fault(
            codes::GUARD_TYPE,
            "`Bytes.get` takes an `int` index",
            span,
        ));
    };
    builtin_bytes_get_i64(&recv, index, span)
}

/// Direct checked `Bytes.get` leaf. Its language result remains
/// `Option<int>`—only receiver dispatch and index boxing are removed.
pub fn builtin_bytes_get_i64(recv: &Value, index: i64, span: Span) -> Result<Value, RtError> {
    let b = bytes_arg(recv, "get", span)?;
    if index >= 0 && (index as usize) < b.len() {
        Ok(Value::Some(Rc::new(Value::Int(b[index as usize] as i64))))
    } else {
        Ok(Value::None)
    }
}

/// §8 `b.slice(start, end) -> Bytes` — the half-open `[start, end)` sub-array,
/// CLAMPED to the byte bounds (`start` to `[0, len]`, `end` to `[start, len]`), so
/// an out-of-range or inverted range yields a shorter/empty `Bytes`, NEVER a fault
/// (the SAME policy as `arr.slice`/`str.slice`). A non-`int` bound faults GUARD_TYPE.
pub fn builtin_bytes_slice(
    recv: Value,
    start: Value,
    end: Value,
    span: Span,
) -> Result<Value, RtError> {
    let _ = bytes_arg(&recv, "slice", span)?;
    let bound = |v: Value| match v {
        Value::Int(n) => Ok(n),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`Bytes.slice` takes `int` bounds, found `{}`", other.kind()),
            span,
        )),
    };
    let start = bound(start)?;
    let end = bound(end)?;
    builtin_bytes_slice_i64(&recv, start, end, span)
}

/// Direct checked `Bytes.slice` leaf over already-proved integer bounds.
pub fn builtin_bytes_slice_i64(
    recv: &Value,
    start: i64,
    end: i64,
    span: Span,
) -> Result<Value, RtError> {
    let b = bytes_arg(recv, "slice", span)?;
    let len = b.len() as i64;
    let s = start.clamp(0, len);
    let e = end.clamp(s, len);
    Ok(Value::Bytes(Rc::from(&b[s as usize..e as usize])))
}

pub fn builtin_bytes_to_array(recv: Value, span: Span) -> Result<Value, RtError> {
    let b = bytes_arg(&recv, "toArray", span)?;
    Ok(Value::array(
        b.iter().map(|byte| Value::Int(*byte as i64)).collect(),
    ))
}

/// §8 `Bytes.concat(a, b) -> Bytes` — a NEW `Bytes` of `a`'s bytes followed by
/// `b`'s (total; both engines share this leaf).
pub fn builtin_bytes_concat(a: Value, b: Value, span: Span) -> Result<Value, RtError> {
    let xa = bytes_arg(&a, "concat", span)?;
    let xb = bytes_arg(&b, "concat", span)?;
    let mut out = Vec::with_capacity(xa.len() + xb.len());
    out.extend_from_slice(&xa);
    out.extend_from_slice(&xb);
    Ok(Value::Bytes(Rc::from(out.as_slice())))
}
