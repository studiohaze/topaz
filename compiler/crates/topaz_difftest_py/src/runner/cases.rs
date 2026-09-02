use crate::trace::*;
use crate::*;

use super::python::{decode_hex, run_python_batch};
use super::reference::{build_reference, run_reference_bin};

pub(crate) fn checked_aliases_for_unit(
    unit: &topaz_resolve::ResolveOutput,
    version: LangVersion,
) -> Result<CheckedAliasSurfaces, String> {
    let modules: Vec<topaz_check::UnitModule> = unit
        .modules
        .iter()
        .map(|m| topaz_check::UnitModule {
            identity: m.identity.clone(),
            is_entry: m.is_entry,
            is_extern: m.is_extern,
            is_generated_std: m.is_generated_std,
            extern_replay_error: m.extern_replay_error.clone(),
            src: unit.map.file(m.file).src(),
            program: &m.program,
        })
        .collect();
    let checked = topaz_check::check_unit_with_version(&modules, version);
    if checked.diagnostics.is_empty() {
        Ok(checked.local_aliases)
    } else {
        Err(format!("type diagnostics: {:?}", checked.diagnostics))
    }
}

pub(crate) fn unit_has_type_alias(unit: &topaz_resolve::ResolveOutput) -> bool {
    fn stmt_has_type_alias(stmt: &topaz_syntax::ast::Stmt) -> bool {
        match &stmt.kind {
            StmtKind::TypeAlias(_) => true,
            StmtKind::Export(inner) => stmt_has_type_alias(inner),
            _ => false,
        }
    }

    unit.modules
        .iter()
        .any(|module| module.program.items.iter().any(stmt_has_type_alias))
}

pub(crate) fn emit_module_for_python_witness(
    unit: &topaz_resolve::ResolveOutput,
    version: LangVersion,
) -> Result<String, String> {
    if unit_has_type_alias(unit) {
        let aliases = checked_aliases_for_unit(unit, version)?;
        emit_module_with_checked_aliases_and_extern_replay_and_policies(
            unit,
            Some(&aliases),
            None,
            &[],
        )
        .map_err(|error| format!("Python emit failed: {error}"))
    } else {
        emit_module(unit).map_err(|error| format!("Python emit failed: {error}"))
    }
}

pub(crate) fn run_fixture(
    python: &Path,
    compiler_dir: &Path,
    tmp_root: &Path,
    fixture: &FixtureSpec,
    cases: &[Case],
) -> Result<(), String> {
    let fixture_tmp = tmp_root.join(fixture.name);
    fs::create_dir_all(&fixture_tmp).map_err(|e| format!("{}: create tmp: {e}", fixture.name))?;
    fs::write(fixture_tmp.join("topaz_py_rt.py"), PY_RT)
        .map_err(|e| format!("{}: write runtime: {e}", fixture.name))?;

    let provider = PhysicalProvider::new(compiler_dir);
    let unit = resolve_with_version(&provider, fixture.entry, None, LangVersion::CURRENT);
    if !unit.diagnostics.is_empty() {
        return Err(format!(
            "{}: resolve diagnostics: {:?}",
            fixture.name, unit.diagnostics
        ));
    }
    let generated = emit_module_for_python_witness(&unit, LangVersion::CURRENT)
        .map_err(|e| format!("{}: {e}", fixture.name))?;
    let script = fixture_tmp.join(format!("{}.py", fixture.name));
    fs::write(&script, generated).map_err(|e| format!("{}: write script: {e}", fixture.name))?;

    let reference = build_reference(
        &compiler_dir.join(fixture.reference),
        &fixture_tmp.join(format!("{}_ref", fixture.name)),
    )
    .map_err(|e| format!("{}: {e}", fixture.name))?;
    let py_traces = run_python_batch(python, &fixture_tmp, &script, cases)
        .map_err(|e| format!("{}: {e}", fixture.name))?;

    match fixture.kind {
        CorpusKind::Badness => compare_badness_batch(fixture, &unit, &reference, cases, &py_traces),
        CorpusKind::JustLatin
        | CorpusKind::Just
        | CorpusKind::LinebreakClassify
        | CorpusKind::Dp => {
            compare_single_output_cases(fixture, &unit, &reference, cases, &py_traces)
        }
    }
}

pub(crate) fn compare_badness_batch(
    fixture: &FixtureSpec,
    unit: &topaz_resolve::ResolveOutput,
    reference: &Path,
    cases: &[Case],
    py_traces: &BTreeMap<String, PyTrace>,
) -> Result<(), String> {
    let batch_input = cases
        .iter()
        .map(|case| case.input.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let rust_stdout = run_reference_bin(reference, &batch_input)
        .map_err(|e| format!("{}: reference batch failed: {e}", fixture.name))?;
    let rust_lines = split_lines_exact(rust_stdout.trim_end_matches('\n'));

    let interp_host = topaz_interp::TestHost::new();
    interp_host.set_input(batch_input);
    let interp_outcome = topaz_interp::Machine::run_unit(unit, &interp_host);
    if let Err(error) = interp_outcome {
        return Err(format!(
            "{}: interpreter batch faulted: {error:?}",
            fixture.name
        ));
    }
    if interp_host.stdout().len() != 1 {
        return Err(format!(
            "{}: interpreter batch stdout shape {:?}",
            fixture.name,
            interp_host.stdout()
        ));
    }
    let interp_stdout = interp_host.stdout();
    let interp_lines = split_lines_exact(&interp_stdout[0]);
    if rust_lines.len() != cases.len() || interp_lines.len() != cases.len() {
        return Err(format!(
            "{}: batch length mismatch cases={} rust={} interp={}",
            fixture.name,
            cases.len(),
            rust_lines.len(),
            interp_lines.len()
        ));
    }
    let mut failures = Vec::new();
    for (idx, case) in cases.iter().enumerate() {
        let expected = rust_lines[idx];
        if interp_lines[idx] != expected {
            failures.push(format!(
                "{}::{}: interpreter != rust: {:?} vs {:?}",
                fixture.name, case.name, interp_lines[idx], expected
            ));
        }
        compare_python_trace(
            fixture.name,
            case,
            py_traces,
            &[expected.to_string()],
            None,
            &mut failures,
        );
    }
    finish_failures(fixture.name, failures)
}

pub(crate) fn compare_single_output_cases(
    fixture: &FixtureSpec,
    unit: &topaz_resolve::ResolveOutput,
    reference: &Path,
    cases: &[Case],
    py_traces: &BTreeMap<String, PyTrace>,
) -> Result<(), String> {
    let mut failures = Vec::new();
    for case in cases {
        let rust_stdout = match run_reference_bin(reference, &case.input) {
            Ok(stdout) => stdout,
            Err(error) => {
                failures.push(format!(
                    "{}::{}: reference failed: {error}",
                    fixture.name, case.name
                ));
                continue;
            }
        };
        let expected = rust_stdout.trim_end_matches('\n').to_string();
        let interp_host = topaz_interp::TestHost::new();
        interp_host.set_input(case.input.clone());
        match topaz_interp::Machine::run_unit(unit, &interp_host) {
            Ok(_) => {
                let stdout = interp_host.stdout();
                if stdout != [expected.clone()] {
                    failures.push(format!(
                        "{}::{}: interpreter stdout mismatch\n  interp: {:?}\n  rust:   {:?}",
                        fixture.name, case.name, stdout, expected
                    ));
                }
            }
            Err(error) => failures.push(format!(
                "{}::{}: interpreter faulted: {error:?}",
                fixture.name, case.name
            )),
        }
        compare_python_trace(
            fixture.name,
            case,
            py_traces,
            &[expected],
            None,
            &mut failures,
        );
    }
    finish_failures(fixture.name, failures)
}

pub(crate) fn compare_python_trace(
    fixture_name: &str,
    case: &Case,
    py_traces: &BTreeMap<String, PyTrace>,
    expected_stdout: &[String],
    expected_value: Option<TraceValue>,
    failures: &mut Vec<String>,
) {
    let Some(trace) = py_traces.get(&case.name) else {
        failures.push(format!(
            "{}::{}: missing Python trace",
            fixture_name, case.name
        ));
        return;
    };
    if trace.version != 1 || trace.status != "ok" || trace.fault.is_some() {
        failures.push(format!(
            "{}::{}: Python returned non-ok trace: {:?}",
            fixture_name, case.name, trace
        ));
        return;
    }
    if !trace.files.is_empty() {
        failures.push(format!(
            "{}::{}: Python trace reported unexpected file state: {:?}",
            fixture_name, case.name, trace.files
        ));
        return;
    }
    if !trace.defer_errors.is_empty() {
        failures.push(format!(
            "{}::{}: Python trace reported unexpected defer errors: {:?}",
            fixture_name, case.name, trace.defer_errors
        ));
        return;
    }
    if trace.stdout != expected_stdout {
        failures.push(format!(
            "{}::{}: Python stdout mismatch\n  python: {:?}\n  rust:   {:?}",
            fixture_name, case.name, trace.stdout, expected_stdout
        ));
    }
    if trace.value != expected_value {
        failures.push(format!(
            "{}::{}: Python trace value mismatch\n  python: {:?}\n  expected: {:?}",
            fixture_name, case.name, trace.value, expected_value
        ));
    }
}

pub(crate) fn finish_failures(fixture_name: &str, failures: Vec<String>) -> Result<(), String> {
    if failures.is_empty() {
        Ok(())
    } else {
        let shown = failures.iter().take(20).cloned().collect::<Vec<_>>();
        Err(format!(
            "{fixture_name}: {} mismatch(es)\n{}",
            failures.len(),
            shown.join("\n")
        ))
    }
}

pub(crate) fn trace_file_string_map(
    files: &[TraceFile],
) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for file in files {
        let TraceValue::Str(content) = &file.content else {
            return Err(format!(
                "file {} carried non-string content {:?}",
                file.path, file.content
            ));
        };
        out.insert(file.path.clone(), content.clone());
    }
    Ok(out)
}

pub(crate) fn load_cases(python: &Path, compiler_dir: &Path, fixture: &FixtureSpec) -> Vec<Case> {
    let runner = compiler_dir.join(fixture.runner);
    let output = Command::new(python)
        .arg("-c")
        .arg(CORPUS_LOADER)
        .arg(corpus_kind_arg(fixture.kind))
        .arg(&runner)
        .env("PYTHONHASHSEED", "0")
        .env("LC_ALL", "C")
        .env("PYTHONIOENCODING", "utf-8")
        .output()
        .expect("load Python corpus");
    assert!(
        output.status.success(),
        "{} corpus loader failed\nstdout:\n{}\nstderr:\n{}",
        fixture.name,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8 corpus output");
    stdout
        .lines()
        .map(|line| {
            let (name, hex) = line.split_once('\t').expect("corpus line has name and hex");
            let input = String::from_utf8(decode_hex(hex)).expect("utf8 Topaz stdin fixture");
            Case {
                name: name.to_string(),
                input,
            }
        })
        .collect()
}

pub(crate) fn corpus_kind_arg(kind: CorpusKind) -> &'static str {
    match kind {
        CorpusKind::Badness => "badness",
        CorpusKind::JustLatin => "just_latin",
        CorpusKind::Just => "just",
        CorpusKind::LinebreakClassify => "linebreak",
        CorpusKind::Dp => "dp",
    }
}

const CORPUS_LOADER: &str = r#"
import importlib.util
import sys

sys.dont_write_bytecode = True

kind = sys.argv[1]
path = sys.argv[2]
spec = importlib.util.spec_from_file_location("topaz_linebreaker_runner", path)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)

def emit(name, stdin_text):
    sys.stdout.write(str(name) + "\t" + stdin_text.encode("utf-8").hex() + "\n")

if kind == "badness":
    for index, case in enumerate(module.cases()):
        emit("badness_%04d" % index, case)
elif kind == "just_latin":
    for name, policy, clusters in module.CASES:
        header = " ".join(str(x) for x in policy)
        emit(name, header + "\n" + "\n".join(str(a) + "\t" + t for a, t in clusters))
elif kind == "just":
    for name, policy, clusters in module.CASES:
        header = " ".join(str(x) for x in policy)
        emit(name, header + "\n" + "\n".join(str(a) + "\t" + t for a, t in clusters))
elif kind == "linebreak":
    for name, clusters in module.CASES.items():
        emit(name, "\n".join(clusters))
elif kind == "dp":
    for name, target, clusters in module.CASES:
        body = "\n".join(str(module.adv(c)) + "\t" + c for c in clusters)
        emit(name, str(target) + "\n" + body)
else:
    raise SystemExit("unknown corpus kind: " + kind)
"#;

pub(crate) fn split_lines_exact(value: &str) -> Vec<&str> {
    if value.is_empty() {
        Vec::new()
    } else {
        value.split('\n').collect()
    }
}
pub(crate) fn compiler_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("compiler dir")
}

pub(crate) fn temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
}
