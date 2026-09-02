use super::super::*;

/// The §22 `JSONValue.kind()` discriminant name.
pub(in crate::value) fn json_kind_name(node: &JsonValue) -> &'static str {
    match node {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "bool",
        JsonValue::String(_) => "string",
        JsonValue::Number(_) => "number",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

/// The EXACT `i64` value of a validated JSON number lexeme when it denotes an
/// integer in `i64` range, else `None` — by DECIMAL analysis, never f64 (so no
/// rounding mislabels a non-integer as integral). `1e10`->`Some(10^10)`,
/// `1.0`->`Some(1)`, `1.5`->`None`, `-0`->`Some(0)`, an over-`i64` integer
/// ->`None`. Bounded: an `i128` overflow short-circuits to `None`.
pub(in crate::value) fn json_exact_int(lexeme: &str) -> Option<i64> {
    let neg = lexeme.starts_with('-');
    let body = lexeme.strip_prefix('-').unwrap_or(lexeme);
    let (mantissa, exp_str) = match body.split_once(['e', 'E']) {
        Some((m, e)) => (m, e),
        None => (body, ""),
    };
    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((i, f)) => (i, f),
        None => (mantissa, ""),
    };
    // Exact integer value = (int_part ++ frac_part) * 10^(exp - frac_part.len()),
    // computed by STRING normalization — never numeric exponentiation — so an
    // adversarial exponent/mantissa can neither overflow/panic nor loop. `exp` is
    // i128 (so a 19-digit exponent fits); `shift` saturates rather than wrapping.
    let exp: i128 = if exp_str.is_empty() {
        0
    } else {
        exp_str.parse().ok()?
    };
    let shift = exp.saturating_sub(frac_part.len() as i128);
    // Significant digits with leading zeros stripped; an all-zero mantissa is
    // exactly 0 for ANY shift (so a huge exponent never builds/loops here).
    let combined = format!("{int_part}{frac_part}");
    let trimmed = combined.trim_start_matches('0');
    if trimmed.is_empty() {
        return Some(0);
    }
    let mut digits = trimmed.to_string();
    let dl = digits.len() as i128;
    if shift >= 0 {
        // Append `shift` zeros, but only if the result can fit i64's 19 digits.
        let width = dl.checked_add(shift)?;
        if width > 19 {
            return None;
        }
        digits.push_str(&"0".repeat(shift as usize));
    } else if shift <= -dl {
        // Every significant digit is fractional (mantissa is non-zero) → not integral.
        return None;
    } else {
        // Drop the last `-shift` digits; integral iff they are all zero.
        let drop = (-shift) as usize; // 0 < drop < dl, so the cast is safe
        let (keep, dropped) = digits.split_at(digits.len() - drop);
        if dropped.bytes().any(|b| b != b'0') {
            return None;
        }
        digits = keep.to_string();
    }
    // Parse with the sign so `i64::MIN` (whose magnitude exceeds `i64::MAX`) parses.
    if neg {
        format!("-{digits}").parse::<i64>().ok()
    } else {
        digits.parse::<i64>().ok()
    }
}

/// §22 `JSON.parse` shared leaf (interpreter ≡ emitted Rust): a strict, PURE,
/// dependency-free recursive-descent RFC-8259 parser. Produces an immutable
/// [`JsonValue`] tree (objects key-sorted, duplicate keys REJECTED) or a
/// structured [`JsonParseError`] with a 1-based line/column. Depth-bounded by
/// `JSON_MAX_DEPTH` (the same bound `JSON.stringify` uses).
pub fn json_parse(input: &str) -> Result<JsonValue, JsonParseError> {
    let mut p = JsonParser {
        chars: input.chars().collect(),
        pos: 0,
        line: 1,
        col: 1,
    };
    p.skip_ws();
    let value = p.parse_value(0)?;
    p.skip_ws();
    if p.peek().is_some() {
        return Err(p.error("unexpected trailing characters after JSON value"));
    }
    Ok(value)
}

pub(in crate::value) struct JsonParser {
    chars: Vec<char>,
    pos: usize,
    line: u32,
    col: u32,
}

impl JsonParser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }
    fn error(&self, message: &str) -> JsonParseError {
        JsonParseError {
            message: message.to_string(),
            line: self.line,
            column: self.col,
        }
    }
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.bump();
        }
    }
    fn parse_value(&mut self, depth: u32) -> Result<JsonValue, JsonParseError> {
        if depth > JSON_MAX_DEPTH {
            return Err(self.error("JSON nesting exceeds the depth limit"));
        }
        match self.peek() {
            None => Err(self.error("unexpected end of input")),
            Some('n') => self.parse_lit("null", JsonValue::Null),
            Some('t') => self.parse_lit("true", JsonValue::Bool(true)),
            Some('f') => self.parse_lit("false", JsonValue::Bool(false)),
            Some('"') => Ok(JsonValue::String(Rc::from(self.parse_string()?.as_str()))),
            Some('[') => self.parse_array(depth),
            Some('{') => self.parse_object(depth),
            Some(c) if c == '-' || c.is_ascii_digit() => self.parse_number(),
            Some(c) => Err(self.error(&format!("unexpected character `{c}`"))),
        }
    }
    fn parse_lit(&mut self, word: &str, val: JsonValue) -> Result<JsonValue, JsonParseError> {
        for expected in word.chars() {
            if self.bump() != Some(expected) {
                return Err(self.error(&format!("invalid literal, expected `{word}`")));
            }
        }
        Ok(val)
    }
    fn parse_string(&mut self) -> Result<String, JsonParseError> {
        self.bump(); // opening quote
        let mut s = String::new();
        loop {
            match self.bump() {
                None => return Err(self.error("unterminated string")),
                Some('"') => return Ok(s),
                Some('\\') => match self.bump() {
                    Some('"') => s.push('"'),
                    Some('\\') => s.push('\\'),
                    Some('/') => s.push('/'),
                    Some('b') => s.push('\u{0008}'),
                    Some('f') => s.push('\u{000C}'),
                    Some('n') => s.push('\n'),
                    Some('r') => s.push('\r'),
                    Some('t') => s.push('\t'),
                    Some('u') => {
                        let cp = self.parse_hex4()?;
                        if (0xD800..=0xDBFF).contains(&cp) {
                            if self.bump() != Some('\\') || self.bump() != Some('u') {
                                return Err(self.error("expected a low surrogate"));
                            }
                            let lo = self.parse_hex4()?;
                            if !(0xDC00..=0xDFFF).contains(&lo) {
                                return Err(self.error("invalid low surrogate"));
                            }
                            let c = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                            match char::from_u32(c) {
                                Some(ch) => s.push(ch),
                                None => return Err(self.error("invalid surrogate pair")),
                            }
                        } else if (0xDC00..=0xDFFF).contains(&cp) {
                            return Err(self.error("unexpected low surrogate"));
                        } else {
                            match char::from_u32(cp) {
                                Some(ch) => s.push(ch),
                                None => return Err(self.error("invalid \\u escape")),
                            }
                        }
                    }
                    _ => return Err(self.error("invalid string escape")),
                },
                Some(c) if (c as u32) < 0x20 => {
                    return Err(self.error("unescaped control character in string"));
                }
                Some(c) => s.push(c),
            }
        }
    }
    fn parse_hex4(&mut self) -> Result<u32, JsonParseError> {
        let mut v = 0u32;
        for _ in 0..4 {
            match self.bump() {
                Some(c) if c.is_ascii_hexdigit() => v = v * 16 + c.to_digit(16).unwrap(),
                _ => return Err(self.error("invalid \\u escape")),
            }
        }
        Ok(v)
    }
    fn parse_number(&mut self) -> Result<JsonValue, JsonParseError> {
        let start = self.pos;
        if self.peek() == Some('-') {
            self.bump();
        }
        match self.peek() {
            Some('0') => {
                self.bump();
                if matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                    return Err(self.error("leading zeros are not allowed"));
                }
            }
            Some(c) if c.is_ascii_digit() => {
                while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                    self.bump();
                }
            }
            _ => return Err(self.error("invalid number")),
        }
        let mut is_float = false;
        if self.peek() == Some('.') {
            is_float = true;
            self.bump();
            if !matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                return Err(self.error("expected digits after the decimal point"));
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.bump();
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            is_float = true;
            self.bump();
            if matches!(self.peek(), Some('+' | '-')) {
                self.bump();
            }
            if !matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                return Err(self.error("expected digits in the exponent"));
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.bump();
            }
        }
        let lexeme: String = self.chars[start..self.pos].iter().collect();
        let _ = is_float;
        // The exact i64 value when the number denotes an integer in range, by exact
        // decimal analysis (handles `1e10`, `1.0`, rejects `1.5`); else None.
        let int = json_exact_int(&lexeme);
        Ok(JsonValue::Number(JsonNumber {
            lexeme: Rc::from(lexeme.as_str()),
            int,
        }))
    }
    fn parse_array(&mut self, depth: u32) -> Result<JsonValue, JsonParseError> {
        self.bump(); // [
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(']') {
            self.bump();
            return Ok(JsonValue::Array(Rc::from(items)));
        }
        loop {
            self.skip_ws();
            items.push(self.parse_value(depth + 1)?);
            self.skip_ws();
            match self.bump() {
                Some(',') => {}
                Some(']') => return Ok(JsonValue::Array(Rc::from(items))),
                _ => return Err(self.error("expected `,` or `]` in array")),
            }
        }
    }
    fn parse_object(&mut self, depth: u32) -> Result<JsonValue, JsonParseError> {
        self.bump(); // {
        let mut map: std::collections::BTreeMap<Rc<str>, JsonValue> =
            std::collections::BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some('}') {
            self.bump();
            return Ok(JsonValue::Object(Rc::new(map)));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some('"') {
                return Err(self.error("expected a string key in object"));
            }
            let key: Rc<str> = Rc::from(self.parse_string()?.as_str());
            self.skip_ws();
            if self.bump() != Some(':') {
                return Err(self.error("expected `:` after the object key"));
            }
            self.skip_ws();
            let value = self.parse_value(depth + 1)?;
            if map.contains_key(&key) {
                return Err(self.error("duplicate object key"));
            }
            map.insert(key, value);
            self.skip_ws();
            match self.bump() {
                Some(',') => {}
                Some('}') => return Ok(JsonValue::Object(Rc::new(map))),
                _ => return Err(self.error("expected `,` or `}` in object")),
            }
        }
    }
}

/// §22 `JSON.parse(text)` builtin: the SHARED entry both engines call. A string
/// arg parses to `Ok(Value::Json(tree))`; invalid JSON is a VALUE error
/// `Err({message, line, column})` (1-based), NOT a runtime fault. A non-string
/// arg faults (reachable only under `--unchecked`).
pub fn builtin_json_parse(arg: Value, span: Span) -> Result<Value, RtError> {
    let text = match &arg {
        Value::Str(s) => s.clone(),
        other => {
            return Err(fault(
                codes::GUARD_TYPE,
                format!("`JSON.parse` takes a string; got `{}` (§22)", other.kind()),
                span,
            ));
        }
    };
    Ok(match json_parse(&text) {
        Ok(tree) => Value::Ok(Rc::new(Value::Json(Rc::new(tree)))),
        Err(e) => {
            let mut fields = std::collections::BTreeMap::new();
            fields.insert("column".to_string(), Value::Int(e.column as i64));
            fields.insert("line".to_string(), Value::Int(e.line as i64));
            fields.insert("message".to_string(), Value::str(e.message));
            Value::Err(Rc::new(Value::Record(Rc::new(fields))))
        }
    })
}
