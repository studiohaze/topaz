pub(super) use std::io::{Read, Write};
pub(super) use std::net::{TcpListener, TcpStream};
pub(super) use std::path::{Path, PathBuf};
pub(super) use std::process::{Command, Stdio};
#[cfg(target_os = "linux")]
pub(super) use std::time::{Duration, Instant};

pub(super) use serde_json::Value as JsonValue;

pub(super) fn topaz() -> Command {
    Command::new(env!("CARGO_BIN_EXE_topaz"))
}

// Broad pre-5.16 regression cases pin the Rust compiler explicitly. Dedicated
// tests cover default selection and self-compiler refusal, so this helper is
// intentionally local rather than the global test default.
pub(super) fn rust_topaz() -> Command {
    let mut command = topaz();
    command.args(["--compiler", "rust"]);
    command
}

#[cfg(target_os = "linux")]
pub(super) fn direct_child_process_ids(parent: u32) -> Vec<u32> {
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

#[cfg(target_os = "linux")]
pub(super) fn terminate_process_tree(root: u32) {
    for child in direct_child_process_ids(root) {
        let _ = Command::new("kill")
            .args(["-KILL", &child.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

pub(super) fn unused_loopback_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("ephemeral listener")
        .local_addr()
        .expect("listener address")
        .port()
}

pub(super) fn http_get(port: u16, target: &str) -> String {
    http_get_with_started(port, target, None)
}

pub(super) fn http_get_with_started(
    port: u16,
    target: &str,
    started: Option<std::sync::mpsc::Sender<()>>,
) -> String {
    let mut stream = service_connection(port);
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(3)))
        .expect("read timeout");
    write!(
        stream,
        "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .expect("request write");
    if let Some(started) = started {
        started.send(()).expect("request-start notification");
    }
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("response read");
    response
}

pub(super) fn service_connection(port: u16) -> TcpStream {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => break stream,
            Err(error) if std::time::Instant::now() < deadline => {
                let _ = error;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(error) => panic!("service did not accept loopback connections: {error}"),
        }
    }
}

pub(super) fn http_exchange(port: u16, request: &[u8]) -> String {
    let mut stream = service_connection(port);
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(3)))
        .expect("read timeout");
    stream.write_all(request).expect("request write");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("response read");
    response
}

pub(super) fn bounded_socket_response(mut stream: TcpStream, timeout_ms: u64) -> String {
    stream
        .set_read_timeout(Some(std::time::Duration::from_millis(timeout_ms)))
        .expect("bounded read timeout");
    let mut response = Vec::new();
    match stream.read_to_end(&mut response) {
        Ok(_) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) => {}
        Err(error) => panic!("hostile socket read failed: {error}"),
    }
    String::from_utf8_lossy(&response).into_owned()
}

pub(super) fn output_with_stdin(mut cmd: Command, stdin: &[u8]) -> std::process::Output {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary spawns");
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin)
        .expect("stdin write");
    child.wait_with_output().expect("binary runs")
}

pub(super) fn lsp_frame(body: &str) -> String {
    format!("Content-Length: {}\r\n\r\n{body}", body.len())
}

pub(super) fn lsp_bodies(bytes: &[u8]) -> Vec<String> {
    let mut bodies = Vec::new();
    let mut remaining = bytes;
    while !remaining.is_empty() {
        let marker = b"\r\n\r\n";
        let header_end = remaining
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("LSP header terminator");
        let header = std::str::from_utf8(&remaining[..header_end]).expect("LSP header UTF-8");
        let length = header
            .strip_prefix("Content-Length:")
            .expect("LSP content length")
            .trim()
            .parse::<usize>()
            .expect("LSP content length number");
        let body_start = header_end + marker.len();
        let body_end = body_start + length;
        bodies.push(
            std::str::from_utf8(&remaining[body_start..body_end])
                .expect("LSP body UTF-8")
                .to_string(),
        );
        remaining = &remaining[body_end..];
    }
    bodies
}

pub(super) fn repo_root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

pub(super) fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

pub(super) fn cpython_31314() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(path) = std::env::var("TOPAZ_PYTHON_31314") {
        candidates.push(PathBuf::from(path));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin/python3.13"));
    candidates.push(PathBuf::from("python3.13"));
    for candidate in candidates {
        let Ok(output) = Command::new(&candidate)
            .arg("-c")
            .arg("import sys; print(sys.version.split()[0]); print(sys.implementation.cache_tag)")
            .output()
        else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut lines = stdout.lines();
        if lines.next() == Some("3.13.14") && lines.next() == Some("cpython-313") {
            return Some(candidate);
        }
    }
    None
}

pub(super) fn python_311_or_newer() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(path) = std::env::var("TOPAZ_PYTHON") {
        candidates.push(PathBuf::from(path));
    }
    candidates.push(PathBuf::from("python3"));
    candidates.push(PathBuf::from("/opt/homebrew/bin/python3"));
    for candidate in candidates {
        let Ok(output) = Command::new(&candidate)
            .arg("-c")
            .arg("import sys; raise SystemExit(0 if sys.version_info >= (3, 11) else 1)")
            .output()
        else {
            continue;
        };
        if output.status.success() {
            return Some(candidate);
        }
    }
    None
}

pub(super) fn file_uri(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let encoded = normalized.replace('%', "%25").replace(' ', "%20");
    if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

pub(super) fn json_string(value: &str) -> String {
    let mut out = String::from('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => {
                use std::fmt::Write as _;
                write!(out, "\\u{:04x}", u32::from(ch)).expect("write to string");
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

pub(super) fn emit_fixture() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/emit_basic.tpz"
    ))
}

pub(super) fn package_manifest() -> String {
    r#"[package]
name = "pkg_mode"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.4"
"#
    .to_string()
}

pub(super) fn package_lock(manifest: &str) -> String {
    let hash = topaz_package::manifest_sha256(manifest);
    format!(
        r#"[[package]]
name = "pkg_mode"
version = "0.1.0"
source = "root"
manifest_hash = "{hash}"
"#
    )
}
