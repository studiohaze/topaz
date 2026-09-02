use super::*;

#[test]
fn emits_local_lambda_named_calls() {
    let generated = emit_source(
        r#"
function main() -> int {
    let f = (a: int, b: int) => a - b
    f(b: 3, a: 10) + f(10, b: 3)
}
main()
"#,
    );
    assert!(
        generated.contains("_t_66(_t_62=3, _t_61=10)"),
        "{generated}"
    );
    assert!(generated.contains("_t_66(10, _t_62=3)"), "{generated}");
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("local lambda named call Python gate failed: {e}"));
}

#[test]
fn emits_dynamic_function_value_calls_from_index_and_record_field() {
    let generated = emit_source(
        r#"
function add(a: int, b: int) -> int {
    a + b
}
function addDefault(a: int, b: int = 2) -> int {
    a + b
}
function main() -> int {
    let callbacks = [add]
    let i = 0
    let ops = { plus: add, plusDefault: addDefault }
    callbacks[0](b: 3, a: 10) + callbacks[i](20, 22) + ops.plus(20, 22) + ops.plusDefault(a: 5)
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(tpz_index("),
        "array-index function values should call through tpz_call: {generated}"
    );
    assert!(
        generated.contains("\"_t_62\": 3") && generated.contains("\"_t_61\": 10"),
        "static array-index function values should preserve named arguments: {generated}"
    );
    assert!(
        generated.contains("tpz_call(tpz_member("),
        "record-field function values should call through tpz_call: {generated}"
    );
    assert_generated_python_ok_int(&generated, 104, "dynamic function-value call parity");
}

#[test]
fn emits_homogeneous_dynamic_index_named_default_function_value_calls() {
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
    callbacks[i](c: 5, a: 1) + callbacks[i](2) + callbacks[j](c: 5, a: 1) + callbacks[j](2)
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(tpz_index(")
            && generated.contains("\"_t_63\": 5")
            && generated.contains("\"_t_61\": 1"),
        "homogeneous dynamic array-index calls should preserve named/default metadata: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        -26,
        "homogeneous dynamic array-index function-value named/default parity",
    );
}

#[test]
fn emits_side_effecting_homogeneous_dynamic_index_named_default_function_value_calls() {
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
    let value = callbacks[tick(1, 1)](c: tick(2, 5), a: tick(3, 1))
    order * 100 + value
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(tpz_index(")
            && generated.contains("\"_t_63\": _t_7469636b(2, 5)")
            && generated.contains("\"_t_61\": _t_7469636b(3, 1)"),
        "side-effecting homogeneous dynamic array-index calls should preserve named/default metadata: {generated}"
    );

    let index_tick = generated
        .find("_t_7469636b(1, 1)")
        .unwrap_or_else(|| panic!("missing dynamic index tick call: {generated}"));
    let c_tick = generated
        .find("_t_7469636b(2, 5)")
        .unwrap_or_else(|| panic!("missing named c tick call: {generated}"));
    let a_tick = generated
        .find("_t_7469636b(3, 1)")
        .unwrap_or_else(|| panic!("missing named a tick call: {generated}"));
    assert!(
        index_tick < c_tick && c_tick < a_tick,
        "side-effecting dynamic array-index calls should lower callee/index before source-order named arguments: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        123408,
        "side-effecting homogeneous dynamic array-index function-value named/default parity",
    );
}

#[test]
fn emits_side_effecting_homogeneous_dynamic_index_variadic_direct_calls() {
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

    let callbacks = [sum, sum]
    let value = callbacks[tick(1, 1)](...pack(2, 5, 6), seed: 7)
    order * 100 + value
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(__tpz_vararg_0")
            && generated.contains("tpz_index(")
            && generated.contains("tpz_spread_values(")
            && generated.contains("\"_t_7873\": [*__tpz_vararg_")
            && generated.contains("\"_t_73656564\": __tpz_vararg_"),
        "side-effecting homogeneous dynamic-index variadic direct calls should preserve dynamic callee and spread-tail metadata: {generated}"
    );

    let index_tick = generated
        .find("_t_7469636b(1, 1)")
        .unwrap_or_else(|| panic!("missing dynamic index tick call: {generated}"));
    let pack_call = generated
        .find("_t_7061636b(2, 5, 6)")
        .unwrap_or_else(|| panic!("missing spread pack call: {generated}"));
    assert!(
        index_tick < pack_call,
        "side-effecting homogeneous dynamic-index variadic direct calls should bind the callee before spread-tail values: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        12318,
        "side-effecting homogeneous dynamic-index variadic direct-call parity",
    );
}

#[test]
fn emits_statement_lowered_dynamic_index_variadic_direct_side_effect_order() {
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

    let callbacks = [sum, sum]
    let value = callbacks[tick(1, 1)](...loop {
        break pack(2, 5, 6)
    }, seed: 7)
    order * 100 + value
}
main()
"#,
    );
    assert!(
        generated.contains("__tpz_call_callee_")
            && generated.contains("__tpz_call_spread_")
            && generated.contains("tpz_call(")
            && generated.contains("tpz_index(")
            && generated.contains("tpz_spread_values(")
            && generated.contains("\"_t_7873\": [*__tpz_call_spread_")
            && generated.contains("\"_t_73656564\": __tpz_call_arg_"),
        "statement-lowered homogeneous dynamic-index variadic direct calls should bind the dynamic callee and spread values before tpz_call: {generated}"
    );

    let index_tick = generated
        .find("_t_7469636b(1, 1)")
        .unwrap_or_else(|| panic!("missing dynamic index tick call: {generated}"));
    let pack_call = generated
        .find("_t_7061636b(2, 5, 6)")
        .unwrap_or_else(|| panic!("missing spread pack call: {generated}"));
    assert!(
        index_tick < pack_call,
        "statement-lowered homogeneous dynamic-index variadic direct calls should bind the callee before spread-tail values: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        12318,
        "side-effecting statement-lowered homogeneous dynamic-index variadic direct-call parity",
    );
}

#[test]
fn keeps_mismatched_dynamic_index_named_default_function_value_calls_unpromoted() {
    let cases = [
        (
            "parameter_name_mismatch",
            r#"
function add(a: int, b: int = 2, c: int = 4) -> int {
    a + b + c
}
function sub(x: int, b: int = 2, c: int = 4) -> int {
    x - b - c
}
function main() -> int {
    let seeds = [0, 1]
    let i = seeds[0]
    let callbacks = [sub, add]
    callbacks[i](c: 5, a: 1)
}
main()
"#,
        ),
        (
            "default_shape_mismatch",
            r#"
function add(a: int, b: int = 2, c: int = 4) -> int {
    a + b + c
}
function strict(a: int, b: int, c: int = 4) -> int {
    a - b - c
}
function main() -> int {
    let seeds = [0, 1]
    let i = seeds[0]
    let callbacks = [strict, add]
    callbacks[i](c: 5, a: 1)
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
                assert_eq!(what, "call argument shape", "{name}");
            }
            other => panic!("{name}: expected unsupported error, got {other:?}"),
        }
    }
}

#[test]
fn emits_map_get_typed_rebind_function_value_direct_calls() {
    let generated = emit_source(
        r#"
function add(a: int, b: int = 2, c: int = 4) -> int {
    a * 100 + b * 10 + c
}
function main() -> int {
    let callbacks = map { "plus": add }
    match callbacks.get("plus") {
        case Some(raw) => {
            let cb: (int, int, int) -> int = raw
            cb(c: 5, a: 1) + cb(2, 3, 4)
        }
        case None => 0
    }
}
main()
"#,
    );
    assert!(
        generated.contains("(\"function\", 3, False)"),
        "function typed-let should emit a shape-only function type spec: {generated}"
    );
    assert!(
        generated.contains("_t_6362(_t_63=5, _t_61=1)") && generated.contains("_t_6362(2, 3, 4)"),
        "typed rebind should preserve named/default call metadata on the local carrier: {generated}"
    );
    assert!(
        !generated.contains("_t_616464(_t_63=5, _t_61=1)")
            && !generated.contains("_t_616464(2, 3, 4)"),
        "typed rebind direct calls must not bypass the local carrier: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        359,
        "Map.get typed-rebind function-value direct-call parity",
    );
}
