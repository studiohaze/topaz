use super::*;

#[test]
fn emits_an_integer_program() {
    let src = emit_unit(&unit_of("42")).expect("emit");
    assert!(
        src.contains("let __topaz_init_value = Value::Int(42);"),
        "got:\n{src}"
    );
    assert!(src.contains("pub fn run_with_host(host: Rc<dyn Host>) -> RunOutcome"));
    assert!(src.contains("pub const TOPAZ_EXPLICIT_MAIN: bool = false"));
    assert!(src.contains("pub fn topaz_export_names() -> &'static [&'static str]"));
    assert!(src.contains("&[]"));
    assert!(src.contains("pub fn run_with_host_and_input"));
    assert!(src.contains("async fn entry(cx: RtCx, __topaz_args: Value, __topaz_stdin: Value)"));
    assert!(src.contains("async fn __topaz_initialize(cx: RtCx)"));
    assert!(src.contains("__topaz_initialize(cx).await?"));
    assert!(!src.contains("__topaz_initialize(cx.clone()"));
}

#[test]
fn explicit_main_preserves_entry_input_for_the_main_call() {
    let src = emit_unit(&unit_of(
        "export function main(args: Array<string>, stdin: string) -> int { 42 }",
    ))
    .expect("explicit main emits");
    assert!(src.contains("pub const TOPAZ_EXPLICIT_MAIN: bool = true"));
    assert!(src.contains("__topaz_initialize(cx.clone()).await?"));
    assert!(src.contains("vec![__topaz_args, __topaz_stdin]"));
}

#[test]
fn v520_typed_json_emits_selected_qualified_and_nested_imported_schemas() {
    let unit = unit_with_files_at(
        "main.tpz",
        &[
            (
                "scalar.tpz",
                "export type Scalar = int\nexport record Hidden { name: string }\n",
            ),
            (
                "model.tpz",
                "import scalar { Scalar }\nexport record User { name: string, rank: Scalar = 0 }\n",
            ),
            (
                "selected.tpz",
                "import scalar { Scalar, Hidden }\nexport type UserAlias = Hidden\nexport record Box<T> { value: T, rank: Scalar }\n",
            ),
            (
                "main.tpz",
                "import model\nimport selected { Box, UserAlias }\nlet qualified = JSON.parseAs<model.User>(\"\\{\\\"name\\\":\\\"Ada\\\"\\}\")\nlet aliased = JSON.parseAs<UserAlias>(\"\\{\\\"name\\\":\\\"Bea\\\"\\}\")\nJSON.parseAs<Box<int>>(\"\\{\\\"value\\\":7,\\\"rank\\\":8\\}\")\n",
            ),
        ],
        topaz_syntax::LangVersion::V5_20,
    );
    let generated = emit_unit(&unit).expect("5.20 imported typed JSON emits");
    assert!(
        generated.contains("Some(Rc::from(\"model::User\"))")
            && generated.contains("Some(Rc::from(\"scalar::Hidden\"))")
            && generated.contains("Some(Rc::from(\"selected::Box\"))"),
        "generated schemas must retain defining declaration identities:\n{generated}"
    );
}

#[test]
fn v520_same_spelled_nominal_patterns_emit_declaration_identity_tests() {
    let unit = unit_with_files_at(
        "main.tpz",
        &[
            ("alpha.tpz", "export record User { id: int }\n"),
            ("beta.tpz", "export record User { id: int }\n"),
            (
                "main.tpz",
                "import alpha { User as AlphaUser }\nimport beta { User as BetaUser }\nlet alpha = AlphaUser { id: 1 }\nlet beta = BetaUser { id: 1 }\nlet distinct = alpha != beta\nmatch alpha {\ncase BetaUser { id } => 0\ncase AlphaUser { id } => id\n}\n",
            ),
        ],
        topaz_syntax::LangVersion::V5_20,
    );
    let generated = emit_unit(&unit).expect("5.20 stable nominal patterns emit");
    assert!(
        generated.contains("is_nominal_record_declaration(\"alpha::User\")")
            && generated.contains("is_nominal_record_declaration(\"beta::User\")")
            && generated.contains("nominal_record_with_identities(\"User\", \"alpha::User\"")
            && generated.contains("nominal_record_with_identities(\"User\", \"beta::User\""),
        "generated values and patterns must use defining declaration identities:\n{generated}"
    );
}

#[test]
fn newtype_first_class_value_bridge_lowers_to_a_bound_builtin() {
    let src = emit_unit(&unit_of(
        "newtype UserId = int\nlet id: UserId = UserId(7)\nlet get: () -> int = id.value\nget()",
    ))
    .expect("accepted first-class newtype bridge should emit");
    assert!(
        src.contains("member_value_required") && src.contains("\"value\""),
        "newtype `.value` should lower through the shared bound-method leaf:\n{src}"
    );
}

#[test]
fn selected_generic_newtype_typed_pattern_uses_defining_base_type() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import model { Box as ForeignBox }
let value: ForeignBox<int> = ForeignBox(7)
match value {
case found: ForeignBox<int> => found.value()
case _ => 0
}
"#,
            ),
            ("model.tpz", "export newtype Box<T> = T"),
        ],
    );
    let src = emit_unit(&unit).expect("selected generic newtype pattern should emit");
    assert!(
        src.contains("Value::Newtype") && src.contains("Value::Int"),
        "selected generic newtype patterns must lower the defining base under use-site args:\n{src}"
    );
}

#[test]
fn bare_qualified_generic_nominal_declines_before_rust_emission() {
    let unit = unit_with_files_at(
        "main.tpz",
        &[
            (
                "lib.tpz",
                "export newtype Box<T> = T\nexport let value: Box<int> = Box(7)\n",
            ),
            (
                "main.tpz",
                "import lib\nmatch lib.value {\ncase found: lib.Box => 1\ncase _ => 0\n}\n",
            ),
        ],
        topaz_syntax::LangVersion::V5_20,
    );
    let error = emit_unit(&unit).expect_err("missing type argument declines");
    assert_eq!(error.to_string(), "unsupported: typed pattern type");
}

#[test]
fn bare_named_generic_nominals_decline_before_rust_emission() {
    let local = unit_with_files_at(
        "main.tpz",
        &[(
            "main.tpz",
            "record Box<T> { value: T }\nlet value: Box<int> = Box { value: 7 }\nmatch value {\ncase found: Box => 1\ncase _ => 0\n}\n",
        )],
        topaz_syntax::LangVersion::V5_20,
    );
    let local_error = emit_unit(&local).expect_err("local missing type argument declines");
    assert_eq!(local_error.to_string(), "unsupported: typed pattern type");

    let selected = unit_with_files_at(
        "main.tpz",
        &[
            (
                "lib.tpz",
                "export newtype Box<T> = T\nexport function value() -> Box<int> { Box(7) }\n",
            ),
            (
                "main.tpz",
                "import lib { Box as ForeignBox, value }\nmatch value() {\ncase found: ForeignBox => 1\ncase _ => 0\n}\n",
            ),
        ],
        topaz_syntax::LangVersion::V5_20,
    );
    let selected_error = emit_unit(&selected).expect_err("selected missing type argument declines");
    assert_eq!(
        selected_error.to_string(),
        "unsupported: typed pattern type"
    );
}

#[test]
fn receiver_method_module_value_capture_uses_a_delayed_top_cell() {
    let unit = unit_of(
        r#"
record Point { x: int }
let offset = 2
impl Point {
function shifted(self) -> int { self.x + offset }
}
Point { x: 1 }.shifted()
"#,
    );
    let source = emit_unit(&unit).expect("module value capture should emit");
    let offset = mangle("offset");
    assert!(source.contains(&format!("let {offset} = top_cell();")));
    assert!(source.contains(&format!("top_cell_set(&{offset}, Value::Int(2));")));
    assert!(source.contains(&format!("top_cell_get(&{offset}, \"offset\"")));
    assert!(source.contains("__method_register(\"__entry__::Point\", \"shifted\""));
    assert!(source.contains("Some(\"__entry__::Point\")"));
}

#[test]
fn exported_receiver_method_registers_in_its_defining_module() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                "import model\nlet p: model.Point = model.make(4)\np.coordinate()",
            ),
            (
                "model.tpz",
                "export record Point { x: int }\nimpl Point { export function coordinate(self) -> int { self.x } }\nexport function make(x: int) -> Point { Point { x: x } }",
            ),
        ],
    );
    let source = emit_unit(&unit).expect("exported receiver method should emit");
    assert!(source.contains("__method_register(\"model::Point\", \"coordinate\""));
    assert!(source.contains("Some(\"model::Point\")"));
    assert!(source.contains("method_dispatch_id()"));
}

#[test]
fn exported_method_capture_reads_the_value_from_its_top_cell() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                "import model\nlet p: model.Point = model.make()\np.shifted() + model.offset",
            ),
            (
                "model.tpz",
                "export record Point { x: int }\nexport let offset = 2\nimpl Point { export function shifted(self) -> int { self.x + offset } }\nexport function make() -> Point { Point { x: 1 } }",
            ),
        ],
    );
    let source = emit_unit(&unit).expect("exported captured value should emit");
    let offset = mangle("offset");
    assert!(source.contains(&format!(
        "(\"offset\".to_string(), top_cell_value(&{offset}, \"offset\")?)"
    )));
}

#[test]
fn exported_typed_let_keeps_its_guard_and_runtime_export() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            ("main.tpz", "import model { value }\nvalue + 1"),
            ("model.tpz", "export let value: int = 41"),
        ],
    );
    let source = emit_unit(&unit).expect("exported typed let should emit");
    let value = mangle("value");
    assert!(
        source.contains(&format!("let {value} = {{ let __v = Value::Int(41)"))
            && source.contains("fault(codes::GUARD_TYPE")
            && source.contains(&format!(
                "Value::record([(\"value\".to_string(), {value}.clone())])"
            )),
        "{source}"
    );
}

#[test]
fn protocol_impl_module_value_capture_reads_the_top_cell() {
    let unit = unit_of(
        r#"
protocol Shift { function shifted(value: Self) -> int }
record Point { x: int }
let offset = 2
impl Shift<Point> {
function shifted(value: Point) -> int { value.x + offset }
}
Shift.shifted(Point { x: 1 })
"#,
    );
    let source = emit_unit(&unit).expect("protocol module value capture should emit");
    let offset = mangle("offset");
    assert!(
        source.contains(&format!("top_cell_get(&{offset}, \"offset\""))
            && source.contains(
                "__protocol_method_register(\"__entry__\", \"Shift\", \"Point\", \"shifted\"",
            )
            && source
                .contains("__protocol_method_lookup(\"__entry__\", \"Shift\", __id, \"shifted\")",),
        "{source}"
    );
}

#[test]
fn emits_host_callable_entry_exports() {
    let src = emit_unit(&unit_of(
        "export function add(x: int) -> int { x + 1 }\n\
         export const K = 2\n\
         export type Id = int\n\
         add(K)",
    ))
    .expect("entry exports lower");
    assert!(src.contains("pub fn topaz_export_names() -> &'static [&'static str]"));
    assert!(src.contains("&[\"add\", \"K\"]"), "got:\n{src}");
    assert!(src.contains("pub fn call_export_with_host("), "got:\n{src}");
    assert!(
        src.contains("pub fn call_export_with_host_until(")
            && src.contains("block_on_until(deadline"),
        "got:\n{src}"
    );
    assert!(
        src.contains("pub fn call_export_json_with_host("),
        "got:\n{src}"
    );
    assert!(
        src.contains("canonical_abi_decode_args(args_json)"),
        "got:\n{src}"
    );
    assert!(src.contains("async fn __topaz_exports("), "got:\n{src}");
    assert!(
        src.contains("async fn __topaz_call_export(")
            && src.contains("block_on(__topaz_call_export(cx, name, args, __span))")
            && src.contains("__topaz_call_export(cx, &name, args, __span).await"),
        "got:\n{src}"
    );
    assert_eq!(
        src.matches("let __exports = __topaz_exports(cx.clone()).await?;")
            .count(),
        1,
        "export initialization must have one generated owner:\n{src}"
    );
    assert!(
        src.contains("(\"add\".to_string(), top_cell_value(&"),
        "got:\n{src}"
    );
    assert!(
        src.contains("(\"K\".to_string(),") && !src.contains("(\"Id\".to_string(),"),
        "got:\n{src}"
    );
}

#[test]
fn emit_module_is_emit_unit_minus_the_crate_attribute() {
    // The two shapes share the lowering; only the envelope differs,
    // so the differential harness (which include!s emit_module)
    // proves exactly what `topaz emit` (emit_unit) ships.
    let unit = unit_of("1 + 2");
    let full = emit_unit(&unit).unwrap();
    let module = emit_module(&unit).unwrap();
    assert!(full.starts_with("#![forbid(unsafe_code)]\n"));
    assert!(!module.contains("#!["));
    assert_eq!(full, format!("#![forbid(unsafe_code)]\n{module}"));
    assert!(module.contains("pub fn run_with_host"));
}

#[test]
fn refuses_return_in_a_concurrent_arm() {
    // An arm lowers as a closure with no function boundary; the interpreter faults a bare
    // `return` in it, so the emitter refuses rather than `return` out of the closure.
    let err = emit_unit(&unit_of(
        "function f() {\n  concurrent {\n    a: { return 5 }\n    b: 1\n  }\n}\nf()",
    ))
    .unwrap_err();
    assert_eq!(
        err,
        EmitError::unsupported("`return`/`?` in a concurrent arm")
    );
}

#[test]
fn refuses_return_in_a_concurrent_timeout_else() {
    // A non-instant zero-timeout arm can really reach the else closure; until that path
    // has control-flow-aware lowering, keep the old loud boundary.
    let err = emit_unit(&unit_of(
        "function f() {\n  let r = concurrent(timeout: 0ms) {\n    slow: { while true {} }\n    fast: 1\n  } else {\n    return 5\n  }\n  r\n}\nf()",
    ))
    .unwrap_err();
    assert_eq!(
        err,
        EmitError::unsupported("`return`/`?` in a concurrent else")
    );

    let instant_try = emit_unit(&unit_of(
        "function fail() -> Result<int, string> {\n  Err(\"boom\")\n}\nfunction f() -> Result<int, string> {\n  let r = concurrent(timeout: 0ms) {\n    x: 1\n  } else {\n    { x: fail()? }\n  }\n  Ok(r.x)\n}\nf()",
    ))
    .expect("zero-timeout single instant arm keeps the else-try thunk unreachable");
    assert!(
        instant_try.contains("concurrent_join_timeout(cx.clone(), 0"),
        "instant else-try lowering must retain the frozen-clock executor:\n{instant_try}"
    );
}

#[test]
fn lowers_zero_timeout_single_instant_concurrent_timeout() {
    let small = emit_unit(&unit_of(
        "function f() {\n  let r = concurrent(timeout: 0ms) {\n    x: 21 * 2\n  } else {\n    { x: 0 }\n  }\n  r.x\n}\nf()",
    ))
    .expect("zero-timeout single instant record lowers");
    assert!(
        small.contains("match (async { Ok::<Value, RtError>")
            && !small.contains("concurrent_join_timeout(cx.clone()"),
        "zero-timeout single instant should lower through direct evaluate/catch:\n{small}"
    );

    let large = emit_unit(&unit_of(
        "function f() {\n  let r = concurrent(timeout: 0ms) {\n    x: [0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29]\n  } else {\n    { x: [-1] }\n  }\n  r.x[0]\n}\nf()",
    ))
    .expect("zero-timeout single large instant record lowers");
    assert!(
        large.contains("match (async { Ok::<Value, RtError>")
            && !large.contains("concurrent_join_timeout(cx.clone()"),
        "zero-timeout single large instant should lower through direct evaluate/catch:\n{large}"
    );

    let noninstant = emit_unit(&unit_of(
        "function slow() { 1 }\nfunction f() {\n  let r = concurrent(timeout: 0ms) {\n    x: slow()\n  } else {\n    { x: 0 }\n  }\n  r.x\n}\nf()",
    ))
    .expect("zero-timeout single non-instant record lowers");
    assert!(
        noninstant.contains("concurrent_join_timeout(cx.clone(), 0"),
        "zero-timeout single non-instant call must use the frozen-clock timeout executor:\n{noninstant}"
    );
}

#[test]
fn lowers_return_in_instant_concurrent_timeout_else_paths() {
    let zero_single = emit_unit(&unit_of(
        "function main() {\n  concurrent(timeout: 0ms) {\n    x: {\n      let xs = [5]\n      xs[1]\n    }\n  } else {\n    return 88\n  }\n  return 99\n}\nmain()",
    ))
    .expect("zero-timeout single else return lowers inline");
    let return_88 = zero_single
        .find("return Ok(Value::Int(88));")
        .expect("single-instant else return should emit");
    let return_99 = zero_single
        .find("return Ok(Value::Int(99));")
        .expect("post-concurrent return should emit");
    assert!(
        zero_single.contains("match (async { Ok::<Value, RtError>")
            && !zero_single.contains("concurrent_join_timeout(cx.clone()")
            && return_88 < return_99,
        "zero-timeout single-instant else return must be inline, not a timeout thunk:\n{zero_single}"
    );

    let zero_multi = emit_unit(&unit_of(
        "function main() {\n  let r = concurrent(timeout: 0ms) {\n    x: 1\n    y: 2\n  } else {\n    return 99\n  }\n  r.x\n}\nmain()",
    ))
    .expect("zero-timeout multi else return lowers");
    assert!(
        zero_multi.contains("return Ok(Value::Int(99));")
            && zero_multi.contains(&format!("let {}: Value =", mangle("r")))
            && !zero_multi.contains("concurrent_join_timeout(cx.clone()"),
        "zero-timeout multi-instant else return should inline the enclosing return:\n{zero_multi}"
    );

    let nonzero = emit_unit(&unit_of(
        "function main() {\n  let r = concurrent(timeout: 1m) {\n    x: 21 * 2\n  } else {\n    return 77\n  }\n  r.x\n}\nmain()",
    ))
    .expect("non-zero instant else return dead path lowers");
    assert!(
        nonzero.contains("concurrent_join_timeout(cx.clone()")
            && nonzero.contains("return Ok(Value::Int(77));"),
        "non-zero instant arms may carry a dead else-return thunk:\n{nonzero}"
    );
}

#[test]
fn refuses_top_level_concurrent_else_return() {
    let err = emit_unit(&unit_of(
        "let r = concurrent(timeout: 0ms) {\n  x: 1\n  y: 2\n} else {\n  return 1\n}\nr.x",
    ))
    .unwrap_err();
    assert_eq!(err, EmitError::unsupported("return outside a function"));
}

#[test]
fn refuses_unchecked_top_level_loop_escapes() {
    assert_eq!(
        emit_unit(&unit_of("loop {\n    return 1\n}")),
        Err(EmitError::unsupported("return outside a function"))
    );
    assert_eq!(
        emit_unit(&unit_of(
            "function fail() -> Result<int, string> {\n    return Err(\"x\")\n}\nloop {\n    break fail()?\n}"
        )),
        Err(EmitError::unsupported("return outside a function"))
    );
}

#[test]
fn lowers_input_to_the_shared_host_leaf() {
    // §22 `input()` lowers to the shared `builtin_input` leaf over the run
    // context's host — the same effect boundary the interpreter calls, so the
    // host payload reaches both engines identically.
    let src = emit_unit(&unit_of("input()")).expect("input() lowers");
    assert!(src.contains("builtin_input(&*cx.host())"), "got:\n{src}");
}

#[test]
fn refuses_a_concurrent_timeout_duration_that_overflows() {
    // Parses as `u64` but cannot be represented in the §15 millisecond unit.
    let err = emit_unit(&unit_of(
        "concurrent(timeout: 99999999999999999m) {\n  a: 1\n} else {\n  0\n}",
    ))
    .unwrap_err();
    assert_eq!(
        err,
        EmitError::unsupported("concurrent timeout duration overflows u64 milliseconds")
    );
}

#[test]
fn lowers_entry_module_value_exports() {
    // §17 a top-level `export function`/`export const`/`export let` in the ENTRY module
    // is unwrapped to its inner decl and lowers exactly as the unexported form (the
    // export is inert — nothing imports the entry). A forward-referencing exported
    // function still resolves via the top-fn cell seeding.
    let src = emit_unit(&unit_of(
        "export const K = 2\nexport function f() -> int { g() }\nexport function g() -> int { K }\nexport let v = f()\nv",
    ))
    .expect("entry value exports lower");
    assert!(src.contains("Value::Int(2)"), "got:\n{src}");
}

#[test]
fn refuses_an_exported_prelude_named_top_function() {
    // Unwrapping `export function` must NOT bypass the prelude-shadow refusal: an
    // exported top-level function named like a prelude value is refused, exactly as
    // its unexported form is.
    let err = emit_unit(&unit_of(
        "export function print(s: string) -> int { 0 }\nprint(\"x\")",
    ))
    .unwrap_err();
    assert_eq!(
        err,
        EmitError::unsupported("top-level function shadows a prelude name")
    );
}
