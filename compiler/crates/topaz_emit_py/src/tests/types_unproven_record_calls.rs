use super::*;

#[test]
fn keeps_unproven_returned_record_field_variadic_function_value_calls_unpromoted() {
    let cases = [
        (
            "statementful_return_record",
            r#"
function sum(seed: int = 0, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function makeCallbacks() {
    let callbacks = { total: sum }
    callbacks
}
function main() -> int {
    makeCallbacks().total(3, ...[1, 2])
}
main()
"#,
        ),
        (
            "mismatched_branch_signature",
            r#"
function sum(seed: int = 0, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function scale(seed: int, factor: int = 10, ...xs: int) -> int {
    let mut total = seed * factor
    for x in xs {
        total = total + x * factor
    }
    total
}
function makeCallbacks(flag: bool) {
    if flag {
        { total: sum }
    } else {
        { total: scale }
    }
}
function main() -> int {
    makeCallbacks(true).total(3, ...[1, 2])
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
fn emits_nested_record_field_function_value_calls() {
    let generated = emit_source(
        r#"
function add(a: int, b: int) -> int {
    a + b
}
function addDefault(a: int, b: int = 2) -> int {
    a + b
}
function subDefault(a: int, b: int = 2) -> int {
    a - b
}
function main() -> int {
    let direct = { nested: { plus: add, plusDefault: addDefault } }
    let alias = add
    let inner = { plus: alias }
    let first = { nested: inner }
    let callbacks = first
    let mut mutableCallbacks = { nested: { plus: add, plusDefault: addDefault } }
    let mut mutableInner = { plus: addDefault }
    let callbacksFromMutableInner = { nested: mutableInner }
    mutableInner.plus = subDefault
    direct.nested.plus(b: 3, a: 10) + direct.nested.plusDefault(a: 5) + callbacks.nested.plus(20, 22) + mutableCallbacks.nested.plusDefault(a: 6) + callbacksFromMutableInner.nested.plus(a: 8) + mutableInner.plus(a: 8)
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_call(tpz_member(tpz_member("),
        "nested record-field function values should still read the callable through runtime members: {generated}"
    );
    assert!(
        generated.contains("\"_t_62\": 3") && generated.contains("\"_t_61\": 10"),
        "nested record-field function values should preserve named arguments: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        86,
        "nested record-field function-value call parity",
    );
}

#[test]
fn keeps_unproven_nested_record_field_function_value_calls_unpromoted() {
    let cases = [
        (
            "mutable_inner_record_alias_after_dynamic_write",
            r#"
function add(a: int, b: int) -> int {
    a + b
}
function main() -> int {
    let mut inner = { plus: add }
    inner["plus"] = add
    let callbacks = { nested: inner }
    callbacks.nested.plus(b: 3, a: 10)
}
main()
"#,
        ),
        (
            "mixed_record_array_path",
            r#"
function add(a: int, b: int) -> int {
    a + b
}
function main() -> int {
    let callbacks = { nested: [add] }
    callbacks.nested[0](b: 3, a: 10)
}
main()
"#,
        ),
        (
            "non_callable_nested_field",
            r#"
function main() -> int {
    let callbacks = { nested: { plain: 42 } }
    callbacks.nested.plain(a: 1)
}
main()
"#,
        ),
        (
            "mutable_record_field_call_inside_loop_before_later_assignment",
            r#"
function addDefault(a: int, b: int = 2) -> int {
    a + b
}
function subDefault(a: int, b: int = 2) -> int {
    a - b
}
function main() -> int {
    let mut inner = { plus: addDefault }
    let mut i = 0
    let mut total = 0
    while i < 2 {
        total = total + inner.plus(a: 5)
        inner.plus = subDefault
        i = i + 1
    }
    total
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
