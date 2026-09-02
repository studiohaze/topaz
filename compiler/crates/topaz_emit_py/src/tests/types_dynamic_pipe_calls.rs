use super::*;

#[test]
fn emits_homogeneous_dynamic_index_variadic_pipe_function_value_calls() {
    let generated = emit_source(
        r#"
function sum(seed: int = 0, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function main() -> int {
    let seeds = [0, 1]
    let i = seeds[1]
    let j = seeds[0]
    let callbacks = [sum, sum]
    let a = 1 |> callbacks[i](seed: _)
    let b = 2 |> callbacks[j](...[3, 4], seed: _)
    a * 10 + b
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(")
            && generated.contains("tpz_index(")
            && generated.contains("tpz_spread_values([3, 4],")
            && generated.contains("\"_t_7873\": [*__tpz_vararg_")
            && generated.contains("\"_t_73656564\": __tpz_vararg_"),
        "homogeneous dynamic-index variadic pipe calls should preserve named fixed and spread-tail metadata: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        19,
        "homogeneous dynamic-index variadic pipe function-value parity",
    );
}

#[test]
fn emits_statement_lowered_homogeneous_dynamic_index_variadic_pipe_function_value_calls() {
    let generated = emit_source(
        r#"
function sum(seed: int = 0, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function main() -> int {
    let seeds = [0, 1]
    let i = seeds[1]
    let j = seeds[0]
    let callbacks = [sum, sum]
    let a = loop {
        break 1
    } |> callbacks[i](seed: _)
    let b = loop {
        break 2
    } |> callbacks[j](...loop {
        break [3, 4]
    }, seed: _)
    a * 10 + b
}
main()
"#,
    );
    assert!(
        generated.contains("__tpz_pipe_value_")
            && generated.contains("__tpz_call_callee_")
            && generated.contains("__tpz_call_spread_")
            && generated.contains("tpz_call(__tpz_call_callee_")
            && generated.contains("tpz_index(")
            && generated.contains("\"_t_7873\": [*__tpz_call_spread_")
            && generated.contains("\"_t_73656564\": __tpz_call_arg_"),
        "statement-lowered homogeneous dynamic-index variadic pipe calls should bind the dynamic callee, piped value, and spread values before tpz_call: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        19,
        "statement-lowered homogeneous dynamic-index variadic pipe function-value parity",
    );
}

#[test]
fn emits_statement_lowered_dynamic_index_variadic_pipe_side_effect_order() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut order = 0

    function tick(label: int, value: int) -> int {
        order = order * 10 + label
        value
    }

    function pack(label: int, a: int, b: int) -> Array<int> {
        order = order * 10 + label
        [a, b]
    }

    function sum(seed: int = 0, ...xs: int) -> int {
        order = order * 10 + 4
        let mut total = seed
        for x in xs {
            total = total + x
        }
        total
    }

    let callbacks = [sum, sum]
    let value = loop {
        break tick(1, 1)
    } |> callbacks[tick(2, 1)](...loop {
        break pack(3, 5, 6)
    }, seed: _)
    order * 100 + value
}
main()
"#,
    );
    assert!(
        generated.contains("__tpz_pipe_value_")
            && generated.contains("__tpz_call_callee_")
            && generated.contains("__tpz_call_spread_")
            && generated.contains("tpz_call(__tpz_call_callee_")
            && generated.contains("tpz_spread_values(")
            && generated.contains("tpz_index(")
            && generated.contains("\"_t_7873\": [*__tpz_call_spread_")
            && generated.contains("\"_t_73656564\": __tpz_call_arg_"),
        "statement-lowered dynamic-index variadic pipe calls should bind the dynamic callee before spread-tail values: {generated}"
    );

    let index_tick = generated
        .find("_t_7469636b(2, 1)")
        .unwrap_or_else(|| panic!("missing dynamic index tick call: {generated}"));
    let pack_call = generated
        .find("_t_7061636b(3, 5, 6)")
        .unwrap_or_else(|| panic!("missing spread pack call: {generated}"));
    assert!(
        index_tick < pack_call,
        "statement-lowered dynamic-index variadic pipe calls should bind the callee before the spread value is evaluated: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        123412,
        "side-effecting statement-lowered homogeneous dynamic-index variadic pipe parity",
    );
}

#[test]
fn emits_plain_spread_dynamic_index_variadic_pipe_side_effect_order() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut order = 0

    function tick(label: int, value: int) -> int {
        order = order * 10 + label
        value
    }

    function pack(label: int, a: int, b: int) -> Array<int> {
        order = order * 10 + label
        [a, b]
    }

    function sum(seed: int = 0, ...xs: int) -> int {
        order = order * 10 + 4
        let mut total = seed
        for x in xs {
            total = total + x
        }
        total
    }

    let callbacks = [sum, sum]
    let value = loop {
        break tick(1, 1)
    } |> callbacks[tick(2, 1)](...pack(3, 5, 6), seed: _)
    order * 100 + value
}
main()
"#,
    );
    assert!(
        generated.contains("__tpz_pipe_value_")
            && generated.contains("__tpz_call_callee_")
            && generated.contains("__tpz_call_spread_")
            && generated.contains("tpz_call(__tpz_call_callee_")
            && generated.contains("tpz_spread_values(")
            && generated.contains("tpz_index(")
            && generated.contains("\"_t_7873\": [*__tpz_call_spread_")
            && generated.contains("\"_t_73656564\": __tpz_call_arg_"),
        "plain-spread dynamic-index variadic pipe calls should bind the dynamic callee before eager spread-tail values: {generated}"
    );

    let index_tick = generated
        .find("_t_7469636b(2, 1)")
        .unwrap_or_else(|| panic!("missing dynamic index tick call: {generated}"));
    let pack_call = generated
        .find("_t_7061636b(3, 5, 6)")
        .unwrap_or_else(|| panic!("missing spread pack call: {generated}"));
    assert!(
        index_tick < pack_call,
        "plain-spread dynamic-index variadic pipe calls should bind the callee before the spread value is evaluated: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        123412,
        "side-effecting plain-spread homogeneous dynamic-index variadic pipe parity",
    );
}

#[test]
fn emits_homogeneous_dynamic_index_named_default_pipe_function_value_calls() {
    let generated = emit_source(
        r#"
function add(a: int, b: int = 2, c: int = 4) -> int {
    a + b + c
}
function sub(a: int, b: int = 10, c: int = 20) -> int {
    a - b - c
}
function main() -> int {
    let seeds = [0, 1]
    let i = seeds[1]
    let j = seeds[0]
    let callbacks = [sub, add]
    let a = 1 |> callbacks[i](c: 5, a: _)
    let b = 20 |> callbacks[j](c: 5, a: _)
    a * 10 + b
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(tpz_index(")
            && generated.contains("\"_t_63\": 5")
            && generated.contains("\"_t_61\": __tpz_piped"),
        "homogeneous dynamic-index pipe function-value calls should preserve named/default metadata: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        85,
        "homogeneous dynamic-index pipe function-value named/default parity",
    );
}

#[test]
fn emits_side_effecting_homogeneous_dynamic_index_named_default_pipe_function_value_calls() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut order = 0

    function tick(label: int, value: int) -> int {
        order = order * 10 + label
        value
    }

    function add(a: int, b: int = 2, c: int = 4) -> int {
        order = order * 10 + 4
        a + b + c
    }

    function sub(a: int, b: int = 10, c: int = 20) -> int {
        order = order * 10 + 5
        a - b - c
    }

    let callbacks = [sub, add]
    let value = tick(1, 1) |> callbacks[tick(2, 1)](c: tick(3, 5), a: _)
    order * 100 + value
}
main()
"#,
    );
    assert!(
        generated.contains("(lambda __tpz_piped: tpz_call(tpz_index(")
            && generated.contains("\"_t_63\": _t_7469636b(3, 5)")
            && generated.contains("\"_t_61\": __tpz_piped")
            && generated.contains(")(_t_7469636b(1, 1))"),
        "side-effecting homogeneous dynamic-index pipe calls should preserve named/default metadata: {generated}"
    );

    let index_tick = generated
        .find("_t_7469636b(2, 1)")
        .unwrap_or_else(|| panic!("missing dynamic index tick call: {generated}"));
    let c_tick = generated
        .find("_t_7469636b(3, 5)")
        .unwrap_or_else(|| panic!("missing named c tick call: {generated}"));
    assert!(
        index_tick < c_tick,
        "side-effecting dynamic-index pipe calls should keep index before named c in the lambda body: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        123408,
        "side-effecting homogeneous dynamic-index pipe function-value named/default parity",
    );
}

#[test]
fn emits_statement_lowered_dynamic_index_pipe_side_effect_order() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut order = 0

    function tick(label: int, value: int) -> int {
        order = order * 10 + label
        value
    }

    function add(a: int, b: int = 2, c: int = 4) -> int {
        order = order * 10 + 4
        a + b + c
    }

    function sub(a: int, b: int = 10, c: int = 20) -> int {
        order = order * 10 + 5
        a - b - c
    }

    let callbacks = [sub, add]
    let value = loop {
        break tick(1, 1)
    } |> callbacks[tick(2, 1)](c: loop {
        break tick(3, 5)
    }, a: _)
    order * 100 + value
}
main()
"#,
    );
    assert!(
        generated.contains("__tpz_pipe_value_")
            && generated.contains("__tpz_call_callee_")
            && generated.contains("tpz_call(__tpz_call_callee_")
            && generated.contains("tpz_index(")
            && generated.contains("\"_t_63\": __tpz_")
            && generated.contains("\"_t_61\": __tpz_pipe_value_"),
        "side-effecting statement-lowered homogeneous dynamic-index pipe calls should bind the pipe value, dynamic callee, and named argument before tpz_call: {generated}"
    );

    let index_tick = generated
        .find("_t_7469636b(2, 1)")
        .unwrap_or_else(|| panic!("missing dynamic index tick call: {generated}"));
    let c_tick = generated
        .find("_t_7469636b(3, 5)")
        .unwrap_or_else(|| panic!("missing named c tick call: {generated}"));
    assert!(
        index_tick < c_tick,
        "statement-lowered dynamic-index pipe calls should bind the callee before named c is evaluated: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        123408,
        "side-effecting statement-lowered homogeneous dynamic-index pipe function-value named/default parity",
    );
}

#[test]
fn emits_statement_lowered_homogeneous_dynamic_index_named_default_pipe_function_value_calls() {
    let generated = emit_source(
        r#"
function add(a: int, b: int = 2, c: int = 4) -> int {
    a + b + c
}
function sub(a: int, b: int = 10, c: int = 20) -> int {
    a - b - c
}
function main() -> int {
    let seeds = [0, 1]
    let i = seeds[1]
    let j = seeds[0]
    let callbacks = [sub, add]
    let a = loop {
        break 1
    } |> callbacks[i](c: loop {
        break 5
    }, a: _)
    let b = loop {
        break 20
    } |> callbacks[j](c: loop {
        break 5
    }, a: _)
    a * 10 + b
}
main()
"#,
    );
    assert!(
        generated.contains("__tpz_pipe_value_")
            && generated.contains("tpz_call(")
            && generated.contains("tpz_index(")
            && generated.contains("\"_t_63\": __tpz_")
            && generated.contains("\"_t_61\": __tpz_pipe_value_"),
        "statement-lowered homogeneous dynamic-index pipe calls should bind piped and named values before tpz_call: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        85,
        "statement-lowered homogeneous dynamic-index pipe function-value named/default parity",
    );
}

#[test]
fn emits_spread_array_appended_dynamic_index_named_default_pipe_function_value_calls() {
    let generated = emit_source(
        r#"
function addDefault(a: int, b: int = 2) -> int {
    a + b
}
function scaleDefault(a: int, b: int = 100) -> int {
    a * b
}
function main() -> int {
    let base = [addDefault]
    let callbacks = [...base, scaleDefault]
    let seeds = [0, 1]
    let fromSpread = 5 |> callbacks[seeds[0]](a: _)
    let appended = 5 |> callbacks[seeds[1]](a: _)
    fromSpread * 1000 + appended
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(tpz_index(")
            && generated.contains("\"_t_61\": __tpz_piped")
            && !generated.contains("tpz_array_map__co("),
        "spread-array appended-slot dynamic indices should preserve named/default pipe metadata without HOF recovery: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        7500,
        "spread-array appended-slot dynamic-index named/default pipe function-value parity",
    );
}

#[test]
fn emits_spread_array_appended_dynamic_index_variadic_pipe_function_value_calls() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut order = 0

    function tick(label: int, value: int) -> int {
        order = order * 10 + label
        value
    }

    function pack(label: int, a: int, b: int) -> Array<int> {
        order = order * 10 + label
        [a, b]
    }

    function sum(seed: int = 0, ...xs: int) -> int {
        order = order * 10 + 3
        let mut total = seed
        for x in xs {
            total = total + x
        }
        total
    }

    function scale(seed: int = 0, ...xs: int) -> int {
        order = order * 10 + 4
        let mut total = seed
        for x in xs {
            total = total + x * 10
        }
        total
    }

    let base = [sum]
    let callbacks = [...base, scale]
    let value = tick(1, 7) |> callbacks[tick(2, 1)](...pack(3, 5, 6), seed: _)
    order * 1000 + value
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(")
            && generated.contains("tpz_index(")
            && generated.contains("tpz_spread_values(")
            && generated.contains("\"_t_7873\": [*")
            && generated.contains("\"_t_73656564\": __tpz_vararg_")
            && !generated.contains("tpz_array_map__co("),
        "spread-array appended-slot dynamic indices should preserve variadic pipe metadata without HOF recovery: {generated}"
    );

    let index_tick = generated
        .find("_t_7469636b(2, 1)")
        .unwrap_or_else(|| panic!("missing dynamic index tick call: {generated}"));
    let pack_call = generated
        .find("_t_7061636b(3, 5, 6)")
        .unwrap_or_else(|| panic!("missing spread pack call: {generated}"));
    assert!(
        index_tick < pack_call,
        "spread-array appended-slot dynamic-index variadic pipe calls should bind the callee before spread-tail values: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        1234117,
        "spread-array appended-slot dynamic-index variadic pipe function-value parity",
    );
}

#[test]
fn tracks_local_mutable_dynamic_index_pipe_and_declines_heterogeneous_calls() {
    let mutable_homogeneous = emit_source(
        r#"
function add(a: int, b: int = 2, c: int = 4) -> int {
    a + b + c
}
function main() -> int {
    let seeds = [0, 1]
    let i = seeds[1]
    let mut callbacks = [add, add]
    1 |> callbacks[i](c: 5, a: _)
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &mutable_homogeneous,
        8,
        "fresh mutable Array homogeneous dynamic-index pipe metadata",
    );

    let cases = [
        (
            "mixed_shape_dynamic_index_pipe",
            r#"
function add(a: int, b: int = 2, c: int = 4) -> int {
    a + b + c
}
function sub(x: int, b: int = 2, c: int = 4) -> int {
    x - b - c
}
function main() -> int {
    let seeds = [0, 1]
    let i = seeds[1]
    let callbacks = [sub, add]
    1 |> callbacks[i](c: 5, a: _)
}
main()
"#,
        ),
        (
            "mixed_variadic_shape_dynamic_index_pipe",
            r#"
function sum(seed: int = 0, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function total(seed: int = 0, ...ys: int) -> int {
    let mut out = seed
    for y in ys {
        out = out + y
    }
    out
}
function main() -> int {
    let seeds = [0, 1]
    let i = seeds[1]
    let callbacks = [sum, total]
    1 |> callbacks[i](...[2], seed: _)
}
main()
"#,
        ),
        (
            "spread_array_dynamic_index_non_callable_prefix_pipe",
            r#"
function add(a: int, b: int = 2, c: int = 4) -> int {
    a + b + c
}
function main() -> int {
    let base = [42]
    let callbacks = [...base, add]
    let seeds = [1]
    let i = seeds[0]
    1 |> callbacks[i](c: 5, a: _)
}
main()
"#,
        ),
    ];

    for (name, src) in cases {
        let error = emit_error_for_source(src);
        assert_eq!(error.code(), "TPZ6PY0001", "{name}");
        match error.kind {
            PyEmitErrorKind::Unsupported(what) => {
                assert_eq!(what, "pipe stage call target", "{name}");
            }
            other => panic!("{name}: expected unsupported error, got {other:?}"),
        }
    }
}
