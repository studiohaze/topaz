use crate::*;

pub(crate) const MAX_VALUE_DEPTH: usize = 256;
/// An immutable, lossless `lispex.embed-value/v1` carrier.
///
/// Construction validates the complete byte stream before retaining it. The
/// bytes are never projected through JSON or Topaz structural values, so
/// symbols, exact numbers, improper lists, vectors, bytevectors, records, and
/// result multiplicity remain intact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LispexValue {
    canonical: Arc<[u8]>,
}

impl LispexValue {
    /// Validate application-provided canonical input. Host-record values are
    /// admitted by the provider contract on the input side only. The selected
    /// rule's lower `canonical_input_bytes` limit is enforced again by the
    /// evaluation boundary.
    pub fn from_canonical(bytes: impl Into<Vec<u8>>) -> Result<Self, LispexValueError> {
        Self::from_bytes(bytes.into(), true)
    }

    /// Validate a canonical evaluator result. The provider contract refuses
    /// host records on this side of the boundary.
    pub fn from_guest_result(bytes: impl Into<Vec<u8>>) -> Result<Self, LispexValueError> {
        Self::from_bytes(bytes.into(), false)
    }

    fn from_bytes(bytes: Vec<u8>, allow_host_record: bool) -> Result<Self, LispexValueError> {
        if bytes.len() > MAX_CANONICAL_VALUE_BYTES {
            return Err(LispexValueError::new("value-byte-limit"));
        }
        validate_value(&bytes, allow_host_record).map_err(LispexValueError::new)?;
        Ok(Self {
            canonical: Arc::from(bytes),
        })
    }

    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }

    #[must_use]
    pub fn into_canonical_bytes(self) -> Vec<u8> {
        self.canonical.as_ref().to_vec()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Stable canonical-value admission failure identified by a closed code.
pub struct LispexValueError {
    code: &'static str,
}

impl LispexValueError {
    const fn new(code: &'static str) -> Self {
        Self { code }
    }

    #[must_use]
    pub const fn code(self) -> &'static str {
        self.code
    }
}

impl fmt::Display for LispexValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Lispex canonical value refused: {}", self.code)
    }
}

impl std::error::Error for LispexValueError {}

/// Admits one canonical value with an explicit input-side host-record policy.
pub fn validate_value(bytes: &[u8], allow_host_record: bool) -> Result<(), &'static str> {
    let mut cursor = ValueCursor::new(bytes);
    parse_value(&mut cursor, 1, allow_host_record)?;
    if cursor.offset != bytes.len() {
        return Err("value-trailing");
    }
    Ok(())
}

fn parse_value(
    cursor: &mut ValueCursor<'_>,
    depth: usize,
    allow_host_record: bool,
) -> Result<(), &'static str> {
    if depth > MAX_VALUE_DEPTH {
        return Err("value-depth");
    }
    match cursor.byte()? {
        0..=2 => {}
        3 => {
            if !valid_integer(cursor.field()?) {
                return Err("value-integer");
            }
        }
        4 => {
            let numerator = cursor.field()?;
            let denominator = cursor.field()?;
            if !valid_integer(numerator)
                || !valid_positive_integer(denominator)
                || denominator == b"1"
            {
                return Err("value-rational");
            }
            let numerator = BigInt::parse_bytes(numerator, 10).ok_or("value-rational")?;
            let denominator = BigInt::parse_bytes(denominator, 10).ok_or("value-rational")?;
            if numerator.gcd(&denominator) != BigInt::from(1_u8) {
                return Err("value-rational");
            }
        }
        5 => {
            if !f64::from_bits(cursor.u64()?).is_finite() {
                return Err("value-real");
            }
        }
        6 => {
            if char::from_u32(cursor.u32()?).is_none() {
                return Err("value-character");
            }
        }
        7 | 8 => {
            std::str::from_utf8(cursor.field()?).map_err(|_| "value-utf8")?;
        }
        9 | 11 => {
            let count = cursor.count()?;
            for _ in 0..count {
                parse_value(cursor, depth + 1, allow_host_record)?;
            }
        }
        10 => {
            let count = cursor.count()?;
            for _ in 0..count {
                parse_value(cursor, depth + 1, allow_host_record)?;
            }
            parse_value(cursor, depth + 1, allow_host_record)?;
        }
        12 => {
            let _ = cursor.field()?;
        }
        13 => {
            if !allow_host_record {
                return Err("value-host-record-result");
            }
            let count = cursor.count()?;
            let mut previous: Option<Vec<u8>> = None;
            for _ in 0..count {
                let key = cursor.field()?.to_vec();
                if key.is_empty() || std::str::from_utf8(&key).is_err() {
                    return Err("value-record-key");
                }
                if previous.as_ref().is_some_and(|value| value >= &key) {
                    return Err("value-record-order");
                }
                previous = Some(key);
                parse_value(cursor, depth + 1, allow_host_record)?;
            }
        }
        _ => return Err("value-tag"),
    }
    Ok(())
}

fn valid_integer(bytes: &[u8]) -> bool {
    match bytes {
        b"0" => true,
        [b'1'..=b'9', rest @ ..] => rest.iter().all(u8::is_ascii_digit),
        [b'-', b'1'..=b'9', rest @ ..] => rest.iter().all(u8::is_ascii_digit),
        _ => false,
    }
}

fn valid_positive_integer(bytes: &[u8]) -> bool {
    matches!(bytes, [b'1'..=b'9', rest @ ..] if rest.iter().all(u8::is_ascii_digit))
}

struct ValueCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> ValueCursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], &'static str> {
        let end = self.offset.checked_add(length).ok_or("value-overflow")?;
        let value = self.bytes.get(self.offset..end).ok_or("value-truncated")?;
        self.offset = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, &'static str> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, &'static str> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().map_err(|_| "value-u32")?,
        ))
    }

    fn u64(&mut self) -> Result<u64, &'static str> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().map_err(|_| "value-u64")?,
        ))
    }

    fn count(&mut self) -> Result<u64, &'static str> {
        let count = self.u64()?;
        if count > (self.bytes.len() - self.offset) as u64 {
            return Err("value-count");
        }
        Ok(count)
    }

    fn field(&mut self) -> Result<&'a [u8], &'static str> {
        let length = usize::try_from(self.u64()?).map_err(|_| "value-length")?;
        self.take(length)
    }
}

#[must_use]
/// Computes lowercase SHA-256 without adding a scheme prefix.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex_lower(&Sha256::digest(bytes))
}

pub(crate) fn strip_sha256_prefix(value: &str) -> Result<&str, RunError> {
    let Some(value) = value.strip_prefix("sha256:") else {
        return Err(RunError::SelectionRefusal("sha256-prefix"));
    };
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(RunError::SelectionRefusal("sha256-digest"));
    }
    Ok(value)
}

#[must_use]
pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[(byte >> 4) as usize]));
        output.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    output
}
