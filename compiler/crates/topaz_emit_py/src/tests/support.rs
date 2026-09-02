use super::*;

pub(super) struct UnsupportedCase {
    pub(super) name: &'static str,
    pub(super) what: &'static str,
    pub(super) src: &'static str,
}

pub(super) struct UnsupportedCaseWithFiles {
    pub(super) name: &'static str,
    pub(super) src: &'static str,
    pub(super) files: &'static [(&'static str, &'static str)],
}

pub(super) fn with_direct_tail_function<R>(
    src: &str,
    function_name: &str,
    f: impl FnOnce(&FunctionDecl, &Ctx<'_>, &BTreeSet<String>) -> R,
) -> R {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", src);
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_4);
    assert!(
        unit.diagnostics.is_empty(),
        "direct-tail fixture must resolve cleanly: {:?}",
        unit.diagnostics
    );
    let entry = unit
        .modules
        .iter()
        .find(|module| module.is_entry)
        .expect("entry module");
    let decl = entry
        .program
        .items
        .iter()
        .find_map(|stmt| {
            let item = exported_inner(stmt);
            match &item.kind {
                StmtKind::Function(decl)
                    if text_in_map(&unit.map, decl.name.span) == function_name =>
                {
                    Some(decl)
                }
                _ => None,
            }
        })
        .expect("fixture function");
    let ctx = Ctx::new(
        &unit.map,
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        None,
    );
    let module_top_bound_names =
        module_top_bound_names_for_direct_tail(&entry.program.items, &unit.map);
    f(decl, &ctx, &module_top_bound_names)
}

pub(super) fn direct_tail_shape_for_function(
    src: &str,
    function_name: &str,
) -> Option<ReceiverShape> {
    with_direct_tail_function(src, function_name, |decl, ctx, module_top_bound_names| {
        direct_tail_metadata(decl, ctx, module_top_bound_names).return_shape
    })
}

pub(super) fn direct_tail_metadata_for_function(
    src: &str,
    function_name: &str,
) -> DirectTailMetadata {
    with_direct_tail_function(src, function_name, |decl, ctx, module_top_bound_names| {
        let info = function_info(decl, mangle(function_name), ctx, module_top_bound_names);
        DirectTailMetadata {
            return_shape: info.return_shape,
            result_ok_shape: info
                .return_wrapped_metadata
                .root(super::RecordWrapper::ResultOk)
                .receiver_shape,
        }
    })
}

pub(super) fn emit_source(src: &str) -> String {
    emit_source_with_files(src, &[])
}

pub(super) fn emit_source_with_files(src: &str, files: &[(&'static str, &'static str)]) -> String {
    emit_source_with_files_and_version(src, files, LangVersion::V5_4)
}

pub(super) fn emit_source_with_files_and_version(
    src: &str,
    files: &[(&'static str, &'static str)],
    version: LangVersion,
) -> String {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", src);
    for (path, content) in files {
        provider.add_file(*path, *content);
    }
    let unit = resolve_with_version(&provider, "main.tpz", None, version);
    assert!(
        unit.diagnostics.is_empty(),
        "positive fixture must resolve cleanly: {:?}",
        unit.diagnostics
    );
    emit_module(&unit).expect("positive fixture should emit through topaz_emit_py")
}

pub(super) fn emit_checked_alias_source(src: &str) -> String {
    emit_checked_alias_source_with_files(src, &[])
}

pub(super) fn emit_checked_alias_source_with_files(
    src: &str,
    files: &[(&'static str, &'static str)],
) -> String {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", src);
    for (path, content) in files {
        provider.add_file(*path, *content);
    }
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_4);
    assert!(
        unit.diagnostics.is_empty(),
        "checked-alias fixture must resolve cleanly: {:?}",
        unit.diagnostics
    );
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
    let checked = topaz_check::check_unit_with_version(&modules, LangVersion::V5_4);
    assert!(
        checked.diagnostics.is_empty(),
        "checked-alias fixture must type-check cleanly: {:?}",
        checked.diagnostics
    );
    emit_module_with_checked_aliases_and_extern_replay_and_policies(
        &unit,
        Some(&checked.local_aliases),
        None,
        &[],
    )
    .expect("checked-alias fixture should emit through topaz_emit_py")
}

#[test]
pub(super) fn declines_fixed_deflate_before_emitting_python() {
    let error = emit_error_for_source(
        r#"
function main() -> Result<Bytes, string> {
  Codec.deflateFixedCompress(Bytes.encodeUtf8("hello"))
}
main()
"#,
    );
    assert_eq!(error.code(), "TPZ6PY0001");
    assert!(
        error.span.is_some(),
        "the decline must retain a source span"
    );
    match error.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(what, "codec fixed DEFLATE on the Python target")
        }
        other => panic!("expected a fixed-DEFLATE target decline, got {other:?}"),
    }
}

#[test]
pub(super) fn declines_fixed_zlib_before_emitting_python() {
    let error = emit_error_for_source(
        r#"
function main() -> Result<Bytes, string> {
  Codec.zlibFixedCompress(Bytes.encodeUtf8("hello"))
}
main()
"#,
    );
    assert_eq!(error.code(), "TPZ6PY0001");
    assert!(
        error.span.is_some(),
        "the decline must retain a source span"
    );
    match error.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(what, "codec fixed zlib on the Python target")
        }
        other => panic!("expected a fixed-zlib target decline, got {other:?}"),
    }
}

#[test]
pub(super) fn declines_reed_solomon_protection_before_emitting_python() {
    let error = emit_error_for_source(
        r#"
function main() -> Result<Bytes, string> {
  Codec.reedSolomon255223Protect(Bytes.encodeUtf8("hello"))
}
main()
"#,
    );
    assert_eq!(error.code(), "TPZ6PY0001");
    assert!(
        error.span.is_some(),
        "the decline must retain a source span"
    );
    match error.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(what, "Reed-Solomon protection on the Python target")
        }
        other => panic!("expected a Reed-Solomon target decline, got {other:?}"),
    }
}

pub(super) fn emit_checked_alias_error_for_source(src: &str) -> PyEmitError {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", src);
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_4);
    assert!(
        unit.diagnostics.is_empty(),
        "checked-alias negative fixture must resolve cleanly: {:?}",
        unit.diagnostics
    );
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
    let checked = topaz_check::check_unit_with_version(&modules, LangVersion::V5_4);
    assert!(
        checked.diagnostics.is_empty(),
        "checked-alias negative fixture must type-check cleanly: {:?}",
        checked.diagnostics
    );
    emit_module_with_checked_aliases_and_extern_replay_and_policies(
        &unit,
        Some(&checked.local_aliases),
        None,
        &[],
    )
    .expect_err("checked-alias negative fixture should decline in topaz_emit_py")
}

pub(super) fn checked_alias_diagnostics_for_source(src: &str) -> Vec<topaz_diag::Diagnostic> {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", src);
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_4);
    assert!(
        unit.diagnostics.is_empty(),
        "checked-alias diagnostic fixture must resolve cleanly: {:?}",
        unit.diagnostics
    );
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
    topaz_check::check_unit_with_version(&modules, LangVersion::V5_4).diagnostics
}

pub(super) fn assert_generated_python_ok_int(generated: &str, expected: i64, context: &str) {
    assert_generated_python_gates(generated)
        .unwrap_or_else(|e| panic!("{context} Python gate failed: {e}"));
    let Some(python) = cpython_31314() else {
        eprintln!("skipping {context} Python trace witness: CPython 3.13.14 was not found");
        return;
    };
    let tmp = temp_dir("topaz-py-nested");
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");
    let script = tmp.join("fixture.py");
    fs::write(&script, generated).expect("write generated Python");
    let py = run_python(&python, &script, "");
    assert!(
        py.status.success(),
        "{context} generated Python exited nonzero\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&py.stdout),
        String::from_utf8_lossy(&py.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&py.stderr), "", "{context}");
    let expected_trace = format!(
        "{{\"v\":1,\"status\":\"ok\",\"stdout\":[],\"files\":[],\"defer_errors\":[],\"fault\":null,\"value\":{{\"int\":{expected}}}}}\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&py.stdout),
        expected_trace,
        "{context}"
    );
    let _ = fs::remove_dir_all(&tmp);
}

pub(super) fn assert_generated_python_ok_int_with_files_and_stdout(
    generated: &str,
    expected: i64,
    files: &[(&str, &str)],
    expected_stdout: &[&str],
    context: &str,
) {
    assert_generated_python_gates(generated)
        .unwrap_or_else(|e| panic!("{context} Python gate failed: {e}"));
    let Some(python) = cpython_31314() else {
        eprintln!("skipping {context} Python trace witness: CPython 3.13.14 was not found");
        return;
    };
    let tmp = temp_dir("topaz-py-files");
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");
    fs::write(tmp.join("fixture.py"), generated).expect("write generated Python");
    let mut sorted_files = files.to_vec();
    sorted_files.sort_by_key(|(path, _)| *path);
    let files_py = sorted_files
        .iter()
        .map(|(path, content)| format!("{}: {}", super::py_string(path), super::py_string(content)))
        .collect::<Vec<_>>()
        .join(", ");
    let runner = tmp.join("runner.py");
    fs::write(
        &runner,
        format!("import sys\nimport fixture\nsys.stdout.write(fixture.run('', {{{files_py}}}))\n"),
    )
    .expect("write Python runner");
    let py = run_python(&python, &runner, "");
    assert!(
        py.status.success(),
        "{context} generated Python exited nonzero\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&py.stdout),
        String::from_utf8_lossy(&py.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&py.stderr), "", "{context}");
    let stdout_json = expected_stdout
        .iter()
        .map(|value| super::py_string(value))
        .collect::<Vec<_>>()
        .join(",");
    let files_json = sorted_files
        .iter()
        .map(|(path, content)| {
            format!(
                "{{\"path\":{},\"content\":{{\"str\":{}}}}}",
                super::py_string(path),
                super::py_string(content)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let expected_trace = format!(
        "{{\"v\":1,\"status\":\"ok\",\"stdout\":[{stdout_json}],\"files\":[{files_json}],\"defer_errors\":[],\"fault\":null,\"value\":{{\"int\":{expected}}}}}"
    );
    assert_eq!(
        String::from_utf8_lossy(&py.stdout),
        expected_trace,
        "{context}"
    );
    let _ = fs::remove_dir_all(&tmp);
}

pub(super) fn assert_generated_python_ok_string(generated: &str, expected: &str, context: &str) {
    assert_generated_python_gates(generated)
        .unwrap_or_else(|e| panic!("{context} Python gate failed: {e}"));
    let Some(python) = cpython_31314() else {
        eprintln!("skipping {context} Python trace witness: CPython 3.13.14 was not found");
        return;
    };
    let tmp = temp_dir("topaz-py-string");
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");
    let script = tmp.join("fixture.py");
    fs::write(&script, generated).expect("write generated Python");
    let py = run_python(&python, &script, "");
    assert!(
        py.status.success(),
        "{context} generated Python exited nonzero\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&py.stdout),
        String::from_utf8_lossy(&py.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&py.stderr), "", "{context}");
    let expected_trace = format!(
        "{{\"v\":1,\"status\":\"ok\",\"stdout\":[],\"files\":[],\"defer_errors\":[],\"fault\":null,\"value\":{{\"str\":{expected:?}}}}}\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&py.stdout),
        expected_trace,
        "{context}"
    );
    let _ = fs::remove_dir_all(&tmp);
}

pub(super) fn assert_generated_python_fault_code(generated: &str, code: &str, context: &str) {
    assert_generated_python_gates(generated)
        .unwrap_or_else(|e| panic!("{context} Python gate failed: {e}"));
    let Some(python) = cpython_31314() else {
        eprintln!("skipping {context} Python trace witness: CPython 3.13.14 was not found");
        return;
    };
    let tmp = temp_dir("topaz-py-fault");
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");
    let script = tmp.join("fixture.py");
    fs::write(&script, generated).expect("write generated Python");
    let py = run_python(&python, &script, "");
    assert!(
        py.status.success(),
        "{context} generated Python trace process exited nonzero\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&py.stdout),
        String::from_utf8_lossy(&py.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&py.stderr), "", "{context}");
    let trace = String::from_utf8_lossy(&py.stdout);
    assert!(
        trace.contains("\"status\":\"fault\"") && trace.contains(&format!("\"code\":{code:?}")),
        "{context} expected fault {code}, got: {trace}"
    );
    let _ = fs::remove_dir_all(&tmp);
}

pub(super) fn emit_unchecked_source(src: &str) -> String {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", src);
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_4);
    emit_module(&unit).expect("unchecked fixture should emit through topaz_emit_py")
}

pub(super) fn emit_unchecked_error_for_source_with_files(
    src: &str,
    files: &[(&'static str, &'static str)],
) -> PyEmitError {
    emit_unchecked_error_and_unit_for_source_with_files(src, files).0
}

pub(super) fn emit_unchecked_error_and_unit_for_source_with_files(
    src: &str,
    files: &[(&'static str, &'static str)],
) -> (PyEmitError, ResolveOutput) {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", src);
    for (path, content) in files {
        provider.add_file(*path, *content);
    }
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_4);
    let error =
        emit_module(&unit).expect_err("unchecked fixture should be rejected by topaz_emit_py");
    (error, unit)
}

pub(super) fn emit_error_for_source(src: &str) -> PyEmitError {
    emit_error_for_source_with_files(src, &[])
}

pub(super) fn emit_error_for_source_with_files(
    src: &str,
    files: &[(&'static str, &'static str)],
) -> PyEmitError {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", src);
    for (path, content) in files {
        provider.add_file(*path, *content);
    }
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_4);
    assert!(
        unit.diagnostics.is_empty(),
        "negative fixture must reach the Python emitter without resolver diagnostics: {:?}",
        unit.diagnostics
    );
    emit_module(&unit).expect_err("negative fixture should be rejected by topaz_emit_py")
}

pub(super) fn selected_badness_corpus() -> Vec<&'static str> {
    vec![
        "0|120000|0|0|latin_space|0",
        "62400|120000|1|1|latin_hyphen|1",
        "500001|500000|2|5|hard_fallback|0",
        "115000000000|23000000000|3|20|forced_end|1",
    ]
}

pub(super) fn selected_just_latin_corpus() -> Vec<(&'static str, String)> {
    vec![
        ("last_line", "100 0 0 0 0 1".to_string()),
        (
            "fits_no_adjust",
            latin_case("100 1 0 10 20 0", &[(40, "a"), (10, " "), (50, "b")]),
        ),
        (
            "trim_trailing_space",
            latin_case(
                "100 1 0 10 20 0",
                &[(40, "a"), (10, " "), (50, "b"), (10, " ")],
            ),
        ),
        (
            "missing_gap_cap",
            latin_case("120 1 0 100 100 0", &[(50, "a"), (50, "b")]),
        ),
        (
            "over_max_adj",
            latin_case("200 1 0 10 100 0", &[(40, "a"), (10, " "), (50, "b")]),
        ),
        (
            "gap_width_over",
            latin_case("130 1 0 100 10 0", &[(40, "a"), (10, " "), (50, "b")]),
        ),
        (
            "nbsp_is_not_u0020_gap",
            latin_case(
                "120 1 0 100 100 0",
                &[(50, "a"), (10, "\u{00A0}"), (50, "b")],
            ),
        ),
    ]
}

pub(super) fn latin_case(header: &str, items: &[(i64, &str)]) -> String {
    let mut input = header.to_string();
    for (advance, text) in items {
        input.push('\n');
        input.push_str(&format!("{advance}\t{text}"));
    }
    input
}

pub(super) fn selected_linebreak_classify_corpus(
    python: &Path,
    compiler_dir: &Path,
) -> Vec<(String, String)> {
    let runner = compiler_dir.join("fixtures/topaz_emit_py/atlas-poc/parity-runner.py");
    let script = r#"
import importlib.util
import sys

spec = importlib.util.spec_from_file_location("linebreak_parity_runner", sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
for name, clusters in module.CASES.items():
    sys.stdout.write(name)
    for cluster in clusters:
        sys.stdout.write("\t" + cluster)
    sys.stdout.write("\n")
"#;
    let output = Command::new(python)
        .arg("-c")
        .arg(script)
        .arg(&runner)
        .env("PYTHONHASHSEED", "0")
        .env("LC_ALL", "C")
        .env("PYTHONIOENCODING", "utf-8")
        .output()
        .expect("load linebreak classify corpus");
    assert!(
        output.status.success(),
        "linebreak classify corpus import failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8 corpus");
    stdout
        .lines()
        .map(|line| {
            let mut parts = line.split('\t');
            let name = parts.next().expect("case name").to_string();
            let clusters = parts.collect::<Vec<_>>();
            (name, clusters.join("\n"))
        })
        .collect()
}

pub(super) fn selected_dp_corpus(python: &Path, compiler_dir: &Path) -> Vec<(String, String)> {
    let runner = compiler_dir.join("fixtures/topaz_emit_py/atlas-poc/dp-runner.py");
    let script = r#"
import importlib.util
import sys

spec = importlib.util.spec_from_file_location("linebreak_dp_runner", sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
for name, target, clusters in module.CASES:
    stdin_text = str(target) + "\n" + "\n".join(str(module.adv(c)) + "\t" + c for c in clusters)
    sys.stdout.write(name + "\t" + stdin_text.encode("utf-8").hex() + "\n")
"#;
    let output = Command::new(python)
        .arg("-c")
        .arg(script)
        .arg(&runner)
        .env("PYTHONHASHSEED", "0")
        .env("LC_ALL", "C")
        .env("PYTHONIOENCODING", "utf-8")
        .output()
        .expect("load dp corpus");
    assert!(
        output.status.success(),
        "dp corpus import failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8 corpus");
    stdout
        .lines()
        .map(|line| {
            let (name, hex) = line.split_once('\t').expect("name and hex input");
            let bytes = decode_hex(hex);
            let input = String::from_utf8(bytes).expect("utf8 dp input");
            (name.to_string(), input)
        })
        .collect()
}

pub(super) fn decode_hex(hex: &str) -> Vec<u8> {
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

pub(super) fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => panic!("invalid hex byte {byte}"),
    }
}

pub(super) fn numeric_vector_script() -> String {
    let mut script = String::from(
        "from topaz_py_rt import TpzFault, tpz_add_i64, tpz_div_trunc_i64, tpz_i64, tpz_mul_i64, tpz_pow_i64, tpz_rem_trunc_i64, tpz_sub_i64\n\
             SP = (7, 11, 13)\n\
             def run(label, fn, *args):\n\
             \ttry:\n\
             \t\tprint(label + ':ok:' + str(fn(*args, SP)))\n\
             \texcept TpzFault as fault:\n\
             \t\tfile, lo, hi = fault.span\n\
             \t\tprint(label + ':fault:' + fault.code + ':' + str(file) + ':' + str(lo) + ':' + str(hi) + ':' + fault.message)\n",
    );
    let cases = [
        ("i64_ok", "tpz_i64", "9223372036854775807"),
        ("i64_overflow", "tpz_i64", "9223372036854775808"),
        ("add_ok", "tpz_add_i64", "2, 3"),
        ("add_overflow", "tpz_add_i64", "9223372036854775807, 1"),
        ("sub_ok", "tpz_sub_i64", "2, 3"),
        ("sub_overflow", "tpz_sub_i64", "-9223372036854775808, 1"),
        ("mul_ok", "tpz_mul_i64", "6, 7"),
        ("mul_overflow", "tpz_mul_i64", "9223372036854775807, 2"),
        ("div_ok", "tpz_div_trunc_i64", "7, 2"),
        ("div_neg", "tpz_div_trunc_i64", "-7, 2"),
        ("div_zero", "tpz_div_trunc_i64", "1, 0"),
        (
            "div_overflow",
            "tpz_div_trunc_i64",
            "-9223372036854775808, -1",
        ),
        ("rem_ok", "tpz_rem_trunc_i64", "7, 2"),
        ("rem_neg", "tpz_rem_trunc_i64", "-7, 2"),
        ("rem_zero", "tpz_rem_trunc_i64", "1, 0"),
        (
            "rem_overflow",
            "tpz_rem_trunc_i64",
            "-9223372036854775808, -1",
        ),
        ("pow_zero_zero", "tpz_pow_i64", "0, 0"),
        ("pow_one_u32_max", "tpz_pow_i64", "1, 4294967295"),
        ("pow_neg_even", "tpz_pow_i64", "-1, 2"),
        ("pow_neg_odd", "tpz_pow_i64", "-1, 3"),
        ("pow_2_62", "tpz_pow_i64", "2, 62"),
        ("pow_2_63", "tpz_pow_i64", "2, 63"),
        ("pow_negative_exp", "tpz_pow_i64", "3, -1"),
        ("pow_exp_above_u32", "tpz_pow_i64", "2, 4294967296"),
    ];
    for (label, func, args) in cases {
        script.push_str(&format!("run({label:?}, {func}, {args})\n"));
    }
    script
}

pub(super) fn runtime_helper_vector_script() -> String {
    String::from(
        "from topaz_py_rt import Some, tpz_array_get, tpz_string_byte_length\n\
             SP = (7, 11, 13)\n\
             def show(label, value):\n\
             \tif isinstance(value, Some):\n\
             \t\tprint(label + ':some:' + value.value)\n\
             \telif value is None:\n\
             \t\tprint(label + ':none')\n\
             \telse:\n\
             \t\tprint(label + ':value:' + str(value))\n\
             show('get_zero', tpz_array_get(['a', 'b'], 0, SP))\n\
             show('get_neg', tpz_array_get(['a', 'b'], -1, SP))\n\
             show('get_oob', tpz_array_get(['a', 'b'], 2, SP))\n\
             show('byte_ascii', tpz_string_byte_length('A', SP))\n\
             show('byte_hangul', tpz_string_byte_length('가', SP))\n\
             show('byte_emoji', tpz_string_byte_length('😀', SP))\n",
    )
}

pub(super) fn record_update_vector_script() -> String {
    String::from(
        "from dataclasses import dataclass\n\
             import json\n\
             from topaz_py_rt import TpzFault, tpz_record_update, tpz_trace_value\n\
             SP = (7, 11, 13)\n\
             @dataclass(frozen=True, slots=True)\n\
             class Empty:\n\
             \t__topaz_record_fields__ = ()\n\
             @dataclass(frozen=True, slots=True)\n\
             class Pair:\n\
             \t__topaz_record_fields__ = ((\"_t_78\", \"x\"),)\n\
             \t_t_78: object\n\
             def mark(label, value):\n\
             \tprint('eval:' + label)\n\
             \treturn value\n\
             added = tpz_record_update(Empty(), [(\"_t_78\", \"x\", lambda: mark('x', 1)), (\"_t_79\", \"y\", lambda: mark('y', 2)), (\"_t_78\", \"x\", lambda: mark('x2', 3))], SP)\n\
             print('empty_add:' + str(added._t_78) + ':' + str(added._t_79) + ':' + json.dumps(tpz_trace_value(added), sort_keys=True, separators=(',', ':')))\n\
             try:\n\
             \ttpz_record_update(Pair(1), [(\"_t_79\", \"y\", lambda: mark('unknown', 2))], SP)\n\
             except TpzFault as fault:\n\
             \tfile, lo, hi = fault.span\n\
             \tprint('unknown_fault:' + fault.code + ':' + str(file) + ':' + str(lo) + ':' + str(hi) + ':' + fault.message)\n",
    )
}

pub(super) fn float_vector_script() -> String {
    let mut script = String::from(
        "from topaz_py_rt import tpz_add, tpz_div, tpz_f64_bits, tpz_f64_from_bits, tpz_mul, tpz_pow, tpz_render, tpz_sub\n",
    );
    for golden in FLOAT_RENDER_GOLDENS {
        let bits = golden.bits;
        script.push_str(&format!(
            "print('bits:{bits:016x}:%016x' % tpz_f64_bits(tpz_f64_from_bits(0x{bits:016x})))\n"
        ));
    }
    for golden in FLOAT_RENDER_GOLDENS {
        let bits = golden.bits;
        script.push_str(&format!(
            "print('render:{bits:016x}:' + tpz_render(tpz_f64_from_bits(0x{bits:016x})))\n"
        ));
    }
    script.push_str("span = (7, 11, 13)\n");
    script.push_str("nan = tpz_f64_from_bits(0xfff8000000000042)\n");
    for (name, expression) in [
        ("add", "tpz_add(nan, 1.0, span)"),
        ("sub", "tpz_sub(nan, 1.0, span)"),
        ("mul", "tpz_mul(nan, 1.0, span)"),
        ("div", "tpz_div(0.0, 0.0, span)"),
        ("pow", "tpz_pow(-1.0, 0.5, span)"),
    ] {
        script.push_str(&format!(
            "print('arith:{name}:%016x' % tpz_f64_bits({expression}))\n"
        ));
    }
    script
}

pub(super) fn expected_numeric_vectors() -> String {
    let mut lines = Vec::new();
    lines.push("i64_ok:ok:9223372036854775807".to_string());
    lines.push(format!(
        "i64_overflow:fault:TPZ4004:{}:{}:{}:integer value is outside i64 range",
        SP.file.0, SP.lo, SP.hi
    ));
    lines.push(render_i64("add_ok", int_add(2, 3, SP)));
    lines.push(render_i64("add_overflow", int_add(i64::MAX, 1, SP)));
    lines.push(render_i64("sub_ok", int_sub(2, 3, SP)));
    lines.push(render_i64("sub_overflow", int_sub(i64::MIN, 1, SP)));
    lines.push(render_i64("mul_ok", int_mul(6, 7, SP)));
    lines.push(render_i64("mul_overflow", int_mul(i64::MAX, 2, SP)));
    lines.push(render_i64("div_ok", int_div(7, 2, SP)));
    lines.push(render_i64("div_neg", int_div(-7, 2, SP)));
    lines.push(render_i64("div_zero", int_div(1, 0, SP)));
    lines.push(render_i64("div_overflow", int_div(i64::MIN, -1, SP)));
    lines.push(render_i64("rem_ok", int_rem(7, 2, SP)));
    lines.push(render_i64("rem_neg", int_rem(-7, 2, SP)));
    lines.push(render_i64("rem_zero", int_rem(1, 0, SP)));
    lines.push(render_i64("rem_overflow", int_rem(i64::MIN, -1, SP)));
    lines.push(render_i64("pow_zero_zero", int_pow(0, 0, SP)));
    lines.push(render_i64(
        "pow_one_u32_max",
        int_pow(1, u32::MAX as i64, SP),
    ));
    lines.push(render_i64("pow_neg_even", int_pow(-1, 2, SP)));
    lines.push(render_i64("pow_neg_odd", int_pow(-1, 3, SP)));
    lines.push(render_i64("pow_2_62", int_pow(2, 62, SP)));
    lines.push(render_i64("pow_2_63", int_pow(2, 63, SP)));
    lines.push(render_i64("pow_negative_exp", int_pow(3, -1, SP)));
    lines.push(render_i64(
        "pow_exp_above_u32",
        int_pow(2, u32::MAX as i64 + 1, SP),
    ));
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

pub(super) fn expected_float_vectors() -> String {
    let mut out = String::new();
    for golden in FLOAT_RENDER_GOLDENS {
        let bits = golden.bits;
        out.push_str(&format!("bits:{bits:016x}:{bits:016x}\n"));
    }
    for golden in FLOAT_RENDER_GOLDENS {
        assert_eq!(
            render_float(f64::from_bits(golden.bits)),
            golden.render,
            "{}",
            golden.name
        );
        out.push_str(&format!("render:{:016x}:{}\n", golden.bits, golden.render));
    }
    use topaz_syntax::ast::BinaryOp::{Add, Div, Mul, Pow, Sub};
    let noncanonical_nan = f64::from_bits(0xfff8_0000_0000_0042);
    for (name, op, a, b) in [
        ("add", Add, noncanonical_nan, 1.0),
        ("sub", Sub, noncanonical_nan, 1.0),
        ("mul", Mul, noncanonical_nan, 1.0),
        ("div", Div, 0.0, 0.0),
        ("pow", Pow, -1.0, 0.5),
    ] {
        let bits = float_arith(op, a, b).to_bits();
        assert_eq!(bits, CANONICAL_ARITHMETIC_NAN_BITS, "{name}");
        out.push_str(&format!("arith:{name}:{bits:016x}\n"));
    }
    out
}

pub(super) fn expected_runtime_helper_vectors() -> String {
    [
        "get_zero:some:a",
        "get_neg:none",
        "get_oob:none",
        "byte_ascii:value:1",
        "byte_hangul:value:3",
        "byte_emoji:value:4",
        "",
    ]
    .join("\n")
}

pub(super) fn expected_record_update_vectors() -> String {
    let empty = Value::Record(Rc::new(BTreeMap::new()));
    let base = record_update_base(empty, SP).expect("empty record base accepted");
    let merged = record_update_merge(
        base,
        vec![
            ("x".to_string(), Value::Int(1)),
            ("y".to_string(), Value::Int(2)),
            ("x".to_string(), Value::Int(3)),
        ],
        SP,
    )
    .expect("empty base update adds fields");
    let Value::Record(fields) = merged else {
        panic!("record update leaf returned non-record");
    };
    let x = match fields.get("x").expect("x field") {
        Value::Int(value) => *value,
        other => panic!("x field was {other:?}"),
    };
    let y = match fields.get("y").expect("y field") {
        Value::Int(value) => *value,
        other => panic!("y field was {other:?}"),
    };

    let nonempty = Value::Record(Rc::new(BTreeMap::from([("x".to_string(), Value::Int(1))])));
    let base = record_update_base(nonempty, SP).expect("nonempty record base accepted");
    let unknown = record_update_merge(base, vec![("y".to_string(), Value::Int(2))], SP)
        .expect_err("unknown field faults on nonempty base");

    let lines = [
        "eval:x".to_string(),
        "eval:y".to_string(),
        "eval:x2".to_string(),
        format!("empty_add:{x}:{y}:{{\"record\":{{\"x\":{{\"int\":{x}}},\"y\":{{\"int\":{y}}}}}}}"),
        "eval:unknown".to_string(),
        render_error("unknown_fault", &unknown),
    ];
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

pub(super) fn render_i64(label: &str, result: Result<i64, RtError>) -> String {
    match result {
        Ok(value) => format!("{label}:ok:{value}"),
        Err(error) => format!(
            "{label}:fault:{}:{}:{}:{}:{}",
            error.code, error.span.file.0, error.span.lo, error.span.hi, error.message
        ),
    }
}

pub(super) fn render_error(label: &str, error: &RtError) -> String {
    format!(
        "{label}:{}:{}:{}:{}:{}",
        error.code, error.span.file.0, error.span.lo, error.span.hi, error.message
    )
}

pub(super) fn assert_generated_python_gates(generated: &str) -> Result<(), String> {
    assert_no_forbidden_python_ops(generated)?;
    assert_span_gate(generated)
}

pub(super) fn assert_generated_source_assignment(generated: &str, source_name: &str, rhs: &str) {
    let py_prefix = crate::mangle(source_name);
    let expected_tail = format!(" = {rhs}  # {source_name}");
    assert!(
        generated.lines().any(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with(&py_prefix) && trimmed.contains(&expected_tail)
        }),
        "missing generated assignment for `{source_name}` with `{rhs}`:\n{generated}"
    );
}

pub(super) fn assert_no_forbidden_python_ops(generated: &str) -> Result<(), String> {
    for (lineno, raw) in generated.lines().enumerate() {
        let line = mask_python_strings_and_comments(raw);
        for token in ["//", "%", "**", " + ", " - ", " * ", " / "] {
            if line.contains(token) {
                return Err(format!(
                    "line {} contains forbidden raw Topaz operator token `{token}`: {raw}",
                    lineno + 1
                ));
            }
        }
        for token in [
            "open(",
            "os.",
            "socket",
            "time.",
            "random",
            "subprocess",
            "environ",
            "str(",
            "repr(",
        ] {
            if line.contains(token) {
                return Err(format!(
                    "line {} contains forbidden ambient/raw runtime token `{token}`: {raw}",
                    lineno + 1
                ));
            }
        }
        if line.contains(" = {") || line.contains(" = set(") || line.contains(" = dict(") {
            return Err(format!(
                "line {} contains a raw Python dict/set shape: {raw}",
                lineno + 1
            ));
        }
    }
    Ok(())
}

pub(super) fn assert_span_gate(generated: &str) -> Result<(), String> {
    let helpers = [
        "tpz_add(",
        "tpz_add_i64(",
        "tpz_sub(",
        "tpz_sub_i64(",
        "tpz_mul(",
        "tpz_mul_i64(",
        "tpz_div(",
        "tpz_div_trunc_i64(",
        "tpz_rem_trunc_i64(",
        "tpz_pow(",
        "tpz_pow_i64(",
        "tpz_lt(",
        "tpz_lt_i64(",
        "tpz_le(",
        "tpz_le_i64(",
        "tpz_gt(",
        "tpz_gt_i64(",
        "tpz_ge(",
        "tpz_ge_i64(",
        "tpz_eq(",
        "tpz_ne(",
        "tpz_neg(",
        "tpz_array_clear(",
        "tpz_array_get(",
        "tpz_array_index_of(",
        "tpz_array_insert(",
        "tpz_array_join(",
        "tpz_array_pop(",
        "tpz_array_push(",
        "tpz_array_remove_at(",
        "tpz_array_reverse(",
        "tpz_array_slice(",
        "tpz_get(",
        "tpz_bytes_concat(",
        "tpz_bytes_decode_utf8(",
        "tpz_bytes_encode_utf8(",
        "tpz_bytes_from_array(",
        "tpz_bytes_from_base64(",
        "tpz_bytes_from_hex(",
        "tpz_bytes_is_empty(",
        "tpz_bytes_slice(",
        "tpz_bytes_to_array(",
        "tpz_bytes_to_base64(",
        "tpz_bytes_to_hex(",
        "tpz_clear(",
        "tpz_condition(",
        "tpz_for_items(",
        "tpz_for_pattern(",
        "tpz_fs_list(",
        "tpz_fs_read_bytes(",
        "tpz_fs_read_text(",
        "tpz_fs_write_bytes(",
        "tpz_fs_write_text(",
        "tpz_impossible_match(",
        "tpz_in(",
        "tpz_is_empty(",
        "tpz_nominal_record(",
        "tpz_record_field(",
        "tpz_record_update(",
        "tpz_map_contains_key(",
        "tpz_map_get_or(",
        "tpz_map_insert(",
        "tpz_map_of(",
        "tpz_member(",
        "tpz_remove(",
        "tpz_call_order_fault(",
        "tpz_nonvariadic_spread_call(",
        "tpz_nonvariadic_static_spread_call(",
        "tpz_set_add(",
        "tpz_set_contains(",
        "tpz_set_difference(",
        "tpz_set_intersection(",
        "tpz_set_of(",
        "tpz_set_union(",
        "tpz_to_int(",
        "tpz_from_code_point(",
        "tpz_length(",
        "tpz_to_array(",
        "tpz_string_split(",
        "tpz_string_code_point_at(",
        "tpz_string_byte_length(",
        "tpz_immutable_assignment(",
        "tpz_index(",
        "tpz_index_slot(",
        "host.print(",
    ];
    for (lineno, raw) in generated.lines().enumerate() {
        let line = mask_python_strings_and_comments(raw);
        for helper in helpers {
            let mut search = line.as_str();
            let mut base = 0usize;
            while let Some(pos) = search.find(helper) {
                let absolute = base + pos;
                if !call_has_span_tuple(&line[absolute + helper.len()..]) {
                    return Err(format!(
                        "line {} helper `{}` is missing a Topaz span tuple: {raw}",
                        lineno + 1,
                        helper.trim_end_matches('(')
                    ));
                }
                base = absolute + helper.len();
                search = &line[base..];
            }
        }
    }
    Ok(())
}

pub(super) fn call_has_span_tuple(after_open: &str) -> bool {
    let mut depth = 1i32;
    let bytes = after_open.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return false;
                }
            }
            b',' if depth == 1 => {
                let rest = after_open[i + 1..].trim_start();
                if starts_with_span_tuple(rest) {
                    return true;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

pub(super) fn starts_with_span_tuple(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('(') else {
        return false;
    };
    let mut rest = rest.trim_start();
    for idx in 0..3 {
        let consumed = rest
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .map(char::len_utf8)
            .sum::<usize>();
        if consumed == 0 {
            return false;
        }
        rest = rest[consumed..].trim_start();
        if idx < 2 {
            let Some(next) = rest.strip_prefix(',') else {
                return false;
            };
            rest = next.trim_start();
        }
    }
    rest.starts_with(')')
}

pub(super) fn mask_python_strings_and_comments(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for ch in line.chars() {
        if let Some(q) = quote {
            out.push(' ');
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == q {
                quote = None;
            }
            continue;
        }
        if ch == '#' {
            out.extend(std::iter::repeat_n(' ', line.len() - out.len()));
            break;
        }
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
}

pub(super) fn compiler_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("compiler dir")
}

pub(super) fn temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}-{counter}", std::process::id()))
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

pub(super) fn python_identity(python: &Path) -> String {
    let output = Command::new(python)
        .arg("-c")
        .arg("import sys; print(sys.executable); print(sys.version); print(sys.implementation.cache_tag)")
        .output()
        .expect("run Python identity command");
    assert!(output.status.success(), "Python identity command failed");
    String::from_utf8(output.stdout).expect("UTF-8 Python identity")
}

pub(super) fn run_badness_reference(compiler_dir: &Path, tmp: &Path, input: &str) -> String {
    let src = compiler_dir.join("fixtures/topaz_emit_py/atlas-poc/badness_fp.rs");
    let bin = tmp.join("badness_fp");
    run_reference(&src, &bin, input)
}

pub(super) fn run_reference(src: &Path, bin: &Path, input: &str) -> String {
    let bin = build_reference(src, bin);
    run_reference_bin(&bin, input)
}

pub(super) fn build_reference(src: &Path, bin: &Path) -> PathBuf {
    let build = Command::new("rustc")
        .arg("-O")
        .arg(src)
        .arg("-o")
        .arg(bin)
        .output()
        .expect("run rustc");
    assert!(
        build.status.success(),
        "Rust reference build failed for {}: {}",
        src.display(),
        String::from_utf8_lossy(&build.stderr)
    );
    bin.to_path_buf()
}

pub(super) fn run_reference_bin(bin: &Path, input: &str) -> String {
    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn Rust reference");
    child
        .stdin
        .as_mut()
        .expect("ref stdin")
        .write_all(input.as_bytes())
        .expect("write ref stdin");
    let output = child.wait_with_output().expect("wait reference");
    assert!(
        output.status.success(),
        "Rust reference failed for {}: {}",
        bin.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    String::from_utf8(output.stdout).expect("utf8 ref stdout")
}

pub(super) fn run_python(python: &Path, script: &Path, input: &str) -> std::process::Output {
    let mut child = Command::new(python)
        .arg("-u")
        .arg(script)
        .env("PYTHONHASHSEED", "0")
        .env("LC_ALL", "C")
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn python");
    child
        .stdin
        .as_mut()
        .expect("python stdin")
        .write_all(input.as_bytes())
        .expect("write python stdin");
    child.wait_with_output().expect("wait python")
}

pub(super) fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                use std::fmt::Write as _;
                write!(out, "\\u{:04x}", c as u32).expect("write json escape");
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
