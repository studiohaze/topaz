//! Live MCP product guarantees, isolated from the compiler-heavy CLI suite so
//! the product's bounded wall-clock check is measured without sibling-test load.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde_json::{Value as JsonValue, json};

fn topaz() -> Command {
    Command::new(env!("CARGO_BIN_EXE_topaz"))
}

static MCP_TEST_NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct McpServer {
    child: Child,
    stdin: Option<ChildStdin>,
    messages: mpsc::Receiver<String>,
    stdout: Arc<Mutex<Vec<u8>>>,
    stderr: Arc<Mutex<Vec<u8>>>,
    stdout_reader: Option<std::thread::JoinHandle<()>>,
    stderr_reader: Option<std::thread::JoinHandle<()>>,
}

impl McpServer {
    fn start(root: &Path) -> Self {
        let mut command = topaz();
        command
            .args(["mcp", "serve"])
            .current_dir(root)
            .env("TMP", root)
            .env("TEMP", root)
            .env("TMPDIR", root)
            .env("HOME", root)
            .env("USERPROFILE", root)
            .env("XDG_CACHE_HOME", root)
            .env("XDG_CONFIG_HOME", root)
            .env("XDG_DATA_HOME", root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("MCP server spawns");
        let stdin = child.stdin.take().expect("MCP stdin");
        let stdout_pipe = child.stdout.take().expect("MCP stdout");
        let stderr_pipe = child.stderr.take().expect("MCP stderr");
        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let (messages_tx, messages) = mpsc::channel();
        let stdout_capture = Arc::clone(&stdout);
        let stdout_reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout_pipe).lines() {
                let line = line.expect("MCP stdout is UTF-8 lines");
                {
                    let mut capture = stdout_capture.lock().expect("stdout capture lock");
                    capture.extend_from_slice(line.as_bytes());
                    capture.push(b'\n');
                }
                if messages_tx.send(line).is_err() {
                    break;
                }
            }
        });
        let stderr_capture = Arc::clone(&stderr);
        let stderr_reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            BufReader::new(stderr_pipe)
                .read_to_end(&mut bytes)
                .expect("MCP stderr read");
            *stderr_capture.lock().expect("stderr capture lock") = bytes;
        });
        let mut server = Self {
            child,
            stdin: Some(stdin),
            messages,
            stdout,
            stderr,
            stdout_reader: Some(stdout_reader),
            stderr_reader: Some(stderr_reader),
        };
        server.send(json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "topaz-cli-test", "version": "1" }
            }
        }));
        // Initialization inspects the installed toolchain and compiler image.
        // Keep that transport deadline separate from the short per-request
        // deadlines exercised below.
        let initialized = server.receive(Duration::from_secs(120));
        assert_eq!(initialized.get("id").and_then(JsonValue::as_u64), Some(0));
        server.send(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }));
        server
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn send(&mut self, message: JsonValue) {
        let stdin = self.stdin.as_mut().expect("MCP stdin remains open");
        serde_json::to_writer(&mut *stdin, &message).expect("MCP request JSON");
        stdin.write_all(b"\n").expect("MCP request delimiter");
        stdin.flush().expect("MCP request flush");
    }

    fn call(&mut self, id: u64, source: &str) {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": "topaz_run",
                "arguments": { "source": source }
            }
        }));
    }

    fn check(&mut self, id: u64, source: &str, profile: Option<&str>) {
        let arguments = match profile {
            Some(profile) => json!({ "source": source, "profile": profile }),
            None => json!({ "source": source }),
        };
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": "topaz_check",
                "arguments": arguments
            }
        }));
    }

    fn cancel(&mut self, id: u64) {
        self.send(json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": { "requestId": id, "reason": "integration test" }
        }));
    }

    fn receive(&mut self, timeout: Duration) -> JsonValue {
        let line = self
            .messages
            .recv_timeout(timeout)
            .unwrap_or_else(|error| panic!("MCP response within {timeout:?}: {error}"));
        serde_json::from_str(&line)
            .unwrap_or_else(|error| panic!("MCP response JSON: {error}: {line}"))
    }

    fn finish(mut self) -> (Vec<u8>, Vec<u8>) {
        drop(self.stdin.take());
        let status = self.child.wait().expect("MCP server exits");
        assert!(status.success(), "MCP server status: {status}");
        self.stdout_reader
            .take()
            .expect("stdout reader")
            .join()
            .expect("stdout reader joins");
        self.stderr_reader
            .take()
            .expect("stderr reader")
            .join()
            .expect("stderr reader joins");
        let stdout = self.stdout.lock().expect("stdout capture lock").clone();
        let stderr = self.stderr.lock().expect("stderr capture lock").clone();
        (stdout, stderr)
    }
}

impl Drop for McpServer {
    fn drop(&mut self) {
        drop(self.stdin.take());
        if matches!(self.child.try_wait(), Ok(None)) {
            terminate_process_tree(self.child.id());
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(reader) = self.stdout_reader.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.stderr_reader.take() {
            let _ = reader.join();
        }
    }
}

fn mcp_result(response: &JsonValue) -> &JsonValue {
    response
        .pointer("/result/structuredContent")
        .unwrap_or_else(|| panic!("MCP tool response has structured content: {response}"))
}

fn mcp_status(response: &JsonValue) -> &str {
    mcp_result(response)
        .get("status")
        .and_then(JsonValue::as_str)
        .unwrap_or_else(|| panic!("MCP run result has status: {response}"))
}

#[cfg(windows)]
fn direct_child_process_ids(parent: u32) -> Vec<u32> {
    let query = format!(
        "Get-CimInstance Win32_Process -Filter 'ParentProcessId = {parent}' | \
         Select-Object -ExpandProperty ProcessId"
    );
    let output = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &query,
        ])
        .output()
        .expect("PowerShell process query runs");
    assert!(output.status.success(), "process query: {output:?}");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

#[cfg(windows)]
fn terminate_process_tree(root: u32) {
    let _ = Command::new("taskkill.exe")
        .args(["/PID", &root.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(unix)]
fn direct_child_process_ids(parent: u32) -> Vec<u32> {
    let output = Command::new("ps")
        .args(["-e", "-o", "pid=", "-o", "ppid="])
        .output()
        .expect("ps process query runs");
    assert!(output.status.success(), "process query: {output:?}");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse::<u32>().ok()?;
            let ppid = fields.next()?.parse::<u32>().ok()?;
            (ppid == parent).then_some(pid)
        })
        .collect()
}

#[cfg(unix)]
fn terminate_process_tree(root: u32) {
    for child in direct_child_process_ids(root) {
        let _ = Command::new("kill")
            .args(["-KILL", &child.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn wait_for_direct_children(parent: u32, expected: usize) -> Vec<u32> {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut observed = Vec::new();
    let mut maximum_observed = Vec::new();
    while Instant::now() < deadline {
        observed = direct_child_process_ids(parent);
        if observed.len() > maximum_observed.len() {
            maximum_observed.clone_from(&observed);
        }
        if observed.len() == expected {
            observed.sort_unstable();
            return observed;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    panic!(
        "MCP server {parent} has {expected} direct workers; maximum observed {maximum_observed:?}, final observation {observed:?}"
    );
}

fn files_containing(root: &Path, needle: &[u8]) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut matches = Vec::new();
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(&path).expect("scan MCP writable root") {
            let entry = entry.expect("MCP writable-root entry");
            let file_type = entry.file_type().expect("MCP writable-root file type");
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                let bytes = std::fs::read(entry.path()).expect("read MCP writable-root file");
                if bytes.windows(needle.len()).any(|window| window == needle) {
                    matches.push(entry.path());
                }
            }
        }
    }
    matches.sort();
    matches
}

#[test]
fn mcp_live_server_enforces_execution_guarantees() {
    let nonce = MCP_TEST_NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "topaz_mcp_guarantees_{}_{}",
        std::process::id(),
        nonce
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create isolated MCP writable root");
    std::fs::write(
        root.join("route-secret.txt"),
        "native fallback exposed secret",
    )
    .expect("write refused-route probe");
    let mut server = McpServer::start(&root);

    let source_marker = format!("mcp_source_probe_{}_{}", std::process::id(), nonce);
    let marked_source = format!("let {source_marker} = 40 + 2\n{source_marker}");
    server.call(1, &marked_source);
    let marked = server.receive(Duration::from_secs(8));
    assert_eq!(marked.get("id").and_then(JsonValue::as_u64), Some(1));
    assert_eq!(mcp_status(&marked), "completed");
    assert_eq!(
        mcp_result(&marked).get("value").and_then(JsonValue::as_str),
        Some("42")
    );

    let check_source_marker = format!("mcp_check_source_probe_{}_{}", std::process::id(), nonce);
    server.check(4, &format!("let {check_source_marker}: int = 42\n"), None);
    let checked = server.receive(Duration::from_secs(8));
    assert_eq!(checked.get("id").and_then(JsonValue::as_u64), Some(4));
    assert_eq!(mcp_status(&checked), "clean");
    assert_eq!(
        mcp_result(&checked)
            .get("source_retained")
            .and_then(JsonValue::as_bool),
        Some(false)
    );
    assert_eq!(
        mcp_result(&checked)
            .get("source_logged")
            .and_then(JsonValue::as_bool),
        Some(false)
    );

    let profiled_check_source_marker = format!(
        "mcp_profiled_check_source_probe_{}_{}",
        std::process::id(),
        nonce
    );
    server.check(
        5,
        &format!("let {profiled_check_source_marker}: int = 42\n"),
        Some("agent-pack"),
    );
    let profiled_checked = server.receive(Duration::from_secs(8));
    assert_eq!(
        profiled_checked.get("id").and_then(JsonValue::as_u64),
        Some(5)
    );
    assert_eq!(mcp_status(&profiled_checked), "clean");
    assert_eq!(
        mcp_result(&profiled_checked)
            .get("source_retained")
            .and_then(JsonValue::as_bool),
        Some(false)
    );
    assert_eq!(
        mcp_result(&profiled_checked)
            .get("source_logged")
            .and_then(JsonValue::as_bool),
        Some(false)
    );

    let state_name = format!("mcp_call_state_{}_{}", std::process::id(), nonce);
    server.call(2, &format!("let {state_name} = 73\n{state_name}"));
    let defined = server.receive(Duration::from_secs(8));
    assert_eq!(defined.get("id").and_then(JsonValue::as_u64), Some(2));
    assert_eq!(mcp_status(&defined), "completed");
    server.call(3, &state_name);
    let isolated = server.receive(Duration::from_secs(8));
    assert_eq!(isolated.get("id").and_then(JsonValue::as_u64), Some(3));
    assert_eq!(
        mcp_status(&isolated),
        "static-rejected",
        "a later call must not resolve a binding created by an earlier call: {isolated}"
    );
    assert!(
        mcp_result(&isolated)
            .get("diagnostics")
            .and_then(JsonValue::as_array)
            .is_some_and(|diagnostics| !diagnostics.is_empty()),
        "undefined cross-call binding must produce a diagnostic: {isolated}"
    );

    let spin = "while true {\n  let spin_value = 1\n}\n0";
    server.call(10, spin);
    let first_worker = wait_for_direct_children(server.pid(), 1);
    server.cancel(10);
    let cancelled = server.receive(Duration::from_secs(3));
    assert_eq!(cancelled.get("id").and_then(JsonValue::as_u64), Some(10));
    assert_eq!(mcp_status(&cancelled), "cancelled");
    assert!(wait_for_direct_children(server.pid(), 0).is_empty());

    server.call(11, spin);
    let second_worker = wait_for_direct_children(server.pid(), 1);
    assert_ne!(
        first_worker, second_worker,
        "successive calls must execute in different worker processes"
    );
    server.cancel(11);
    let cancelled = server.receive(Duration::from_secs(3));
    assert_eq!(cancelled.get("id").and_then(JsonValue::as_u64), Some(11));
    assert_eq!(mcp_status(&cancelled), "cancelled");
    assert!(wait_for_direct_children(server.pid(), 0).is_empty());

    server.call(20, spin);
    server.call(21, spin);
    let active_workers = wait_for_direct_children(server.pid(), 2);
    assert_eq!(
        active_workers.len(),
        2,
        "v2 admits exactly two active execution workers"
    );
    server.call(22, "42");
    let busy = server.receive(Duration::from_secs(2));
    assert_eq!(busy.get("id").and_then(JsonValue::as_u64), Some(22));
    assert_eq!(
        mcp_status(&busy),
        "busy",
        "a third run must be refused instead of queued: {busy}"
    );
    server.cancel(20);
    server.cancel(21);
    let first_cancelled = server.receive(Duration::from_secs(3));
    let second_cancelled = server.receive(Duration::from_secs(3));
    let mut cancelled_ids = [
        first_cancelled
            .get("id")
            .and_then(JsonValue::as_u64)
            .expect("first cancellation response id"),
        second_cancelled
            .get("id")
            .and_then(JsonValue::as_u64)
            .expect("second cancellation response id"),
    ];
    cancelled_ids.sort_unstable();
    assert_eq!(cancelled_ids, [20, 21]);
    assert_eq!(mcp_status(&first_cancelled), "cancelled");
    assert_eq!(mcp_status(&second_cancelled), "cancelled");
    assert!(wait_for_direct_children(server.pid(), 0).is_empty());
    server.call(23, "40 + 2");
    let after_cancellation = server.receive(Duration::from_secs(3));
    assert_eq!(
        after_cancellation.get("id").and_then(JsonValue::as_u64),
        Some(23)
    );
    assert_eq!(
        mcp_status(&after_cancellation),
        "completed",
        "cancellation must release execution capacity: {after_cancellation}"
    );
    assert_eq!(
        mcp_result(&after_cancellation)
            .get("value")
            .and_then(JsonValue::as_str),
        Some("42")
    );

    server.call(30, "FS.readText(\"route-secret.txt\")");
    let refused = server.receive(Duration::from_secs(3));
    assert_eq!(refused.get("id").and_then(JsonValue::as_u64), Some(30));
    assert_eq!(mcp_status(&refused), "completed");
    assert_eq!(
        mcp_result(&refused)
            .get("value")
            .and_then(JsonValue::as_str),
        Some("Err(no-capability host denies `open`)"),
        "a refused capability route must not fall back to the native host: {refused}"
    );

    let (stdout, stderr) = server.finish();
    for marker in [
        &source_marker,
        &check_source_marker,
        &profiled_check_source_marker,
    ] {
        assert!(
            !stdout
                .windows(marker.len())
                .any(|window| window == marker.as_bytes()),
            "MCP stdout logged submitted source marker {marker}: {}",
            String::from_utf8_lossy(&stdout)
        );
        assert!(
            !stderr
                .windows(marker.len())
                .any(|window| window == marker.as_bytes()),
            "MCP stderr logged submitted source marker {marker}: {}",
            String::from_utf8_lossy(&stderr)
        );
        let persisted = files_containing(&root, marker.as_bytes());
        assert!(
            persisted.is_empty(),
            "submitted source marker {marker} persisted beneath isolated MCP cwd/temp/home root: {persisted:?}"
        );
    }
    assert!(
        stderr.is_empty(),
        "MCP server stderr: {}",
        String::from_utf8_lossy(&stderr)
    );

    std::fs::remove_dir_all(&root).expect("remove isolated MCP writable root");
}
