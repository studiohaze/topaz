//! A host with no ambient capability.
//!
//! The host admits only two explicit per-call channels: immutable submitted
//! input and bounded captured output. It never delegates to the native host
//! and never touches files, environment, network, processes, databases, or an
//! ambient clock.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::fmt;

pub use topaz_value::{Host, HostDirEntry, ResourceId, Value};

pub const DENIAL_PREFIX: &str = "no-capability host denies";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoCapabilityLimits {
    pub input_bytes: usize,
    pub stdout_bytes: usize,
    pub deferred_error_bytes: usize,
    pub logical_clock_step_millis: u64,
}

impl NoCapabilityLimits {
    pub const fn new(
        input_bytes: usize,
        stdout_bytes: usize,
        deferred_error_bytes: usize,
        logical_clock_step_millis: u64,
    ) -> Self {
        Self {
            input_bytes,
            stdout_bytes,
            deferred_error_bytes,
            logical_clock_step_millis,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoCapabilityHostError {
    InputLimit { actual: usize, limit: usize },
    LogicalClockStepZero,
}

impl fmt::Display for NoCapabilityHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputLimit { actual, limit } => {
                write!(
                    formatter,
                    "submitted input exceeds no-capability host limit: {actual} > {limit} bytes"
                )
            }
            Self::LogicalClockStepZero => {
                write!(formatter, "logical scheduler clock step must be nonzero")
            }
        }
    }
}

impl std::error::Error for NoCapabilityHostError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoCapabilitySnapshot {
    pub stdout: Vec<String>,
    pub deferred_errors: Vec<String>,
    pub denied_operations: Vec<&'static str>,
    pub stdout_limit_exceeded: bool,
    pub deferred_error_limit_exceeded: bool,
}

#[derive(Debug)]
pub struct NoCapabilityHost {
    input: String,
    limits: NoCapabilityLimits,
    logical_now: Cell<u64>,
    state: RefCell<CapturedState>,
}

#[derive(Debug, Default)]
struct CapturedState {
    stdout: Vec<String>,
    stdout_bytes: usize,
    deferred_errors: Vec<String>,
    deferred_error_bytes: usize,
    denied_operations: BTreeSet<&'static str>,
    stdout_limit_exceeded: bool,
    deferred_error_limit_exceeded: bool,
}

impl NoCapabilityHost {
    pub fn try_new(
        input: impl Into<String>,
        limits: NoCapabilityLimits,
    ) -> Result<Self, NoCapabilityHostError> {
        let input = input.into();
        if input.len() > limits.input_bytes {
            return Err(NoCapabilityHostError::InputLimit {
                actual: input.len(),
                limit: limits.input_bytes,
            });
        }
        if limits.logical_clock_step_millis == 0 {
            return Err(NoCapabilityHostError::LogicalClockStepZero);
        }
        Ok(Self {
            input,
            limits,
            logical_now: Cell::new(0),
            state: RefCell::new(CapturedState::default()),
        })
    }

    pub fn snapshot(&self) -> NoCapabilitySnapshot {
        let state = self.state.borrow();
        NoCapabilitySnapshot {
            stdout: state.stdout.clone(),
            deferred_errors: state.deferred_errors.clone(),
            denied_operations: state.denied_operations.iter().copied().collect(),
            stdout_limit_exceeded: state.stdout_limit_exceeded,
            deferred_error_limit_exceeded: state.deferred_error_limit_exceeded,
        }
    }

    fn record_denial(&self, operation: &'static str) {
        self.state.borrow_mut().denied_operations.insert(operation);
    }

    fn denied(&self, operation: &'static str) -> String {
        self.record_denial(operation);
        format!("{DENIAL_PREFIX} `{operation}`")
    }

    fn record_close_denial(&self) {
        self.state.borrow_mut().denied_operations.insert("close");
    }
}

fn capture_line(
    lines: &mut Vec<String>,
    used_bytes: &mut usize,
    limit: usize,
    exceeded: &mut bool,
    line: &str,
) {
    if *exceeded {
        return;
    }
    let required = line.len().saturating_add(1);
    let Some(next) = used_bytes.checked_add(required) else {
        *exceeded = true;
        return;
    };
    if next > limit {
        *exceeded = true;
        return;
    }
    lines.push(line.to_string());
    *used_bytes = next;
}

impl Host for NoCapabilityHost {
    fn print(&self, line: &str) {
        let mut state = self.state.borrow_mut();
        let CapturedState {
            stdout,
            stdout_bytes,
            stdout_limit_exceeded,
            ..
        } = &mut *state;
        capture_line(
            stdout,
            stdout_bytes,
            self.limits.stdout_bytes,
            stdout_limit_exceeded,
            line,
        );
    }

    fn open(&self, _path: &str) -> Result<ResourceId, String> {
        Err(self.denied("open"))
    }

    fn read(&self, _handle: ResourceId) -> Result<String, String> {
        Err(self.denied("read"))
    }

    fn write(&self, _handle: ResourceId, _text: &str) -> Result<(), String> {
        Err(self.denied("write"))
    }

    fn close(&self, _handle: ResourceId) {
        self.record_close_denial();
    }

    fn now_millis(&self) -> u64 {
        let now = self.logical_now.get();
        self.logical_now
            .set(now.saturating_add(self.limits.logical_clock_step_millis));
        now
    }

    fn defer_error(&self, rendered: &str) {
        let mut state = self.state.borrow_mut();
        let CapturedState {
            deferred_errors,
            deferred_error_bytes,
            deferred_error_limit_exceeded,
            ..
        } = &mut *state;
        capture_line(
            deferred_errors,
            deferred_error_bytes,
            self.limits.deferred_error_bytes,
            deferred_error_limit_exceeded,
            rendered,
        );
    }

    fn lispex_application(
        &self,
        _request: topaz_value::LispexApplicationRequest,
    ) -> topaz_value::LispexApplicationResponse {
        self.record_denial("lispex_application");
        topaz_value::LispexApplicationResponse::OperationalFault {
            code: "target-unavailable".into(),
            detail: None,
        }
    }

    fn input(&self) -> String {
        self.input.clone()
    }

    fn read_bytes(&self, _path: &str) -> Result<Vec<u8>, String> {
        Err(self.denied("read_bytes"))
    }

    fn write_bytes(&self, _path: &str, _bytes: &[u8]) -> Result<(), String> {
        Err(self.denied("write_bytes"))
    }

    fn list_dir(&self, _path: &str) -> Result<Vec<HostDirEntry>, String> {
        Err(self.denied("list_dir"))
    }

    fn extern_call(
        &self,
        _module: &str,
        _function: &str,
        _args: &[Value],
    ) -> Result<Value, String> {
        Err(self.denied("extern_call"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use topaz_diag::{FileId, Span};
    use topaz_interp::Machine;
    use topaz_parser::{ParseOptions, parse_with_options};
    use topaz_syntax::LangVersion;

    const LIMITS: NoCapabilityLimits = NoCapabilityLimits::new(64, 12, 12, 2);

    fn host(input: &str) -> NoCapabilityHost {
        NoCapabilityHost::try_new(input, LIMITS).expect("bounded host")
    }

    #[test]
    fn input_is_explicit_and_bounded() {
        let host = host("hello");
        assert_eq!(host.input(), "hello");
        assert_eq!(host.input(), "hello");
        let error = NoCapabilityHost::try_new("too long", NoCapabilityLimits::new(3, 1, 1, 1))
            .expect_err("input limit");
        assert_eq!(
            error,
            NoCapabilityHostError::InputLimit {
                actual: 8,
                limit: 3
            }
        );
        assert_eq!(
            NoCapabilityHost::try_new("", NoCapabilityLimits::new(1, 1, 1, 0))
                .expect_err("logical scheduler progress"),
            NoCapabilityHostError::LogicalClockStepZero
        );
    }

    #[test]
    fn every_effect_method_is_explicitly_denied() {
        let host = host("");
        let handle = ResourceId(9);
        assert_eq!(
            host.open("secret").unwrap_err(),
            "no-capability host denies `open`"
        );
        assert_eq!(
            host.read(handle).unwrap_err(),
            "no-capability host denies `read`"
        );
        assert_eq!(
            host.write(handle, "x").unwrap_err(),
            "no-capability host denies `write`"
        );
        host.close(handle);
        assert_eq!(
            host.read_bytes("secret").unwrap_err(),
            "no-capability host denies `read_bytes`"
        );
        assert_eq!(
            host.write_bytes("secret", b"x").unwrap_err(),
            "no-capability host denies `write_bytes`"
        );
        assert_eq!(
            host.list_dir(".").unwrap_err(),
            "no-capability host denies `list_dir`"
        );
        assert_eq!(
            host.extern_call("m", "f", &[]).unwrap_err(),
            "no-capability host denies `extern_call`"
        );
        assert_eq!(
            host.lispex_application(topaz_value::LispexApplicationRequest::Rule {
                target_identity: "rule".into(),
            }),
            topaz_value::LispexApplicationResponse::OperationalFault {
                code: "target-unavailable".into(),
                detail: None,
            }
        );
        assert_eq!(
            host.snapshot().denied_operations,
            vec![
                "close",
                "extern_call",
                "lispex_application",
                "list_dir",
                "open",
                "read",
                "read_bytes",
                "write",
                "write_bytes"
            ]
        );
    }

    #[test]
    fn captured_channels_fail_closed_at_byte_limits() {
        let host = host("");
        host.print("hello");
        host.print("");
        host.print("world");
        host.defer_error("first");
        host.defer_error("second");
        let snapshot = host.snapshot();
        assert_eq!(snapshot.stdout, vec!["hello", ""]);
        assert!(snapshot.stdout_limit_exceeded);
        assert_eq!(snapshot.deferred_errors, vec!["first"]);
        assert!(snapshot.deferred_error_limit_exceeded);

        host.print("");
        let snapshot = host.snapshot();
        assert_eq!(snapshot.stdout, vec!["hello", ""]);
        assert!(snapshot.stdout_limit_exceeded);
    }

    #[test]
    fn scheduler_clock_is_logical_and_instance_local() {
        let first = host("");
        let second = host("");
        assert_eq!((first.now_millis(), first.now_millis()), (0, 2));
        assert_eq!(second.now_millis(), 0);
    }

    #[test]
    fn interpreter_observes_the_no_capability_boundary() {
        let source = "FS.readText(\"secret.txt\")";
        let parsed = parse_with_options(
            FileId(0),
            source,
            ParseOptions {
                language_version: LangVersion::V5_16,
            },
        );
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let host = host("");
        let result = Machine::new(source, &host)
            .run_program(&parsed.program)
            .expect("denial is a Topaz Result value");
        let Value::Err(message) = result else {
            panic!("expected Err result, found {result:?}");
        };
        let Value::Str(message) = message.as_ref() else {
            panic!("expected string error, found {message:?}");
        };
        assert_eq!(&**message, "no-capability host denies `open`");
    }

    #[test]
    fn emitted_runtime_interface_observes_the_same_denial() {
        let host = host("");
        let result = topaz_rt::builtin_fs_read_text(
            &host,
            Value::str("secret.txt"),
            Span::new(FileId(0), 0, 1),
        )
        .expect("denial is a Topaz Result value");
        let Value::Err(message) = result else {
            panic!("expected Err result, found {result:?}");
        };
        let Value::Str(message) = message.as_ref() else {
            panic!("expected string error, found {message:?}");
        };
        assert_eq!(&**message, "no-capability host denies `open`");
    }
}
