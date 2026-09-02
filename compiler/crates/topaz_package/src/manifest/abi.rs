use crate::*;

/// Parses the closed extern ABI type grammar.
pub fn parse_abi_type(raw: &str) -> Result<AbiType, PackageError> {
    parse_abi_type_field("extern ABI type", raw)
}

pub(super) fn parse_abi_type_field(field: &str, raw: &str) -> Result<AbiType, PackageError> {
    let mut parser = AbiTypeParser::new(field, raw);
    let ty = parser.parse_type()?;
    parser.skip_ws();
    if !parser.is_done() {
        return Err(PackageError::new(format!(
            "{field} has trailing text in extern ABI type `{raw}`"
        )));
    }
    Ok(ty)
}

struct AbiTypeParser<'a> {
    field: &'a str,
    raw: &'a str,
    pos: usize,
}

impl<'a> AbiTypeParser<'a> {
    fn new(field: &'a str, raw: &'a str) -> Self {
        Self { field, raw, pos: 0 }
    }

    fn parse_type(&mut self) -> Result<AbiType, PackageError> {
        self.skip_ws();
        if self.consume_byte(b'(') {
            self.skip_ws();
            self.expect_byte(b')')?;
            return Ok(AbiType::Unit);
        }
        let ident = self.parse_ident()?;
        match ident.as_str() {
            "bool" => Ok(AbiType::Bool),
            "int" => Ok(AbiType::Int),
            "float" => Ok(AbiType::Float),
            "string" => Ok(AbiType::String),
            "Bytes" => Ok(AbiType::Bytes),
            "Array" => Ok(AbiType::Array(Box::new(self.parse_unary_ctor("Array")?))),
            "Option" => Ok(AbiType::Option(Box::new(self.parse_unary_ctor("Option")?))),
            "Result" => {
                let mut args = self.parse_ctor_args("Result")?;
                if args.len() != 2 {
                    return Err(PackageError::new(format!(
                        "{} Result ABI type expects 2 arguments, got {}",
                        self.field,
                        args.len()
                    )));
                }
                let err = args.pop().expect("arity checked");
                let ok = args.pop().expect("arity checked");
                Ok(AbiType::Result(Box::new(ok), Box::new(err)))
            }
            other => Err(PackageError::new(format!(
                "{} uses unsupported extern ABI type `{other}`",
                self.field
            ))),
        }
    }

    fn parse_unary_ctor(&mut self, ctor: &str) -> Result<AbiType, PackageError> {
        let mut args = self.parse_ctor_args(ctor)?;
        if args.len() != 1 {
            return Err(PackageError::new(format!(
                "{} {ctor} ABI type expects 1 argument, got {}",
                self.field,
                args.len()
            )));
        }
        Ok(args.pop().expect("arity checked"))
    }

    fn parse_ctor_args(&mut self, ctor: &str) -> Result<Vec<AbiType>, PackageError> {
        self.skip_ws();
        if !self.consume_byte(b'<') {
            return Err(PackageError::new(format!(
                "{} {ctor} ABI type must include type arguments",
                self.field
            )));
        }
        let mut args = Vec::new();
        loop {
            args.push(self.parse_type()?);
            self.skip_ws();
            if self.consume_byte(b',') {
                continue;
            }
            self.expect_byte(b'>')?;
            break;
        }
        Ok(args)
    }

    fn parse_ident(&mut self) -> Result<String, PackageError> {
        self.skip_ws();
        let start = self.pos;
        let Some(first) = self.peek_byte() else {
            return Err(PackageError::new(format!(
                "{} must be a non-empty extern ABI type",
                self.field
            )));
        };
        if !is_ident_start(first) {
            return Err(PackageError::new(format!(
                "{} has malformed extern ABI type `{}`",
                self.field, self.raw
            )));
        }
        self.pos += 1;
        while self.peek_byte().is_some_and(is_ident_continue) {
            self.pos += 1;
        }
        Ok(self.raw[start..self.pos].to_string())
    }

    fn skip_ws(&mut self) {
        while self.peek_byte().is_some_and(|b| b.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    fn expect_byte(&mut self, expected: u8) -> Result<(), PackageError> {
        self.skip_ws();
        if self.consume_byte(expected) {
            return Ok(());
        }
        Err(PackageError::new(format!(
            "{} has malformed extern ABI type `{}`",
            self.field, self.raw
        )))
    }

    fn consume_byte(&mut self, expected: u8) -> bool {
        if self.peek_byte() == Some(expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn peek_byte(&self) -> Option<u8> {
        self.raw.as_bytes().get(self.pos).copied()
    }

    fn is_done(&self) -> bool {
        self.pos == self.raw.len()
    }
}

pub(super) fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

pub(super) fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}
