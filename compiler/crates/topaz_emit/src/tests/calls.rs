use super::*;

#[test]
fn emits_a_function_declaration_and_call() {
    // §7 `add` mangles to `_t_616464`; a top-level function binds a forward-ref
    // `top_cell` (seeded `None`, filled `top_cell_set` at its declaration), and
    // the call reads it through the guarded `top_cell_get(..)?`. Concrete param
    // types are guarded at closure entry.
    let src = emit_unit(&unit_of(
        "function add(x: int, y: int) { x + y }\nadd(3, 4)",
    ))
    .expect("emit");
    assert!(src.contains("let _t_616464 = top_cell();"), "got:\n{src}");
    assert!(
        src.contains("fn __topaz_make_function_0_9_12() -> Value")
            && src.contains("top_cell_set(&_t_616464, __topaz_make_function_0_9_12());"),
        "got:\n{src}"
    );
    assert!(src.contains("params: &[\"x\", \"y\"]"), "got:\n{src}");
    assert!(
        src.contains("call_value(top_cell_get(&_t_616464, \"add\","),
        "got:\n{src}"
    );
}

#[test]
fn a_self_recursive_function_emits_through_a_cell() {
    // §7 A TOP-LEVEL self-referenced `function` binds a forward-ref `top_cell`
    // (seeded `None`), reaches itself by the guarded `top_cell_get(..)?`, and is
    // `top_cell_set` into the cell at its declaration (mangle("f") = `_t_66`).
    let src = emit_unit(&unit_of("function f(n: int) { f(n) }\nf(1)")).expect("emit");
    assert!(src.contains("let _t_66 = top_cell();"), "got:\n{src}");
    assert!(src.contains("top_cell_set(&_t_66,"), "got:\n{src}");
    // the self-call reads the cell through the guarded getter
    assert!(
        src.contains("call_value(top_cell_get(&_t_66, \"f\","),
        "got:\n{src}"
    );
}

#[test]
fn mutual_recursion_emits_both_functions_as_top_cells() {
    // §7 At the TOP LEVEL, EVERY (non-builtin-named) function binds a forward-ref
    // `top_cell` seeded module-wide, so `isEven`/`isOdd` mutual recursion resolves
    // regardless of order — both are `top_cell` + `top_cell_set`, no `ImmCell`
    // cluster (that stays for nested/block-local functions).
    let src = emit_unit(&unit_of(
        "function isEven(n: int) -> bool { if n == 0 { true } else { isOdd(n - 1) } }\nfunction isOdd(n: int) -> bool { if n == 0 { false } else { isEven(n - 1) } }\nisEven(2)",
    ))
    .expect("emit");
    for f in ["isEven", "isOdd"] {
        assert!(
            src.contains(&format!("let {} = top_cell();", mangle(f))),
            "{f} should be a top cell; got:\n{src}"
        );
        assert!(
            src.contains(&format!("top_cell_set(&{},", mangle(f))),
            "{f} should be filled with top_cell_set; got:\n{src}"
        );
    }
    // No top-level ImmCell recursion cluster is emitted (superseded by TopFnCell).
    assert!(
        !src.contains("cell_new(Value::Unit)"),
        "top-level functions must not use the ImmCell cluster; got:\n{src}"
    );
}

#[test]
fn a_function_reassignment_is_refused() {
    // A `function` name is immutable (`ImmCell`): the interpreter faults
    // TPZ5003, the emitter over-refuses rather than `cell_set` a new value.
    assert_eq!(
        emit_unit(&unit_of(
            "function f(n: int) { if n == 0 { 0 } else { f(n - 1) } }\nf = (x: int) => 99\nf(1)"
        )),
        Err(EmitError::unsupported("assign to immutable"))
    );
}

#[test]
fn a_same_name_redeclaration_in_a_separate_cluster_still_faults() {
    // The recursion-cell dispatch is keyed by STATEMENT INDEX, not name: a
    // recursive `function f` (a cell) followed — across a `let` — by a second
    // `function f` is a same-scope redeclaration. A name-keyed flag would have
    // misclassified the second `f` as a cell fill and `cell_set` over it; the
    // index key makes it hit the redeclaration refusal instead (the
    // interpreter faults `f is already declared in this scope`).
    assert_eq!(
        emit_unit(&unit_of(
            "let r = { function f(n: int) { if n == 0 { 0 } else { f(n - 1) } }\nlet y = f(3)\nfunction f(n: int) { n }\ny }\nr"
        )),
        Err(EmitError::unsupported("same-scope redeclaration"))
    );
}

#[test]
fn a_forward_call_across_a_statement_compiles_through_a_top_cell() {
    // §7 `function a … function b` separated by a non-function statement now
    // COMPILES: every top-level function is a forward-ref `top_cell`, so `a`'s
    // reference to the later `b` is a guarded `top_cell_get`. (Here `let x = a()`
    // calls `a` BEFORE `b` is filled, so BOTH engines fault `GUARD_UNBOUND` at
    // runtime — run≡build — but emit no longer refuses to COMPILE it.)
    let src = emit_unit(&unit_of(
        "function a() -> int { b() }\nlet x = a()\nfunction b() -> int { 5 }\nx",
    ))
    .expect("emit");
    assert!(
        src.contains(&format!("call_value(top_cell_get(&{}, \"b\",", mangle("b"))),
        "a's body must read b through the guarded top cell; got:\n{src}"
    );
}

#[test]
fn nested_function_shadow_keeps_outer_value_until_its_declaration() {
    let src = emit_unit(&unit_of(
        "function later() -> int { 0 }\nfunction main() -> int {\nlet value = (() => later())()\nfunction later() -> int { 1 }\nvalue\n}\nmain()",
    ))
    .expect("emit");
    assert!(
        src.contains("let __cap_t_6c61746572 = _t_6c61746572.clone();"),
        "the outer top-function cell must be captured; got:\n{src}"
    );
    assert!(
        src.contains("top_cell_set(&__top_fn_seed_")
            && src.contains("top_cell_get(&_t_6c61746572, \"later\""),
        "the body-local positional cell must start from the outer value; got:\n{src}"
    );
}

#[test]
fn emits_a_generic_function() {
    // §3 a GENERIC `function id<T>(x: T)` erases its type params: the
    // interpreter binds args positionally without consulting `T`, so it
    // lowers to the same monomorphic closure a non-generic function would —
    // the type params are simply ignored.
    let src = emit_unit(&unit_of("function id<T>(x: T) { x }\nid(5)")).expect("emit");
    assert!(
        src.contains("Value::Closure(Rc::new(EmittedClosure {"),
        "got:\n{src}"
    );
    assert!(src.contains("params: &[\"x\"]"), "got:\n{src}");
}

#[test]
fn emits_a_variadic_function() {
    // §5 a trailing `...rest` parameter: only the FIXED params are slots
    // (`params: &["a"]`, `variadic: true`); the body binds `rest` by
    // collecting the surplus `__args` into an array. With a concrete element
    // type (`...rest: int`) the §6 guard checks each surplus argument first.
    let src = emit_unit(&unit_of(
        "function f(a: int, ...rest: int) { rest }\nf(1, 2)",
    ))
    .expect("emit");
    assert!(src.contains("params: &[\"a\"]"), "got:\n{src}");
    assert!(src.contains("variadic: true"), "got:\n{src}");
    // Each surplus element is guarded, then the rest array is bound.
    assert!(
        src.contains("for __e in &__rest")
            && src.contains("Value::array(__rest)")
            && src.contains("argument does not match parameter type (§6)"),
        "got:\n{src}"
    );
    // An UNGUARDABLE variadic element type keeps the bare collect (no guard).
    let generic = emit_unit(&unit_of("function h<T>(...rest: T) { rest }\nh(1)")).expect("emit");
    assert!(
        generic.contains("Value::array(__args.collect::<Vec<_>>())"),
        "got:\n{generic}"
    );
    // a non-variadic function carries `variadic: false`.
    let plain = emit_unit(&unit_of("function g(a: int) { a }\ng(1)")).expect("emit");
    assert!(plain.contains("variadic: false"), "got:\n{plain}");
}

#[test]
fn emits_a_return_inside_a_function_body() {
    // §7 `return e` inside a function body returns from the emitted async
    // block. A bare `return` returns Unit.
    let src = emit_unit(&unit_of("function f(x: int) { return x }\nf(1)")).expect("emit");
    assert!(src.contains("return Ok(_t_78.clone());"), "got:\n{src}");
    emit_unit(&unit_of(
        "function g(x: int) { if x > 0 { return 1 }\nx }\ng(5)",
    ))
    .expect("early return in an if-arm emits");
}

#[test]
fn a_top_level_return_is_unsupported() {
    // A `return` outside any function/lambda is refused (the interpreter
    // runtime-faults it as "return outside a function").
    assert_eq!(
        emit_unit(&unit_of("return 1")),
        Err(EmitError::unsupported("return outside a function"))
    );
    // …including one nested in a top-level control-flow construct.
    assert_eq!(
        emit_unit(&unit_of("let x = 5\nif x > 0 { return 1 }\nx")),
        Err(EmitError::unsupported("return outside a function"))
    );
}

#[test]
fn emits_a_function_default_parameter() {
    // §7 a scalar/unit LITERAL default is pre-evaluated into the closure's
    // `defaults` (parallel to `params`); `call_value` fills an unsupplied
    // slot. A function with NO defaults emits an empty defaults vec.
    let src = emit_unit(&unit_of("function f(a: int, b: int = 10) { a + b }\nf(5)")).expect("emit");
    assert!(
        src.contains("defaults: vec![None, Some(EmittedDefault::Value(Value::Int(10)))]"),
        "got:\n{src}"
    );
    let none = emit_unit(&unit_of("function g(a: int) { a }\ng(1)")).expect("emit");
    assert!(none.contains("defaults: Vec::new()"), "got:\n{none}");
    // A non-interpolated STRING default decodes at emit time to a constant.
    let str_def = emit_unit(&unit_of("function f(x: string = \"hi\") { x }\nf()")).expect("emit");
    assert!(
        str_def.contains("defaults: vec![Some(EmittedDefault::Value(Value::str(\"hi\")))]"),
        "got:\n{str_def}"
    );
    // A non-faulting scalar const expression folds before it enters the
    // closure defaults vector.
    let expr_def = emit_unit(&unit_of("function f(x: int = 1 + 2) { x }\nf()")).expect("emit");
    assert!(
        expr_def.contains("defaults: vec![Some(EmittedDefault::Value(Value::Int(3)))]"),
        "got:\n{expr_def}"
    );
    // A required parameter after a defaulted one remains required: it is stored
    // as `None` in the defaults vector, letting the shared call_value arity path
    // own the missing-argument fault.
    let required_after_default = emit_unit(&unit_of(
        "function f(a: int = 1, b: int) { a + b }\nf(2, 3)",
    ))
    .expect("emit");
    assert!(
        required_after_default
            .contains("defaults: vec![Some(EmittedDefault::Value(Value::Int(1))), None]"),
        "got:\n{required_after_default}"
    );
    // §7 a type-CORRECT unary/parenthesized literal default folds to a constant:
    // arithmetic `- +` of a numeric literal, logical `!` of a bool literal.
    let neg = emit_unit(&unit_of("function f(x: int = -1) { x }\nf()")).expect("emit");
    assert!(
        neg.contains("defaults: vec![Some(EmittedDefault::Value(Value::Int(-1)))]"),
        "got:\n{neg}"
    );
    let notb = emit_unit(&unit_of("function f(b: bool = !true) { b }\nf()")).expect("emit");
    assert!(
        notb.contains("defaults: vec![Some(EmittedDefault::Value(Value::Bool(false)))]"),
        "got:\n{notb}"
    );
    let dynamic = emit_unit(&unit_of(
        "let mut k = 1\nfunction f(x: int = k) { x }\nk = 2\nf()",
    ))
    .expect("emit");
    assert!(
        dynamic.contains("EmittedDefault::Thunk(Rc::new")
            && dynamic.contains("cell_get(&")
            && dynamic.contains(&format!("{}: Rc<std::cell::RefCell<Value>>", mangle("k")))
            && dynamic.contains("Ok("),
        "got:\n{dynamic}"
    );
    emit_unit(&unit_of("function f(n: int = (-3)) { n }\nf()")).expect("paren-neg emit");
    emit_unit(&unit_of("function f(n: int = --4) { n }\nf()")).expect("double-neg emit");
    // Negative controls for the soundness boundary: a
    // type-MISMATCHED unary default would `unary_value`-FAULT at closure creation
    // in the emitted binary while the interpreter (lazy, call-time) skips a supplied
    // slot and never faults — a divergence. The operand-type gate refuses them ALL
    // (no faulting binary), so this stays a sound loud over-refusal.
    for bad in [
        "function f(x: int = !1) { x }\nf(5)",       // ! of an int
        "function f(x: int = -\"x\") { 0 }\nf(5)",   // - of a string
        "function f(x: int = +true) { 0 }\nf(5)",    // + of a bool
        "function f(x: int = -null) { 0 }\nf(5)",    // - of null
        "function f(x: int = -()) { 0 }\nf(5)",      // - of unit
        "function f(x: int = !(-1)) { x }\nf(5)",    // ! of a numeric (type flip)
        "function f(x: int = -(!true)) { x }\nf(5)", // - of a bool (type flip)
    ] {
        assert_eq!(
            emit_unit(&unit_of(bad)),
            Err(EmitError::unsupported("function default shape")),
            "expected a type-mismatched unary default to refuse: {bad}"
        );
    }
}

#[test]
fn emits_named_call_arguments() {
    // §5/§7 a call with NAMED arguments lowers through `call_value_named`
    // (positional vec + named (name, value) vec); a purely positional call
    // keeps `call_value`.
    let src = emit_unit(&unit_of("let f = (a, b) => a - b\nf(b: 3, a: 10)")).expect("emit");
    assert!(
        src.contains("call_value_named(")
            && src.contains("vec![(\"b\".to_string(),")
            && src.contains("(\"a\".to_string(),"),
        "got:\n{src}"
    );
    let pos = emit_unit(&unit_of("let f = (a, b) => a - b\nf(10, 3)")).expect("emit");
    assert!(
        pos.contains("call_value(") && !pos.contains("call_value_named("),
        "got:\n{pos}"
    );
}

#[test]
fn module_namespace_function_named_calls_lower_as_value_calls() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import math
function mark(label: string, n: int) -> int {
print(label)
n
}
math.add(b: mark("b", 22), a: mark("a", 20))
"#,
            ),
            (
                "math.tpz",
                r#"
export function add(a: int, b: int) -> int {
a + b
}
"#,
            ),
        ],
    );
    let src = emit_module(&unit).expect("emit module namespace named call");
    assert!(src.contains("call_value_named("), "got:\n{src}");
    assert!(
        src.contains("member_value_required") && src.contains("\"add\""),
        "got:\n{src}"
    );
    assert!(
        src.contains("(\"b\".to_string(),") && src.contains("(\"a\".to_string(),"),
        "got:\n{src}"
    );
}

#[test]
fn namespace_named_call_arguments_preserve_source_order() {
    let bytes = emit_unit(&unit_of(
        "function make(label: string, text: string) -> Bytes { print(label)\nBytes.encodeUtf8(text) }\nBytes.concat(b: make(\"b\", \"B\"), a: make(\"a\", \"A\"))",
    ))
    .expect("emit");
    assert!(bytes.contains("let __tpz_ns_arg_0 ="), "got:\n{bytes}");
    assert!(bytes.contains("let __tpz_ns_arg_1 ="), "got:\n{bytes}");
    assert!(
        bytes.contains("builtin_bytes_concat(__tpz_ns_arg_1, __tpz_ns_arg_0,"),
        "got:\n{bytes}"
    );

    let math = emit_unit(&unit_of(
        "function make(label: string, n: float) -> float { print(label)\nn }\nMath.min(b: make(\"b\", 2.0), a: make(\"a\", 1.0))",
    ))
    .expect("emit");
    assert!(math.contains("let __tpz_ns_arg_0 ="), "got:\n{math}");
    assert!(math.contains("let __tpz_ns_arg_1 ="), "got:\n{math}");
    assert!(
        math.contains("builtin_math_min(__tpz_ns_arg_1, __tpz_ns_arg_0,"),
        "got:\n{math}"
    );

    let zstd = emit_unit(&unit_of(
        r#"
function makeBytes(label: string) -> Bytes {
print(label)
Bytes.encodeUtf8(label)
}
function makeLevel(label: string, n: int) -> int {
print(label)
n
}
Codec.zstdCompress(level: makeLevel("level", 3), bytes: makeBytes("bytes"))
"#,
    ))
    .expect("emit");
    assert!(zstd.contains("let __tpz_ns_arg_0 ="), "got:\n{zstd}");
    assert!(zstd.contains("let __tpz_ns_arg_1 ="), "got:\n{zstd}");
    assert!(
        zstd.contains("builtin_codec_zstd_compress(__tpz_ns_arg_1, __tpz_ns_arg_0,"),
        "got:\n{zstd}"
    );
}

#[test]
fn map_update_named_call_arguments_preserve_source_order() {
    let src = emit_unit(&unit_of(
        "function double(x: int) -> int { x * 2 }\nlet mut m = map { \"a\": 1 }\nm.update(f: double, initial: 0, k: \"a\")",
    ))
    .expect("emit");
    assert!(
        src.contains(
            "call_value_named(__field, vec![], vec![(\"f\".to_string(), __a0), (\"initial\".to_string(), __a1), (\"k\".to_string(), __a2)]"
        ),
        "got:\n{src}"
    );
    assert!(
        src.contains("let __k = __a2; let __init = __a1; let __f = __a0;"),
        "got:\n{src}"
    );
}

// An enclosing binding is captured by a closure, then shadowed by
// a later same-scope declaration, diverges — the interpreter's whole-env
// capture observes the new binding; the emitter froze the old value. Both
// the lambda and `function` forms must refuse.

#[test]
fn a_lambda_capture_shadowed_by_a_later_binding_is_refused() {
    assert_eq!(
        emit_unit(&unit_of("let x = 1\n{ let f = () => x\nlet x = 2\nf() }")),
        Err(EmitError::unsupported(
            "declaration shadows a captured binding"
        ))
    );
}

#[test]
fn a_function_capture_shadowed_by_a_later_binding_is_refused() {
    assert_eq!(
        emit_unit(&unit_of(
            "let x = 1\n{ function f() { x }\nlet x = 2\nf() }"
        )),
        Err(EmitError::unsupported(
            "declaration shadows a captured binding"
        ))
    );
}

// A capture that is NOT shadowed later still emits (no over-refusal of the
// ordinary case).

#[test]
fn a_capture_not_shadowed_later_still_emits() {
    emit_unit(&unit_of("let x = 1\n{ let f = () => x\nf() }")).expect("emit");
}

// A nested `function` body's references participate in the outer
// function's capture analysis (so an enclosing binding used only inside the
// nested function is captured, not refused as a free identifier), and the
// nested name binds (so it is not misread as an enclosing mutable capture).

#[test]
fn a_nested_function_capturing_an_enclosing_binding_emits() {
    emit_unit(&unit_of(
        "let x = 1\nfunction outer() { function inner() { x }\ninner() }\nouter()",
    ))
    .expect("emit");
}

#[test]
fn a_nested_function_shadowing_an_outer_mutable_emits() {
    emit_unit(&unit_of(
        "let mut inner = 0\nfunction outer() { function inner() { 1 }\ninner() }\nouter()",
    ))
    .expect("emit");
}

#[test]
fn a_nested_function_assign_before_local_shadow_captures_outer_cell() {
    let src = emit_unit(&unit_of(
        r#"
function main() -> int {
let mut seed = 1
function bad() -> int {
    seed = 2
    let mut seed = 0
    seed
}
let got = bad()
seed + got
}
main()
"#,
    ))
    .expect("emit");
    assert!(
        src.contains("cell_new(Value::Int(1))")
            && src.contains("cell_set(&_t_73656564, Value::Int(2));")
            && src.contains("let mut _t_73656564 = Value::Int(0);"),
        "assign-before-local-shadow should capture the outer seed as a cell, got:\n{src}"
    );
}

#[test]
fn a_nested_function_block_exited_shadow_write_captures_outer_cell() {
    let src = emit_unit(&unit_of(
        r#"
function main() -> int {
let mut seed = 1
function bad() -> int {
    if true {
        let mut seed = 0
        seed = seed + 1
    }
    seed = seed + 1
    seed
}
let got = bad()
seed + got
}
main()
"#,
    ))
    .expect("emit");
    assert!(
        src.contains("cell_new(Value::Int(1))")
            && src.contains("cell_set(&_t_73656564")
            && src.contains("let mut _t_73656564 = Value::Int(0);"),
        "block-exited local shadow should leave the outer seed cell writable, got:\n{src}"
    );
}

// A closure created in a nested loop body that ESCAPES by assignment to an
// enclosing mutable still records its capture against the enclosing scope,
// so a later shadow of the captured binding is refused.

#[test]
fn an_escaping_loop_body_capture_shadowed_later_is_refused() {
    assert_eq!(
        emit_unit(&unit_of(
            "let x = 1\n{ let mut f = 0\nwhile false { f = () => x }\nlet x = 2\nf() }"
        )),
        Err(EmitError::unsupported(
            "declaration shadows a captured binding"
        ))
    );
}

// A function body runs in the call environment that holds the parameters
// (`ClosureBody::Block`), so a body-top-level declaration colliding with a
// PARAM is a same-scope redeclaration the interpreter faults on — refuse,
// do not Rust-shadow. (Both the `function` and `let` body forms.)

#[test]
fn a_function_body_redeclaring_a_param_is_refused() {
    assert_eq!(
        emit_unit(&unit_of(
            "function f(x: int) { function x() { 1 }\nx() }\nf(0)"
        )),
        Err(EmitError::unsupported("same-scope redeclaration"))
    );
    assert_eq!(
        emit_unit(&unit_of("function f(x: int) { let x = 2\nx }\nf(0)")),
        Err(EmitError::unsupported("same-scope redeclaration"))
    );
}

#[test]
fn emits_a_record_field_access() {
    let src = emit_unit(&unit_of("let r = { x: 10, y: 20 }\nr.x")).expect("emit");
    assert!(
        src.contains("member_value_required(&(_t_72.clone()), \"x\","),
        "got:\n{src}"
    );
}

#[test]
fn receiver_method_values_bind_after_mutable_root_admission() {
    // Receiver method values preserve record-field shadowing and bind the
    // concrete receiver identity. An immutable mutator still reaches its
    // acquisition-time GUARD_IMMUTABLE fault; a mutable root yields a callable.
    let immutable = emit_unit(&unit_of("let a = [1, 2]\na.push")).expect("emit");
    assert!(immutable.contains("GUARD_IMMUTABLE"), "got:\n{immutable}");
    let mutable = emit_unit(&unit_of(
        "let mut a = [1, 2]\nlet push = a.push\npush(3)\na",
    ))
    .expect("emit");
    assert!(
        mutable.contains("bind_receiver_builtin(__recv, \"push\"")
            && mutable.contains("call_value("),
        "got:\n{mutable}"
    );
    let src = emit_unit(&unit_of("let a = [1, 2]\nlet f = a.get\nf(0)")).expect("emit");
    assert!(
        src.contains("bind_receiver_builtin(__recv, \"get\","),
        "got:\n{src}"
    );
}

#[test]
fn eager_and_callback_option_bridges_are_first_class_receiver_values() {
    // Both eager and callback-driven bridges carry the receiver catalog
    // identity through plain, optional, and pipe-field member projection.
    emit_unit(&unit_of("Some(1).okOr")).expect("plain eager bridge value emits");
    emit_unit(&unit_of("let o = Some(Some(1))\no?.okOr"))
        .expect("optional eager bridge value emits");
    emit_unit(&unit_of("Some(1) |> .okOr")).expect("pipe eager bridge value emits");
    emit_unit(&unit_of("Some(1).okOrElse")).expect("plain callback bridge value emits");
    emit_unit(&unit_of("let o = Some(Some(1))\no?.okOrElse"))
        .expect("optional callback bridge value emits");
    emit_unit(&unit_of("Some(1) |> .okOrElse")).expect("pipe callback bridge value emits");
}

#[test]
fn emits_an_array_get_call() {
    // §22.2 `a.get(i)` → the member_value-first dispatch: a record field
    // named `get` would be called as a closure; an array falls to
    // `call_method`, which dispatches the receiver-typed builtin.
    let src = emit_unit(&unit_of("let a = [1, 2]\na.get(0)")).expect("emit");
    assert!(
        src.contains("match member_value(&__recv, \"get\",")
            && src.contains("call_method(__recv, \"get\","),
        "got:\n{src}"
    );
    let spread = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
let a = [1, 2]
a.get(...mark("arr", [0]))
"#,
    ))
    .expect("receiver spread fault emits");
    assert!(
        spread.contains("call_value_spread(__field")
            && spread.contains("__tpz_recv_spread")
            && spread.contains("spread arguments require a variadic parameter"),
        "got:\n{spread}"
    );
}

#[test]
fn emits_spread_named_and_named_before_spread_faults() {
    let static_named = emit_unit(&unit_of(
        r#"
function markArray(label: string, xs: Array<string>) -> Array<string> {
print(label)
xs
}
function markString(label: string, value: string) -> string {
print(label)
value
}
print(...markArray("spread", ["x"]), value: markString("named", "y"))
"#,
    ))
    .expect("static spread+named fault emits");
    assert!(
        static_named.contains(
            "call_value_spread_named(SpreadNamedCall::new(Value::Builtin { kind: Builtin::Print",
        ) && static_named.contains("\"value\".to_string()"),
        "got:\n{static_named}"
    );

    let static_order = emit_unit(&unit_of(
        r#"
function markArray(label: string, xs: Array<string>) -> Array<string> {
print(label)
xs
}
function markString(label: string, value: string) -> string {
print(label)
value
}
print(value: markString("named", "y"), ...markArray("spread", ["x"]))
"#,
    ))
    .expect("static named-before-spread fault emits");
    assert!(
        static_order.contains("named arguments must follow spread arguments (§5)")
            && !static_order.contains("__tpz_order_spread"),
        "got:\n{static_order}"
    );

    let receiver_named = emit_unit(&unit_of(
        r#"
function markArray(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
function markInt(label: string, value: int) -> int {
print(label)
value
}
let xs = [10]
xs.get(...markArray("spread", [0]), i: markInt("named", 0))
"#,
    ))
    .expect("receiver spread+named fault emits");
    assert!(
        receiver_named.contains("call_value_spread_named(SpreadNamedCall::new(__field")
            && receiver_named.contains("__tpz_recv_spread")
            && receiver_named.contains("spread arguments require a variadic parameter"),
        "got:\n{receiver_named}"
    );

    let receiver_order = emit_unit(&unit_of(
        r#"
function markArray(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
function markInt(label: string, value: int) -> int {
print(label)
value
}
let xs = [10]
xs.get(i: markInt("named", 0), ...markArray("spread", [0]))
"#,
    ))
    .expect("receiver named-before-spread fault emits");
    assert!(
        receiver_order.contains("named arguments must follow spread arguments (§5)")
            && !receiver_order.contains("__tpz_order_spread"),
        "got:\n{receiver_order}"
    );

    let optional_receiver_named = emit_unit(&unit_of(
        r#"
function markArray(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
function markInt(label: string, value: int) -> int {
print(label)
value
}
let xs: Option<Array<int>> = Some([10])
xs?.get(...markArray("spread", [0]), i: markInt("named", 0))
"#,
    ))
    .expect("optional receiver spread+named fault emits");
    assert!(
        optional_receiver_named.contains("call_value_spread_named(SpreadNamedCall::new(__f")
            && optional_receiver_named.contains("__tpz_recv_spread")
            && optional_receiver_named.contains("spread arguments require a variadic parameter"),
        "got:\n{optional_receiver_named}"
    );

    let optional_receiver_order = emit_unit(&unit_of(
        r#"
function markArray(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
function markInt(label: string, value: int) -> int {
print(label)
value
}
let xs: Option<Array<int>> = Some([10])
xs?.get(i: markInt("named", 0), ...markArray("spread", [0]))
"#,
    ))
    .expect("optional receiver named-before-spread fault emits");
    assert!(
        optional_receiver_order.contains("named arguments must follow spread arguments (§5)")
            && !optional_receiver_order.contains("__tpz_order_spread"),
        "got:\n{optional_receiver_order}"
    );

    let namespace_named = emit_unit(&unit_of(
        r#"
function markArray(label: string, xs: Array<Bytes>) -> Array<Bytes> {
print(label)
xs
}
function markBytes(label: string, value: Bytes) -> Bytes {
print(label)
value
}
Bytes.concat(Bytes.encodeUtf8("prefix"), ...markArray("spread", [Bytes.encodeUtf8("x")]), b: markBytes("named", Bytes.encodeUtf8("y")))
"#,
    ))
    .expect("namespace spread+named fault emits");
    assert!(
        namespace_named.contains("__tpz_ns_spread")
            && namespace_named.contains("spread arguments require a variadic parameter"),
        "got:\n{namespace_named}"
    );
}

#[test]
fn emits_a_string_scalars_call() {
    // §22.2 `s.scalars()` — a zero-arg read-only string method.
    let src = emit_unit(&unit_of("\"abc\".scalars()")).expect("emit");
    assert!(
        src.contains("call_method(__recv, \"scalars\", vec![]"),
        "got:\n{src}"
    );
}

#[test]
fn emits_an_in_place_mutator_call() {
    // §9/§22.2 a mutator on a `let mut` LOCAL lowers to the `member_value`-first dispatch
    // (a record field SHADOWS via `call_value`, else the shared `call_method` leaf); the
    // `Rc<RefCell>` clone still mutates the one collection.
    let src = emit_unit(&unit_of("let mut a = [1, 2]\na.push(3)\na")).expect("emit");
    assert!(
        src.contains("member_value(&__recv, \"push\""),
        "got:\n{src}"
    );
    assert!(src.contains("call_method(__recv, \"push\""), "got:\n{src}");
}

#[test]
fn emits_byte_buffer_writes_through_the_shared_direct_leaf() {
    let src = emit_unit(&unit_of(
        "let mut buffer = ByteBuffer.allocate(4)\nbuffer.set(1, 255)\nbuffer",
    ))
    .expect("emit ByteBuffer direct write");
    assert!(
        src.contains("if matches!(&__recv, Value::ByteBuffer(_))")
            && src.contains("builtin_byte_buffer_set(__recv"),
        "got:\n{src}"
    );
    let immutable = emit_unit(&unit_of(
        "let buffer = ByteBuffer.allocate(4)\nbuffer.set(1, 255)\nbuffer",
    ))
    .expect("emit immutable ByteBuffer write guard");
    assert!(
        immutable.contains("if matches!(&__recv, Value::ByteBuffer(_))")
            && immutable.contains("GUARD_IMMUTABLE"),
        "got:\n{immutable}"
    );
}

#[test]
fn emits_byte_buffer_reads_through_the_raw_direct_leaf() {
    let src = emit_unit(&unit_of(
        "let buffer = ByteBuffer.allocate(4)\nbuffer.get(1)",
    ))
    .expect("emit ByteBuffer direct read");
    assert!(
        src.contains("if matches!(&__recv, Value::ByteBuffer(_))")
            && src.contains("Value::Int(builtin_byte_buffer_get_i64(&__recv"),
        "got:\n{src}"
    );

    // `get` is not globally specialized: Array and record-field receivers
    // retain member-first generic dispatch and its async callable path.
    let array =
        emit_unit(&unit_of("let values = [1, 2]\nvalues.get(0)")).expect("emit Array.get fallback");
    assert!(
        array.contains("match member_value(&__recv, \"get\"")
            && array.contains("call_method(__recv, \"get\""),
        "got:\n{array}"
    );
    let shadow = emit_unit(&unit_of(
        "let value = { get: (index: int) => index + 1 }\nvalue.get(2)",
    ))
    .expect("emit record get shadow");
    assert!(
        shadow.contains("match member_value(&__recv, \"get\"") && shadow.contains("call_value(__f"),
        "got:\n{shadow}"
    );
}

#[test]
fn emits_direct_mutator_spread_faults() {
    let push = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
let mut xs = [1]
xs.push(...mark("push", [2]))
"#,
    ))
    .expect("mutating receiver push spread fault emits");
    assert!(
        push.contains("member_value(&__recv, \"push\",")
            && push.contains("call_value_spread(__f")
            && push.contains("__tpz_method_spread"),
        "got:\n{push}"
    );

    let remove = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<string>) -> Array<string> {
print(label)
xs
}
let mut m = map { "x": 1 }
m.remove(...mark("remove", ["x"]))
"#,
    ))
    .expect("mutating receiver remove spread fault emits");
    assert!(
        remove.contains("member_value(&__recv, \"remove\",")
            && remove.contains("__tpz_method_spread"),
        "got:\n{remove}"
    );

    let immutable = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
let xs = [1]
xs.push(...mark("push", [2]))
"#,
    ))
    .expect("immutable mutating receiver spread emits guard fault");
    assert!(
        immutable.contains("GUARD_IMMUTABLE") && immutable.contains("member_value(&__recv"),
        "got:\n{immutable}"
    );
}

#[test]
fn emits_direct_callback_mutator_spread_faults() {
    let sort_by = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<(int) -> int>) -> Array<(int) -> int> {
print(label)
xs
}
function key(x: int) -> int { x }
let mut xs = [2, 1]
xs.sortBy(...mark("sort", [key]))
"#,
    ))
    .expect("sortBy spread fault emits");
    assert!(
        sort_by.contains("member_value(&__recv, \"sortBy\",")
            && sort_by.contains("call_value_spread(__field")
            && sort_by.contains("__tpz_recv_spread"),
        "got:\n{sort_by}"
    );

    let retain = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<(int) -> bool>) -> Array<(int) -> bool> {
print(label)
xs
}
function keep(x: int) -> bool { x > 0 }
let mut xs = [1]
xs.retain(...mark("retain", [keep]))
"#,
    ))
    .expect("retain spread fault emits");
    assert!(
        retain.contains("member_value(&__recv, \"retain\",")
            && retain.contains("__tpz_recv_spread"),
        "got:\n{retain}"
    );

    let update = emit_unit(&unit_of(
        r#"
function mark(label: string, fs: Array<(int) -> int>) -> Array<(int) -> int> {
print(label)
fs
}
function inc(x: int) -> int { x + 1 }
let mut m = map { "x": 1 }
m.update("x", 0, ...mark("update", [inc]))
"#,
    ))
    .expect("Map.update spread fault emits");
    assert!(
        update.contains("member_value(&__recv, \"update\",")
            && update.contains("__tpz_recv_spread"),
        "got:\n{update}"
    );
}

#[test]
fn an_in_place_mutator_on_an_immutable_receiver_faults_guard_immutable() {
    // An immutable receiver no longer REFUSES at emit time — `member_value` resolves
    // first (a record field of that name would shadow), and the collection-mutator
    // arm encodes the `require_mut_root` fault (GUARD_IMMUTABLE) for the immutable
    // binding, after the type gate.
    let src = emit_unit(&unit_of("let a = [1, 2]\na.push(3)")).expect("emit");
    assert!(src.contains("GUARD_IMMUTABLE"), "got:\n{src}");
}

#[test]
fn emits_optional_member_access() {
    // §12 `object?.field` → the shared `optional_member` leaf.
    let src = emit_unit(&unit_of("let r = { n: 5 }\nr?.n")).expect("emit");
    assert!(
        src.contains("optional_member(_t_72.clone(), \"n\","),
        "got:\n{src}"
    );
}

#[test]
fn an_optional_mutator_value_checks_the_nonempty_branch() {
    let src = emit_unit(&unit_of("let a = [1, 2]\na?.push")).expect("emit");
    assert!(
        src.contains("Value::None => Value::None")
            && src.contains("bind_receiver_builtin(__recv, \"push\"")
            && src.contains("GUARD_IMMUTABLE"),
        "got:\n{src}"
    );
}

#[test]
fn emits_an_optional_call() {
    // §12 `obj?.field(args)` — the receiver short-circuits None/null, a
    // Some(inner) unwraps + calls + re-wraps (`wrap_optional`), any other
    // value calls directly. The inner dispatch is the bound-method dispatch
    // (member_value-first → call_value, else call_method).
    let src = emit_unit(&unit_of("Some(Array.of(1, 2))?.get(0)")).expect("emit");
    assert!(
        src.contains("Value::None => Value::None")
            && src.contains(
                "Value::Some(__inner) => { let __recv = (*__inner).clone(); wrap_optional("
            )
            && src.contains("call_method(__recv, \"get\","),
        "got:\n{src}"
    );
    let spread = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
let xs: Option<Array<int>> = Some([1, 2])
xs?.get(...mark("some", [0]))
"#,
    ))
    .expect("optional receiver spread fault emits");
    assert!(
        spread.contains("Value::None => Value::None")
            && spread.contains("__tpz_recv_spread")
            && spread.contains("spread arguments require a variadic parameter"),
        "got:\n{spread}"
    );
    let record_field_spread = emit_unit(&unit_of(
        r#"
function sum(seed: int = 0, ...xs: int) -> int {
let mut total = seed
for x in xs {
    total = total + x
}
total
}
let present = Some({ total: sum })
present?.total(...[1, 2], seed: 3)
"#,
    ))
    .expect("optional record-field spread call emits");
    assert!(
        record_field_spread.contains("wrap_optional(")
            && record_field_spread.contains("member_value(&__recv, \"total\",")
            && record_field_spread.contains("call_value_spread_named(SpreadNamedCall::new(__f"),
        "got:\n{record_field_spread}"
    );
    // A bare optional ACCESS `obj?.field` still emits via the
    // OptionalAccess arm.
    emit_unit(&unit_of("let r = { n: 1 }\nr?.n")).expect("bare optional access still emits");

    let hof = emit_unit(&unit_of("let xs = Some([1, 2])\nxs?.map((x) => x + 1)"))
        .expect("optional receiver HOF emits");
    assert!(
        hof.contains("wrap_optional(") && hof.contains("call_callback_receiver_map("),
        "got:\n{hof}"
    );

    let pipe_hof = emit_unit(&unit_of(
        "let xs = Some([1, 2])\n((x) => x + 1) |> xs?.map()",
    ))
    .expect("optional receiver HOF pipe emits");
    assert!(
        pipe_hof.contains("let __piped =")
            && pipe_hof.contains("wrap_optional(")
            && pipe_hof.contains("call_callback_receiver_map(__recv, __piped"),
        "got:\n{pipe_hof}"
    );

    let named_ok_or_else = emit_unit(&unit_of(
        r#"
function late() -> int {
6
}
let opt: Option<Option<int>> = Some(None)
opt?.okOrElse(f: late)
"#,
    ))
    .expect("optional receiver named okOrElse emits");
    assert!(
        named_ok_or_else.contains("wrap_optional(")
            && named_ok_or_else.contains("member_value(&__recv, \"okOrElse\",")
            && named_ok_or_else.contains("call_value_named(__field")
            && named_ok_or_else
                .contains("call_callback_ok_or_else(__recv, top_cell_get(&_t_6c617465"),
        "got:\n{named_ok_or_else}"
    );
}

#[test]
fn emits_optional_receiver_hof_spread_faults() {
    let map = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
let xs: Option<Array<int>> = Some([1, 2])
xs?.map(...mark("some", [0]))
"#,
    ))
    .expect("optional receiver map spread fault emits");
    assert!(
        map.contains("Value::None => Value::None")
            && map.contains("wrap_optional(")
            && map.contains("member_value(&__recv, \"map\",")
            && map.contains("__tpz_recv_spread"),
        "got:\n{map}"
    );

    let reduce = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
let xs: Option<Array<int>> = Some([1, 2])
xs?.reduce(0, ...mark("reduce", [0]))
"#,
    ))
    .expect("optional receiver reduce spread fault emits");
    assert!(
        reduce.contains("member_value(&__recv, \"reduce\",")
            && reduce.contains("__tpz_recv_spread"),
        "got:\n{reduce}"
    );

    let ok_or_else = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
let opt: Option<Option<int>> = Some(None)
opt?.okOrElse(...mark("ok", [0]))
"#,
    ))
    .expect("optional receiver okOrElse spread fault emits");
    assert!(
        ok_or_else.contains("member_value(&__recv, \"okOrElse\",")
            && ok_or_else.contains("__tpz_recv_spread"),
        "got:\n{ok_or_else}"
    );
}

#[test]
fn emits_optional_callback_mutator_spread_faults() {
    let sort_by = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<(int) -> int>) -> Array<(int) -> int> {
print(label)
xs
}
function key(x: int) -> int { x }
let mut xs: Option<Array<int>> = Some([2, 1])
xs?.sortBy(...mark("sort", [key]))
"#,
    ))
    .expect("optional sortBy spread fault emits");
    assert!(
        sort_by.contains("Value::None => Value::None")
            && sort_by.contains("wrap_optional(")
            && sort_by.contains("member_value(&__recv, \"sortBy\",")
            && sort_by.contains("__tpz_recv_spread"),
        "got:\n{sort_by}"
    );

    let retain = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<(int) -> bool>) -> Array<(int) -> bool> {
print(label)
xs
}
function keep(x: int) -> bool { x > 0 }
let mut xs: Option<Array<int>> = Some([1])
xs?.retain(...mark("retain", [keep]))
"#,
    ))
    .expect("optional retain spread fault emits");
    assert!(
        retain.contains("member_value(&__recv, \"retain\",")
            && retain.contains("__tpz_recv_spread"),
        "got:\n{retain}"
    );

    let update = emit_unit(&unit_of(
        r#"
function mark(label: string, fs: Array<(int) -> int>) -> Array<(int) -> int> {
print(label)
fs
}
function inc(x: int) -> int { x + 1 }
let mut m = map { "x": 1 }
let mut om: Option<Map<string, int>> = Some(m)
om?.update("x", 0, ...mark("update", [inc]))
"#,
    ))
    .expect("optional Map.update spread fault emits");
    assert!(
        update.contains("member_value(&__recv, \"update\",")
            && update.contains("__tpz_recv_spread"),
        "got:\n{update}"
    );

    let immutable = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<(int) -> int>) -> Array<(int) -> int> {
print(label)
xs
}
function key(x: int) -> int { x }
let xs: Option<Array<int>> = Some([2, 1])
xs?.sortBy(...mark("immutable", [key]))
"#,
    ))
    .expect("immutable optional sortBy spread emits guard fault");
    assert!(
        immutable.contains("Value::None => Value::None") && immutable.contains("GUARD_IMMUTABLE"),
        "got:\n{immutable}"
    );
}

#[test]
fn emits_optional_pipe_callback_mutator_spread_faults() {
    let sort_by = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
function key(x: int) -> int { x }
let mut xs: Option<Array<int>> = Some([2, 1])
key |> xs?.sortBy(...mark("sort", [0]))
"#,
    ))
    .expect("optional pipe sortBy spread fault emits");
    assert!(
        sort_by.contains("let __piped =")
            && sort_by.contains("Value::None => Value::None")
            && sort_by.contains("member_value(&__recv, \"sortBy\",")
            && sort_by.contains("__tpz_recv_spread"),
        "got:\n{sort_by}"
    );

    let retain = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
function keep(x: int) -> bool { x > 0 }
let mut xs: Option<Array<int>> = Some([1])
keep |> xs?.retain(...mark("retain", [0]))
"#,
    ))
    .expect("optional pipe retain spread fault emits");
    assert!(
        retain.contains("member_value(&__recv, \"retain\",")
            && retain.contains("__tpz_recv_spread"),
        "got:\n{retain}"
    );

    let update = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
function inc(x: int) -> int { x + 1 }
let mut m: Option<Map<string, int>> = Some(map { "x": 1 })
inc |> m?.update("x", 0, _, ...mark("update", [0]))
"#,
    ))
    .expect("optional pipe Map.update spread fault emits");
    assert!(
        update.contains("member_value(&__recv, \"update\",")
            && update.contains("__tpz_recv_spread"),
        "got:\n{update}"
    );

    let immutable = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
function key(x: int) -> int { x }
let xs: Option<Array<int>> = Some([2, 1])
key |> xs?.sortBy(...mark("immutable", [0]))
"#,
    ))
    .expect("immutable optional pipe sortBy spread emits guard fault");
    assert!(
        immutable.contains("Value::None => Value::None") && immutable.contains("GUARD_IMMUTABLE"),
        "got:\n{immutable}"
    );
}

#[test]
fn emits_optional_pipe_receiver_hof_spread_faults() {
    let map = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
function inc(x: int) -> int { x + 1 }
let xs: Option<Array<int>> = Some([1])
inc |> xs?.map(...mark("map", [0]))
"#,
    ))
    .expect("optional pipe map spread fault emits");
    assert!(
        map.contains("let __piped =")
            && map.contains("Value::None => Value::None")
            && map.contains("member_value(&__recv, \"map\",")
            && map.contains("__tpz_recv_spread"),
        "got:\n{map}"
    );

    let reduce = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
function add(a: int, b: int) -> int { a + b }
let xs: Option<Array<int>> = Some([1])
0 |> xs?.reduce(add, ...mark("reduce", [0]))
"#,
    ))
    .expect("optional pipe reduce spread fault emits");
    assert!(
        reduce.contains("member_value(&__recv, \"reduce\",")
            && reduce.contains("__tpz_recv_spread"),
        "got:\n{reduce}"
    );

    let ok_or_else = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
function fallback() -> int { 7 }
let opt: Option<Option<int>> = Some(None)
fallback |> opt?.okOrElse(...mark("ok", [0]))
"#,
    ))
    .expect("optional pipe okOrElse spread fault emits");
    assert!(
        ok_or_else.contains("member_value(&__recv, \"okOrElse\",")
            && ok_or_else.contains("__tpz_recv_spread"),
        "got:\n{ok_or_else}"
    );

    let map_filter = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
function keep(k: string, v: int) -> bool { v > 0 }
let m: Option<Map<string, int>> = Some(map { "x": 1 })
keep |> m?.filter(...mark("filter", [0]))
"#,
    ))
    .expect("optional pipe Map.filter spread fault emits");
    assert!(
        map_filter.contains("member_value(&__recv, \"filter\",")
            && map_filter.contains("__tpz_recv_spread"),
        "got:\n{map_filter}"
    );

    assert_eq!(
        emit_unit(&unit_of(
            r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
function inc(x: int) -> int { x + 1 }
let xs = [1]
inc |> xs.map(...mark("map", [0]), f: inc)
"#
        )),
        Err(EmitError::unsupported("call argument shape"))
    );
    assert_eq!(
        emit_unit(&unit_of(
            r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
function inc(x: int) -> int { x + 1 }
let xs: Option<Array<int>> = Some([1])
inc |> xs?.map(...mark("map", [0]), f: inc)
"#
        )),
        Err(EmitError::unsupported("receiver call spread argument"))
    );
}

#[test]
fn an_optional_mutating_method_call_lowers() {
    // §9/§12 the MUTATING methods are now supported in the optional call — the mut-root
    // keys on the path root (`mutation_root`); `Some(a)` is a non-`Ident` base (root
    // `None`) → no mut-guard, matching the interpreter's `require_mut_root(None)`.
    let src = emit_unit(&unit_of("let mut a = [1, 2]\nSome(a)?.push(3)")).expect("emit");
    assert!(src.contains("call_method(__recv, \"push\""), "got:\n{src}");
    // §22.3 a RESOURCE method now lowers through the host leaf (not `call_method`).
    let rsrc = emit_unit(&unit_of("let x = 5\nSome(x)?.read()")).expect("emit");
    assert!(
        rsrc.contains("call_resource_method(&*cx.host(), __recv, \"read\""),
        "got:\n{rsrc}"
    );

    let sort_by =
        emit_unit(&unit_of("let mut xs = Some([2, 1])\nxs?.sortBy((x) => x)")).expect("emit");
    assert!(
        sort_by.contains("collect_callback_keys(__items, __f")
            && sort_by.contains("sorted_by_keys(&__items, &__keys")
            && sort_by.contains("wrap_optional("),
        "got:\n{sort_by}"
    );

    let update = emit_unit(&unit_of(
        "let mut m = Map.new()\nm.insert(\"a\", 1)\nlet mut om = Some(m)\nom?.update(\"a\", 0, (v) => v + 1)",
    ))
    .expect("emit");
    assert!(
        update.contains("call_callback_map_update(__cell, __k, __init, __f")
            && update.contains("wrap_optional("),
        "got:\n{update}"
    );
}

#[test]
fn emits_the_optional_and_result_constructors() {
    // §22.1 Some/Ok/Err wrap one argument, mapping the name straight to
    // the Value variant (the interpreter's KCtor).
    assert!(
        emit_unit(&unit_of("Some(1)"))
            .unwrap()
            .contains("Value::Some(Rc::new(Value::Int(1)))")
    );
    assert!(
        emit_unit(&unit_of("Ok(2)"))
            .unwrap()
            .contains("Value::Ok(Rc::new(Value::Int(2)))")
    );
    assert!(
        emit_unit(&unit_of("Err(3)"))
            .unwrap()
            .contains("Value::Err(Rc::new(Value::Int(3)))")
    );
    assert!(
        emit_unit(&unit_of("Some(value: 1)"))
            .unwrap()
            .contains("Value::Some(Rc::new(Value::Int(1)))")
    );
    assert!(
        emit_unit(&unit_of("Ok(value: 2)"))
            .unwrap()
            .contains("Value::Ok(Rc::new(Value::Int(2)))")
    );
    assert!(
        emit_unit(&unit_of("Err(value: 3)"))
            .unwrap()
            .contains("Value::Err(Rc::new(Value::Int(3)))")
    );
}

#[test]
fn emits_a_bare_none_value() {
    // §22.1 a bare `None` (not a local) is the prelude nullary constructor
    // value `Value::None` — the interpreter's `ExprKind::Ident` None arm.
    assert!(emit_unit(&unit_of("None")).unwrap().contains("Value::None"));
    assert!(
        emit_unit(&unit_of("let x = None\nx"))
            .unwrap()
            .contains("Value::None")
    );
    // The coalesce LHS `None ?? 7` lowers the prelude None as its operand.
    assert!(
        emit_unit(&unit_of("None ?? 7"))
            .unwrap()
            .contains("Value::None")
    );
}

#[test]
fn emits_to_int_as_a_value() {
    // §22 a bare `toInt` (not a local, not a direct call) is the prelude
    // builtin VALUE `Value::Builtin { kind: ToInt, recv: None }`, dispatched
    // by call_value; a DIRECT `toInt(x)` stays the Call arm's builtin path.
    let val = emit_unit(&unit_of("let f = toInt\nf(\"42\")")).expect("emit");
    assert!(
        val.contains("Value::Builtin { kind: Builtin::ToInt, recv: None }"),
        "got:\n{val}"
    );
    // A direct call still lowers to the shared leaf, not a builtin value.
    let direct = emit_unit(&unit_of("toInt(\"42\")")).expect("emit");
    assert!(
        direct.contains("builtin_to_int(") && !direct.contains("Value::Builtin"),
        "got:\n{direct}"
    );
    // §22.2 `print` as a value lowers to `Value::Builtin { Print }`; a direct
    // `print(x)` stays the Call arm's builtin path.
    let pval = emit_unit(&unit_of("let f = print\nf(\"hi\")")).expect("emit");
    assert!(
        pval.contains("Value::Builtin { kind: Builtin::Print, recv: None }"),
        "got:\n{pval}"
    );
    let pdirect = emit_unit(&unit_of("print(\"hi\")")).expect("emit");
    assert!(
        pdirect.contains("builtin_print(") && !pdirect.contains("Value::Builtin"),
        "got:\n{pdirect}"
    );
    // §22.3 `open` as a value lowers to `Value::Builtin { Open }`; a direct
    // `open(x)` also lowers through it (call_value dispatches the host open).
    let oval = emit_unit(&unit_of("let f = open\nf(\"p\")")).expect("emit");
    assert!(
        oval.contains("Value::Builtin { kind: Builtin::Open, recv: None }"),
        "got:\n{oval}"
    );
    let odirect = emit_unit(&unit_of("open(\"p\")")).expect("emit");
    assert!(
        odirect.contains("Value::Builtin { kind: Builtin::Open, recv: None }")
            && odirect.contains("call_value("),
        "got:\n{odirect}"
    );
    // §22 the HOF builtins map/filter/reduce as values lower to
    // Value::Builtin{kind}; direct and value calls share the callback-HOF driver.
    let mval = emit_unit(&unit_of("let m = map\nm(Array.of(1), (x) => x)")).expect("emit");
    assert!(
        mval.contains("Value::Builtin { kind: Builtin::MapFn, recv: None }"),
        "got:\n{mval}"
    );
    let mdirect = emit_unit(&unit_of("map(Array.of(1), (x) => x)")).expect("emit");
    assert!(
        mdirect.contains("call_callback_hof(CallbackHofKind::Map")
            && !mdirect.contains("Value::Builtin"),
        "got:\n{mdirect}"
    );
}

#[test]
fn static_namespace_values_use_the_shared_builtin_catalog() {
    let generated = emit_unit(&unit_of(
        "let arrayOf = Array.of\nlet mapNew = Map.new\nlet setOf = Set.of\nlet jsonParse = JSON.parse\nlet mathAbs = Math.abs\n[arrayOf, mapNew, setOf, jsonParse, mathAbs]",
    ))
    .expect("static namespace values emit");
    for variant in ["ArrayOf", "MapNew", "SetOf", "JsonParse", "MathAbs"] {
        assert!(
            generated.contains(&format!("kind: Builtin::{variant}")),
            "missing {variant} from generated static namespace values:\n{generated}"
        );
    }

    let shadowed = emit_unit(&unit_of("let Math = { abs: 7 }\nMath.abs"))
        .expect("a local binding shadows the prelude namespace");
    assert!(!shadowed.contains("kind: Builtin::MathAbs"));
    assert!(shadowed.contains("member_value_required"));
}

#[test]
fn emits_the_try_operator() {
    // §13 `e?` inside a function lowers to the shared `try_value` leaf with
    // an `Ok` unwrap and an `Err` early-return from the function's async
    // block; a top-level `?` is refused (it would runtime-fault).
    let src = emit_unit(&unit_of("let f = (r) => { r? }\nf(Ok(1))")).expect("emit");
    assert!(
        src.contains("try_value(")
            && src.contains("Ok(__v) => __v")
            // §14 the Err arm is now a BLOCK so a block-level `defer` can drain before
            // the early return; with no enclosing block defers the drain is empty.
            && src.contains("Err(__early) => { return Ok(__early) }"),
        "got:\n{src}"
    );
    // A top-level `?` (outside a function/lambda) is refused, exactly as a
    // top-level `return` is.
    assert_eq!(
        emit_unit(&unit_of("Ok(1)?")),
        Err(EmitError::unsupported("return outside a function"))
    );
}

#[test]
fn emits_function_composition() {
    // §11 `f >> g` → `Value::Composed((f, g))`; the operands lower
    // left-to-right and callability is deferred to the call site.
    let src = emit_unit(&unit_of(
        "let inc = (x) => x + 1\nlet dbl = (x) => x * 2\nlet h = inc >> dbl\nh(5)",
    ))
    .expect("emit");
    assert!(
        src.contains("Value::Composed(Rc::new((") && src.contains("call_value("),
        "got:\n{src}"
    );
}

#[test]
fn emits_the_pipe_operator() {
    // §11 a unary stage `x |> f` → `f(x)` via call_value, the piped value
    // bound to `__piped` first.
    let unary = emit_unit(&unit_of("let inc = (x) => x + 1\n5 |> inc")).expect("emit");
    assert!(
        unary.contains("let __piped =")
            && unary.contains("call_value(")
            && unary.contains("vec![__piped]"),
        "got:\n{unary}"
    );
    // §11 a call stage `x |> f(a)` → first-argument insertion `f(x, a)`.
    let insert = emit_unit(&unit_of("let add = (a, b) => a + b\n5 |> add(3)")).expect("emit");
    assert!(insert.contains("vec![__piped,"), "got:\n{insert}");
    // §11 a generic callable stage can keep named arguments: without `_` the
    // piped value remains the first positional, with `_` the call uses named
    // binding and no first-argument insertion.
    let named_insert = emit_unit(&unit_of(
        "function sub(a: int, b: int) -> int { a - b }\n10 |> sub(b: 3)",
    ))
    .expect("emit");
    assert!(
        named_insert.contains("call_value_named(")
            && named_insert.contains("vec![__piped]")
            && named_insert.contains("(\"b\".to_string(),"),
        "got:\n{named_insert}"
    );
    let named_ph = emit_unit(&unit_of(
        "function sub(a: int, b: int) -> int { a - b }\n10 |> sub(b: 3, a: _)",
    ))
    .expect("emit");
    let ph_local = mangle("_");
    assert!(
        named_ph.contains(&format!("let {ph_local} ="))
            && named_ph.contains("call_value_named(")
            && named_ph.contains("vec![]")
            && named_ph.contains("(\"b\".to_string(),")
            && named_ph.contains("(\"a\".to_string(),"),
        "got:\n{named_ph}"
    );
    let spread_insert = emit_unit(&unit_of(
        "function sum(seed: int, ...xs: int) -> int { seed }\nlet xs = [2, 3]\n1 |> sum(...xs)",
    ))
    .expect("emit");
    assert!(
        spread_insert.contains("call_value_spread(")
            && spread_insert.contains("vec![__piped]")
            && spread_insert.contains("call_spread_extend("),
        "got:\n{spread_insert}"
    );
    let spread_ph = emit_unit(&unit_of(
        "function sum(seed: int, ...xs: int) -> int { seed }\nlet xs = [2, 3]\n1 |> sum(..._)",
    ))
    .expect("emit");
    assert!(
        spread_ph.contains(&format!("let {ph_local} ="))
            && spread_ph.contains("call_value_spread(")
            && spread_ph.contains("vec![]")
            && spread_ph.contains(&format!("call_spread_extend(&mut __sp, {ph_local}.clone()")),
        "got:\n{spread_ph}"
    );
    // §11 a FIELD stage `r |> .x` → the pure member access leaf.
    let field = emit_unit(&unit_of("let r = { x: 5 }\nr |> .x")).expect("emit");
    assert!(
        field.contains("member_value_required(") && field.contains("\"x\""),
        "got:\n{field}"
    );
    // §11 a PLACEHOLDER stage `x |> f(_, y)` binds `_` to the piped value
    // (a `let <mangle(_)> = …`) and runs the call with NO first-arg insertion
    // (no `__piped` first); the `_` arg reads the bound local.
    let ph = emit_unit(&unit_of("let sub = (a, b) => a - b\n3 |> sub(10, _)")).expect("emit");
    assert!(
        ph.contains(&format!("let {ph_local} ="))
            && !ph.contains("vec![__piped,")
            && ph.contains("call_value("),
        "got:\n{ph}"
    );
    // A field pipe naming a read-only bound method (`xs |> .get`) is the
    // same member-value bridge as `xs.get`.
    let pf = emit_unit(&unit_of("let xs = Array.of(1, 2)\nxs |> .get")).expect("emit");
    assert!(
        pf.contains("bind_receiver_builtin(__recv, \"get\","),
        "got:\n{pf}"
    );
    // §11 a READ-ONLY bound-method stage `x |> recv.get()` inserts the piped
    // value as the first arg through the bound-method dispatch.
    let pm = emit_unit(&unit_of("let xs = Array.of(1, 2)\n0 |> xs.get()")).expect("emit");
    assert!(
        pm.contains("let __piped =")
            && pm.contains("member_value(&__recv, \"get\"")
            && pm.contains("call_method(__recv, \"get\", vec![__piped]"),
        "got:\n{pm}"
    );
    // §9 a MUTATING bound-method stage now lowers — a `let mut` local root reaches
    // the shared `call_method` mutator leaf (the lead is the pushed element).
    let pmut = emit_unit(&unit_of("let mut xs = Array.of(1, 2)\n0 |> xs.push()")).expect("emit");
    assert!(
        pmut.contains("call_method(__recv, \"push\", vec![__piped]"),
        "got:\n{pmut}"
    );
    // An IMMUTABLE root still lowers, but to the GUARD_IMMUTABLE fault (run≡build).
    let pimm = emit_unit(&unit_of("let xs = Array.of(1, 2)\n0 |> xs.push()")).expect("emit");
    assert!(pimm.contains("codes::GUARD_IMMUTABLE"), "got:\n{pimm}");
    // Named arguments and a placeholder retain their source labels while the
    // placeholder consumes the piped lead.
    let named = emit_unit(&unit_of(
        "let mut xs = Array.of(1, 2)\n3 |> xs.insert(index: 0, value: _)\nxs",
    ))
    .expect("emit named receiver pipe stage");
    assert!(
        named.contains("call_method_named(__recv, \"insert\"")
            && named.contains("(\"index\".to_string(), Value::Int(0))")
            && named.contains("(\"value\".to_string(), _t_5f.clone())"),
        "got:\n{named}"
    );
    // Receiver HOF method-call stages need the HOF-specific member dispatch,
    // not the generic first-argument insertion fallback.
    let phof = emit_unit(&unit_of("let xs = [1, 2]\n((x) => x + 1) |> xs.map()")).expect("emit");
    assert!(
        phof.contains("member_value(&__recv, \"map\"")
            && phof.contains("call_callback_receiver_map(__recv, __piped"),
        "got:\n{phof}"
    );
    let phof_named =
        emit_unit(&unit_of("let xs = [1, 2]\n((x) => x + 1) |> xs.map(f: _)")).expect("emit");
    assert!(
        phof_named.contains("call_value_named(__field")
            && phof_named.contains("(\"f\".to_string(), __piped)")
            && phof_named.contains("call_callback_receiver_map(__recv, __piped"),
        "got:\n{phof_named}"
    );
    let popt_hof_named = emit_unit(&unit_of(
        "let xs = Some([1, 2])\n((x) => x + 1) |> xs?.map(f: _)",
    ))
    .expect("emit");
    assert!(
        popt_hof_named.contains("call_value_named(__field")
            && popt_hof_named.contains("(\"f\".to_string(), __piped)")
            && popt_hof_named.contains("wrap_optional("),
        "got:\n{popt_hof_named}"
    );
    let preduce = emit_unit(&unit_of(
        "let xs = [1, 2]\n10 |> xs.reduce((a, x) => a + x)",
    ))
    .expect("emit");
    assert!(
        preduce.contains("member_value(&__recv, \"reduce\"")
            && preduce.contains("call_callback_hof(CallbackHofKind::Reduce"),
        "got:\n{preduce}"
    );
    let preduce_named = emit_unit(&unit_of(
        "function add(a: int, b: int) -> int { a + b }\nlet xs = [1, 2]\n0 |> xs.reduce(initial: _, f: add)"
    ))
    .expect("emit");
    assert!(
        preduce_named.contains("call_value_named(__field")
            && preduce_named.contains("(\"initial\".to_string(), __piped)")
            && preduce_named.contains("(\"f\".to_string(), __a1)")
            && preduce_named.contains("call_callback_hof(CallbackHofKind::Reduce"),
        "got:\n{preduce_named}"
    );
    let preduce_named_nested = emit_unit(&unit_of(
        "function add(a: int, b: int) -> int { a + b }\nfunction mark(label: string, n: int) -> int { print(label)\n n }\nlet xs = [1, 2]\n10 |> xs.reduce(initial: mark(\"initial\", 1) + _, f: add)"
    ))
    .expect("emit");
    assert!(
        preduce_named_nested.contains("let __piped =")
            && preduce_named_nested.contains(&format!("let {} = __piped.clone();", mangle("_")))
            && preduce_named_nested.contains("(\"initial\".to_string(), __a0)")
            && preduce_named_nested.contains("(\"f\".to_string(), __a1)")
            && preduce_named_nested.contains("call_callback_hof(CallbackHofKind::Reduce"),
        "got:\n{preduce_named_nested}"
    );
    let pupdate_named = emit_unit(&unit_of(
        "function inc(x: int) -> int { x + 1 }\nlet mut m = map { \"a\": 1 }\n10 |> m.update(k: \"a\", initial: _, f: inc)"
    ))
    .expect("emit");
    assert!(
        pupdate_named.contains("call_value_named(__field")
            && pupdate_named.contains("(\"initial\".to_string(), __piped)")
            && pupdate_named.contains("(\"f\".to_string(), __a2)")
            && pupdate_named.contains("let __init = __piped")
            && pupdate_named.contains("call_callback_map_update(__cell, __k, __init, __f"),
        "got:\n{pupdate_named}"
    );
    let psort =
        emit_unit(&unit_of("let mut xs = [2, 1]\n((x) => x) |> xs.sortBy()")).expect("emit");
    assert!(
        psort.contains("collect_callback_keys(__items, __f")
            && psort.contains("sorted_by_keys(&__items, &__keys"),
        "got:\n{psort}"
    );
    let pupdate_f = emit_unit(&unit_of(
        "function bump(v: int) -> int { v + 5 }\nlet mut m = map { \"a\": 1 }\nbump |> m.update(\"a\", 0, _)",
    ))
    .expect("emit");
    assert!(
        pupdate_f.contains("let __piped =")
            && pupdate_f.contains("call_callback_map_update(__cell, __k, __init, __f"),
        "got:\n{pupdate_f}"
    );
    emit_unit(&unit_of(
        "function bump(v: int) -> int { v + 5 }\nlet mut m = map { \"a\": 1 }\n\"a\" |> m.update(_, 0, bump)",
    ))
    .expect("pipe update key placeholder emits");
    emit_unit(&unit_of(
        "function bump(v: int) -> int { v + 5 }\nlet mut m = map { \"a\": 1 }\n7 |> m.update(\"b\", _, bump)",
    ))
    .expect("pipe update initial placeholder emits");
    // §11/§12 an OPTIONAL-call stage `x |> r?.f()` threads the lead through
    // the optional-call dispatch (None/Null short-circuit, Some re-wrap).
    let po = emit_unit(&unit_of("let r = Some(1)\n2 |> r?.f()")).expect("emit");
    assert!(
        po.contains("let __piped =")
            && po.contains("Value::None => Value::None")
            && po.contains("wrap_optional(")
            && po.contains("vec![__piped]"),
        "got:\n{po}"
    );
    // §9 a MUTATING optional-method stage now lowers (the mut-root keys on the path
    // root); a RESOURCE optional-method stage now lowers through the host leaf.
    let pm = emit_unit(&unit_of("let mut r = Some([1])\n2 |> r?.push()")).expect("emit");
    assert!(pm.contains("call_method(__recv, \"push\""), "got:\n{pm}");
    let pomupdate = emit_unit(&unit_of(
        "function bump(v: int) -> int { v + 5 }\nlet mut m = map { \"a\": 1 }\nlet mut om = Some(m)\nbump |> om?.update(\"a\", 0, _)",
    ))
    .expect("optional pipe update callback placeholder emits");
    assert!(
        pomupdate.contains("wrap_optional(")
            && pomupdate.contains("call_callback_map_update(__cell, __k, __init, __f"),
        "got:\n{pomupdate}"
    );
    let pr = emit_unit(&unit_of("let r = Some(1)\n2 |> r?.write()")).expect("emit");
    assert!(
        pr.contains("call_resource_method(&*cx.host(), __recv, \"write\""),
        "got:\n{pr}"
    );
}

#[test]
fn emits_print_as_the_shared_builtin_through_the_host() {
    // `print(s)` lowers to the shared `builtin_print`, writing through
    // the host on `cx`; the call expression's span is threaded.
    let src = emit_unit(&unit_of(r#"print("hello")"#)).expect("emit");
    assert!(
        src.contains(r#"builtin_print(&*cx.host(), Value::str("hello"), Span::new(FileId("#),
        "got:\n{src}"
    );
}

#[test]
fn print_of_a_non_string_still_emits_and_is_guarded_at_runtime() {
    // The emitter does not itself reject a non-string arg — the shared
    // builtin faults it (string-only) at runtime, like the interpreter.
    let src = emit_unit(&unit_of("print(5)")).expect("emit");
    assert!(
        src.contains("builtin_print(&*cx.host(), Value::Int(5),"),
        "got:\n{src}"
    );
}

#[test]
fn an_unknown_free_call_is_unsupported() {
    // A free, non-builtin callee reaches the generic call path, but
    // lowering the unresolved callee `foo` refuses it as a free identifier.
    assert_eq!(
        emit_unit(&unit_of("foo(1)")),
        Err(EmitError::unsupported("free identifier"))
    );
}

#[test]
fn print_with_the_wrong_argument_shape_is_unsupported() {
    // `print` takes exactly one positional argument.
    assert_eq!(
        emit_unit(&unit_of(r#"print("a", "b")"#)),
        Err(EmitError::unsupported("builtin call shape"))
    );
}

#[test]
fn emits_to_int_as_the_shared_pure_builtin() {
    // `toInt(s)` lowers to the shared (host-free) builtin.
    let src = emit_unit(&unit_of(r#"toInt("42")"#)).expect("emit");
    assert!(
        src.contains(r#"builtin_to_int(Value::str("42"), Span::new(FileId("#),
        "got:\n{src}"
    );
}

#[test]
fn emits_range_via_the_shared_make_range() {
    // `lo .. hi` lowers to the shared builder; no `by` is `None`.
    let src = emit_unit(&unit_of("0..3")).expect("emit");
    assert!(
        src.contains("make_range(Value::Int(0), Value::Int(3),"),
        "got:\n{src}"
    );
    assert!(src.contains(", None, Span::new(FileId("), "got:\n{src}");
}

#[test]
fn emits_range_with_a_step() {
    let src = emit_unit(&unit_of("0..10 by 2")).expect("emit");
    assert!(
        src.contains("make_range(Value::Int(0), Value::Int(10),"),
        "got:\n{src}"
    );
    assert!(src.contains("Some(Value::Int(2)),"), "got:\n{src}");
}

#[test]
fn emits_for_over_a_range() {
    // `i` mangles to `_t_69`; the iterable is a range value.
    let src = emit_unit(&unit_of("let mut s = 0\nfor i in 0..3 { s = s + i }\ns")).expect("emit");
    assert!(
        src.contains("for _t_69 in for_items(&(make_range(Value::Int(0), Value::Int(3),"),
        "got:\n{src}"
    );
}

#[test]
fn break_in_a_while_condition_is_refused() {
    // In the interpreter a `break` reached
    // while evaluating a (nested) `while` condition targets an OUTER
    // loop, because this loop's frame is not pushed until the
    // condition tests true. An UNLABELED Rust `break` cannot express
    // that from a `while` condition (E0590), so the emitter must
    // refuse rather than emit code that fails to compile. The
    // condition is lowered as not-in-loop, so the inner `break` is
    // refused even though the `while` is nested in an outer loop.
    let program =
        "let mut a = true\nwhile a { while (if a { break } else { true }) { }\na = false }\n99";
    assert_eq!(
        emit_unit(&unit_of(program)),
        Err(EmitError::unsupported("break outside loop"))
    );
}
