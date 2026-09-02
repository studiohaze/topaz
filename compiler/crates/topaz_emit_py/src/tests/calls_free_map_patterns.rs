use super::*;

#[test]
fn emits_map_get_inferred_pattern_function_value_direct_calls() {
    let generated = emit_source(
        r#"
function add(a: int, b: int = 2, c: int = 4) -> int {
    a * 100 + b * 10 + c
}
function main() -> int {
    let callbacks = map { "plus": add }
    match callbacks.get("plus") {
        case Some(cb) => {
            let left = cb(c: 5, a: 1)
            left + cb(2, 3, 4)
        }
        case None => 0
    }
}
main()
"#,
    );
    assert!(
        generated.contains("_t_6362(_t_63=5, _t_61=1)") && generated.contains("_t_6362(2, 3, 4)"),
        "inferred Some(cb) carrier should preserve named/default callable metadata: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        359,
        "Map.get inferred pattern function-value direct-call parity",
    );

    let expr_position = emit_source(
        r#"
function add(a: int, b: int = 2, c: int = 4) -> int {
    a * 100 + b * 10 + c
}
function main() -> int {
    let callbacks = map { "plus": add }
    let total = match callbacks.get("plus") {
        case Some(cb) => {
            let observed = cb(c: 5, a: 1)
            observed
        }
        case None => 0
    }
    total
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &expr_position,
        125,
        "Map.get inferred pattern expression-position direct-call parity",
    );

    let pure_expression = emit_source(
        r#"
function add(a: int, b: int = 2, c: int = 4) -> int {
    a * 100 + b * 10 + c
}
function main() -> int {
    let callbacks = map { "plus": add }
    let total = match callbacks.get("plus") {
        case Some(cb) => cb(c: 5, a: 1)
        case None => 0
    }
    total
}
main()
"#,
    );
    assert!(
        pure_expression.contains("_t_6362(_t_63=5, _t_61=1)"),
        "pure expression match arm should preserve named/default metadata: {pure_expression}"
    );
    assert_generated_python_ok_int(
        &pure_expression,
        125,
        "Map.get inferred pattern pure expression-arm direct-call parity",
    );

    let pure_default = emit_source(
        r#"
function add(a: int, b: int = 2, c: int = 4) -> int {
    a * 100 + b * 10 + c
}
function main() -> int {
    let callbacks = map { "plus": add }
    let total = match callbacks.get("plus") {
        case Some(cb) => cb(a: 1)
        case None => 0
    }
    total
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &pure_default,
        124,
        "Map.get inferred pattern pure expression-arm default-call parity",
    );

    let pure_guarded = emit_source(
        r#"
function add(a: int, b: int = 2, c: int = 4) -> int {
    a * 100 + b * 10 + c
}
function main() -> int {
    let callbacks = map { "plus": add }
    let total = match callbacks.get("plus") {
        case Some(cb) if cb(a: 1) == 124 => cb(2, 3, 4)
        case Some(cb) => cb(c: 5, a: 1)
        case None => 0
    }
    total
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &pure_guarded,
        234,
        "Map.get inferred pattern pure guarded expression-arm direct-call parity",
    );

    let pure_shadow = emit_source(
        r#"
function fallback(a: int) -> int {
    a + 1
}
function add(a: int, b: int = 2, c: int = 4) -> int {
    a * 100 + b * 10 + c
}
function main() -> int {
    let cb = fallback
    let callbacks = map { "plus": add }
    let inside = match callbacks.get("plus") {
        case Some(cb) => cb(c: 5, a: 1)
        case None => 0
    }
    inside + cb(4)
}
main()
"#,
    );
    assert!(
        pure_shadow.contains("_t_6362__s"),
        "pure expression match arm shadow should use an arm-local Python name: {pure_shadow}"
    );
    assert_generated_python_ok_int(
        &pure_shadow,
        130,
        "pure expression Map.get metadata should not leak past the arm",
    );

    let guarded = emit_source(
        r#"
function add(a: int, b: int = 2, c: int = 4) -> int {
    a * 100 + b * 10 + c
}
function main() -> int {
    let callbacks = map { "plus": add }
    let ready = true
    match callbacks.get("plus") {
        case Some(cb) if ready => {
            let observed = cb(c: 5, a: 1)
            observed
        }
        case Some(cb) => {
            let observed = cb(2, 3, 4)
            observed
        }
        case None => 0
    }
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &guarded,
        125,
        "guarded Map.get inferred pattern direct-call parity",
    );

    let shadow = emit_source(
        r#"
function fallback(a: int) -> int {
    a + 1
}
function add(a: int, b: int = 2, c: int = 4) -> int {
    a * 100 + b * 10 + c
}
function main() -> int {
    let cb = fallback
    let callbacks = map { "plus": add }
    let inside = match callbacks.get("plus") {
        case Some(cb) => {
            let observed = cb(c: 5, a: 1)
            observed
        }
        case None => 0
    }
    inside + cb(4)
}
main()
"#,
    );
    assert!(
        shadow.contains("_t_6362__s"),
        "match arm shadow should be renamed into an arm-local metadata scope: {shadow}"
    );
    assert_generated_python_ok_int(
        &shadow,
        130,
        "Map.get pattern callable metadata should not leak past the arm",
    );

    let dynamic_homogeneous = emit_source(
        r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
function main() -> int {
    let callbacks = map { "plus": add }
    let key = "plus"
    match callbacks.get(key) {
        case Some(cb) => cb(5, 2)
        case None => 0
    }
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &dynamic_homogeneous,
        7,
        "dynamic Map.get homogeneous inferred pattern direct-call parity",
    );

    let mutable_direct = emit_source(
        r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
function main() -> int {
    let mut callbacks = map { "plus": add }
    match callbacks.get("plus") {
        case Some(cb) => cb(5, 2)
        case None => 0
    }
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &mutable_direct,
        7,
        "fresh mutable Map static-key inferred pattern direct-call parity",
    );

    let pure_dynamic_homogeneous = emit_source(
        r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
function main() -> int {
    let callbacks = map { "plus": add }
    let key = "plus"
    let total = match callbacks.get(key) {
        case Some(cb) => cb(5, 2)
        case None => 0
    }
    total
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &pure_dynamic_homogeneous,
        7,
        "pure expression dynamic Map.get homogeneous pattern direct-call parity",
    );

    let pure_mutable = emit_source(
        r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
function main() -> int {
    let mut callbacks = map { "plus": add }
    let total = match callbacks.get("plus") {
        case Some(cb) => cb(5, 2)
        case None => 0
    }
    total
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &pure_mutable,
        7,
        "fresh mutable Map expression pattern direct-call parity",
    );

    let nested_pattern_error = emit_error_for_source(
        r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
function main() -> int {
    let callbacks = map { "plus": add }
    let total = match callbacks.get("plus") {
        case Some(Some(cb)) => cb(5, 2)
        case Some(cb) => 0
        case None => 0
    }
    total
}
main()
"#,
    );
    assert_eq!(
        nested_pattern_error.kind,
        PyEmitErrorKind::Unsupported("call target")
    );
}

#[test]
fn typed_callable_maps_preserve_declared_value_abi_through_get_patterns() {
    let generated = emit_checked_alias_source(
        r#"
type CallbackMap = Map<string, (int, ...int) -> int>

function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}

function invoke(callbacks: CallbackMap, key: string, seed: int) -> int {
    match callbacks.get(key) {
        case Some(callback) => callback(seed, ...[1, 2])
        case None => 0
    }
}

function invokeDirect(callbacks: Map<string, (int, ...int) -> int>, key: string, seed: int) -> int {
    match callbacks.get(key) {
        case Some(callback) => callback(seed, ...[1, 2])
        case None => 0
    }
}

function main() -> int {
    let base: CallbackMap = map { "sum": sum }
    let fromParam = invoke(base, "sum", 7)
    let fromDirectParam = invokeDirect(base, "sum", 6)

    let mut callbacks: CallbackMap = base
    if true {
        callbacks = map { "sum": sum }
    } else {
        callbacks = map {}
    }
    callbacks.insert("sum", sum)
    let key = "sum"
    let local = match callbacks.get(key) {
        case Some(callback) => 10 |> callback(...[3, 4])
        case None => 0
    }
    fromParam * 10000 + fromDirectParam * 100 + local
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        100917,
        "typed callable Map alias/direct parameters and mutable local declared value ABI",
    );

    let error = emit_checked_alias_error_for_source(
        r#"
type CallbackMap = Map<string, (int, ...int) -> int>

function sum(seed: int, ...xs: int) -> int {
    seed
}

function invoke(callbacks: CallbackMap) -> int {
    match callbacks.get("sum") {
        case Some(callback) => callback(__tpz_type_param_0: 7)
        case None => 0
    }
}

function main() -> int {
    invoke(map { "sum": sum })
}
main()
"#,
    );
    assert_eq!(error.code(), "TPZ6PY0001");
    match error.kind {
        PyEmitErrorKind::Unsupported(what) => assert_eq!(what, "call argument shape"),
        other => panic!("synthetic type slots must not become named parameters: {other:?}"),
    }
}

#[test]
fn typed_maps_preserve_declared_value_receiver_shapes_through_get_patterns() {
    let generated = emit_checked_alias_source(
        r#"
type ArrayMap = Map<string, Array<int>>
type OptionalArrayMap = Map<string, Option<Array<int>>>

function readAlias(values: ArrayMap, key: string) -> int {
    match values.get(key) {
        case Some(xs) => xs.sorted()[0] + xs.length
        case None => 0
    }
}

function readDirect(values: Map<string, Array<int>>, key: string) -> int {
    match values.get(k: key) {
        case Some(xs) if xs.length > 0 => xs.sorted()[0] + xs.length
        case Some(_) => 0
        case None => 0
    }
}

function readOptional(values: OptionalArrayMap, key: string) -> int {
    match values.get(key) {
        case Some(xs) => xs?.sorted()?.length ?? 0
        case None => 0
    }
}

function main() -> int {
    let base: ArrayMap = map { "xs": [3, 1, 2] }
    let aliasValue = readAlias(base, "xs")
    let directValue = readDirect(base, "xs")

    let mut mutableValues: ArrayMap = base
    if true {
        mutableValues = map { "xs": [6, 4, 5] }
    } else {
        mutableValues = map {}
    }
    mutableValues.insert("xs", [9, 7, 8])
    let key = "xs"
    let mutableValue = match mutableValues.get(key) {
        case Some(xs) => xs.sorted()[0] + xs.length
        case None => 0
    }

    let optionalValues: OptionalArrayMap = map { "xs": Some([4, 2, 3]) }
    let optionalValue = readOptional(optionalValues, key)
    aliasValue * 1000000 + directValue * 10000 + mutableValue * 100 + optionalValue
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        4041003,
        "typed Map root and Option-inner value receiver shapes through get patterns",
    );

    let imported = emit_checked_alias_source_with_files(
        r#"
import selectedValues { selectedArrays }
import namespaceValues as namespace

function main() -> int {
    let selected = match selectedArrays.get("xs") {
        case Some(xs) => xs.sorted()[0] + xs.length
        case None => 0
    }
    let namespaced = match namespace.arrays.get("xs") {
        case Some(xs) => xs.sorted()[0] + xs.length
        case None => 0
    }
    let optional = match namespace.optionalArrays.get("xs") {
        case Some(xs) => xs?.sorted()?.length ?? 0
        case None => 0
    }
    selected * 100 + namespaced * 10 + optional
}
main()
"#,
        &[
            (
                "selectedValues.tpz",
                r#"
type ArrayMap = Map<string, Array<int>>
export let selectedArrays: ArrayMap = map { "xs": [8, 6, 7] }
"#,
            ),
            (
                "namespaceValues.tpz",
                r#"
type ArrayMap = Map<string, Array<int>>
type OptionalArrayMap = Map<string, Option<Array<int>>>

export let arrays: ArrayMap = map { "xs": [8, 6, 7] }
export let optionalArrays: OptionalArrayMap = map { "xs": Some([7, 5, 6]) }
"#,
            ),
        ],
    );
    assert_generated_python_ok_int(
        &imported,
        993,
        "selected and namespace imported typed Map value receiver shapes",
    );
}
