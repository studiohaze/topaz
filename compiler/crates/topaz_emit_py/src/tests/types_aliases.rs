use super::*;

#[test]
fn composed_wrapper_paths_preserve_record_cooperative_callback_targets() {
    let generated = emit_checked_alias_source(
        r#"
type Handler = { callback: (int) -> int }

function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x + 1
}

function make() -> Result<Option<Handler>, string> {
    Ok(Some({ callback: spin }))
}

function main() -> int {
    let handlers: Map<string, Result<Option<Handler>, string>> = map {
        "main": make()
    }
    match handlers.get("main") {
        case Some(outcome) => match outcome {
            case Ok(maybe) => match maybe {
                case Some(handler) => {
                    let result = concurrent {
                        slow: [4].map(handler.callback)[0]
                        fast: 0
                    }
                    result.slow
                }
                case None => 0
            }
            case Err(_) => 0
        }
        case None => 0
    }
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        5,
        "Map-Result-Option record cooperative callback metadata",
    );
}

#[test]
fn emits_map_get_typed_rebind_function_value_expr_position_calls() {
    let generated = emit_source(
        r#"
function add(a: int, b: int = 2, c: int = 4) -> int {
    a * 100 + b * 10 + c
}
function main() -> int {
    let callbacks = map { "plus": add }
    let total = match callbacks.get("plus") {
        case Some(raw) => {
            let cb: (int, int, int) -> int = raw
            cb(c: 5, a: 1)
        }
        case None => 0
    }
    total
}
main()
"#,
    );
    assert!(
        generated.contains("(\"function\", 3, False)"),
        "expression-position typed let should still emit function type spec: {generated}"
    );
    assert!(
        generated.contains("_t_6362(_t_63=5, _t_61=1)"),
        "expression-position typed rebind should preserve named/default metadata: {generated}"
    );
    assert!(
        generated.contains("_t_746f74616c ="),
        "match expression assigned to let binding should lower through the target binding: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        125,
        "Map.get typed-rebind expression-position function-value direct-call parity",
    );
}

#[test]
fn emits_checked_type_alias_typed_let_specs() {
    let generated = emit_checked_alias_source(
        r#"
type Id = int
type UserId = Id
function main() -> int {
    let x: UserId = 41
    x + 1
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_type_matches(") && generated.contains("\"int\""),
        "transitive monomorphic alias should lower to the resolved int spec: {generated}"
    );
    assert_generated_python_ok_int(&generated, 42, "checked type alias typed-let parity");
}

#[test]
fn top_level_typed_let_guards_match_stage0_before_global_binding() {
    let entry = emit_source("let value: int = \"bad\"\nvalue");
    let value_temp = entry
        .find("__tpz_typed_let_value_")
        .expect("entry typed let value temp");
    let guard = entry
        .find("tpz_let_pattern(tpz_type_matches(")
        .expect("entry typed let runtime guard");
    let binding = entry
        .rfind("globals()[\"_t_76616c7565\"]")
        .expect("entry typed let global binding");
    assert!(value_temp < guard && guard < binding, "{entry}");
    assert_generated_python_fault_code(&entry, "TPZ5001", "entry top-level typed let guard");

    let imported = emit_source_with_files(
        "import model { value }\nvalue",
        &[("model.tpz", "export let value: int = \"bad\"")],
    );
    assert_generated_python_fault_code(&imported, "TPZ5001", "imported top-level typed let guard");

    let mutable = emit_source("let mut value: int = \"bad\"\nvalue");
    assert!(
        !mutable.contains("tpz_let_pattern(tpz_type_matches("),
        "mutable annotations are static and must not become typed-pattern guards: {mutable}"
    );
    assert_generated_python_ok_string(
        &mutable,
        "bad",
        "top-level mutable annotation remains runtime-unchecked",
    );
}

#[test]
fn nested_typed_let_guards_match_stage0_before_local_binding() {
    let guarded = emit_source(
        r#"
function main() -> string {
    let value: int = "bad"
    value
}
main()
"#,
    );
    let value_temp = guarded
        .find("__tpz_typed_let_value_")
        .expect("nested typed let value temp");
    let guard = guarded
        .find("tpz_let_pattern(tpz_type_matches(")
        .expect("nested typed let runtime guard");
    let binding = guarded[guard..]
        .find(" = __tpz_typed_let_value_")
        .map(|offset| guard + offset)
        .expect("nested typed let local binding after guard");
    assert!(value_temp < guard && guard < binding, "{guarded}");
    assert_generated_python_fault_code(&guarded, "TPZ5001", "nested immutable typed let guard");

    let mutable = emit_source(
        r#"
function main() -> string {
    let mut value: int = "bad"
    value
}
main()
"#,
    );
    assert!(
        !mutable.contains("tpz_let_pattern(tpz_type_matches("),
        "nested mutable annotations are static and must not become typed-pattern guards: {mutable}"
    );
    assert_generated_python_ok_string(
        &mutable,
        "bad",
        "nested mutable annotation remains runtime-unchecked",
    );
}

#[test]
fn emits_checked_type_alias_array_element_optional_receiver_inner_shapes() {
    let generated = emit_checked_alias_source(
        r#"
type MaybeInt = Option<int>
type MaybeMaybeValues = Array<Option<MaybeInt>>

function fallback() -> int {
    6
}

function main() -> int {
    let values: MaybeMaybeValues = [Some(None)]
    let out = values[0]?.okOrElse(fallback)
    match out {
        case Some(result) => match result {
            case Ok(_) => -1
            case Err(code) => code
        }
        case None => -2
    }
}

main()
"#,
    );
    assert!(
        generated.contains("tpz_wrap_optional(tpz_option_ok_or_else(")
            && generated.contains("tpz_index("),
        "checked alias Array<Option<Alias<Option<int>>>> should preserve optional array-index receiver inner shape: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        6,
        "checked alias optional array-index receiver inner-shape parity",
    );
}

#[test]
fn emits_checked_type_alias_array_element_optional_receiver_array_inner_shapes() {
    let generated = emit_checked_alias_source(
        r#"
type MaybeInts = Option<Array<int>>
type MaybeArrayValues = Array<MaybeInts>

function inc(x: int) -> int {
    x + 1
}

function main() -> int {
    let values: MaybeArrayValues = [Some([2, 3])]
    let mapped = values[0]?.map(inc)
    match mapped {
        case Some(xs) => xs[0] * 10 + xs[1]
        case None => -1
    }
}

main()
"#,
    );
    assert!(
        generated.contains("tpz_wrap_optional(tpz_array_map(") && generated.contains("tpz_index("),
        "checked alias Array<Option<Array<int>>> should preserve optional array-index receiver Array inner shape: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        34,
        "checked alias optional array-index receiver Array inner-shape parity",
    );
}

#[test]
fn checked_type_alias_array_element_optional_receiver_innerless_rejects_loudly() {
    let diagnostics = checked_alias_diagnostics_for_source(
        r#"
type MaybeInt = Option<int>
type MaybeIntValues = Array<MaybeInt>

function fallback() -> int {
    6
}

function main() -> int {
    let values: MaybeIntValues = [Some(1)]
    let out = values[0]?.okOrElse(fallback)
    0
}

main()
"#,
    );
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_str() == "TPZ5006"
                && diagnostic.message == "`int` has no member named `okOrElse`"
        }),
        "checked alias Array<Option<int>> optional array-index receivers should reject before emit instead of wiring an inner int receiver: {diagnostics:?}"
    );
}

#[test]
fn checked_generic_type_alias_use_substitutes_arguments() {
    let generated = emit_checked_alias_source(
        r#"
type Box<T> = Array<T>
function main() -> int {
    let xs: Box<int> = [1]
    xs[0]
}
main()
"#,
    );
    assert_eq!(
        generated.matches("(\"array\", \"int\")").count(),
        2,
        "generic alias should emit the substituted runtime type in normal and cooperative variants: {generated}"
    );
    assert_generated_python_ok_int(&generated, 1, "generic alias substitution");
}

#[test]
fn emits_map_get_typed_rebind_function_type_alias_direct_calls() {
    let generated = emit_checked_alias_source(
        r#"
type Handler = (int, int, int) -> int
function add(a: int, b: int = 2, c: int = 4) -> int {
    a * 100 + b * 10 + c
}
function main() -> int {
    let callbacks = map { "plus": add }
    match callbacks.get("plus") {
        case Some(raw) => {
            let cb: Handler = raw
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
        "function alias typed-let should emit a function type spec: {generated}"
    );
    assert!(
        generated.contains("_t_6362(_t_63=5, _t_61=1)") && generated.contains("_t_6362(2, 3, 4)"),
        "function alias typed rebind should preserve named/default metadata: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        359,
        "Map.get function type-alias typed-rebind direct-call parity",
    );
}

#[test]
fn emits_map_get_typed_rebind_function_type_alias_expr_position_calls() {
    let generated = emit_checked_alias_source(
        r#"
type Handler = (int, int, int) -> int
function add(a: int, b: int = 2, c: int = 4) -> int {
    a * 100 + b * 10 + c
}
function main() -> int {
    let callbacks = map { "plus": add }
    let total = match callbacks.get("plus") {
        case Some(raw) => {
            let cb: Handler = raw
            cb(c: 5, a: 1)
        }
        case None => 0
    }
    total
}
main()
"#,
    );
    assert!(
        generated.contains("(\"function\", 3, False)"),
        "expression-position function alias typed let should emit function spec: {generated}"
    );
    assert!(
        generated.contains("_t_6362(_t_63=5, _t_61=1)"),
        "expression-position function alias typed rebind should preserve metadata: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        125,
        "Map.get function type-alias expression-position parity",
    );
}

#[test]
fn emits_map_get_composed_typed_rebind_function_value_direct_calls() {
    let generated = emit_source(
        r#"
function inc(x: int) -> int {
    x + 1
}
function dbl(x: int) -> int {
    x * 2
}
function main() -> int {
    let callbacks = map { "both": inc >> dbl }
    match callbacks.get("both") {
        case Some(raw) => {
            let cb: (int) -> int = raw
            cb(5)
        }
        case None => 0
    }
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_compose("),
        "composed Map.get values should preserve the composed runtime carrier: {generated}"
    );
    assert!(
        generated.contains("(\"function\", 1, False)"),
        "function typed-let should emit a shape-only composed function type spec: {generated}"
    );
    assert!(
        generated.contains("_t_6362(5)"),
        "typed rebind should call the local composed carrier: {generated}"
    );
    assert!(
        !generated.contains("_t_696e63(5)") && !generated.contains("_t_64626c(5)"),
        "typed rebind direct calls must not bypass the composed local carrier: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        12,
        "Map.get composed typed-rebind function-value direct-call parity",
    );

    let direct = emit_source(
        r#"
function inc(x: int) -> int {
    x + 1
}
function dbl(x: int) -> int {
    x * 2
}
function main() -> int {
    let callbacks = map { "both": inc >> dbl }
    match callbacks.get("both") {
        case Some(cb) => {
            let observed = cb(5)
            observed
        }
        case None => 0
    }
}
main()
"#,
    );
    assert!(
        direct.contains("tpz_compose(") && direct.contains("_t_6362(5)"),
        "inferred composed Some(cb) carrier should call the local composed value: {direct}"
    );
    assert_generated_python_ok_int(
        &direct,
        12,
        "Map.get composed inferred pattern direct-call parity",
    );
}
