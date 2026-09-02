use super::*;

#[test]
fn emits_typed_callable_array_variadic_direct_calls() {
    let generated = emit_checked_alias_source(
        r#"
type CallbackArray = Array<(int, ...int) -> int>

function sum(seed: int = 0, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function useStatic(callbacks: CallbackArray) -> int {
    callbacks[0](7, ...[1, 2])
}
function useDynamic(callbacks: CallbackArray, i: int) -> int {
    callbacks[i](10, ...[3, 4])
}
function localStatic(source: CallbackArray) -> int {
    let callbacks: CallbackArray = source
    callbacks[0](20, ...[5, 6])
}
function localDynamic(source: CallbackArray, i: int) -> int {
    let callbacks: CallbackArray = source
    callbacks[i](30, ...[7, 8])
}
function main() -> int {
    let callbacks = [sum]
    useStatic(callbacks) + useDynamic(callbacks, 0) + localStatic(callbacks) + localDynamic(callbacks, 0)
}
main()
"#,
    );
    assert!(
        generated.matches("\"__tpz_variadic_tail__\": [*").count() >= 4
            && generated.contains("tpz_index("),
        "typed callable-array variadic direct calls should use declared element function type metadata: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        103,
        "typed callable-array variadic direct-call parity",
    );
}

#[test]
fn typed_mutable_callable_arrays_preserve_declared_element_abi() {
    let generated = emit_checked_alias_source(
        r#"
type CallbackArray = Array<(int, ...int) -> int>

function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}

function main() -> int {
    let mut callbacks: CallbackArray = [sum]
    if true {
        callbacks = [sum]
    } else {
        callbacks = []
    }
    let staticValue = callbacks[0](7, ...[1, 2])

    callbacks[0] = sum
    let i = 0
    let dynamicValue = callbacks[i](10, ...[3, 4])
    let pipeValue = 20 |> callbacks[i](...[5, 6])
    staticValue * 10000 + dynamicValue * 100 + pipeValue
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        101731,
        "typed mutable callable Array declared positional and variadic ABI",
    );
}

#[test]
fn keeps_unproven_typed_callable_array_variadic_direct_calls_unpromoted() {
    let cases = [
        (
            "untyped_local_alias",
            r#"
type CallbackArray = Array<(int, ...int) -> int>

function sum(seed: int = 0, ...xs: int) -> int {
    seed
}
function call(source: CallbackArray) -> int {
    let callbacks = source
    callbacks[0](7, ...[1, 2])
}
function main() -> int {
    call([sum])
}
main()
"#,
        ),
        (
            "named_fixed_slot",
            r#"
type CallbackArray = Array<(int, ...int) -> int>

function sum(seed: int = 0, ...xs: int) -> int {
    seed
}
function call(callbacks: CallbackArray) -> int {
    callbacks[0](seed: 7)
}
function main() -> int {
    call([sum])
}
main()
"#,
        ),
    ];

    for (name, src) in cases {
        let error = emit_checked_alias_error_for_source(src);
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
fn emits_typed_callable_array_variadic_pipe_calls() {
    let generated = emit_checked_alias_source(
        r#"
type CallbackArray = Array<(int, ...int) -> int>

function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function useStatic(callbacks: CallbackArray) -> int {
    7 |> callbacks[0](...[1, 2])
}
function useDynamic(callbacks: CallbackArray, i: int) -> int {
    10 |> callbacks[i](...[3, 4])
}
function localStatic(source: CallbackArray) -> int {
    let callbacks: CallbackArray = source
    20 |> callbacks[0](...[5, 6])
}
function localDynamic(source: CallbackArray, i: int) -> int {
    let callbacks: CallbackArray = source
    30 |> callbacks[i](...[7, 8])
}
function main() -> int {
    let callbacks = [sum]
    useStatic(callbacks) + useDynamic(callbacks, 0) + localStatic(callbacks) + localDynamic(callbacks, 0)
}
main()
"#,
    );
    assert!(
        generated.matches("tpz_call(").count() >= 4
            && generated.contains("tpz_index(")
            && generated.contains("__tpz_piped")
            && generated.contains("\"__tpz_variadic_tail__\": [*"),
        "typed callable-array variadic pipe calls should use declared element function type metadata: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        103,
        "typed callable-array variadic pipe-call parity",
    );
}

#[test]
fn emits_statement_lowered_typed_callable_array_variadic_pipe_order() {
    let generated = emit_checked_alias_source(
        r#"
type CallbackArray = Array<(int, ...int) -> int>

function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}

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

    let callbacks: CallbackArray = [sum, sum]
    let value = loop {
        break tick(1, 1)
    } |> callbacks[tick(2, 1)](...loop {
        break pack(3, 5, 6)
    })
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
            && generated.contains("tpz_index(")
            && generated.contains("[*__tpz_call_spread_"),
        "statement-lowered typed callable-array variadic pipe calls should bind the piped value, dynamic callee, and spread value before tpz_call: {generated}"
    );

    let lhs_tick = generated
        .find("_t_7469636b(1, 1)")
        .unwrap_or_else(|| panic!("missing pipe lhs tick call: {generated}"));
    let index_tick = generated
        .find("_t_7469636b(2, 1)")
        .unwrap_or_else(|| panic!("missing dynamic index tick call: {generated}"));
    let pack_call = generated
        .find("_t_7061636b(3, 5, 6)")
        .unwrap_or_else(|| panic!("missing spread pack call: {generated}"));
    assert!(
        lhs_tick < index_tick && index_tick < pack_call,
        "statement-lowered typed callable-array pipe calls should evaluate lhs, callee/index, then spread value: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        12312,
        "side-effecting statement-lowered typed callable-array variadic pipe parity",
    );
}

#[test]
fn keeps_unproven_typed_callable_array_variadic_pipe_calls_bounded() {
    let decline_cases = [
        (
            "untyped_opaque_alias",
            "pipe stage call target",
            r#"
type CallbackArray = Array<(int, ...int) -> int>

function sum(seed: int, ...xs: int) -> int {
    seed
}
function call(source: CallbackArray) -> int {
    let callbacks = source
    7 |> callbacks[0](...[1, 2])
}
function main() -> int {
    call([sum])
}
main()
"#,
        ),
        (
            "named_fixed_slot",
            "call argument shape",
            r#"
type CallbackArray = Array<(int, ...int) -> int>

function sum(seed: int, ...xs: int) -> int {
    seed
}
function call(callbacks: CallbackArray) -> int {
    7 |> callbacks[0](seed: _)
}
function main() -> int {
    call([sum])
}
main()
"#,
        ),
    ];

    for (name, expected, src) in decline_cases {
        let error = emit_checked_alias_error_for_source(src);
        assert_eq!(error.code(), "TPZ6PY0001", "{name}");
        match error.kind {
            PyEmitErrorKind::Unsupported(what) => {
                assert_eq!(what, expected, "{name}");
            }
            other => panic!("{name}: expected unsupported error, got {other:?}"),
        }
    }

    let default_skip = checked_alias_diagnostics_for_source(
        r#"
type CallbackArray = Array<(int, int, ...int) -> int>

function sum(seed: int, bonus: int = 20, ...xs: int) -> int {
    let mut total = seed + bonus
    for x in xs {
        total = total + x
    }
    total
}
function call(callbacks: CallbackArray) -> int {
    1 |> callbacks[0](...[2, 3])
}
function main() -> int {
    call([sum])
}
main()
"#,
    );
    assert!(
        default_skip.iter().any(|diagnostic| {
            diagnostic.code.as_str() == "TPZ5004"
                && diagnostic
                    .message
                    .contains("a spread argument cannot skip an unsatisfied fixed parameter")
        }),
        "typed callable-array pipe calls should keep default-bearing skipped fixed slots rejected by the shared checker: {default_skip:?}"
    );
}

#[test]
fn emits_partial_static_spread_array_function_value_calls() {
    let generated = emit_source(
        r#"
function add(a: int, b: int) -> int {
    a + b
}
function addDefault(a: int, b: int = 2) -> int {
    a + b
}
function main() -> int {
    let base = [42]
    let callbacks = [...base, addDefault, add]
    let a = [add]
    let b = [...a]
    let c = [...b, addDefault]
    let nums = [1, 2]
    let fns = [add]
    let mixed = [...nums, ...fns, addDefault]
    callbacks[1](a: 5) + callbacks[2](b: 3, a: 10) + c[0](b: 4, a: 6) + c[1](a: 7) + mixed[2](b: 8, a: 9) + mixed[3](a: 11)
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(tpz_index("),
        "partial spread-array function values should call through tpz_call: {generated}"
    );
    assert!(
        generated.contains("\"_t_61\": 5")
            && generated.contains("\"_t_62\": 3")
            && generated.contains("\"_t_61\": 10")
            && generated.contains("\"_t_62\": 4")
            && generated.contains("\"_t_61\": 6")
            && generated.contains("\"_t_62\": 8")
            && generated.contains("\"_t_61\": 9"),
        "partial spread-array function values should preserve named/default arguments: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        69,
        "partial static spread-array function-value call parity",
    );
}

#[test]
fn emits_empty_static_spread_array_function_value_calls() {
    let generated = emit_source(
        r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
function main() -> int {
    let base = []
    let callbacks = [...base, add]
    callbacks[0](a: 5)
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(tpz_index(") && generated.contains("\"_t_61\": 5"),
        "empty spread-array function values should preserve named/default arguments: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        7,
        "empty static spread-array function-value call parity",
    );
}

#[test]
fn tracks_local_mutable_static_spread_and_declines_unproven_function_values() {
    let mutable_spread = emit_source(
        r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
function main() -> int {
    let mut base = [add]
    let callbacks = [...base]
    callbacks[0](a: 5)
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &mutable_spread,
        7,
        "fresh mutable Array spread-source callable metadata",
    );

    let mutable_empty_spread = emit_source(
        r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
function main() -> int {
    let mut base = []
    let callbacks = [...base, add]
    callbacks[0](a: 5)
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &mutable_empty_spread,
        7,
        "fresh mutable empty Array spread-source callable metadata",
    );

    let cases = [
        (
            "non_callable_spread_slot",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
function main() -> int {
    let base = [42]
    let callbacks = [...base, add]
    callbacks[0](a: 5)
}
main()
"#,
        ),
        (
            "dynamic_index_non_callable_prefix_spread_slot",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
function main() -> int {
    let base = [42]
    let callbacks = [...base, add]
    let seeds = [1]
    let i = seeds[0]
    callbacks[i](a: 5)
}
main()
"#,
        ),
        (
            "empty_spread_out_of_range",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
function main() -> int {
    let base = []
    let callbacks = [...base, add]
    callbacks[1](a: 5)
}
main()
"#,
        ),
        (
            "empty_spread_non_callable_suffix",
            r#"
function main() -> int {
    let base = []
    let callbacks = [...base, 42]
    callbacks[0](a: 5)
}
main()
"#,
        ),
        (
            "out_of_range_spread_index",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
function main() -> int {
    let base = [add]
    let callbacks = [...base]
    callbacks[2](a: 5)
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
