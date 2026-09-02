use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value as JsonValue, json};
use topaz_mcp_worker::protocol::{
    MAX_RESPONSE_BYTES, WorkerDiagnostic, WorkerRequest, WorkerResponse, WorkerStatus,
};

const WALL_MILLIS: u64 = 5_000;
const MAXIMUM_ACTIVE_RUNS: usize = 2;
const WORKER_ARGUMENT: &str = "__topaz-mcp-worker";

#[derive(Clone)]
pub struct ExecutionRuntime {
    topaz: PathBuf,
    product_version: String,
}

enum Outcome {
    Response(WorkerResponse),
    WallLimit,
    OutputLimit,
    Cancelled,
    Busy,
    WorkerFault,
    ProtocolFailure,
}

impl ExecutionRuntime {
    pub fn load(topaz: PathBuf, product_version: String) -> Result<Self, String> {
        if !topaz.is_absolute() || !topaz.is_file() {
            return Err("installed Topaz executable is unavailable".to_string());
        }
        Ok(Self {
            topaz,
            product_version,
        })
    }

    pub fn maximum_active_runs(&self) -> usize {
        MAXIMUM_ACTIVE_RUNS
    }

    pub fn metadata(&self) -> &'static str {
        "fresh built-in no-capability worker; 5 s wall and 1 MiB response limits"
    }

    pub fn run(&self, source: String, input: String, cancelled: Arc<AtomicBool>) -> JsonValue {
        let outcome = self.run_inner(source, input, cancelled);
        self.result_json(outcome)
    }

    pub fn busy(&self) -> JsonValue {
        self.result_json(Outcome::Busy)
    }

    fn run_inner(&self, source: String, input: String, cancelled: Arc<AtomicBool>) -> Outcome {
        let frame = match (WorkerRequest { source, input }).encode() {
            Ok(frame) => frame,
            Err(_) => return Outcome::ProtocolFailure,
        };
        let mut child = match Command::new(&self.topaz)
            .arg(WORKER_ARGUMENT)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(_) => return Outcome::WorkerFault,
        };
        let Some(mut stdin) = child.stdin.take() else {
            terminate_and_reap(&mut child);
            return Outcome::WorkerFault;
        };
        if stdin.write_all(&frame).is_err() {
            terminate_and_reap(&mut child);
            return Outcome::WorkerFault;
        }
        drop(stdin);
        let Some(stdout) = child.stdout.take() else {
            terminate_and_reap(&mut child);
            return Outcome::WorkerFault;
        };
        let reader = thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout
                .take((MAX_RESPONSE_BYTES + 13) as u64)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        });
        let deadline = Instant::now() + Duration::from_millis(WALL_MILLIS);
        let status = loop {
            if cancelled.load(Ordering::Acquire) {
                terminate_and_reap(&mut child);
                let _ = reader.join();
                return Outcome::Cancelled;
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                Ok(None) => {
                    terminate_and_reap(&mut child);
                    let _ = reader.join();
                    return Outcome::WallLimit;
                }
                Err(_) => {
                    terminate_and_reap(&mut child);
                    let _ = reader.join();
                    return Outcome::WorkerFault;
                }
            }
        };
        let bytes = match reader.join() {
            Ok(Ok(bytes)) => bytes,
            _ => return Outcome::WorkerFault,
        };
        if bytes.len() > MAX_RESPONSE_BYTES + 12 {
            return Outcome::OutputLimit;
        }
        if !status.success() {
            return Outcome::WorkerFault;
        }
        WorkerResponse::read_from(&mut bytes.as_slice())
            .map(Outcome::Response)
            .unwrap_or(Outcome::ProtocolFailure)
    }

    fn result_json(&self, outcome: Outcome) -> JsonValue {
        let mut result = json!({
            "schema": "topaz.mcp-run-result/v1",
            "semantic_identity": {
                "topaz_product": self.product_version,
                "language_profile": "topaz-5.18"
            },
            "execution_identity": {
                "distribution": "same-installed-topaz-executable",
                "worker": "built-in-hidden-subcommand"
            },
            "component_set": [],
            "fallback": false,
            "source_retained": false,
            "source_logged": false,
            "state_between_calls": "none"
        })
        .as_object()
        .cloned()
        .expect("MCP run base is an object");
        match outcome {
            Outcome::Response(response) => match response.status {
                WorkerStatus::Completed => {
                    result.insert("status".into(), json!("completed"));
                    result.insert("value".into(), json!(response.value));
                    result.insert("stdout".into(), json!(response.stdout));
                    result.insert("deferred_errors".into(), json!(response.deferred_errors));
                }
                WorkerStatus::StaticRejected => {
                    result.insert("status".into(), json!("static-rejected"));
                    result.insert("diagnostics".into(), diagnostics_json(response.diagnostics));
                }
                WorkerStatus::RuntimeRejected => {
                    result.insert("status".into(), json!("runtime-rejected"));
                    result.insert("diagnostics".into(), diagnostics_json(response.diagnostics));
                }
                WorkerStatus::HostLimit => {
                    result.insert("status".into(), json!("host-limit"));
                    result.insert("limit".into(), json!(response.value));
                }
                WorkerStatus::ProtocolRejected => {
                    result.insert("status".into(), json!("protocol-failure"));
                    result.insert("diagnostics".into(), diagnostics_json(response.diagnostics));
                }
            },
            Outcome::WallLimit => set_status(&mut result, "wall-limit"),
            Outcome::OutputLimit => set_status(&mut result, "output-limit"),
            Outcome::Cancelled => set_status(&mut result, "cancelled"),
            Outcome::Busy => set_status(&mut result, "busy"),
            Outcome::WorkerFault => set_status(&mut result, "worker-fault"),
            Outcome::ProtocolFailure => set_status(&mut result, "protocol-failure"),
        }
        JsonValue::Object(result)
    }
}

fn terminate_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn diagnostics_json(diagnostics: Vec<WorkerDiagnostic>) -> JsonValue {
    JsonValue::Array(
        diagnostics
            .into_iter()
            .map(|diagnostic| {
                json!({
                    "code": diagnostic.code,
                    "message": diagnostic.message,
                    "range": {
                        "encoding": "source-byte-offset",
                        "lo": diagnostic.lo,
                        "hi": diagnostic.hi
                    }
                })
            })
            .collect(),
    )
}

fn set_status(result: &mut serde_json::Map<String, JsonValue>, status: &'static str) {
    result.insert("status".into(), json!(status));
}
