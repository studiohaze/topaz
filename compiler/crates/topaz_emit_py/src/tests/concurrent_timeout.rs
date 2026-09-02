use super::*;

#[test]
fn emits_concurrent_nonzero_timeout_join_record() {
    let generated = emit_source(
        r#"
function main() -> int {
    let r = concurrent(timeout: 1m) {
        x: 1 + 1
        y: 2 * 3
    } else {
        -1
    }
    print("{r.x} {r.y}")
    0
}
main()
"#,
    );
    assert!(
        generated.contains("_tr_78_79("),
        "non-zero timeout with instant arms should construct the arm-result record: {generated}"
    );
    assert!(
        !generated.contains("concurrent timeout"),
        "non-zero timeout with instant arms should not hit the timeout decline: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("concurrent non-zero timeout Python gate failed: {e}"));
}

#[test]
fn refuses_a_concurrent_timeout_duration_that_overflows() {
    let mut provider = InMemoryProvider::new();
    provider.add_file(
        "main.tpz",
        "concurrent(timeout: 307445734561826m) {\n    a: 1\n} else {\n    { a: 0 }\n}",
    );
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_4);
    let error = emit_module(&unit).expect_err("overflowing duration must not be emitted");
    assert_eq!(
        error.kind,
        PyEmitErrorKind::Unsupported("concurrent timeout duration overflows u64 milliseconds")
    );
}

#[test]
fn emits_concurrent_zero_timeout_single_instant_fault_else() {
    let cases = [
        (
            "small instant record",
            r#"
function main() -> int {
    let r = concurrent(timeout: 0ms) {
        x: 21 * 2
    } else {
        { x: 0 }
    }
    r.x
}
main()
"#,
            42,
        ),
        (
            "fault fallback",
            r#"
function main() -> int {
    let r = concurrent(timeout: 0ms) {
        x: 1 / 0
    } else {
        { x: 7 }
    }
    r.x
}
main()
"#,
            7,
        ),
        (
            "large instant record",
            r#"
function main() -> int {
    let r = concurrent(timeout: 0ms) {
        x: [0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29]
    } else {
        { x: [-1] }
    }
    r.x[0]
}
main()
"#,
            0,
        ),
        (
            "dynamic index fallback",
            r#"
function main() -> int {
    let r = concurrent(timeout: 0ms) {
        x: {
            let xs = [5]
            xs[1]
        }
    } else {
        { x: 9 }
    }
    r.x
}
main()
"#,
            9,
        ),
    ];
    for (name, src, expected) in cases {
        let generated = emit_source(src);
        assert!(
            generated.contains("try:") && generated.contains("except TpzFault:"),
            "{name}: zero-timeout single-arm instant path should guard fault fallback: {generated}"
        );
        assert!(
            generated.contains("_tr_78("),
            "{name}: zero-timeout single-arm instant path should construct an arm record: {generated}"
        );
        assert!(
            !generated.contains("concurrent timeout"),
            "{name}: zero-timeout single-arm instant path should not decline: {generated}"
        );
        assert_generated_python_ok_int(&generated, expected, name);
    }
}

#[test]
fn emits_concurrent_zero_timeout_multi_instant_else() {
    let generated = emit_source(
        r#"
function main() -> int {
    let r = concurrent(timeout: 0ms) {
        x: 1 + 1
        y: 2 * 3
    } else {
        { x: 0, y: 0 }
    }
    print("{r.x},{r.y}")
    0
}
main()
"#,
    );
    assert!(
        generated.contains("_tr_78_79(0, 0"),
        "zero-timeout multi-arm path should emit the else record: {generated}"
    );
    assert!(
        !generated.contains("# concurrent x") && !generated.contains("# concurrent y"),
        "zero-timeout multi-arm path should abandon arm lowering and emit else only: {generated}"
    );
    assert_generated_python_gates(&generated).unwrap_or_else(|e| {
        panic!("concurrent zero-timeout multi-arm else Python gate failed: {e}")
    });
}

#[test]
fn emits_concurrent_zero_timeout_multi_block_arm_else() {
    let generated = emit_source(
        r#"
function main() -> int {
    let r = concurrent(timeout: 0ms) {
        x: {
            let a = 1
            a
        }
        y: 2
    } else {
        { x: 0, y: 0 }
    }
    r.x
}
main()
"#,
    );
    assert!(
        generated.contains("_tr_78_79(0, 0"),
        "zero-timeout multi-arm path should emit the else record for instant block arms: {generated}"
    );
    assert!(
        !generated.contains("# concurrent x") && !generated.contains("# concurrent y"),
        "zero-timeout multi-arm block path should abandon arm lowering and emit else only: {generated}"
    );
    assert_generated_python_gates(&generated).unwrap_or_else(|e| {
        panic!("concurrent zero-timeout multi block else Python gate failed: {e}")
    });
}

#[test]
fn concurrent_zero_timeout_multi_noninstant_runs_else() {
    let generated = emit_source(
        r#"
function f() -> int { 1 }
function main() -> int {
    let r = concurrent(timeout: 0ms) {
        x: f()
        y: 2
    } else {
        { x: 0, y: 0 }
    }
    r.x
}
main()
"#,
    );
    assert_generated_python_ok_int(&generated, 0, "zero-timeout multi non-instant else");
}

#[test]
fn concurrent_zero_timeout_single_noninstant_completes_before_expiry_sample() {
    let generated = emit_source(
        r#"
function f() -> int { 1 }
function main() -> int {
    let r = concurrent(timeout: 0ms) {
        x: f()
    } else {
        { x: 0 }
    }
    r.x
}
main()
"#,
    );
    assert_generated_python_ok_int(&generated, 1, "zero-timeout single non-instant join");
}

#[test]
fn emits_concurrent_else_bare_return_paths() {
    let zero_multi = emit_source(
        r#"
function main() -> int {
    let r = concurrent(timeout: 0ms) {
        x: 1
        y: 2
    } else {
        return 99
    }
    r.x
}
main()
"#,
    );
    assert!(
        zero_multi.contains("raise TpzReturn(99)") && zero_multi.contains("_tr_78_79"),
        "zero-timeout multi-instant else return should lower through TpzReturn: {zero_multi}"
    );
    assert!(
        !zero_multi.contains("# concurrent x") && !zero_multi.contains("# concurrent y"),
        "zero-timeout multi-instant else return should not lower abandoned arms: {zero_multi}"
    );
    assert_generated_python_gates(&zero_multi).unwrap_or_else(|e| {
        panic!("concurrent zero-timeout multi else return Python gate failed: {e}")
    });

    assert!(
        PY_RT.contains("class TpzFault(Exception):")
            && PY_RT.contains("class TpzReturn(Exception):"),
        "TpzFault and TpzReturn must remain sibling runtime exceptions"
    );

    let nonzero_instant = emit_source(
        r#"
function main() -> int {
    let r = concurrent(timeout: 1m) {
        x: 21 * 2
    } else {
        return 77
    }
    r.x
}
main()
"#,
    );
    assert!(
        nonzero_instant.contains("tpz_concurrent_join_timeout(")
            && nonzero_instant.contains("TpzReturn(77)"),
        "non-zero instant concurrent should retain a lazy else thunk: {nonzero_instant}"
    );
    assert_generated_python_ok_int(&nonzero_instant, 42, "non-zero instant timeout join");
}

#[test]
fn emits_zero_timeout_single_concurrent_else_return_paths() {
    let success = emit_source(
        r#"
function main() -> int {
    let r = concurrent(timeout: 0ms) {
        x: [0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29]
    } else {
        return 88
    }
    r.x[0]
}
main()
"#,
    );
    assert!(
        success.contains("except TpzFault:") && success.contains("raise TpzReturn(88)"),
        "zero-timeout single success path should lower the else return only behind the fault handler: {success}"
    );
    assert_generated_python_ok_int(
        &success,
        0,
        "zero-timeout single instant else return success",
    );

    let fault = emit_source(
        r#"
function main() -> int {
    let r = concurrent(timeout: 0ms) {
        x: {
            let xs = [5]
            xs[1]
        }
    } else {
        return 88
    }
    r.x
}
main()
"#,
    );
    assert!(
        fault.contains("except TpzFault:") && fault.contains("raise TpzReturn(88)"),
        "zero-timeout single fault path should lower the else return through TpzReturn: {fault}"
    );
    assert_generated_python_ok_int(&fault, 88, "zero-timeout single instant fault else return");
}

#[test]
fn zero_timeout_single_concurrent_else_try_uses_timeout_thunk() {
    let generated = emit_source(
        r#"
function fail() -> Result<int, string> {
    Err("boom")
}
function main() -> Result<int, string> {
    let r = concurrent(timeout: 0ms) {
        x: 1
    } else {
        { x: fail()? }
    }
    Ok(r.x)
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_concurrent_join_timeout(") && generated.contains("tpz_try("),
        "zero-timeout single else `?` must lower through the lazy timeout thunk: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|error| panic!("zero-timeout single else `?` gate failed: {error}"));
}
