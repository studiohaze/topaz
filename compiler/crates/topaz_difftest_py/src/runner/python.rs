use crate::trace::{PyTrace, parse_trace_v1};
use crate::*;

pub(crate) fn run_python_batch(
    python: &Path,
    tmp: &Path,
    script: &Path,
    cases: &[Case],
) -> Result<BTreeMap<String, PyTrace>, String> {
    let runner = tmp.join("run_batch.py");
    fs::write(&runner, PY_BATCH_RUNNER).map_err(|e| format!("write batch runner: {e}"))?;
    let input = tmp.join("batch-input.tsv");
    let mut input_text = String::new();
    for case in cases {
        input_text.push_str(&case.name);
        input_text.push('\t');
        input_text.push_str(&hex_encode(case.input.as_bytes()));
        input_text.push('\n');
    }
    fs::write(&input, input_text).map_err(|e| format!("write Python batch input: {e}"))?;

    let output = Command::new(python)
        .arg("-u")
        .arg(&runner)
        .arg(script)
        .arg(&input)
        .env("PYTHONHASHSEED", "0")
        .env("LC_ALL", "C")
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("run Python batch: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Python batch exited nonzero\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    if !output.stderr.is_empty() {
        return Err(format!(
            "Python batch wrote stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stdout =
        String::from_utf8(output.stdout).map_err(|e| format!("utf8 Python stdout: {e}"))?;
    let mut traces = BTreeMap::new();
    for line in stdout.lines() {
        let (name, trace) = line
            .split_once('\t')
            .ok_or_else(|| format!("malformed Python batch line: {line:?}"))?;
        let parsed = parse_trace_v1(trace).map_err(|e| format!("{name}: {e}"))?;
        traces.insert(name.to_string(), parsed);
    }
    if traces.len() != cases.len() {
        return Err(format!(
            "Python trace count mismatch: expected {}, got {}",
            cases.len(),
            traces.len()
        ));
    }
    Ok(traces)
}

pub(crate) fn run_python_once_with_files(
    python: &Path,
    tmp: &Path,
    script: &Path,
    stdin_text: &str,
    files: &BTreeMap<String, String>,
) -> Result<PyTrace, String> {
    let runner = tmp.join("run_with_files.py");
    fs::write(&runner, PY_FILE_RUNNER).map_err(|e| format!("write file runner: {e}"))?;
    let files_input = tmp.join("files-input.tsv");
    let mut files_text = String::new();
    for (path, content) in files {
        files_text.push_str(&hex_encode(path.as_bytes()));
        files_text.push('\t');
        files_text.push_str(&hex_encode(content.as_bytes()));
        files_text.push('\n');
    }
    fs::write(&files_input, files_text).map_err(|e| format!("write files input: {e}"))?;

    let output = Command::new(python)
        .arg("-u")
        .arg(&runner)
        .arg(script)
        .arg(hex_encode(stdin_text.as_bytes()))
        .arg(&files_input)
        .env("PYTHONHASHSEED", "0")
        .env("LC_ALL", "C")
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("run Python file runner: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Python file runner exited nonzero\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    if !output.stderr.is_empty() {
        return Err(format!(
            "Python file runner wrote stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stdout =
        String::from_utf8(output.stdout).map_err(|e| format!("utf8 Python stdout: {e}"))?;
    let line = stdout
        .trim_end_matches('\n')
        .lines()
        .next()
        .ok_or_else(|| "Python file runner produced no trace".to_string())?;
    parse_trace_v1(line)
}

const PY_FILE_RUNNER: &str = r#"
from __future__ import annotations

import importlib.util
import sys

sys.dont_write_bytecode = True

script = sys.argv[1]
stdin_text = bytes.fromhex(sys.argv[2]).decode("utf-8")
files_path = sys.argv[3]
spec = importlib.util.spec_from_file_location("topaz_candidate", script)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)

files = {}
with open(files_path, "r", encoding="utf-8") as files_input:
    for raw in files_input:
        raw = raw.rstrip("\n")
        if not raw:
            continue
        path_hex, content_hex = raw.split("\t", 1)
        files[bytes.fromhex(path_hex).decode("utf-8")] = bytes.fromhex(content_hex).decode("utf-8")

sys.stdout.write(module.run(stdin_text, files))
sys.stdout.write("\n")
"#;

const PY_BATCH_RUNNER: &str = r#"
from __future__ import annotations

import importlib.util
import sys

sys.dont_write_bytecode = True

script = sys.argv[1]
input_path = sys.argv[2]
spec = importlib.util.spec_from_file_location("topaz_candidate", script)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)

with open(input_path, "r", encoding="utf-8") as input_file:
    for raw in input_file:
        raw = raw.rstrip("\n")
        if not raw:
            continue
        name, hex_input = raw.split("\t", 1)
        stdin_text = bytes.fromhex(hex_input).decode("utf-8")
        trace = module.run(stdin_text)
        sys.stdout.write(name + "\t" + trace + "\n")
"#;

pub(crate) fn cpython_31314() -> PathBuf {
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
            return candidate;
        }
    }
    panic!("CPython 3.13.14 with cache tag cpython-313 is required for topaz_difftest_py")
}
pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

pub(crate) fn decode_hex(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0, "hex input length");
    let bytes = hex.as_bytes();
    let mut out = Vec::with_capacity(hex.len() / 2);
    let mut i = 0usize;
    while i < bytes.len() {
        let hi = hex_nibble(bytes[i]);
        let lo = hex_nibble(bytes[i + 1]);
        out.push((hi << 4) | lo);
        i += 2;
    }
    out
}

pub(crate) fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => panic!("invalid hex byte {byte}"),
    }
}
