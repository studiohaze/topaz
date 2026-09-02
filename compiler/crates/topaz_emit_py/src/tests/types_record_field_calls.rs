use super::*;

#[test]
fn emits_optional_variadic_record_field_function_value_calls() {
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
    let present = Some({ total: sum })
    let a = match present?.total(...[1, 2], seed: 3) {
        case Some(value) => value
        case None => 0
    }
    a
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_wrap_optional(")
            && generated.contains("tpz_call(")
            && generated.contains("\"_t_7873\": [*")
            && generated.contains("\"_t_73656564\":"),
        "optional variadic record-field function values should short-circuit and call through tpz_call: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        6,
        "optional variadic record-field function-value call parity",
    );
}

#[test]
fn emits_optional_nested_variadic_record_field_function_value_calls() {
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
    let present = Some({ nested: { total: sum } })
    let a = match present?.nested?.total(3, ...[1, 2]) {
        case Some(value) => value
        case None => 0
    }
    a
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_optional_member(")
            && generated.contains("tpz_wrap_optional(")
            && generated.contains("tpz_call(")
            && generated.contains("\"_t_7873\": [*"),
        "nested optional variadic record-field function values should short-circuit and call through tpz_call: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        6,
        "optional nested variadic record-field function-value call parity",
    );
}

#[test]
fn emits_returned_record_field_variadic_function_value_calls() {
    let generated = emit_source(
        r#"
function sum(seed: int = 0, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function makeCallbacks() {
    { total: sum }
}
function main() -> int {
    let first = makeCallbacks().total(3, ...[1, 2])
    let callbacks = makeCallbacks()
    let second = callbacks.total(...[1])
    first + second
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(") && generated.contains("\"_t_7873\": [*"),
        "returned record-field variadic function values should reuse static variadic lowering: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        7,
        "returned record-field variadic function-value call parity",
    );
}

#[test]
fn emits_opaque_record_type_field_variadic_function_value_calls() {
    let generated = emit_source(
        r#"
function apply(cb: { total: (int, ...int) -> int }) -> int {
    cb.total(3, ...[1, 2])
}
function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function main() -> int {
    apply({ total: sum })
}
main()
"#,
    );
    assert!(
        generated.contains("__tpz_variadic_tail__")
            && generated.contains("tpz_host_callable(_t_73756d, host, _t_73756d__co, \"_t_7873\")")
            && generated.contains("tpz_call("),
        "opaque record type field variadic calls should bridge anonymous type tails to host callable tails: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        6,
        "opaque record type field variadic function-value call parity",
    );
}

#[test]
fn emits_returned_nested_record_field_variadic_function_value_calls() {
    let generated = emit_source(
        r#"
function sum(seed: int = 0, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function makeCallbacks() {
    { nested: { total: sum } }
}
function main() -> int {
    makeCallbacks().nested.total(3, ...[1, 2])
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(") && generated.contains("\"_t_7873\": [*"),
        "returned nested record-field variadic function values should reuse static variadic lowering: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        6,
        "returned nested record-field variadic function-value call parity",
    );
}

#[test]
fn emits_returned_optional_record_field_variadic_function_value_calls() {
    let generated = emit_source(
        r#"
function sum(seed: int = 0, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function maybeCallbacks() {
    Some({ total: sum })
}
function main() -> int {
    maybeCallbacks()?.total(3, ...[1, 2]) ?? 0
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_wrap_optional(")
            && generated.contains("tpz_call(")
            && generated.contains("\"_t_7873\": [*"),
        "returned optional record-field variadic function values should short-circuit through static variadic lowering: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        6,
        "returned optional record-field variadic function-value call parity",
    );
}

#[test]
fn emits_static_variadic_function_value_spread_skip_fault() {
    let generated = emit_source(
        r#"
function sum(seed: int, ...xs: int) -> int {
    seed
}
function main() -> int {
    let callbacks = [sum]
    callbacks[0](...[1, 2])
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call_order_fault([__tpz_vararg_")
            && generated
                .contains("a spread argument cannot skip an unsatisfied fixed parameter (§5)"),
        "spread over a required fixed parameter should stay a loud call-order fault: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("static variadic function-value spread fault gate failed: {e}"));
}

#[test]
fn emits_static_spread_array_function_value_calls() {
    let generated = emit_source(
        r#"
function add(a: int, b: int) -> int {
    a + b
}
function addDefault(a: int, b: int = 2) -> int {
    a + b
}
function main() -> int {
    let base = [add]
    let first = [...base, addDefault]
    let callbacks = [...first, add]
    callbacks[1](a: 5) + callbacks[2](b: 3, a: 10)
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(tpz_index("),
        "spread-array function values should call through tpz_call: {generated}"
    );
    assert!(
        generated.contains("\"_t_61\": 5")
            && generated.contains("\"_t_62\": 3")
            && generated.contains("\"_t_61\": 10"),
        "spread-array function values should preserve named/default arguments: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        20,
        "static spread-array function-value call parity",
    );
}

#[test]
fn emits_static_spread_array_dynamic_index_function_value_calls() {
    let generated = emit_source(
        r#"
function addDefault(a: int, b: int = 2) -> int {
    a + b
}
function main() -> int {
    let base = [addDefault]
    let callbacks = [...base]
    let seeds = [0]
    let i = seeds[0]
    callbacks[i](a: 5)
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(tpz_index(") && generated.contains("\"_t_61\": 5"),
        "complete immutable spread-array dynamic indices should preserve named/default call metadata: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        7,
        "static spread-array dynamic-index function-value call parity",
    );
}

#[test]
fn emits_spread_array_appended_dynamic_index_named_default_function_value_calls() {
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
    let fromSpread = callbacks[seeds[0]](a: 5)
    let appended = callbacks[seeds[1]](a: 5)
    fromSpread * 1000 + appended
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(tpz_index(")
            && generated.contains("\"_t_61\": 5")
            && !generated.contains("tpz_array_map__co("),
        "spread-array appended-slot dynamic indices should preserve named/default direct-call metadata without HOF recovery: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        7500,
        "spread-array appended-slot dynamic-index named/default function-value call parity",
    );
}

#[test]
fn emits_spread_array_appended_dynamic_index_variadic_direct_calls() {
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
    let value = callbacks[tick(1, 1)](...pack(2, 5, 6), seed: 7)
    order * 1000 + value
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(__tpz_vararg_0")
            && generated.contains("tpz_index(")
            && generated.contains("\"_t_7873\": [*")
            && generated.contains("\"_t_73656564\": __tpz_vararg_"),
        "spread-array appended-slot dynamic indices should preserve variadic and named fixed-slot metadata: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        124117,
        "spread-array appended-slot dynamic-index variadic function-value call parity",
    );
}
