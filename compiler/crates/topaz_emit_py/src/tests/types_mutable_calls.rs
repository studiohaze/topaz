use super::*;

#[test]
fn keeps_control_flow_mutated_function_value_direct_calls_dynamic() {
    let generated = emit_source(
        r#"
function addDefault(a: int, b: int = 2) -> int {
    a + b
}

function subDefault(a: int, b: int = 2) -> int {
    a - b
}

function main() -> int {
    let mut callbacks = [addDefault]
    let mut i = 0
    while i < 1 {
        callbacks[0] = subDefault
        i = i + 1
    }
    callbacks[0](5, 2)
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(tpz_index("),
        "control-flow-contained array writes must clear direct-call metadata and keep runtime index dispatch: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        3,
        "control-flow-mutated array function-value direct call",
    );

    let record_error = emit_error_for_source(
        r#"
function addDefault(a: int, b: int = 2) -> int {
    a + b
}

function subDefault(a: int, b: int = 2) -> int {
    a - b
}

function main() -> int {
    let mut handlers = { total: addDefault }
    let mut i = 0
    while i < 1 {
        handlers.total = subDefault
        i = i + 1
    }
    handlers.total(5, 2)
}
main()
"#,
    );
    assert_eq!(record_error.code(), "TPZ6PY0001");
    match record_error.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(what, "member call");
        }
        other => {
            panic!("expected member-call decline after record metadata clear, got {other:?}")
        }
    }
}

#[test]
fn emits_straight_line_mutable_function_value_direct_calls() {
    let generated = emit_source(
        r#"
function addDefault(a: int, b: int = 2) -> int {
    a + b
}

function subDefault(a: int, b: int = 2) -> int {
    a - b
}

function main() -> int {
    let mut callbacks = [addDefault]
    let beforeArray = callbacks[0](a: 5)
    callbacks[0] = subDefault
    let afterArray = callbacks[0](a: 5)
    let mut handlers = { total: addDefault }
    let beforeRecord = handlers.total(a: 5)
    handlers.total = subDefault
    let afterRecord = handlers.total(a: 5)
    beforeArray * 1000 + afterArray * 100 + beforeRecord * 10 + afterRecord
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(tpz_index(")
            && generated.contains("tpz_call(tpz_member(")
            && generated.contains("\"_t_61\": 5"),
        "straight-line mutable array and record function-value direct calls should preserve named/default metadata: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        7373,
        "straight-line mutable function-value direct-call parity",
    );
}

#[test]
fn emits_straight_line_mutable_variadic_function_value_direct_calls() {
    let generated = emit_source(
        r#"
function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}

function scale(seed: int, ...xs: int) -> int {
    let mut total = seed * 2
    for x in xs {
        total = total + x * 2
    }
    total
}

function main() -> int {
    let mut callbacks = [sum]
    let beforeArray = callbacks[0](1, 2, 3)
    callbacks[0] = scale
    let afterArray = callbacks[0](1, 2, 3)
    let mut handlers = { total: sum }
    let beforeRecord = handlers.total(1, 2, 3)
    handlers.total = scale
    let afterRecord = handlers.total(1, 2, 3)
    beforeArray * 1000 + afterArray * 100 + beforeRecord * 10 + afterRecord
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_index(")
            && generated.contains("tpz_member(")
            && generated.contains("tpz_call(__tpz_vararg_")
            && generated.contains("\"_t_7873\": ["),
        "straight-line mutable variadic function-value direct calls should keep variadic metadata after storage reassignment: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        7272,
        "straight-line mutable variadic function-value direct-call parity",
    );
}

#[test]
fn emits_straight_line_mutable_variadic_spread_named_function_value_direct_calls() {
    let generated = emit_source(
        r#"
function sum(seed: int = 0, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}

function scale(seed: int = 0, ...ys: int) -> int {
    let mut total = seed * 10
    for y in ys {
        total = total + y * 10
    }
    total
}

function main() -> int {
    let mut callbacks = [sum]
    let beforeArray = callbacks[0](...[2, 3], seed: 1)
    callbacks[0] = scale
    let afterArray = callbacks[0](...[2, 3], seed: 1)
    let mut handlers = { total: sum }
    let beforeRecord = handlers.total(...[2, 3], seed: 1)
    handlers.total = scale
    let afterRecord = handlers.total(...[2, 3], seed: 1)
    beforeArray * 1000 + afterArray * 100 + beforeRecord * 10 + afterRecord
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(__tpz_vararg_")
            && generated.contains("tpz_spread_values([2, 3],")
            && generated.contains("\"_t_7873\": [*__tpz_vararg_")
            && generated.contains("\"_t_7973\": [*__tpz_vararg_")
            && generated.contains("\"_t_73656564\": __tpz_vararg_"),
        "straight-line mutable variadic spread/named function-value direct calls should refresh variadic metadata after storage reassignment: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        12120,
        "straight-line mutable variadic spread/named function-value direct-call parity",
    );
}

#[test]
fn emits_pipe_placeholder_mutable_variadic_spread_named_function_value_calls() {
    let generated = emit_source(
        r#"
function sum(seed: int = 0, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}

function scale(seed: int = 0, ...ys: int) -> int {
    let mut total = seed * 10
    for y in ys {
        total = total + y * 10
    }
    total
}

function main() -> int {
    let mut callbacks = [sum]
    let beforeArray = 1 |> callbacks[0](...[2, 3], seed: _)
    callbacks[0] = scale
    let afterArray = 1 |> callbacks[0](...[2, 3], seed: _)
    let mut handlers = { total: sum }
    let beforeRecord = 1 |> handlers.total(...[2, 3], seed: _)
    handlers.total = scale
    let afterRecord = 1 |> handlers.total(...[2, 3], seed: _)
    beforeArray * 1000 + afterArray * 100 + beforeRecord * 10 + afterRecord
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(__tpz_vararg_")
            && generated.contains("tpz_spread_values([2, 3],")
            && generated.contains("\"_t_7873\": [*__tpz_vararg_")
            && generated.contains("\"_t_7973\": [*__tpz_vararg_")
            && generated.contains("\"_t_73656564\": __tpz_vararg_")
            && generated.contains("__tpz_piped))(1)"),
        "pipe placeholder mutable variadic spread/named function-value calls should bind the piped value into the named fixed slot and refresh variadic metadata after storage reassignment: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        12120,
        "pipe placeholder mutable variadic spread/named function-value call parity",
    );
}

#[test]
fn emits_pipe_placeholder_mutable_variadic_spread_named_effect_order() {
    let generated = emit_source(
        r#"
function sum(seed: int = 0, bonus: int = 0, ...xs: int) -> int {
    let mut total = seed * 100 + bonus * 10
    for x in xs {
        total = total + x
    }
    total
}

function scale(seed: int = 0, bonus: int = 0, ...ys: int) -> int {
    let mut total = seed * 1000 + bonus * 100
    for y in ys {
        total = total + y * 10
    }
    total
}

function main() -> int {
    let mut order = 0
    function tick(tag: int) -> int {
        order = order * 10 + tag
        tag
    }

    let mut callbacks = [sum]
    let arrayBefore = tick(1) |> callbacks[0](...[tick(2), tick(3)], seed: _, bonus: tick(9))
    callbacks[0] = scale
    let arrayAfter = tick(4) |> callbacks[0](...[tick(5), tick(6)], seed: _, bonus: tick(8))

    let mut handlers = { total: sum }
    let recordBefore = tick(7) |> handlers.total(...[tick(1), tick(2)], seed: _, bonus: tick(3))
    handlers.total = scale
    let recordAfter = tick(4) |> handlers.total(...[tick(5), tick(6)], seed: _, bonus: tick(7))

    order + arrayBefore + arrayAfter + recordBefore + recordAfter
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        1_239_456_871_245_215,
        "pipe placeholder mutable variadic spread/named effect-order parity",
    );
    assert!(
        generated.contains(
            "(lambda __tpz_piped: (lambda __tpz_vararg_0, __tpz_vararg_1, __tpz_vararg_2, __tpz_vararg_3: tpz_call("
        )
            && generated.contains("tpz_spread_values([_t_7469636b(2), _t_7469636b(3)]")
            && generated.contains("__tpz_piped, _t_7469636b(9)))(_t_7469636b(1))")
            && generated.contains("tpz_member(_t_68616e646c657273, \"_t_746f74616c\"")
            && generated.contains("__tpz_piped, _t_7469636b(7)))(_t_7469636b(4))"),
        "pipe placeholder mutable variadic calls should preserve pipe-lhs, spread, placeholder, and named-slot evaluation order: {generated}"
    );
}

#[test]
fn emits_variadic_function_value_calls_from_index_and_record_field() {
    let generated = emit_source(
        r#"
function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function sumDefault(seed: int = 0, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function main() -> int {
    let callbacks = [sum]
    let handlers = { total: sumDefault, strict: sum }
    callbacks[0](10, ...[1, 2], 3) + handlers.total(...[4, 5], seed: 6) + handlers.strict(7, 8, 9)
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_index(") && generated.contains("tpz_member("),
        "array-index and record-field function values should still read through runtime helpers: {generated}"
    );
    assert!(
        generated.contains("tpz_call(__tpz_vararg_0,")
            && generated.contains("\"_t_7873\": [*__tpz_vararg_")
            && generated.contains("\"_t_73656564\": __tpz_vararg_"),
        "variadic function-value calls should pass Topaz tail values through the variadic keyword slot: {generated}"
    );
    assert_generated_python_ok_int(&generated, 55, "variadic function-value call parity");
}

#[test]
fn emits_statement_lowered_variadic_function_value_calls() {
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
    let callbacks = [sum]
    callbacks[0](...loop {
        break [1, 2]
    }, seed: loop {
        break 10
    })
}
main()
"#,
    );
    assert!(
        generated.contains("__tpz_call_arg_")
            && generated.contains("__tpz_call_spread_")
            && generated.contains("tpz_call(")
            && generated.contains("\"_t_7873\": [*__tpz_call_spread_")
            && generated.contains("\"_t_73656564\": __tpz_call_arg_"),
        "statement-lowered variadic function-value calls should preserve spread and named slots: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        13,
        "statement-lowered variadic function-value call parity",
    );
}

#[test]
fn emits_pipe_variadic_function_value_calls() {
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
    let callbacks = [sum]
    let handlers = { total: sum }
    (10 |> callbacks[0](...[1, 2], 3)) + (4 |> handlers.total(...[5, 6]))
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(")
            && generated.contains("\"_t_7873\": [*")
            && generated.contains("(__tpz_piped,)"),
        "pipe variadic function-value calls should preserve the piped fixed slot and variadic tail: {generated}"
    );
    assert_generated_python_ok_int(&generated, 31, "pipe variadic function-value call parity");
}

#[test]
fn emits_statement_lowered_pipe_variadic_function_value_calls() {
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
    let callbacks = [sum]
    let handlers = { total: sum }
    let a = loop {
        break 10
    } |> callbacks[0](...loop {
        break [1, 2]
    }, loop {
        break 3
    })
    let b = loop {
        break 4
    } |> handlers.total(...loop {
        break [5, 6]
    })
    a + b
}
main()
"#,
    );
    assert!(
        generated.contains("__tpz_pipe_value_")
            && generated.contains("__tpz_call_spread_")
            && generated.contains("tpz_call(")
            && generated.contains("\"_t_7873\": [*__tpz_call_spread_"),
        "statement-lowered pipe variadic function-value calls should bind piped and spread values before tpz_call: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        31,
        "statement-lowered pipe variadic function-value call parity",
    );
}
