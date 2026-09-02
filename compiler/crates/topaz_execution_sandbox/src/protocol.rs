use std::fmt;
use std::io::{self, Read, Write};

pub const MAX_SOURCE_BYTES: usize = 1_048_576;
pub const MAX_INPUT_BYTES: usize = 65_536;
pub const MAX_RESPONSE_BYTES: usize = 1_048_576;
pub const MAX_PROTOCOL_ITEMS: usize = 1_024;

const REQUEST_MAGIC: &[u8; 8] = b"TPZX3S1\n";
const RESPONSE_MAGIC: &[u8; 8] = b"TPZX3R1\n";
const PROBE_REQUEST_MAGIC: &[u8; 8] = b"TPZX3P1\n";
const PROBE_RESPONSE_MAGIC: &[u8; 8] = b"TPZX3Q1\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    Io(String),
    InvalidMagic,
    InvalidUtf8,
    Limit {
        field: &'static str,
        actual: usize,
        limit: usize,
    },
    InvalidTag(u8),
    TrailingBytes,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(f, "protocol I/O failed: {message}"),
            Self::InvalidMagic => f.write_str("protocol magic is invalid"),
            Self::InvalidUtf8 => f.write_str("protocol text is not UTF-8"),
            Self::Limit {
                field,
                actual,
                limit,
            } => write!(
                f,
                "protocol field `{field}` exceeds {limit} bytes or items (got {actual})"
            ),
            Self::InvalidTag(tag) => write!(f, "protocol tag {tag} is invalid"),
            Self::TrailingBytes => f.write_str("protocol frame has trailing bytes"),
        }
    }
}

impl std::error::Error for ProtocolError {}

impl From<io::Error> for ProtocolError {
    fn from(value: io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRequest {
    pub source: String,
    pub input: String,
}

impl WorkerRequest {
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        let mut frame = Vec::new();
        frame.extend_from_slice(REQUEST_MAGIC);
        put_string(&mut frame, "source", &self.source, MAX_SOURCE_BYTES)?;
        put_string(&mut frame, "input", &self.input, MAX_INPUT_BYTES)?;
        Ok(frame)
    }

    pub fn read_from(reader: &mut impl Read) -> Result<Self, ProtocolError> {
        expect_magic(reader, REQUEST_MAGIC)?;
        let source = read_string(reader, "source", MAX_SOURCE_BYTES)?;
        let input = read_string(reader, "input", MAX_INPUT_BYTES)?;
        ensure_eof(reader)?;
        Ok(Self { source, input })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerDiagnostic {
    pub code: String,
    pub message: String,
    pub lo: u32,
    pub hi: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerStatus {
    Success,
    StaticRejected,
    RuntimeRejected,
    HostLimit,
    ProtocolRejected,
}

impl WorkerStatus {
    fn tag(&self) -> u8 {
        match self {
            Self::Success => 0,
            Self::StaticRejected => 1,
            Self::RuntimeRejected => 2,
            Self::HostLimit => 3,
            Self::ProtocolRejected => 4,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, ProtocolError> {
        match tag {
            0 => Ok(Self::Success),
            1 => Ok(Self::StaticRejected),
            2 => Ok(Self::RuntimeRejected),
            3 => Ok(Self::HostLimit),
            4 => Ok(Self::ProtocolRejected),
            _ => Err(ProtocolError::InvalidTag(tag)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerResponse {
    pub status: WorkerStatus,
    pub value: String,
    pub diagnostics: Vec<WorkerDiagnostic>,
    pub stdout: Vec<String>,
    pub deferred_errors: Vec<String>,
}

impl WorkerResponse {
    pub fn protocol_rejected(message: impl Into<String>) -> Self {
        Self {
            status: WorkerStatus::ProtocolRejected,
            value: String::new(),
            diagnostics: vec![WorkerDiagnostic {
                code: "TOPAZ-EXECUTION-PROTOCOL".to_string(),
                message: message.into(),
                lo: 0,
                hi: 0,
            }],
            stdout: Vec::new(),
            deferred_errors: Vec::new(),
        }
    }

    pub fn write_to(&self, writer: &mut impl Write) -> Result<(), ProtocolError> {
        let mut payload = Vec::new();
        payload.push(self.status.tag());
        put_string(&mut payload, "value", &self.value, MAX_RESPONSE_BYTES)?;
        put_diagnostics(&mut payload, &self.diagnostics)?;
        put_strings(&mut payload, "stdout", &self.stdout)?;
        put_strings(&mut payload, "deferred_errors", &self.deferred_errors)?;
        if payload.len() > MAX_RESPONSE_BYTES {
            return Err(ProtocolError::Limit {
                field: "response",
                actual: payload.len(),
                limit: MAX_RESPONSE_BYTES,
            });
        }
        writer.write_all(RESPONSE_MAGIC)?;
        put_u32(writer, payload.len(), "response")?;
        writer.write_all(&payload)?;
        Ok(())
    }

    pub fn read_from(reader: &mut impl Read) -> Result<Self, ProtocolError> {
        expect_magic(reader, RESPONSE_MAGIC)?;
        let payload_len = read_u32(reader)? as usize;
        if payload_len > MAX_RESPONSE_BYTES {
            return Err(ProtocolError::Limit {
                field: "response",
                actual: payload_len,
                limit: MAX_RESPONSE_BYTES,
            });
        }
        let mut payload = vec![0; payload_len];
        reader.read_exact(&mut payload)?;
        ensure_eof(reader)?;
        let mut cursor = io::Cursor::new(payload);
        let mut tag = [0];
        cursor.read_exact(&mut tag)?;
        let status = WorkerStatus::from_tag(tag[0])?;
        let value = read_string(&mut cursor, "value", MAX_RESPONSE_BYTES)?;
        let diagnostics = read_diagnostics(&mut cursor)?;
        let stdout = read_strings(&mut cursor, "stdout")?;
        let deferred_errors = read_strings(&mut cursor, "deferred_errors")?;
        ensure_eof(&mut cursor)?;
        Ok(Self {
            status,
            value,
            diagnostics,
            stdout,
            deferred_errors,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeRequest {
    pub forbidden_read_path: String,
    pub forbidden_write_path: String,
}

impl ProbeRequest {
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        let mut frame = Vec::new();
        frame.extend_from_slice(PROBE_REQUEST_MAGIC);
        put_string(
            &mut frame,
            "forbidden_read_path",
            &self.forbidden_read_path,
            16_384,
        )?;
        put_string(
            &mut frame,
            "forbidden_write_path",
            &self.forbidden_write_path,
            16_384,
        )?;
        Ok(frame)
    }

    pub fn read_from(reader: &mut impl Read) -> Result<Self, ProtocolError> {
        expect_magic(reader, PROBE_REQUEST_MAGIC)?;
        let forbidden_read_path = read_string(reader, "forbidden_read_path", 16_384)?;
        let forbidden_write_path = read_string(reader, "forbidden_write_path", 16_384)?;
        ensure_eof(reader)?;
        Ok(Self {
            forbidden_read_path,
            forbidden_write_path,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResponse {
    pub inherited_environment_entries: u32,
    pub filesystem_read_denied: bool,
    pub filesystem_write_denied: bool,
    pub network_denied: bool,
    pub child_process_denied: bool,
}

impl ProbeResponse {
    pub fn write_to(&self, writer: &mut impl Write) -> Result<(), ProtocolError> {
        writer.write_all(PROBE_RESPONSE_MAGIC)?;
        writer.write_all(&self.inherited_environment_entries.to_be_bytes())?;
        writer.write_all(&[
            u8::from(self.filesystem_read_denied),
            u8::from(self.filesystem_write_denied),
            u8::from(self.network_denied),
            u8::from(self.child_process_denied),
        ])?;
        Ok(())
    }

    pub fn read_from(reader: &mut impl Read) -> Result<Self, ProtocolError> {
        expect_magic(reader, PROBE_RESPONSE_MAGIC)?;
        let inherited_environment_entries = read_u32(reader)?;
        let mut flags = [0; 4];
        reader.read_exact(&mut flags)?;
        ensure_eof(reader)?;
        if flags.iter().any(|flag| *flag > 1) {
            return Err(ProtocolError::InvalidTag(
                *flags.iter().find(|flag| **flag > 1).unwrap_or(&255),
            ));
        }
        Ok(Self {
            inherited_environment_entries,
            filesystem_read_denied: flags[0] == 1,
            filesystem_write_denied: flags[1] == 1,
            network_denied: flags[2] == 1,
            child_process_denied: flags[3] == 1,
        })
    }
}

fn put_diagnostics(
    writer: &mut impl Write,
    diagnostics: &[WorkerDiagnostic],
) -> Result<(), ProtocolError> {
    put_count(writer, "diagnostics", diagnostics.len())?;
    for diagnostic in diagnostics {
        put_string(writer, "diagnostic.code", &diagnostic.code, 256)?;
        put_string(
            writer,
            "diagnostic.message",
            &diagnostic.message,
            MAX_RESPONSE_BYTES,
        )?;
        writer.write_all(&diagnostic.lo.to_be_bytes())?;
        writer.write_all(&diagnostic.hi.to_be_bytes())?;
    }
    Ok(())
}

fn read_diagnostics(reader: &mut impl Read) -> Result<Vec<WorkerDiagnostic>, ProtocolError> {
    let count = read_count(reader, "diagnostics")?;
    let mut diagnostics = Vec::with_capacity(count);
    for _ in 0..count {
        diagnostics.push(WorkerDiagnostic {
            code: read_string(reader, "diagnostic.code", 256)?,
            message: read_string(reader, "diagnostic.message", MAX_RESPONSE_BYTES)?,
            lo: read_u32(reader)?,
            hi: read_u32(reader)?,
        });
    }
    Ok(diagnostics)
}

fn put_strings(
    writer: &mut impl Write,
    field: &'static str,
    strings: &[String],
) -> Result<(), ProtocolError> {
    put_count(writer, field, strings.len())?;
    for string in strings {
        put_string(writer, field, string, MAX_RESPONSE_BYTES)?;
    }
    Ok(())
}

fn read_strings(reader: &mut impl Read, field: &'static str) -> Result<Vec<String>, ProtocolError> {
    let count = read_count(reader, field)?;
    let mut strings = Vec::with_capacity(count);
    for _ in 0..count {
        strings.push(read_string(reader, field, MAX_RESPONSE_BYTES)?);
    }
    Ok(strings)
}

fn put_count(
    writer: &mut impl Write,
    field: &'static str,
    count: usize,
) -> Result<(), ProtocolError> {
    if count > MAX_PROTOCOL_ITEMS {
        return Err(ProtocolError::Limit {
            field,
            actual: count,
            limit: MAX_PROTOCOL_ITEMS,
        });
    }
    put_u32(writer, count, field)
}

fn read_count(reader: &mut impl Read, field: &'static str) -> Result<usize, ProtocolError> {
    let count = read_u32(reader)? as usize;
    if count > MAX_PROTOCOL_ITEMS {
        return Err(ProtocolError::Limit {
            field,
            actual: count,
            limit: MAX_PROTOCOL_ITEMS,
        });
    }
    Ok(count)
}

fn put_string(
    writer: &mut impl Write,
    field: &'static str,
    value: &str,
    limit: usize,
) -> Result<(), ProtocolError> {
    if value.len() > limit {
        return Err(ProtocolError::Limit {
            field,
            actual: value.len(),
            limit,
        });
    }
    put_u32(writer, value.len(), field)?;
    writer.write_all(value.as_bytes())?;
    Ok(())
}

fn read_string(
    reader: &mut impl Read,
    field: &'static str,
    limit: usize,
) -> Result<String, ProtocolError> {
    let len = read_u32(reader)? as usize;
    if len > limit {
        return Err(ProtocolError::Limit {
            field,
            actual: len,
            limit,
        });
    }
    let mut bytes = vec![0; len];
    reader.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(|_| ProtocolError::InvalidUtf8)
}

fn put_u32(
    writer: &mut impl Write,
    value: usize,
    field: &'static str,
) -> Result<(), ProtocolError> {
    let value = u32::try_from(value).map_err(|_| ProtocolError::Limit {
        field,
        actual: value,
        limit: u32::MAX as usize,
    })?;
    writer.write_all(&value.to_be_bytes())?;
    Ok(())
}

fn read_u32(reader: &mut impl Read) -> Result<u32, ProtocolError> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_be_bytes(bytes))
}

fn expect_magic(reader: &mut impl Read, expected: &[u8; 8]) -> Result<(), ProtocolError> {
    let mut magic = [0; 8];
    reader.read_exact(&mut magic)?;
    if &magic != expected {
        return Err(ProtocolError::InvalidMagic);
    }
    Ok(())
}

fn ensure_eof(reader: &mut impl Read) -> Result<(), ProtocolError> {
    let mut byte = [0];
    match reader.read(&mut byte)? {
        0 => Ok(()),
        _ => Err(ProtocolError::TrailingBytes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_frames_round_trip_without_textual_reencoding() {
        let request = WorkerRequest {
            source: "let 이름 = \"값\"\n이름".to_string(),
            input: "입력".to_string(),
        };
        let encoded = request.encode().expect("request frame");
        assert_eq!(
            WorkerRequest::read_from(&mut encoded.as_slice()).expect("decode"),
            request
        );

        let response = WorkerResponse {
            status: WorkerStatus::StaticRejected,
            value: String::new(),
            diagnostics: vec![WorkerDiagnostic {
                code: "TPZ5001".to_string(),
                message: "잘못된 값".to_string(),
                lo: 2,
                hi: 5,
            }],
            stdout: vec!["줄".to_string()],
            deferred_errors: Vec::new(),
        };
        let mut encoded = Vec::new();
        response.write_to(&mut encoded).expect("response frame");
        assert_eq!(
            WorkerResponse::read_from(&mut encoded.as_slice()).expect("decode"),
            response
        );
    }

    #[test]
    fn oversized_source_fails_before_allocation() {
        let mut frame = REQUEST_MAGIC.to_vec();
        frame.extend_from_slice(&u32::MAX.to_be_bytes());
        assert!(matches!(
            WorkerRequest::read_from(&mut frame.as_slice()),
            Err(ProtocolError::Limit {
                field: "source",
                ..
            })
        ));
    }
}
