use super::*;

#[test]
fn parse_trace_v1_accepts_stage0_ok_envelope() {
    let trace = parse_trace_v1(
            "{\"v\":1,\"status\":\"ok\",\"stdout\":[\"a\",\"b\"],\"files\":[],\"defer_errors\":[],\"fault\":null}",
        )
        .expect("trace v1 ok parses");
    assert_eq!(trace.version, 1);
    assert_eq!(trace.status, "ok");
    assert_eq!(trace.stdout, vec!["a".to_string(), "b".to_string()]);
    assert!(trace.files.is_empty());
    assert!(trace.defer_errors.is_empty());
    assert!(trace.fault.is_none());
    assert!(trace.value.is_none());
}

#[test]
fn parse_trace_v1_accepts_fault_files_defer_errors_and_value() {
    let trace = parse_trace_v1(
            "{\"v\":1,\"status\":\"fault\",\"stdout\":[],\"files\":[{\"path\":\"out.txt\",\"content\":{\"str\":\"hello\"}}],\"defer_errors\":[{\"rendered\":\"TPZ4002: integer division by zero\",\"fault\":{\"code\":\"TPZ4002\",\"message\":\"integer division by zero\",\"span\":{\"file\":7,\"lo\":11,\"hi\":13}}}],\"fault\":{\"code\":\"TPZ5001\",\"message\":\"condition must be `bool`\",\"span\":{\"file\":7,\"lo\":20,\"hi\":24}},\"value\":{\"f64\":\"8000000000000000\"}}",
        )
        .expect("trace v1 broad shape parses");
    assert_eq!(trace.status, "fault");
    assert_eq!(trace.files.len(), 1);
    assert_eq!(trace.files[0].path, "out.txt");
    assert_eq!(trace.files[0].content, TraceValue::Str("hello".to_string()));
    assert_eq!(trace.defer_errors.len(), 1);
    assert_eq!(
        trace.defer_errors[0].rendered,
        "TPZ4002: integer division by zero"
    );
    assert_eq!(
        trace.defer_errors[0]
            .fault
            .as_ref()
            .expect("defer fault")
            .code,
        "TPZ4002"
    );
    assert_eq!(trace.fault.as_ref().expect("top-level fault").span.lo, 20);
    assert_eq!(trace.value, Some(TraceValue::F64(0x8000_0000_0000_0000)));
}

#[test]
fn parse_trace_v1_accepts_range_values() {
    let trace = parse_trace_v1(
            "{\"v\":1,\"status\":\"ok\",\"stdout\":[],\"files\":[],\"defer_errors\":[],\"fault\":null,\"value\":{\"range\":{\"lo\":5,\"hi\":1,\"inclusive\":true,\"step\":-2}}}",
        )
        .expect("trace v1 range value parses");
    assert_eq!(
        trace.value,
        Some(TraceValue::Range {
            lo: 5,
            hi: 1,
            inclusive: true,
            step: -2,
        })
    );
}

#[test]
fn parse_trace_v1_rejects_noncanonical_f64_tags() {
    for bad in [
        "{\"f64\":\"800000000000000\"}",
        "{\"f64\":\"80000000000000000\"}",
        "{\"f64\":\"800000000000000G\"}",
        "{\"f64\":\"800000000000000A\"}",
        "{\"f64\":\"0x8000000000000000\"}",
        "{\"f64\":\"-0.0\"}",
    ] {
        let trace = format!(
            "{{\"v\":1,\"status\":\"ok\",\"stdout\":[],\"files\":[],\"defer_errors\":[],\"fault\":null,\"value\":{bad}}}"
        );
        assert!(
            parse_trace_v1(&trace).is_err(),
            "noncanonical f64 should fail: {bad}"
        );
    }
}

#[test]
fn parse_trace_v1_rejects_unknown_value_tags() {
    let trace = "{\"v\":1,\"status\":\"ok\",\"stdout\":[],\"files\":[],\"defer_errors\":[],\"fault\":null,\"value\":{\"future\":\"x\"}}";
    assert!(parse_trace_v1(trace).is_err());
}

#[test]
fn parse_trace_v1_preserves_nan_f64_bits() {
    let trace = parse_trace_v1(
            "{\"v\":1,\"status\":\"ok\",\"stdout\":[],\"files\":[],\"defer_errors\":[],\"fault\":null,\"value\":{\"f64\":\"7ff8000000000000\"}}",
        )
        .expect("trace v1 NaN f64 bits parse");
    assert_eq!(trace.value, Some(TraceValue::F64(0x7ff8_0000_0000_0000)));
}

#[test]
fn compare_python_trace_rejects_one_ulp_f64_mismatch() {
    let trace = parse_trace_v1(
            "{\"v\":1,\"status\":\"ok\",\"stdout\":[],\"files\":[],\"defer_errors\":[],\"fault\":null,\"value\":{\"f64\":\"3ff0000000000000\"}}",
        )
        .expect("trace v1 f64 parses");
    let mut traces = BTreeMap::new();
    traces.insert("one".to_string(), trace);
    let case = Case {
        name: "one".to_string(),
        input: String::new(),
    };
    let mut failures = Vec::new();
    compare_python_trace(
        "float",
        &case,
        &traces,
        &[],
        Some(TraceValue::F64(0x3ff0_0000_0000_0001)),
        &mut failures,
    );
    assert!(
        failures
            .iter()
            .any(|failure| failure.contains("trace value mismatch")),
        "1-ULP mismatch should fail, got {failures:?}"
    );
}

#[test]
fn linebreaker_python_column_matches_interpreter_and_rust() {
    let python = cpython_31314();
    let compiler_dir = compiler_dir();
    let tmp = temp_dir("topaz-difftest-py-linebreaker");
    fs::create_dir_all(&tmp).expect("create temp dir");

    let mut failures = Vec::new();
    let mut total_cases = 0usize;
    for fixture in FIXTURES {
        let cases = load_cases(&python, &compiler_dir, fixture);
        assert_eq!(
            cases.len(),
            fixture.expected_cases,
            "{} corpus drifted",
            fixture.name
        );
        total_cases += cases.len();
        if let Err(error) = run_fixture(&python, &compiler_dir, &tmp, fixture, &cases) {
            failures.push(error);
        }
    }

    let _ = fs::remove_dir_all(&tmp);
    assert_eq!(FIXTURES.len(), PYTHON_FIXTURE_COUNT);
    assert_eq!(
        total_cases, PYTHON_CASE_COUNT,
        "Python line-breaker corpus count drifted"
    );
    assert!(
        failures.is_empty(),
        "Python differential mismatches:\n{}",
        failures.join("\n")
    );
}
