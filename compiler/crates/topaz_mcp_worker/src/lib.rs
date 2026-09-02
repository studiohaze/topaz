//! No-capability worker for the target-scoped Topaz MCP execution product.
//!
//! The worker accepts exactly one framed request on stdin and emits exactly
//! one framed response on stdout. It has no component loader, discovery,
//! callback, native-host, logging, or persistent-state path.

pub mod protocol;

use topaz_check::UnitModule;
use topaz_diag::{Diagnostic, FileId, has_errors};
use topaz_host_none::{NoCapabilityHost, NoCapabilityLimits};
use topaz_interp::{Machine, render};
use topaz_parser::{ParseOptions, parse_with_options};
use topaz_syntax::LangVersion;

use protocol::{WorkerDiagnostic, WorkerRequest, WorkerResponse, WorkerStatus};

const HOST_LIMITS: NoCapabilityLimits = NoCapabilityLimits::new(65_536, 262_144, 262_144, 1);

pub fn execute(request: WorkerRequest) -> WorkerResponse {
    let parsed = parse_with_options(
        FileId(0),
        &request.source,
        ParseOptions {
            language_version: LangVersion::V5_18,
        },
    );
    if has_errors(&parsed.diagnostics) {
        return static_rejected(&parsed.diagnostics);
    }

    let unit = [UnitModule {
        identity: "main".to_string(),
        is_entry: true,
        is_extern: false,
        is_generated_std: false,
        extern_replay_error: None,
        src: &request.source,
        program: &parsed.program,
    }];
    let checked = topaz_check::check_unit_typed_with_version(&unit, LangVersion::V5_18);
    if has_errors(&checked.diagnostics) {
        return static_rejected(&checked.diagnostics);
    }

    let host = match NoCapabilityHost::try_new(&request.input, HOST_LIMITS) {
        Ok(host) => host,
        Err(_) => return WorkerResponse::protocol_rejected("explicit input exceeds worker policy"),
    };
    let result = Machine::new(&request.source, &host).run_program(&parsed.program);
    let snapshot = host.snapshot();
    if snapshot.stdout_limit_exceeded {
        return WorkerResponse::host_limit("stdout");
    }
    if snapshot.deferred_error_limit_exceeded {
        return WorkerResponse::host_limit("deferred-errors");
    }

    match result {
        Ok(value) => WorkerResponse {
            status: WorkerStatus::Completed,
            value: render(&value),
            diagnostics: Vec::new(),
            stdout: snapshot.stdout,
            deferred_errors: snapshot.deferred_errors,
        },
        Err(error) => WorkerResponse {
            status: WorkerStatus::RuntimeRejected,
            value: String::new(),
            diagnostics: vec![WorkerDiagnostic {
                code: error.code.to_string(),
                message: error.message,
                lo: error.span.lo,
                hi: error.span.hi,
            }],
            stdout: Vec::new(),
            deferred_errors: Vec::new(),
        },
    }
}

fn static_rejected(diagnostics: &[Diagnostic]) -> WorkerResponse {
    WorkerResponse {
        status: WorkerStatus::StaticRejected,
        value: String::new(),
        diagnostics: diagnostics
            .iter()
            .map(|diagnostic| WorkerDiagnostic {
                code: diagnostic.code.as_str().to_string(),
                message: diagnostic.message.clone(),
                lo: diagnostic.primary.span.lo,
                hi: diagnostic.primary.span.hi,
            })
            .collect(),
        stdout: Vec::new(),
        deferred_errors: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_then_runs_with_fresh_no_capability_host() {
        let success = execute(WorkerRequest {
            source: "let 값 = 40 + 2\n값".to_string(),
            input: String::new(),
        });
        assert_eq!(success.status, WorkerStatus::Completed);
        assert_eq!(success.value, "42");

        let denied = execute(WorkerRequest {
            source: "FS.readText(\"secret.txt\")".to_string(),
            input: String::new(),
        });
        assert_eq!(denied.status, WorkerStatus::Completed);
        assert_eq!(denied.value, "Err(no-capability host denies `open`)");
    }

    #[test]
    fn runtime_failure_discards_provisional_output() {
        let failed = execute(WorkerRequest {
            source: "print(\"secret marker\")\n1 / 0".to_string(),
            input: String::new(),
        });
        assert_eq!(failed.status, WorkerStatus::RuntimeRejected);
        assert!(failed.value.is_empty());
        assert!(failed.stdout.is_empty());
        assert!(failed.deferred_errors.is_empty());
    }
}
