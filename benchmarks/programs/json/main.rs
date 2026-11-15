use std::collections::HashMap;
use std::fs;

enum Json {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Json>),
    Object(HashMap<String, Json>),
}

// Rust's standard library has no JSON codec. This ordinary recursive-descent
// parser is part of the workload and its complete source counts toward LOC.
struct Parser {
    chars: Vec<char>,
    position: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.position).copied()
    }

    fn whitespace(&mut self) {
        while matches!(self.peek(), Some(' ' | '\n' | '\r' | '\t')) {
            self.position += 1;
        }
    }

    fn take(&mut self, expected: char) -> Result<(), String> {
        if self.peek() != Some(expected) {
            return Err(format!("expected {expected} at {}", self.position));
        }
        self.position += 1;
        Ok(())
    }

    fn keyword(&mut self, word: &str) -> Result<(), String> {
        for character in word.chars() {
            self.take(character)?;
        }
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let mut value = 0;
        for _ in 0..4 {
            let digit = self.peek().and_then(|c| c.to_digit(16)).ok_or("invalid unicode escape")?;
            value = value * 16 + digit;
            self.position += 1;
        }
        Ok(value)
    }

    fn string(&mut self) -> Result<String, String> {
        self.take('"')?;
        let mut output = String::new();
        loop {
            let character = self.peek().ok_or("unfinished string")?;
            self.position += 1;
            match character {
                '"' => return Ok(output),
                '\\' => {
                    let escape = self.peek().ok_or("unfinished escape")?;
                    self.position += 1;
                    match escape {
                        '"' | '\\' | '/' => output.push(escape),
                        'b' => output.push('\u{0008}'),
                        'f' => output.push('\u{000c}'),
                        'n' => output.push('\n'),
                        'r' => output.push('\r'),
                        't' => output.push('\t'),
                        'u' => {
                            let first = self.hex4()?;
                            let scalar = if (0xd800..=0xdbff).contains(&first) {
                                self.take('\\')?;
                                self.take('u')?;
                                let second = self.hex4()?;
                                if !(0xdc00..=0xdfff).contains(&second) {
                                    return Err("invalid surrogate pair".into());
                                }
                                0x10000 + (first - 0xd800) * 0x400 + second - 0xdc00
                            } else {
                                first
                            };
                            output.push(char::from_u32(scalar).ok_or("invalid unicode scalar")?);
                        }
                        _ => return Err("invalid escape".into()),
                    }
                }
                c if c < '\u{0020}' => return Err("unescaped control character".into()),
                c => output.push(c),
            }
        }
    }

    fn digits(&mut self) -> Result<(), String> {
        let start = self.position;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.position += 1;
        }
        if self.position == start {
            return Err("expected digit".into());
        }
        Ok(())
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.position;
        if self.peek() == Some('-') {
            self.position += 1;
        }
        if self.peek() == Some('0') {
            self.position += 1;
        } else {
            self.digits()?;
        }
        if self.peek() == Some('.') {
            self.position += 1;
            self.digits()?;
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.position += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.position += 1;
            }
            self.digits()?;
        }
        Ok(Json::Number(self.chars[start..self.position].iter().collect()))
    }

    fn value(&mut self) -> Result<Json, String> {
        self.whitespace();
        match self.peek() {
            Some('n') => { self.keyword("null")?; Ok(Json::Null) }
            Some('t') => { self.keyword("true")?; Ok(Json::Bool(true)) }
            Some('f') => { self.keyword("false")?; Ok(Json::Bool(false)) }
            Some('"') => Ok(Json::String(self.string()?)),
            Some('[') => {
                self.position += 1;
                self.whitespace();
                let mut values = Vec::new();
                if self.peek() != Some(']') {
                    loop {
                        values.push(self.value()?);
                        self.whitespace();
                        if self.peek() != Some(',') { break; }
                        self.position += 1;
                    }
                }
                self.take(']')?;
                Ok(Json::Array(values))
            }
            Some('{') => {
                self.position += 1;
                self.whitespace();
                let mut values = HashMap::new();
                if self.peek() != Some('}') {
                    loop {
                        self.whitespace();
                        let key = self.string()?;
                        self.whitespace();
                        self.take(':')?;
                        values.insert(key, self.value()?);
                        self.whitespace();
                        if self.peek() != Some(',') { break; }
                        self.position += 1;
                    }
                }
                self.take('}')?;
                Ok(Json::Object(values))
            }
            Some('-' | '0'..='9') => self.number(),
            _ => Err(format!("expected JSON value at {}", self.position)),
        }
    }
}

fn parse(text: &str) -> Result<Json, String> {
    let mut parser = Parser { chars: text.chars().collect(), position: 0 };
    let value = parser.value()?;
    parser.whitespace();
    if parser.position != parser.chars.len() {
        return Err("trailing data".into());
    }
    Ok(value)
}

fn main() {
    let mut totals = [0i64; 4];
    for _repeat in 0..20 {
        let text = fs::read_to_string("input.json").expect("read input.json");
        let document = parse(&text).expect("valid JSON input");
        let Json::Array(rows) = document else { panic!("expected array") };
        for row in rows {
            let Json::Object(fields) = row else { panic!("expected object") };
            let Json::String(name) = &fields["name"] else { panic!("expected name") };
            let Json::Number(amount) = &fields["amount"] else { panic!("expected amount") };
            let Json::Bool(active) = &fields["active"] else { panic!("expected active") };
            let Json::Array(tags) = &fields["tags"] else { panic!("expected tags") };
            let amount: i64 = amount.parse().expect("integer amount");
            if *active {
                totals[0] += 1;
                totals[1] += amount;
                totals[2] += name.trim().chars().count() as i64;
                totals[3] += tags.len() as i64;
            }
        }
    }
    let encoded: Vec<String> = totals.iter().map(|value| value.to_string()).collect();
    println!("bench-v1:[{}]", encoded.join(","));
}
