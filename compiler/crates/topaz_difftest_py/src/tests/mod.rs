//! Python differential coverage grouped by wide-core, module, and badness cases.
//! Common runners live here so every family compares the same outcome, files,
//! stdout, and deferred-error surfaces.

use super::*;
use crate::runner::*;
use crate::trace::*;
use topaz_resolve::InMemoryProvider;
use topaz_value::{Value, values_equal};

#[derive(Debug)]
struct WideRunReceipt {
    outcome: RunOutcome,
    stdout: Vec<String>,
    files: BTreeMap<String, String>,
    defer_errors: Vec<String>,
}

mod badness;
mod module;
mod wide;

fn run_value_trace_cases(group: &str, cases: &[(&str, &str)]) {
    let python = cpython_31314();
    let tmp = temp_dir(&format!("topaz-difftest-py-{group}"));
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");

    for (name, source) in cases {
        let mut provider = InMemoryProvider::new();
        provider.add_file("main.tpz", *source);
        let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::CURRENT);
        assert!(
            unit.diagnostics.is_empty(),
            "{name}: fixture must resolve cleanly: {:?}",
            unit.diagnostics
        );
        let expected = trace_value_from_topaz(
            &topaz_interp::Machine::run_unit(&unit, &topaz_interp::TestHost::new())
                .unwrap_or_else(|error| panic!("{name}: interpreter faulted: {error:?}")),
        );

        let script = tmp.join(format!("{name}.py"));
        fs::write(
            &script,
            emit_module(&unit).expect("fixture emits to Python"),
        )
        .expect("write generated Python");
        let py_case = Case {
            name: (*name).to_string(),
            input: String::new(),
        };
        let traces = run_python_batch(&python, &tmp, &script, &[py_case])
            .unwrap_or_else(|error| panic!("{name}: run Python batch: {error}"));
        let trace = traces.get(*name).expect("trace for case");
        assert_eq!(trace.status, "ok", "{name}: status");
        assert!(trace.stdout.is_empty(), "{name}: stdout");
        assert!(trace.files.is_empty(), "{name}: files");
        assert!(trace.defer_errors.is_empty(), "{name}: defer errors");
        assert_eq!(trace.value, Some(expected), "{name}: trace value");
    }

    let _ = fs::remove_dir_all(&tmp);
}

fn run_transcript_trace_cases(group: &str, cases: &[(&str, &str)]) {
    let python = cpython_31314();
    let tmp = temp_dir(&format!("topaz-difftest-py-{group}"));
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");

    for (name, source) in cases {
        let mut provider = InMemoryProvider::new();
        provider.add_file("main.tpz", *source);
        let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::CURRENT);
        assert!(
            unit.diagnostics.is_empty(),
            "{name}: fixture must resolve cleanly: {:?}",
            unit.diagnostics
        );
        let interp_host = topaz_interp::TestHost::new();
        let expected_value = topaz_interp::Machine::run_unit(&unit, &interp_host)
            .unwrap_or_else(|error| panic!("{name}: interpreter faulted: {error:?}"));
        let expected_trace_value = trace_value_from_topaz(&expected_value);

        let script = tmp.join(format!("{name}.py"));
        fs::write(
            &script,
            emit_module(&unit).expect("fixture emits to Python"),
        )
        .expect("write generated Python");
        let py_case = Case {
            name: (*name).to_string(),
            input: String::new(),
        };
        let traces = run_python_batch(&python, &tmp, &script, &[py_case])
            .unwrap_or_else(|error| panic!("{name}: run Python batch: {error}"));
        let trace = traces.get(*name).expect("trace for case");
        assert_eq!(trace.status, "ok", "{name}: status");
        assert_eq!(trace.stdout, interp_host.stdout(), "{name}: stdout");
        assert!(trace.files.is_empty(), "{name}: files");
        assert_eq!(
            trace
                .defer_errors
                .iter()
                .map(|entry| entry.rendered.as_str())
                .collect::<Vec<_>>(),
            interp_host
                .defer_errors()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            "{name}: defer errors"
        );
        assert_eq!(
            trace.value,
            Some(expected_trace_value),
            "{name}: trace value"
        );
    }

    let _ = fs::remove_dir_all(&tmp);
}

fn run_wide_interpreter(fixture: &WideCoreFixture) -> Result<WideRunReceipt, String> {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", fixture.source);
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::CURRENT);
    if !unit.diagnostics.is_empty() {
        return Err(format!("resolve diagnostics: {:?}", unit.diagnostics));
    }
    let host = topaz_interp::TestHost::new();
    seed_wide_host(fixture.kind, &host);
    let outcome = match topaz_interp::Machine::run_unit(&unit, &host) {
        Ok(value) => RunOutcome::Completed(value),
        Err(error) => RunOutcome::Faulted(error),
    };
    Ok(WideRunReceipt {
        outcome,
        stdout: host.stdout(),
        files: host.files(),
        defer_errors: host.defer_errors(),
    })
}

fn run_wide_boxed_rust(fixture: &WideCoreFixture) -> WideRunReceipt {
    let host = Rc::new(topaz_interp::TestHost::new());
    seed_wide_host(fixture.kind, &host);
    let host_for_run: Rc<dyn Host> = host.clone();
    let outcome = (fixture.run)(host_for_run);
    WideRunReceipt {
        outcome,
        stdout: host.stdout(),
        files: host.files(),
        defer_errors: host.defer_errors(),
    }
}

fn run_wide_python(
    python: &Path,
    tmp: &Path,
    fixture: &WideCoreFixture,
) -> Result<PyTrace, String> {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", fixture.source);
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::CURRENT);
    if !unit.diagnostics.is_empty() {
        return Err(format!("resolve diagnostics: {:?}", unit.diagnostics));
    }
    let script = tmp.join(format!("wide_{}.py", fixture.name));
    fs::write(
        &script,
        emit_module_for_python_witness(&unit, LangVersion::CURRENT)?,
    )
    .map_err(|error| format!("write generated Python: {error}"))?;

    match fixture.kind {
        WideCoreKind::Regular => {
            let case = Case {
                name: fixture.name.to_string(),
                input: String::new(),
            };
            let traces = run_python_batch(python, tmp, &script, &[case])?;
            traces
                .get(fixture.name)
                .cloned()
                .ok_or_else(|| "missing Python trace".to_string())
        }
        WideCoreKind::FileConfig => {
            let mut seed_files = BTreeMap::new();
            seed_files.insert("config.txt".to_string(), "v=1".to_string());
            run_python_once_with_files(python, tmp, &script, "", &seed_files)
        }
    }
}

fn run_module_interpreter(fixture: &ModuleCoreFixture) -> Result<WideRunReceipt, String> {
    run_module_interpreter_with_files(fixture, &BTreeMap::new())
}

fn run_module_interpreter_with_files(
    fixture: &ModuleCoreFixture,
    files: &BTreeMap<String, String>,
) -> Result<WideRunReceipt, String> {
    let mut provider = InMemoryProvider::new();
    for (path, source) in fixture.files {
        provider.add_file(*path, *source);
    }
    let unit = resolve_with_version(&provider, fixture.entry, None, LangVersion::CURRENT);
    if !unit.diagnostics.is_empty() {
        return Err(format!("resolve diagnostics: {:?}", unit.diagnostics));
    }
    let host = topaz_interp::TestHost::new();
    seed_host_files(&host, files);
    let outcome = match topaz_interp::Machine::run_unit(&unit, &host) {
        Ok(value) => RunOutcome::Completed(value),
        Err(error) => RunOutcome::Faulted(error),
    };
    Ok(WideRunReceipt {
        outcome,
        stdout: host.stdout(),
        files: host.files(),
        defer_errors: host.defer_errors(),
    })
}

fn run_module_boxed_rust(fixture: &ModuleCoreFixture) -> WideRunReceipt {
    run_module_boxed_rust_with_files(fixture, &BTreeMap::new())
}

fn run_module_boxed_rust_with_files(
    fixture: &ModuleCoreFixture,
    files: &BTreeMap<String, String>,
) -> WideRunReceipt {
    let host = Rc::new(topaz_interp::TestHost::new());
    seed_host_files(&host, files);
    let host_for_run: Rc<dyn Host> = host.clone();
    let outcome = (fixture.run)(host_for_run);
    WideRunReceipt {
        outcome,
        stdout: host.stdout(),
        files: host.files(),
        defer_errors: host.defer_errors(),
    }
}

fn seed_host_files(host: &topaz_interp::TestHost, files: &BTreeMap<String, String>) {
    for (path, content) in files {
        host.add_file(path.as_str(), content.as_str());
    }
}

fn run_module_python(
    python: &Path,
    tmp: &Path,
    fixture: &ModuleCoreFixture,
) -> Result<PyTrace, String> {
    let mut provider = InMemoryProvider::new();
    for (path, source) in fixture.files {
        provider.add_file(*path, *source);
    }
    let unit = resolve_with_version(&provider, fixture.entry, None, LangVersion::CURRENT);
    if !unit.diagnostics.is_empty() {
        return Err(format!("resolve diagnostics: {:?}", unit.diagnostics));
    }
    let script = tmp.join(format!("module_{}.py", fixture.name));
    fs::write(
        &script,
        emit_module_for_python_witness(&unit, LangVersion::CURRENT)?,
    )
    .map_err(|error| format!("write generated Python: {error}"))?;

    let case = Case {
        name: fixture.name.to_string(),
        input: String::new(),
    };
    let traces = run_python_batch(python, tmp, &script, &[case])?;
    traces
        .get(fixture.name)
        .cloned()
        .ok_or_else(|| "missing Python trace".to_string())
}

fn run_server_contract_python_report(
    python: &Path,
    tmp: &Path,
    script: &Path,
) -> Result<String, String> {
    let adapter = tmp.join("pym23_contract_adapter.py");
    fs::write(&adapter, PY_SERVER_CONTRACT_ADAPTER)
        .map_err(|error| format!("write server contract adapter: {error}"))?;
    let runner = tmp.join("pym23_contract_report.py");
    fs::write(&runner, PY_SERVER_CONTRACT_RUNNER)
        .map_err(|error| format!("write server contract runner: {error}"))?;

    let output = Command::new(python)
        .arg("-u")
        .arg(&runner)
        .arg(script)
        .arg(&adapter)
        .env("PYTHONHASHSEED", "0")
        .env("LC_ALL", "C")
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("run server contract runner: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "server contract runner exited nonzero\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    if !output.stderr.is_empty() {
        return Err(format!(
            "server contract runner wrote stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| format!("utf8 contract report: {error}"))
}

const PY_SERVER_CONTRACT_ADAPTER: &str = r#"
from __future__ import annotations

import json


def _canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def _ok(status, body):
    return {
        "status": "ok",
        "stdout": [_canonical({"status": status, "body": body})],
        "fault": None,
        "value": {"result": {"ok": {"int": status}}},
    }


def _err(message):
    return {
        "status": "ok",
        "stdout": [],
        "fault": None,
        "value": {"result": {"err": {"str": message}}},
    }


def handle(payload):
    try:
        request = json.loads(payload)
    except json.JSONDecodeError as exc:
        return _err(exc.msg)
    if "method" not in request:
        return _err("missing method")
    if "path" not in request:
        return _err("missing path")
    method = request["method"]
    path = request["path"]
    if not isinstance(method, str):
        return _err("method must be string")
    if not isinstance(path, str):
        return _err("path must be string")
    if path == "/health":
        return _ok(200, "ok")
    if path == "/echo":
        return _ok(200, method + " " + path)
    return _ok(404, path)
"#;

const PY_SERVER_CONTRACT_RUNNER: &str = r#"
from __future__ import annotations

import importlib.util
import json
import sys
import traceback


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


program = load_module("topaz_generated_contract", sys.argv[1])
adapter = load_module("handwritten_contract_adapter", sys.argv[2])

cases = [
    ("health", '{"method":"GET","path":"/health"}'),
    ("echo", '{"method":"POST","path":"/echo"}'),
    ("not_found", '{"method":"GET","path":"/missing"}'),
    ("missing_path", '{"method":"GET"}'),
]

rows = []
raw_python_exceptions = 0
passed = 0

for name, payload in cases:
    generated = None
    expected = None
    error = None
    try:
        generated = json.loads(program.run("", {"request.json": payload}))
        expected = adapter.handle(payload)
        generated_contract = {
            "status": generated.get("status"),
            "stdout": generated.get("stdout"),
            "fault": generated.get("fault"),
            "value": generated.get("value"),
        }
        passed_case = generated_contract == expected
        if generated.get("fault") is not None:
            span = generated["fault"].get("span")
            if not isinstance(span, dict) or not {"file", "lo", "hi"} <= set(span):
                passed_case = False
        if passed_case:
            passed += 1
    except Exception:
        raw_python_exceptions += 1
        passed_case = False
        error = traceback.format_exc()
    rows.append(
        {
            "name": name,
            "passed": passed_case,
            "generated": None
            if generated is None
            else {
                "status": generated.get("status"),
                "stdout": generated.get("stdout"),
                "files": generated.get("files"),
                "defer_errors": generated.get("defer_errors"),
                "fault": generated.get("fault"),
                "value": generated.get("value"),
            },
            "adapter": expected,
            "raw_exception": error,
        }
    )

report = {
    "v": 1,
    "demo": "pym23_server_contract_demo",
    "summary": {
        "total": len(cases),
        "passed": passed,
        "failed": len(cases) - passed,
        "raw_python_exceptions": raw_python_exceptions,
    },
    "cases": rows,
}

sys.stdout.write(json.dumps(report, ensure_ascii=False, sort_keys=True, separators=(",", ":")))
sys.stdout.write("\n")
if report["summary"]["failed"] or raw_python_exceptions:
    raise SystemExit(1)
"#;

fn seed_wide_host(kind: WideCoreKind, host: &topaz_interp::TestHost) {
    match kind {
        WideCoreKind::Regular => {}
        WideCoreKind::FileConfig => host.add_file("config.txt", "v=1"),
    }
}

fn compare_wide_receipts(
    name: &str,
    interp: &WideRunReceipt,
    rust: &WideRunReceipt,
    failures: &mut Vec<String>,
) {
    if !run_outcomes_match(&interp.outcome, &rust.outcome) {
        failures.push(format!(
            "{name}: interpreter != boxed Rust outcome\n  interp: {:?}\n  rust:   {:?}",
            interp.outcome, rust.outcome
        ));
    }
    if interp.stdout != rust.stdout {
        failures.push(format!(
            "{name}: interpreter != boxed Rust stdout\n  interp: {:?}\n  rust:   {:?}",
            interp.stdout, rust.stdout
        ));
    }
    if interp.files != rust.files {
        failures.push(format!(
            "{name}: interpreter != boxed Rust files\n  interp: {:?}\n  rust:   {:?}",
            interp.files, rust.files
        ));
    }
    if interp.defer_errors != rust.defer_errors {
        failures.push(format!(
            "{name}: interpreter != boxed Rust defer errors\n  interp: {:?}\n  rust:   {:?}",
            interp.defer_errors, rust.defer_errors
        ));
    }
}

fn compare_python_trace_to_wide_receipt(
    fixture: &WideCoreFixture,
    trace: &PyTrace,
    expected: &WideRunReceipt,
    failures: &mut Vec<String>,
) {
    compare_python_trace_to_receipt(
        fixture.name,
        matches!(fixture.kind, WideCoreKind::Regular),
        trace,
        expected,
        failures,
    );
}

fn compare_python_trace_to_receipt(
    name: &str,
    expect_final_value: bool,
    trace: &PyTrace,
    expected: &WideRunReceipt,
    failures: &mut Vec<String>,
) {
    match &expected.outcome {
        RunOutcome::Completed(value) => {
            if trace.status != "ok" || trace.fault.is_some() {
                failures.push(format!(
                    "{name}: Python status/fault mismatch for completed run: {:?}",
                    trace
                ));
            }
            if expect_final_value {
                let expected_value = trace_value_from_topaz(value);
                if trace.value != Some(expected_value.clone()) {
                    failures.push(format!(
                        "{name}: Python value mismatch\n  python: {:?}\n  expected: {:?}",
                        trace.value, expected_value
                    ));
                }
            } else if trace.value.is_some() {
                failures.push(format!(
                    "{name}: Python fixture should not report a final value: {:?}",
                    trace.value
                ));
            }
        }
        RunOutcome::Faulted(error) => {
            if trace.status != "fault" || trace.value.is_some() {
                failures.push(format!(
                    "{name}: Python status/value mismatch for faulted run: {:?}",
                    trace
                ));
            }
            match &trace.fault {
                Some(fault) if trace_fault_matches(error, fault) => {}
                Some(fault) => failures.push(format!(
                    "{name}: Python fault mismatch\n  python: {:?}\n  expected: {:?}",
                    fault, error
                )),
                None => failures.push(format!("{name}: Python trace omitted fault")),
            }
        }
    }

    if trace.stdout != expected.stdout {
        failures.push(format!(
            "{name}: Python stdout mismatch\n  python: {:?}\n  expected: {:?}",
            trace.stdout, expected.stdout
        ));
    }

    match trace_file_string_map(&trace.files) {
        Ok(files) if files == expected.files => {}
        Ok(files) => failures.push(format!(
            "{name}: Python files mismatch\n  python: {:?}\n  expected: {:?}",
            files, expected.files
        )),
        Err(error) => failures.push(format!("{name}: Python file trace error: {error}")),
    }

    let py_defer_errors = trace
        .defer_errors
        .iter()
        .map(|entry| entry.rendered.clone())
        .collect::<Vec<_>>();
    if py_defer_errors != expected.defer_errors {
        failures.push(format!(
            "{name}: Python defer errors mismatch\n  python: {:?}\n  expected: {:?}",
            py_defer_errors, expected.defer_errors
        ));
    }
}

fn run_outcomes_match(a: &RunOutcome, b: &RunOutcome) -> bool {
    match (a, b) {
        (RunOutcome::Completed(x), RunOutcome::Completed(y)) => {
            values_equal(x, y) == Ok(true) || trace_value_from_topaz(x) == trace_value_from_topaz(y)
        }
        (RunOutcome::Faulted(x), RunOutcome::Faulted(y)) => {
            x.code == y.code
                && x.message == y.message
                && x.span.file == y.span.file
                && x.span.lo == y.span.lo
                && x.span.hi == y.span.hi
        }
        _ => false,
    }
}

fn trace_fault_matches(error: &topaz_rt::RtError, fault: &TraceFault) -> bool {
    fault.code == error.code
        && fault.message == error.message
        && fault.span.file == error.span.file.0 as i64
        && fault.span.lo == error.span.lo as i64
        && fault.span.hi == error.span.hi as i64
}

fn trace_value_from_topaz(value: &Value) -> TraceValue {
    match value {
        Value::Int(value) => TraceValue::Int(*value),
        Value::Float(value) => TraceValue::F64(value.to_bits()),
        Value::Str(value) => TraceValue::Str(value.to_string()),
        Value::Bool(value) => TraceValue::Bool(*value),
        Value::Unit | Value::Null | Value::None => TraceValue::Null,
        Value::Some(value) => TraceValue::Some(Box::new(trace_value_from_topaz(value))),
        Value::Ok(value) => TraceValue::ResultOk(Box::new(trace_value_from_topaz(value))),
        Value::Err(value) => TraceValue::ResultErr(Box::new(trace_value_from_topaz(value))),
        Value::Array(values) => {
            TraceValue::List(values.borrow().iter().map(trace_value_from_topaz).collect())
        }
        Value::Bytes(bytes) => TraceValue::Bytes(hex_encode(bytes)),
        Value::Map(map) => TraceValue::Map(
            map.borrow()
                .pairs()
                .into_iter()
                .map(|(key, value)| (trace_value_from_topaz(&key), trace_value_from_topaz(&value)))
                .collect(),
        ),
        Value::Set(set) => TraceValue::Set(
            set.borrow()
                .items()
                .iter()
                .map(trace_value_from_topaz)
                .collect(),
        ),
        Value::Enum {
            enum_id,
            variant,
            variant_index,
            payloads,
            ..
        } => TraceValue::Enum {
            id: enum_id.to_string(),
            variant: variant.to_string(),
            index: *variant_index as u64,
            payloads: payloads.iter().map(trace_value_from_topaz).collect(),
        },
        Value::Range {
            lo,
            hi,
            inclusive,
            step,
        } => TraceValue::Range {
            lo: *lo,
            hi: *hi,
            inclusive: *inclusive,
            step: *step,
        },
        Value::Record(fields) => TraceValue::Record(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), trace_value_from_topaz(value)))
                .collect(),
        ),
        Value::NominalRecord { fields, .. } => TraceValue::Record(
            fields
                .iter()
                .map(|(key, value)| (key.to_string(), trace_value_from_topaz(value)))
                .collect(),
        ),
        other => panic!("unsupported trace value oracle: {other:?}"),
    }
}
