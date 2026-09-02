use super::*;

#[test]
fn statement_lowered_yield_from_call_expr_is_top_level_only() {
    assert_eq!(
        super::statement_lowered_yield_from_call_expr(
            "(yield from tpz_option_ok_or_else__co(host, cb))"
        )
        .unwrap(),
        Some("tpz_option_ok_or_else__co(host, cb)")
    );
    assert_eq!(
        super::statement_lowered_yield_from_call_expr("tpz_wrap_optional(value)").unwrap(),
        None
    );
    assert_eq!(
        super::statement_lowered_yield_from_call_expr("tpz_option_ok_or(value, 'yield from')")
            .unwrap(),
        None
    );
    assert_eq!(
        super::statement_lowered_yield_from_call_expr("(yield from f(')'))").unwrap(),
        Some("f(')')")
    );
    assert!(
        super::statement_lowered_yield_from_call_expr("(yield from f())(x)").is_err(),
        "statement-lowered optional wrappers must not balance a drifted call suffix"
    );
    assert!(
        super::statement_lowered_yield_from_call_expr("tpz_wrap_optional((yield from f()))")
            .is_err(),
        "statement-lowered optional wrappers must reject non-top-level yield-from shapes"
    );
}

#[test]
fn optional_receiver_unit_lambda_rejects_yield_from_calls() {
    super::reject_yield_from_inside_optional_receiver_unit_lambda(
        "tpz_array_retain(__tpz_obj.value, cb)",
    )
    .unwrap();
    super::reject_yield_from_inside_optional_receiver_unit_lambda(
        "tpz_map_update(__tpz_obj.value, 'yield from', 0, cb)",
    )
    .unwrap();
    let err = super::reject_yield_from_inside_optional_receiver_unit_lambda(
        "(yield from tpz_array_retain__co(host, __tpz_obj.value, cb))",
    )
    .unwrap_err();
    assert_eq!(err.code(), "TPZ6PY0001");
    assert!(
        matches!(
            err.kind,
            PyEmitErrorKind::Unsupported("optional receiver unit call yield")
        ),
        "unit-call optional receiver lambdas should fail closed before embedding yield-from: {err:?}"
    );
}

#[test]
fn badness_tpz_runs_as_python_trace_under_cpython_31314() {
    let Some(python) = cpython_31314() else {
        eprintln!("skipping Python witness: CPython 3.13.14 was not found");
        return;
    };
    let identity = python_identity(&python);
    assert!(
        identity.contains("3.13.14"),
        "expected CPython 3.13.14, got {identity}"
    );
    eprintln!("CPython identity:\n{identity}");
    let compiler_dir = compiler_dir();
    let provider = PhysicalProvider::new(&compiler_dir);
    let unit = resolve_with_version(
        &provider,
        "fixtures/topaz_emit_py/atlas-poc/badness.tpz",
        None,
        LangVersion::V5_4,
    );
    assert!(
        unit.diagnostics.is_empty(),
        "badness.tpz did not resolve cleanly: {:?}",
        unit.diagnostics
    );
    let generated = emit_module(&unit).expect("badness.tpz emits to Python");
    assert!(
        generated.contains("_t_ecb298eba6ac"),
        "non-ASCII function name `처리` must be mangled"
    );
    assert_generated_python_gates(&generated)
        .expect("generated Python passes forbidden-operation and span checks");

    let tmp = temp_dir("topaz-py-badness");
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");
    let script = tmp.join("badness.py");
    fs::write(&script, generated).expect("write generated Python");

    let input = selected_badness_corpus().join("\n");
    let rust_stdout = run_badness_reference(&compiler_dir, &tmp, &input);
    let expected_host_line = rust_stdout.trim_end_matches('\n');
    let interp_host = topaz_interp::TestHost::new();
    interp_host.set_input(input.clone());
    let interp_outcome = topaz_interp::Machine::run_unit(&unit, &interp_host);
    assert!(
        interp_outcome.is_ok(),
        "interpreter failed: {interp_outcome:?}"
    );
    assert_eq!(interp_host.stdout(), vec![expected_host_line.to_string()]);
    let expected_trace = format!(
        "{{\"v\":1,\"status\":\"ok\",\"stdout\":[{}],\"files\":[],\"defer_errors\":[],\"fault\":null}}\n",
        json_string(expected_host_line)
    );
    let py = run_python(&python, &script, &input);
    assert!(
        py.status.success(),
        "generated Python exited nonzero\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&py.stdout),
        String::from_utf8_lossy(&py.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&py.stderr), "");
    assert_eq!(String::from_utf8_lossy(&py.stdout), expected_trace);
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn just_latin_tpz_runs_as_python_trace_under_cpython_31314() {
    let Some(python) = cpython_31314() else {
        eprintln!("skipping Python just_latin witness: CPython 3.13.14 was not found");
        return;
    };
    let compiler_dir = compiler_dir();
    let provider = PhysicalProvider::new(&compiler_dir);
    let unit = resolve_with_version(
        &provider,
        "fixtures/topaz_emit_py/atlas-poc/just_latin.tpz",
        None,
        LangVersion::V5_4,
    );
    assert!(
        unit.diagnostics.is_empty(),
        "just_latin.tpz did not resolve cleanly: {:?}",
        unit.diagnostics
    );
    let generated = emit_module(&unit).expect("just_latin.tpz emits to Python");
    assert_generated_python_gates(&generated)
        .expect("generated Python passes forbidden-operation and span checks");

    let tmp = temp_dir("topaz-py-just-latin");
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");
    let script = tmp.join("just_latin.py");
    fs::write(&script, generated).expect("write generated Python");

    for (name, input) in selected_just_latin_corpus() {
        let rust_stdout = run_reference(
            &compiler_dir.join("fixtures/topaz_emit_py/atlas-poc/just_latin_fp.rs"),
            &tmp.join(format!("just_latin_fp_{name}")),
            &input,
        );
        let expected_host_line = rust_stdout.trim_end_matches('\n');
        let interp_host = topaz_interp::TestHost::new();
        interp_host.set_input(input.clone());
        let interp_outcome = topaz_interp::Machine::run_unit(&unit, &interp_host);
        assert!(
            interp_outcome.is_ok(),
            "interpreter failed for {name}: {interp_outcome:?}"
        );
        assert_eq!(
            interp_host.stdout(),
            vec![expected_host_line.to_string()],
            "interpreter stdout mismatch for {name}"
        );
        let expected_trace = format!(
            "{{\"v\":1,\"status\":\"ok\",\"stdout\":[{}],\"files\":[],\"defer_errors\":[],\"fault\":null}}\n",
            json_string(expected_host_line)
        );
        let py = run_python(&python, &script, &input);
        assert!(
            py.status.success(),
            "generated Python exited nonzero for {name}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&py.stdout),
            String::from_utf8_lossy(&py.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&py.stderr), "", "{name} stderr");
        assert_eq!(
            String::from_utf8_lossy(&py.stdout),
            expected_trace,
            "Python trace mismatch for {name}"
        );
    }
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn linebreak_classify_tpz_runs_as_python_trace_under_cpython_31314() {
    let Some(python) = cpython_31314() else {
        eprintln!("skipping Python linebreak-classify witness: CPython 3.13.14 was not found");
        return;
    };
    let compiler_dir = compiler_dir();
    let provider = PhysicalProvider::new(&compiler_dir);
    let unit = resolve_with_version(
        &provider,
        "fixtures/topaz_emit_py/atlas-poc/linebreak-classify.tpz",
        None,
        LangVersion::V5_4,
    );
    assert!(
        unit.diagnostics.is_empty(),
        "linebreak-classify.tpz did not resolve cleanly: {:?}",
        unit.diagnostics
    );
    let generated = emit_module(&unit).expect("linebreak-classify.tpz emits to Python");
    assert_generated_python_gates(&generated)
        .expect("generated Python passes forbidden-operation and span checks");

    let tmp = temp_dir("topaz-py-linebreak-classify");
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");
    let script = tmp.join("linebreak_classify.py");
    fs::write(&script, generated).expect("write generated Python");
    let oracle = build_reference(
        &compiler_dir.join("fixtures/topaz_emit_py/atlas-poc/oracle.rs"),
        &tmp.join("oracle"),
    );

    let cases = selected_linebreak_classify_corpus(&python, &compiler_dir);
    assert_eq!(
        cases.len(),
        95,
        "linebreak-classify corpus drifted; update selected_linebreak_classify_corpus if intentional"
    );
    for (name, input) in cases {
        let rust_stdout = run_reference_bin(&oracle, &input);
        let expected_host_line = rust_stdout.trim_end_matches('\n');
        let interp_host = topaz_interp::TestHost::new();
        interp_host.set_input(input.clone());
        let interp_outcome = topaz_interp::Machine::run_unit(&unit, &interp_host);
        assert!(
            interp_outcome.is_ok(),
            "interpreter failed for {name}: {interp_outcome:?}"
        );
        assert_eq!(
            interp_host.stdout(),
            vec![expected_host_line.to_string()],
            "interpreter stdout mismatch for {name}"
        );
        let expected_trace = format!(
            "{{\"v\":1,\"status\":\"ok\",\"stdout\":[{}],\"files\":[],\"defer_errors\":[],\"fault\":null}}\n",
            json_string(expected_host_line)
        );
        let py = run_python(&python, &script, &input);
        assert!(
            py.status.success(),
            "generated Python exited nonzero for {name}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&py.stdout),
            String::from_utf8_lossy(&py.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&py.stderr), "", "{name} stderr");
        assert_eq!(
            String::from_utf8_lossy(&py.stdout),
            expected_trace,
            "Python trace mismatch for {name}"
        );
    }
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn dp_tpz_runs_as_python_trace_under_cpython_31314() {
    let Some(python) = cpython_31314() else {
        eprintln!("skipping Python dp witness: CPython 3.13.14 was not found");
        return;
    };
    let compiler_dir = compiler_dir();
    let provider = PhysicalProvider::new(&compiler_dir);
    let unit = resolve_with_version(
        &provider,
        "fixtures/topaz_emit_py/atlas-poc/dp.tpz",
        None,
        LangVersion::V5_4,
    );
    assert!(
        unit.diagnostics.is_empty(),
        "dp.tpz did not resolve cleanly: {:?}",
        unit.diagnostics
    );
    let generated = emit_module(&unit).expect("dp.tpz emits to Python");
    assert!(
        generated.contains("@dataclass(frozen=True, slots=True)"),
        "dp record literal should emit a frozen slotted dataclass"
    );
    assert!(
        generated.contains("tpz_member("),
        "dp field access should route through span-carrying member helper"
    );
    assert_generated_python_gates(&generated)
        .expect("generated Python passes forbidden-operation and span checks");

    let tmp = temp_dir("topaz-py-dp");
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");
    let script = tmp.join("dp.py");
    fs::write(&script, generated).expect("write generated Python");
    let reference = build_reference(
        &compiler_dir.join("fixtures/topaz_emit_py/atlas-poc/dp_fp.rs"),
        &tmp.join("dp_fp"),
    );

    let cases = selected_dp_corpus(&python, &compiler_dir);
    assert_eq!(
        cases.len(),
        38,
        "dp corpus drifted; update selected_dp_corpus if intentional"
    );
    for (name, input) in cases {
        let rust_stdout = run_reference_bin(&reference, &input);
        let expected_host_line = rust_stdout.trim_end_matches('\n');
        let interp_host = topaz_interp::TestHost::new();
        interp_host.set_input(input.clone());
        let interp_outcome = topaz_interp::Machine::run_unit(&unit, &interp_host);
        assert!(
            interp_outcome.is_ok(),
            "interpreter failed for {name}: {interp_outcome:?}"
        );
        assert_eq!(
            interp_host.stdout(),
            vec![expected_host_line.to_string()],
            "interpreter stdout mismatch for {name}"
        );
        let expected_trace = format!(
            "{{\"v\":1,\"status\":\"ok\",\"stdout\":[{}],\"files\":[],\"defer_errors\":[],\"fault\":null}}\n",
            json_string(expected_host_line)
        );
        let py = run_python(&python, &script, &input);
        assert!(
            py.status.success(),
            "generated Python exited nonzero for {name}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&py.stdout),
            String::from_utf8_lossy(&py.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&py.stderr), "", "{name} stderr");
        assert_eq!(
            String::from_utf8_lossy(&py.stdout),
            expected_trace,
            "Python trace mismatch for {name}"
        );
    }
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn topaz_py_rt_numeric_helpers_match_rust_leaf_vectors() {
    let Some(python) = cpython_31314() else {
        eprintln!("skipping Python runtime leaf vectors: CPython 3.13.14 was not found");
        return;
    };
    let tmp = temp_dir("topaz-py-rt-vectors");
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");
    let script = tmp.join("rt_vectors.py");
    fs::write(&script, numeric_vector_script()).expect("write vector script");
    let py = run_python(&python, &script, "");
    assert!(
        py.status.success(),
        "numeric vector script failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&py.stdout),
        String::from_utf8_lossy(&py.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&py.stderr), "");
    assert_eq!(
        String::from_utf8_lossy(&py.stdout),
        expected_numeric_vectors()
    );
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn topaz_py_rt_helpers_match_vectors() {
    let Some(python) = cpython_31314() else {
        eprintln!("skipping Python runtime helper vectors: CPython 3.13.14 was not found");
        return;
    };
    let tmp = temp_dir("topaz-py-rt-helper-vectors");
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");
    let script = tmp.join("rt_helper_vectors.py");
    fs::write(&script, runtime_helper_vector_script()).expect("write runtime helper vector script");
    let py = run_python(&python, &script, "");
    assert!(
        py.status.success(),
        "runtime helper vector script failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&py.stdout),
        String::from_utf8_lossy(&py.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&py.stderr), "");
    assert_eq!(
        String::from_utf8_lossy(&py.stdout),
        expected_runtime_helper_vectors()
    );
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn topaz_py_rt_record_update_helpers_match_rust_leaf_vectors() {
    let Some(python) = cpython_31314() else {
        eprintln!("skipping Python runtime record-update vectors: CPython 3.13.14 was not found");
        return;
    };
    let tmp = temp_dir("topaz-py-rt-record-update-vectors");
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");
    let script = tmp.join("rt_record_update_vectors.py");
    fs::write(&script, record_update_vector_script()).expect("write record update script");
    let py = run_python(&python, &script, "");
    assert!(
        py.status.success(),
        "record update vector script failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&py.stdout),
        String::from_utf8_lossy(&py.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&py.stderr), "");
    assert_eq!(
        String::from_utf8_lossy(&py.stdout),
        expected_record_update_vectors()
    );
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn topaz_py_rt_float_bits_and_render_golden_vectors_match_rust() {
    let Some(python) = cpython_31314() else {
        eprintln!("skipping Python runtime float vectors: CPython 3.13.14 was not found");
        return;
    };
    let tmp = temp_dir("topaz-py-rt-float-vectors");
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");
    let script = tmp.join("rt_float_vectors.py");
    fs::write(&script, float_vector_script()).expect("write float vector script");
    let py = run_python(&python, &script, "");
    assert!(
        py.status.success(),
        "float vector script failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&py.stdout),
        String::from_utf8_lossy(&py.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&py.stderr), "");
    assert_eq!(
        String::from_utf8_lossy(&py.stdout),
        expected_float_vectors()
    );
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn generated_python_gates_reject_raw_ops_and_missing_spans() {
    assert_generated_python_gates("x = tpz_add_i64(a, b, (0, 1, 2))\n").unwrap();
    for bad in [
        "x = a + b\n",
        "x = a // b\n",
        "x = a % b\n",
        "x = a ** b\n",
        "x = tpz_add_i64(a, b)\n",
        "x = tpz_index(xs, i)\n",
        "x = tpz_index_slot(xs, i)\n",
        "x = tpz_immutable_assignment('xs')\n",
        "x = tpz_array_get(xs, i)\n",
        "x = tpz_string_byte_length(s)\n",
        "x = tpz_impossible_match(v)\n",
        "x = tpz_record_field(r, f, s)\n",
        "x = open(path)\n",
        "x = str(value)\n",
    ] {
        assert!(
            assert_generated_python_gates(bad).is_err(),
            "gate should reject {bad:?}"
        );
    }
    assert_generated_python_gates("x = \"a + b // c % d ** e\"\n").unwrap();
}

#[test]
fn emits_array_index_assignment_through_checked_slot_helper() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut xs = [1, 2, 3]
    xs[0] = 9
    xs[0] * 100 + xs[1] * 10 + xs[2]
}
main()
"#,
    );
    assert_generated_python_ok_int(&generated, 923, "array index assignment");
}

#[test]
fn immutable_array_index_assignment_emits_guard_immutable_fault() {
    let generated = emit_source(
        r#"
function main() -> int {
    let xs = [1]
    xs[0] = 9
    0
}
main()
"#,
    );
    assert_generated_python_fault_code(&generated, "TPZ5003", "immutable direct-index assignment");

    let record_path = emit_source(
        r#"
function main() -> int {
    let record = { nested: { value: 1 } }
    record.nested.value = 2
    0
}
main()
"#,
    );
    assert_generated_python_fault_code(&record_path, "TPZ5003", "immutable record-path assignment");

    let cell_path = emit_source(
        r#"
function main() -> int {
    let rows = [{ value: 1 }]
    rows[0].value = 2
    0
}
main()
"#,
    );
    assert_generated_python_fault_code(
        &cell_path,
        "TPZ5003",
        "immutable record cell-path assignment",
    );
}

#[test]
fn emits_compound_array_index_assignment_with_current_before_rhs() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut xs = [1, 2, 3]
    xs[0] += xs[1]
    xs[0]
}
main()
"#,
    );
    assert_generated_python_ok_int(&generated, 3, "compound array index assignment");
}

#[test]
fn compound_array_index_assignment_reads_current_before_statement_lowered_rhs() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut xs = [1]
    xs[0] += loop {
        xs[0] = 10
        break 2
    }
    xs[0]
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        3,
        "compound index assignment current-before-statement-lowered-RHS parity",
    );
}

#[test]
fn compound_array_index_assignment_uses_general_stage0_arithmetic() {
    let generated = emit_source(
        r#"
function main() -> string {
    let mut strings = ["a"]
    strings[0] += "b"
    let mut floats = [1.5]
    floats[0] *= 2.0
    if floats[0] == 3.0 {
        strings[0]
    } else {
        "wrong"
    }
}
main()
"#,
    );
    assert_generated_python_ok_string(
        &generated,
        "ab",
        "compound index assignment general Stage 0 arithmetic parity",
    );
}

#[test]
fn emits_variable_compound_assignment_with_current_before_rhs() {
    let generated = emit_source(
        r#"
function main() -> string {
    let mut x = 1
    x += loop {
        x = 10
        break 2
    }
    let mut s = "a"
    s += "b"
    let direct = "c" + "d"
    "{x}:{s}:{direct}"
}
main()
"#,
    );
    assert_generated_python_ok_string(
        &generated,
        "3:ab:cd",
        "variable compound assignment current-before-RHS and general arithmetic parity",
    );
}

#[test]
fn simple_identifier_coalescing_assignment_is_lazy_for_captured_cells() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut value: int | null = null
    function fill() {
        value ??= loop { break 7 }
    }
    fill()
    value ??= loop {
        print("wrong")
        break 9
    }
    value
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        7,
        "simple identifier coalescing assignment lazy captured-cell parity",
    );
}

#[test]
fn emits_coalescing_array_index_assignment_with_lazy_rhs() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut xs = [null, 2]
    xs[0] ??= 7
    xs[1] ??= 9
    xs[0] * 10 + xs[1]
}
main()
"#,
    );
    assert_generated_python_ok_int(&generated, 72, "coalescing array index assignment");
}

#[test]
fn non_identifier_root_index_assignment_matches_stage0_writable_cells() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut values = [1, null]
    (if true { values } else { values })[0] += 2
    (if true { values } else { values })[1] ??= 4
    values[0] * 10 + values[1]
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        34,
        "non-identifier-root index assignment writable-cell parity",
    );
}

#[test]
fn direct_index_assignment_supports_statement_lowered_object_and_index() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut values = [10, 20]
    (loop { break values })[loop { break 1 }] += 2
    values[1]
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        22,
        "statement-lowered direct-index object and index parity",
    );
}

#[test]
fn float_literals_emit_by_f64_bits_and_trace_final_value() {
    let cases = [
        ("positive", "1.5", "0x3ff8000000000000"),
        ("negative_zero", "-0.0", "0x8000000000000000"),
        ("unary_plus_zero", "+0.0", "0x0000000000000000"),
        ("below_1e15", "999999999999999.0", "0x430c6bf52633fff8"),
        ("at_1e15", "1000000000000000.0", "0x430c6bf526340000"),
        ("round_trip_hard", "0.1", "0x3fb999999999999a"),
    ];
    for (name, src, bits) in cases {
        let generated = emit_source(src);
        assert!(
            generated.contains(&format!("__tpz_value = tpz_f64_from_bits({bits})")),
            "{name}: generated Python should preserve literal bits, got:\n{generated}"
        );
        assert!(
            generated.contains("return host.trace_ok(__tpz_value)"),
            "{name}: final float literal should be forwarded to trace value"
        );
        assert_generated_python_gates(&generated)
            .unwrap_or_else(|e| panic!("{name}: generated Python gate failed: {e}"));
    }
}

#[test]
fn top_level_method_call_emits_trace_value() {
    let generated = emit_source(r#""a,b,c,".byteLength()"#);
    assert!(
        generated.contains("__tpz_value = tpz_string_byte_length(\"a,b,c,\", "),
        "{generated}"
    );
    assert!(
        generated.contains("return host.trace_ok(__tpz_value)"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("top-level method trace gate failed: {e}"));
}

#[test]
fn top_level_builtin_side_effect_if_and_match_do_not_report_trace_value() {
    let if_generated = emit_source(
        r#"
let choose = true
if choose {
    print("then")
} else {
    print("else")
}
"#,
    );
    assert!(
        if_generated.contains("return host.trace_ok()\n"),
        "{if_generated}"
    );
    assert!(
        !if_generated.contains("__tpz_value =")
            && !if_generated.contains("return host.trace_ok(__tpz_value)"),
        "{if_generated}"
    );
    assert_generated_python_gates(&if_generated)
        .unwrap_or_else(|e| panic!("top-level side-effect if no-trace gate failed: {e}"));

    let match_generated = emit_source(
        r#"
let n = 1
match n {
    case 1 => print("one")
    case _ => print("other")
}
"#,
    );
    assert!(
        match_generated.contains("return host.trace_ok()\n"),
        "{match_generated}"
    );
    assert!(
        !match_generated.contains("__tpz_value =")
            && !match_generated.contains("return host.trace_ok(__tpz_value)"),
        "{match_generated}"
    );
    assert_generated_python_gates(&match_generated)
        .unwrap_or_else(|e| panic!("top-level side-effect match no-trace gate failed: {e}"));
}

#[test]
fn top_level_if_tail_nested_concurrent_reports_trace_value() {
    let generated = emit_source(
        r#"
let choose = true
if choose {
    concurrent {
        x: 1
        y: 2
    }
} else {
    { x: 3, y: 4 }
}
"#,
    );
    assert!(
        generated.contains("# concurrent x") && generated.contains("# concurrent y"),
        "nested concurrent tail should lower through the target path: {generated}"
    );
    assert!(
        generated.contains("return host.trace_ok(__tpz_value)"),
        "nested concurrent tail should be reported as the final trace value: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("top-level nested concurrent trace gate failed: {e}"));
}

#[test]
fn string_level1_methods_emit_through_python_helpers() {
    let generated = emit_source(
        r#"
function main() -> string {
    let text = "  banana  "
    let opt: Option<string> = Some("  hi  ")
    let none: Option<string> = None
    let piped = "na" |> "banana".lastIndexOf()
    let trimmed = opt?.trim() ?? "none"
    let skipped = none?.replace("x", "y") ?? "none"
    let a = text.trim()
    let b = text.trimStart()
    let c = text.trimEnd()
    let d = text.startsWith(prefix: "  b")
    let e = text.endsWith("  ")
    let f = text.contains("nan")
    let g = text.indexOf("na")
    let h = text.lastIndexOf("na")
    let i = text.slice(2, 8)
    let j = text.replace("na", "NA")
    "{a}/{b}/{c}/{d}/{e}/{f}/{g}/{h}/{i}/{j}/{piped}/{trimmed}/{skipped}"
}
main()
"#,
    );
    for needle in [
        "tpz_string_trim(",
        "tpz_string_trim_start(",
        "tpz_string_trim_end(",
        "tpz_string_starts_with(",
        "tpz_string_ends_with(",
        "tpz_string_contains(",
        "tpz_string_index_of(",
        "tpz_string_last_index_of(",
        "tpz_string_slice(",
        "tpz_string_replace(",
    ] {
        assert!(
            generated.contains(needle),
            "{needle} missing from:\n{generated}"
        );
    }
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("string level-1 helper Python gate failed: {e}"));
}

#[test]
fn emits_optional_member_access() {
    let generated = emit_source(
        r#"
function main() {
    let r: Option<{ n: int }> = Some({ n: 7 })
    let none: Option<{ n: int }> = None
    let nested: Option<{ inner: Option<{ n: int }> }> = Some({ inner: Some({ n: 5 }) })
    { a: r?.n, b: none?.n, c: nested?.inner?.n }
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_optional_member(_t_72, \"_t_6e\", \"n\","),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_optional_member(tpz_optional_member(_t_6e6573746564,"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("optional access Python gate failed: {e}"));
}

#[test]
fn emits_optional_receiver_readonly_value_in_expression_positions() {
    let generated = emit_source(
        r#"
function mark(label: string, value: Option<{ n: int, text: string }>) -> Option<{ n: int, text: string }> {
    print(label)
    value
}
function fallbackInt(label: string, value: int) -> int {
    print(label)
    value
}
function fallbackText(label: string, value: string) -> string {
    print(label)
    value
}
function main() -> string {
    let some = mark("some", Some({ n: 0, text: "" }))
    let none = mark("none", None)
    let value = {
        zero: some?.n ?? fallbackInt("zero-fallback", 7),
        empty: some?.text ?? fallbackText("text-fallback", "fallback"),
        missing: none?.n ?? fallbackInt("missing-fallback", 9)
    }
    "{value.zero}:{value.empty}:{value.missing}"
}
main()
"#,
    );
    assert!(
        generated.matches("tpz_optional_member(").count() >= 3,
        "{generated}"
    );
    assert!(
        generated.contains("tpz_coalesce("),
        "optional member values should flow through coalesce as real option values: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("optional receiver value expression gate failed: {e}"));
}

#[test]
fn emits_optional_receiver_readonly_value_in_pipe_stage_arguments() {
    let generated = emit_source(
        r#"
function mark(label: string, value: Option<{ n: int, text: string }>) -> Option<{ n: int, text: string }> {
    print(label)
    value
}
function markInt(label: string, value: int) -> int {
    print(label)
    value
}
function markText(label: string, value: string) -> string {
    print(label)
    value
}
function add(a: int, b: int) -> int {
    a + b
}
function label(prefix: string, value: string) -> string {
    "{prefix}:{value}"
}
function main() -> string {
    let some = mark("some-pipe", Some({ n: 0, text: "" }))
    let none = mark("none-pipe", None)
    let zero = markInt("lhs-zero", 5) |> add(some?.n ?? markInt("zero-fallback", 7))
    let missing = markInt("lhs-missing", 5) |> add(none?.n ?? markInt("missing-fallback", 9))
    let empty = markText("lhs-empty", "x") |> label(some?.text ?? markText("text-fallback", "fallback"))
    "{zero}:{missing}:{empty}"
}
main()
"#,
    );
    assert!(
        generated.contains("(lambda __tpz_piped:"),
        "pipe stage should stay on the pipe lowering path: {generated}"
    );
    assert!(
        generated.matches("tpz_optional_member(").count() >= 3
            && generated.contains("tpz_coalesce("),
        "pipe-stage arguments should preserve optional member values: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("optional receiver value pipe gate failed: {e}"));
}

#[test]
fn emits_optional_readonly_calls() {
    let generated = emit_source(
        r#"
function mark(label: string, n: int) -> int {
    print(label)
    n
}
function main() -> Result<string, string> {
    let xs: Option<Array<int>> = Some([4, 5])
    let noneXs: Option<Array<int>> = None
    let m: Option<Map<string, int>> = Some(map { "a": 2 })
    let bytes: Option<Bytes> = Some(Bytes.encodeUtf8(s: "AZ"))
    let json: Option<JSONValue> = Some(JSON.parse(text: "\{\"x\":3,\"arr\":[10,20]\}")?)
    let got = match xs?.get(i: mark("idx", 1)) {
        case Some(n) => n
        case None => 0
    }
    let skipped = match noneXs?.get(i: mark("skip", 0)) {
        case Some(n) => n
        case None => 7
    }
    let mapValue = m?.getOr(default: 9, k: "a") ?? 0
    let decoded = match bytes?.decodeUtf8() {
        case Some(Ok(text)) => text
        case _ => "none"
    }
    let jsonInt = match json?.get(key: "x") {
        case Some(value) => match value.asInt() {
            case Some(n) => n
            case None => 0
        }
        case None => 0
    }
    Ok(value: "{got}:{skipped}:{mapValue}:{decoded}:{jsonInt}")
}
main()
"#,
    );
    assert!(
        generated.contains("None if __tpz_obj is None else"),
        "optional call should short-circuit None before args: {generated}"
    );
    assert!(
        generated.contains("tpz_wrap_optional(tpz_get(__tpz_obj.value"),
        "Option<Array>.get should call get on Some inner and rewrap: {generated}"
    );
    assert!(
        generated.contains("tpz_map_get_or(__tpz_call_recv"),
        "Option<Map>.getOr should call Map helper on Some inner: {generated}"
    );
    assert!(
        generated.contains("tpz_bytes_decode_utf8(__tpz_obj.value"),
        "Option<Bytes>.decodeUtf8 should call Bytes helper on Some inner: {generated}"
    );
    assert!(
        generated.contains("tpz_get(__tpz_obj.value, \"x\""),
        "Option<JSONValue>.get should call generic get on Some inner: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("optional readonly call Python gate failed: {e}"));
}

#[test]
fn emits_optional_receiver_readonly_spread_faults() {
    let generated = emit_source(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
    print(label)
    xs
}
function main() -> string {
    let some: Option<Array<int>> = Some([10])
    let none: Option<Array<int>> = None
    let a = some?.get(...mark("some", [0]))
    let b = none?.get(...mark("none", [0]))
    "{a}:{b}"
}
main()
"#,
    );
    assert!(
        generated.contains("None if __tpz_obj is None else"),
        "{generated}"
    );
    assert!(
        generated
            .matches("tpz_nonvariadic_static_spread_call(")
            .count()
            >= 2,
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("optional receiver spread Python gate failed: {e}"));

    let named_tail = emit_source(
        r#"
function markArray(label: string, xs: Array<int>) -> Array<int> {
    print(label)
    xs
}
function markInt(label: string, value: int) -> int {
    print(label)
    value
}
function main() -> string {
    let some: Option<Array<int>> = Some([10])
    let none: Option<Array<int>> = None
    let skipped = none?.get(...markArray("skip", [0]), i: markInt("skip-named", 0))
    let hit = some?.get(...markArray("spread", [0]), i: markInt("named", 0))
    "{skipped}:{hit}"
}
main()
"#,
    );
    assert!(
        named_tail.contains("None if __tpz_obj is None else")
            && named_tail.contains("tpz_nonvariadic_static_spread_call(")
            && named_tail.contains("[(\"i\","),
        "{named_tail}"
    );
    assert_generated_python_gates(&named_tail)
        .unwrap_or_else(|e| panic!("optional receiver spread+named Python gate failed: {e}"));

    let order_fault = emit_unchecked_source(
        r#"
function markArray(label: string, xs: Array<int>) -> Array<int> {
    print(label)
    xs
}
function markInt(label: string, value: int) -> int {
    print(label)
    value
}
function main() -> string {
    let some: Option<Array<int>> = Some([10])
    let hit = some?.get(i: markInt("named", 0), ...markArray("spread", [0]))
    "{hit}"
}
main()
"#,
    );
    assert!(
        order_fault.contains("tpz_call_order_fault(")
            && order_fault.contains("named arguments must follow spread arguments (§5)")
            && !order_fault.contains("*tpz_spread_values("),
        "{order_fault}"
    );
    assert_generated_python_gates(&order_fault).unwrap_or_else(|e| {
        panic!("optional receiver named-before-spread Python gate failed: {e}")
    });
}

#[test]
fn emits_optional_string_readonly_calls() {
    let generated = emit_source(
        r#"
function mark(label: string, n: int) -> int {
    print(label)
    n
}
function main() -> int {
    let text: Option<string> = Some("한,ab")
    let noneText: Option<string> = None
    let parts = match text?.split(sep: ",") {
        case Some(xs) => xs.length
        case None => 0
    }
    let cp = match text?.codePointAt(i: 0) {
        case Some(n) => n
        case None => 0
    }
    let bytes = text?.byteLength() ?? 0
    let scalars = text?.scalars()?.length ?? 0
    let skipped = match noneText?.codePointAt(i: mark("skip", 0)) {
        case Some(n) => n
        case None => 7
    }
    parts + cp + bytes + scalars + skipped
}
main()
"#,
    );
    assert!(
        generated.contains("None if __tpz_obj is None else"),
        "optional string calls should short-circuit None before args: {generated}"
    );
    assert!(
        generated.contains("tpz_string_split(__tpz_obj.value"),
        "Option<string>.split should call the string helper on Some inner: {generated}"
    );
    assert!(
        generated.contains("tpz_string_code_point_at(__tpz_obj.value"),
        "Option<string>.codePointAt should call the string helper on Some inner: {generated}"
    );
    assert!(
        generated.contains("tpz_string_byte_length(__tpz_obj.value"),
        "Option<string>.byteLength should call the string helper on Some inner: {generated}"
    );
    assert!(
        generated.contains("tpz_wrap_optional(list(__tpz_obj.value))"),
        "Option<string>.scalars should wrap the scalar array once: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("optional string readonly call Python gate failed: {e}"));
}
