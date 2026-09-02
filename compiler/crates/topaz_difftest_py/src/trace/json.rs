use crate::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum JsonValue {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<JsonValue>),
    Object(BTreeMap<String, JsonValue>),
}

pub(super) struct JsonParser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> JsonParser<'a> {
    pub(super) fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    pub(super) fn parse(mut self) -> Result<JsonValue, String> {
        let value = self.value()?;
        self.skip_ws();
        self.finish()?;
        Ok(value)
    }

    fn value(&mut self) -> Result<JsonValue, String> {
        self.skip_ws();
        match self.peek_char() {
            Some('"') => self.string().map(JsonValue::String),
            Some('{') => self.object(),
            Some('[') => self.array(),
            Some('t') => {
                self.expect("true")?;
                Ok(JsonValue::Bool(true))
            }
            Some('f') => {
                self.expect("false")?;
                Ok(JsonValue::Bool(false))
            }
            Some('n') => {
                self.expect("null")?;
                Ok(JsonValue::Null)
            }
            Some('-' | '0'..='9') => self.number().map(JsonValue::Number),
            Some(other) => Err(format!(
                "unexpected JSON value at byte {}: {other:?}",
                self.pos
            )),
            None => Err("unexpected end of JSON".to_string()),
        }
    }

    fn object(&mut self) -> Result<JsonValue, String> {
        self.expect("{")?;
        let mut out = BTreeMap::new();
        self.skip_ws();
        if self.consume("}") {
            return Ok(JsonValue::Object(out));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.expect(":")?;
            let value = self.value()?;
            if out.insert(key.clone(), value).is_some() {
                return Err(format!("duplicate JSON object key {key:?}"));
            }
            self.skip_ws();
            if self.consume("}") {
                return Ok(JsonValue::Object(out));
            }
            self.expect(",")?;
        }
    }

    fn array(&mut self) -> Result<JsonValue, String> {
        self.expect("[")?;
        let mut out = Vec::new();
        self.skip_ws();
        if self.consume("]") {
            return Ok(JsonValue::Array(out));
        }
        loop {
            out.push(self.value()?);
            self.skip_ws();
            if self.consume("]") {
                return Ok(JsonValue::Array(out));
            }
            self.expect(",")?;
        }
    }

    fn number(&mut self) -> Result<String, String> {
        let start = self.pos;
        self.consume("-");
        match self.peek_char() {
            Some('0') => {
                self.pos += 1;
                if matches!(self.peek_char(), Some('0'..='9')) {
                    return Err(format!("invalid leading zero at byte {start}"));
                }
            }
            Some('1'..='9') => {
                self.pos += 1;
                while matches!(self.peek_char(), Some('0'..='9')) {
                    self.pos += 1;
                }
            }
            _ => return Err(format!("invalid JSON number at byte {start}")),
        }
        if self.consume(".") {
            let frac_start = self.pos;
            while matches!(self.peek_char(), Some('0'..='9')) {
                self.pos += 1;
            }
            if self.pos == frac_start {
                return Err(format!("missing JSON number fraction at byte {frac_start}"));
            }
        }
        if matches!(self.peek_char(), Some('e' | 'E')) {
            self.pos += 1;
            if matches!(self.peek_char(), Some('+' | '-')) {
                self.pos += 1;
            }
            let exp_start = self.pos;
            while matches!(self.peek_char(), Some('0'..='9')) {
                self.pos += 1;
            }
            if self.pos == exp_start {
                return Err(format!("missing JSON number exponent at byte {exp_start}"));
            }
        }
        Ok(self.input[start..self.pos].to_string())
    }

    fn expect(&mut self, expected: &str) -> Result<(), String> {
        if self.input[self.pos..].starts_with(expected) {
            self.pos += expected.len();
            Ok(())
        } else {
            Err(format!(
                "expected {expected:?} at byte {}, got {:?}",
                self.pos,
                &self.input[self.pos..self.input.len().min(self.pos + 40)]
            ))
        }
    }

    fn consume(&mut self, expected: &str) -> bool {
        if self.input[self.pos..].starts_with(expected) {
            self.pos += expected.len();
            true
        } else {
            false
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect("\"")?;
        let mut out = String::new();
        while self.pos < self.input.len() {
            let ch = self.next_char()?;
            match ch {
                '"' => return Ok(out),
                '\\' => {
                    let escaped = self.next_char()?;
                    match escaped {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{0008}'),
                        'f' => out.push('\u{000c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let scalar = self.hex4()?;
                            let Some(ch) = char::from_u32(scalar) else {
                                return Err(format!("invalid unicode escape {scalar:#x}"));
                            };
                            out.push(ch);
                        }
                        other => return Err(format!("unsupported string escape {other:?}")),
                    }
                }
                other if other <= '\u{001f}' => {
                    return Err(format!("unescaped control character {other:?}"));
                }
                other => out.push(other),
            }
        }
        Err("unterminated JSON string".to_string())
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn next_char(&mut self) -> Result<char, String> {
        let Some(ch) = self.input[self.pos..].chars().next() else {
            return Err("unexpected end of JSON".to_string());
        };
        self.pos += ch.len_utf8();
        Ok(ch)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek_char(), Some(' ' | '\n' | '\r' | '\t')) {
            self.pos += 1;
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let end = self.pos + 4;
        let Some(hex) = self.input.get(self.pos..end) else {
            return Err("short unicode escape".to_string());
        };
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("invalid unicode escape {hex:?}"));
        }
        self.pos = end;
        u32::from_str_radix(hex, 16).map_err(|e| format!("parse unicode escape: {e}"))
    }

    fn finish(&self) -> Result<(), String> {
        if self.pos == self.input.len() {
            Ok(())
        } else {
            Err(format!(
                "trailing JSON bytes at {}: {:?}",
                self.pos,
                &self.input[self.pos..]
            ))
        }
    }
}
