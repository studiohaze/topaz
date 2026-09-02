use topaz_check::UnitModule;
use topaz_diag::{Diagnostic, FileId, has_errors};
use topaz_host_none::{NoCapabilityHost, NoCapabilityLimits};
use topaz_interp::{Machine, render};
use topaz_parser::{ParseOptions, parse_with_options};
use topaz_syntax::LangVersion;

use crate::protocol::{WorkerDiagnostic, WorkerRequest, WorkerResponse, WorkerStatus};

const HOST_LIMITS: NoCapabilityLimits = NoCapabilityLimits::new(65_536, 262_144, 262_144, 1);

pub fn execute(request: WorkerRequest) -> WorkerResponse {
    let parsed = parse_with_options(
        FileId(0),
        &request.source,
        ParseOptions {
            language_version: LangVersion::V5_17,
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
    let checked = topaz_check::check_unit_typed_with_version(&unit, LangVersion::V5_17);
    if has_errors(&checked.diagnostics) {
        return static_rejected(&checked.diagnostics);
    }

    let host = match NoCapabilityHost::try_new(&request.input, HOST_LIMITS) {
        Ok(host) => host,
        Err(error) => {
            return WorkerResponse {
                status: WorkerStatus::ProtocolRejected,
                value: String::new(),
                diagnostics: vec![WorkerDiagnostic {
                    code: "TOPAZ-EXECUTION-INPUT".to_string(),
                    message: error.to_string(),
                    lo: 0,
                    hi: 0,
                }],
                stdout: Vec::new(),
                deferred_errors: Vec::new(),
            };
        }
    };
    let result = Machine::new(&request.source, &host).run_program(&parsed.program);
    let snapshot = host.snapshot();
    if snapshot.stdout_limit_exceeded || snapshot.deferred_error_limit_exceeded {
        return WorkerResponse {
            status: WorkerStatus::HostLimit,
            value: if snapshot.stdout_limit_exceeded {
                "stdout"
            } else {
                "deferred-errors"
            }
            .to_string(),
            diagnostics: Vec::new(),
            stdout: Vec::new(),
            deferred_errors: Vec::new(),
        };
    }

    match result {
        Ok(value) => WorkerResponse {
            status: WorkerStatus::Success,
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
            stdout: snapshot.stdout,
            deferred_errors: snapshot.deferred_errors,
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
    fn worker_checks_then_evaluates_with_no_capability_host() {
        let success = execute(WorkerRequest {
            source: "let 값 = 40 + 2\n값".to_string(),
            input: String::new(),
        });
        assert_eq!(success.status, WorkerStatus::Success);
        assert_eq!(success.value, "42");

        let denied = execute(WorkerRequest {
            source: "FS.readText(\"secret.txt\")".to_string(),
            input: String::new(),
        });
        assert_eq!(denied.status, WorkerStatus::Success);
        assert_eq!(denied.value, "Err(no-capability host denies `open`)");
    }
}
