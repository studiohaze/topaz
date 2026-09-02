//! Phase C-5 witnesses: statements and flow — mutability (TPZ5003),
//! same-scope redeclaration (TPZ5008), branch joins for if and
//! concurrent, §10 range typing and iteration, and §11
//! pipeline/composition typing.

use topaz_check::{check_program, check_program_with_version};
use topaz_diag::FileId;
use topaz_parser::{ParseOptions, parse_with_options};
use topaz_syntax::LangVersion;

fn check(src: &str) -> Vec<String> {
    let out = parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: LangVersion::V5_2,
        },
    );
    assert!(out.diagnostics.is_empty(), "test source must parse: {src}");
    check_program(src, &out.program)
        .diagnostics
        .iter()
        .map(|d| format!("{} {}", d.code.as_str(), d.message))
        .collect()
}

fn assert_clean(src: &str) {
    let diags = check(src);
    assert!(diags.is_empty(), "expected clean, got: {diags:?}");
}

fn assert_code(src: &str, code: &str) {
    let diags = check(src);
    assert!(
        diags.iter().any(|d| d.starts_with(code)),
        "expected {code}, got: {diags:?}"
    );
}

// ---- TPZ5003 graduation: mutability ---------------------------------------

#[test]
fn assigning_an_immutable_binding_is_tpz5003() {
    assert_code("let x = 1\nx = 2", "TPZ5003");
    assert_clean("let mut x = 1\nx = 2\nprint(\"{x}\")");
    assert_code(
        "let mut v: Option<int> = None\nlet w = v\nw ??= Some(1)",
        "TPZ5003",
    );
}

#[test]
fn loop_and_parameter_bindings_are_immutable() {
    assert_code(
        "let xs: Array<int> = [1, 2]\nfor x in xs {\n    x = 3\n}",
        "TPZ5003",
    );
    assert_code(
        "function f(a: int) -> int {\n    a = 2\n    return a\n}",
        "TPZ5003",
    );
}

#[test]
fn shadowing_decides_mutability_innermost_first() {
    assert_code("let mut x = 1\n{\n    let x = 2\n    x = 3\n}", "TPZ5003");
    assert_clean(
        "let x = 1\n{\n    let mut x = 2\n    x = 3\n    print(\"{x}\")\n}\nprint(\"{x}\")",
    );
}

#[test]
fn ambient_targets_stay_silent() {
    assert_clean("ambient = 1");
}

// ---- TPZ5008 graduation: same-scope redeclaration --------------------------

#[test]
fn same_scope_redeclaration_is_tpz5008() {
    assert_code("let x = 1\nlet x = 2\nprint(\"{x}\")", "TPZ5008");
    assert_code(
        "function f() -> int {\n    return 1\n}\nlet f = 2\nprint(\"{f}\")",
        "TPZ5008",
    );
    assert_code("const A = 1\nconst A = 2", "TPZ5008");
}

#[test]
fn nested_scopes_shadow_legally() {
    assert_clean("let x = 1\n{\n    let x = 2\n    print(\"{x}\")\n}\nprint(\"{x}\")");
}

// ---- §10 ranges -------------------------------------------------------------

#[test]
fn ranges_type_and_iterate_over_int() {
    assert_clean("for i in 1..5 {\n    let n: int = i\n    print(\"{n}\")\n}");
    assert_code(
        "for i in 1..5 {\n    let s: string = i\n    print(s)\n}",
        "TPZ5001",
    );
    assert_clean("for i in 10..1 by -2 {\n    print(\"{i}\")\n}");
}

#[test]
fn range_endpoints_and_step_are_int() {
    assert_code("let r = \"a\"..\"z\"\nprint(\"{r}\")", "TPZ5001");
    assert_code("let r = 1..10 by \"two\"\nprint(\"{r}\")", "TPZ5001");
}

#[test]
fn a_constant_zero_step_is_a_static_error() {
    assert_code("for i in 1..10 by 0 {\n    print(\"{i}\")\n}", "TPZ5001");
}

#[test]
fn range_membership_takes_int() {
    assert_clean("let hit: bool = 5 in 1..10");
    assert_code("let hit = \"5\" in 1..10\nprint(\"{hit}\")", "TPZ5001");
}

#[test]
fn ranges_feed_the_iterable_builtins() {
    assert_clean("let squares: Array<int> = map(1..4, (x: int) => x * x)");
}

// ---- branch joins: if ---------------------------------------------------------

#[test]
fn if_results_join_the_branch_types() {
    assert_clean("let b: bool = true\nlet n: int = if b { 1 } else { 0 }\nprint(\"{n}\")");
    assert_code(
        "let b: bool = true\nlet n: int = if b { 1 } else { \"zero\" }\nprint(\"{n}\")",
        "TPZ5001",
    );
}

#[test]
fn if_branches_are_context_sites() {
    // §22.1: the annotation reaches both branches.
    assert_clean(
        "let b: bool = true\nlet xs: Array<int> = if b { [] } else { Array.of(1) }\nprint(\"{xs}\")",
    );
}

#[test]
fn if_without_else_is_unit_valued() {
    assert_code(
        "let b: bool = true\nlet n: int = if b { 1 }\nprint(\"{n}\")",
        "TPZ5001",
    );
    // Statement position stays clean.
    assert_clean("let b: bool = true\nif b {\n    print(\"yes\")\n}");
}

#[test]
fn divergent_if_branches_drop_out_of_the_join() {
    assert_clean(
        "function f(b: bool) -> int {\n    let n: int = if b { 1 } else { return 0 }\n    return n\n}",
    );
}

#[test]
fn bare_if_bindings_widen_literal_joins() {
    assert_clean("let b: bool = true\nlet mut n = if b { 1 } else { 0 }\nn = 5\nprint(\"{n}\")");
}

#[test]
fn else_if_chains_propagate_the_context() {
    assert_clean(
        "let n: int = 2\nlet label: string = if n == 1 { \"one\" } else if n == 2 { \"two\" } else { \"many\" }\nprint(label)",
    );
}

// ---- branch joins: concurrent (§15) ---------------------------------------

#[test]
fn concurrent_results_are_records_of_the_arm_types() {
    assert_clean(
        "function f() -> int {\n    return 1\n}\nfunction g() -> string {\n    return \"x\"\n}\nlet r: { a: int, b: string } = concurrent {\n    a: f()\n    b: g()\n}\nprint(\"{r.a} {r.b}\")",
    );
    assert_code(
        "function f() -> int {\n    return 1\n}\nlet r: { a: string } = concurrent {\n    a: f()\n}\nprint(\"{r.a}\")",
        "TPZ5001",
    );
}

#[test]
fn duplicate_concurrent_arms_are_tpz5008() {
    assert_code(
        "function f() -> int {\n    return 1\n}\nlet r = concurrent {\n    a: f()\n    a: f()\n}\nprint(\"{r}\")",
        "TPZ5008",
    );
}

#[test]
fn concurrent_timeout_milliseconds_must_fit_u64() {
    assert_clean(
        "let r = concurrent(timeout: 307445734561825m) {\n    a: 1\n} else {\n    { a: 0 }\n}\nprint(\"{r.a}\")",
    );
    assert_code(
        "let r = concurrent(timeout: 307445734561826m) {\n    a: 1\n} else {\n    { a: 0 }\n}\nprint(\"{r.a}\")",
        "TPZ5001",
    );
}

#[test]
fn concurrent_else_joins_under_branch_compatibility() {
    assert_clean(
        "function f() -> int {\n    return 1\n}\nlet r = concurrent(timeout: 100ms) {\n    a: f()\n} else {\n    { a: 0 }\n}\nprint(\"{r.a}\")",
    );
    assert_code(
        "function f() -> int {\n    return 1\n}\nlet r = concurrent(timeout: 100ms) {\n    a: f()\n} else {\n    \"late\"\n}\nprint(\"{r}\")",
        "TPZ5001",
    );
}

// ---- §11 pipelines -----------------------------------------------------------

#[test]
fn pipe_stages_type_unary_application() {
    assert_clean(
        "function double(xs: Array<int>) -> Array<int> {\n    return map(xs, (x: int) => x * 2)\n}\nlet ys: Array<int> = [1, 2] |> double",
    );
    assert_code(
        "function shout(s: string) -> string {\n    return \"{s}!\"\n}\nlet y = [1, 2] |> shout\nprint(\"{y}\")",
        "TPZ5001",
    );
}

#[test]
fn pipe_inserts_the_value_as_first_argument() {
    assert_clean(
        "function add(a: int, b: int) -> int {\n    return a + b\n}\nlet n: int = 1 |> add(2)",
    );
    assert_code(
        "function add(a: int, b: int) -> int {\n    return a + b\n}\nlet n = 1 |> add(\"two\")\nprint(\"{n}\")",
        "TPZ5001",
    );
    assert_code(
        "function add(a: int, b: int) -> int {\n    return a + b\n}\nlet n = 1 |> add(2, 3)\nprint(\"{n}\")",
        "TPZ5004",
    );
}

#[test]
fn pipe_placeholders_take_the_piped_type() {
    assert_clean("let ys: Array<int> = [1, 2, 3] |> map(_, (x: int) => x + 1)");
    // The lambda parameter solves from the placeholder's type.
    assert_clean("let ys: Array<int> = [1, 2, 3] |> map(_, x => x * 2)");
    assert_clean("let total: int = [1, 2] |> reduce(_, 0, (a: int, b: int) => a + b)");
}

#[test]
fn pipe_field_sugar_is_member_access() {
    assert_clean("let n: string = { name: \"t\" } |> .name");
    assert_code(
        "let n = { name: \"t\" } |> .email\nprint(\"{n}\")",
        "TPZ5006",
    );
}

#[test]
fn non_callable_pipe_stages_are_static_errors() {
    assert_code("let y = 1 |> 2\nprint(\"{y}\")", "TPZ5005");
    assert_code(
        "function zero() -> int {\n    return 0\n}\nlet y = 1 |> zero\nprint(\"{y}\")",
        "TPZ5004",
    );
}

// ---- §11 composition ----------------------------------------------------------

#[test]
fn composition_chains_function_types() {
    assert_clean(
        "function inc(n: int) -> int {\n    return n + 1\n}\nfunction show(n: int) -> string {\n    return \"{n}\"\n}\nlet f = inc >> show\nlet s: string = f(1)\nprint(s)",
    );
    assert_code(
        "function inc(n: int) -> int {\n    return n + 1\n}\nfunction shout(s: string) -> string {\n    return \"{s}!\"\n}\nlet f = inc >> shout\nprint(\"{f(1)}\")",
        "TPZ5001",
    );
    assert_code("let f = 1 >> 2\nprint(\"{f}\")", "TPZ5005");
}

// ---- review fold (r1) -------------------------------------------------------

#[test]
fn declared_returns_reach_the_body_tail() {
    // The reviewer's counterexample: a literal-union return type is
    // the tail's context, so the if-join must not widen first.
    assert_clean(
        "type Mode = \"on\" | \"off\"\nfunction pick(b: bool) -> Mode {\n    if b { \"on\" } else { \"off\" }\n}",
    );
}

#[test]
fn unsolvable_bare_joins_are_tpz5020() {
    assert_code(
        "let b: bool = true\nlet xs = if b { [] } else { [] }\nprint(\"{xs}\")",
        "TPZ5020",
    );
}

#[test]
fn branch_joins_solve_partial_arms_against_each_other() {
    // §22.1 "match-arm expected type": Ok and Err arms mutually
    // complete the Result; Some solves None.
    assert_clean(
        "function divide(a: float, b: float) -> Result<float, string> {\n    return Ok(a / b)\n}\nlet result = match divide(10.0, 2.0) {\n    case Ok(value) => Ok(\"Result: {value * 2.0}\")\n    case Err(error) => Err(error)\n}\nprint(\"{result}\")",
    );
    assert_clean(
        "function divide(a: float, b: float) -> Result<float, string> {\n    return Ok(a / b)\n}\nlet opt = match divide(10.0, 0.0) {\n    case Ok(value) => Some(value)\n    case Err(_) => None\n}\nprint(\"{opt}\")",
    );
}

#[test]
fn nested_placeholders_suppress_insertion() {
    assert_clean(
        "function add(a: int, b: int) -> int {\n    return a + b\n}\nlet n: int = 1 |> add(_ + 1, 2)",
    );
    // After placeholder replacement only one argument remains.
    assert_code(
        "function add(a: int, b: int) -> int {\n    return a + b\n}\nlet n = 1 |> add(_ + 1)\nprint(\"{n}\")",
        "TPZ5004",
    );
}

#[test]
fn placeholder_search_covers_argument_expressions_but_not_nested_pipe_stages() {
    assert_clean(concat!(
        "function identity(value: int) -> int { value }\n",
        "function render(value: string) -> string { value }\n",
        "function add(left: int, right: int) -> int { left + right }\n",
        "let block: int = 5 |> identity({ _ })\n",
        "let branch: int = 6 |> identity(if true { _ } else { 0 })\n",
        "let text: string = 7 |> render(\"value {_}\")\n",
        "let nested: int = 10 |> add(100 |> add(_, 1))\n",
    ));
}

#[test]
fn placeholders_outside_pipelines_are_static_errors() {
    assert_code("let x: int = _", "TPZ5001");
}

#[test]
fn optional_call_pipe_stages_take_the_piped_value() {
    assert_clean("let xs: Option<Array<int>> = Some([1])\nlet n: Option<int> = 0 |> xs?.get()");
}

#[test]
fn concrete_non_iterables_are_static_errors() {
    assert_code("for x in 1 {\n    print(\"{x}\")\n}", "TPZ5001");
    assert_code("let ys = map(1, (x: int) => x)\nprint(\"{ys}\")", "TPZ5001");
}

#[test]
fn destructuring_redeclaration_is_tpz5008() {
    assert_code("let x = 1\nlet { x } = { x: 2 }\nprint(\"{x}\")", "TPZ5008");
}

// ---- S3 (v5.4) destructuring-`let` REFUTABILITY (TPZ5026) ------------------
// A `let` binds unconditionally, so a REFUTABLE pattern would pass `check` then
// fault at runtime ("`let` pattern did not match the value"). Such patterns are
// rejected statically (TPZ5026) with the advice to use `if let`; an IRREFUTABLE
// pattern (one that covers its scrutinee type) stays accepted. These rows need the
// v5.4 grammar (enums/records), so they parse + check at V5_4.

fn check_v54(src: &str) -> Vec<String> {
    check_version(src, LangVersion::V5_4)
}

fn check_version(src: &str, version: LangVersion) -> Vec<String> {
    let out = parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: version,
        },
    );
    assert!(out.diagnostics.is_empty(), "test source must parse: {src}");
    check_program_with_version(src, &out.program, version)
        .diagnostics
        .iter()
        .map(|d| format!("{} {}", d.code.as_str(), d.message))
        .collect()
}

fn assert_clean_v54(src: &str) {
    let diags = check_v54(src);
    assert!(diags.is_empty(), "expected clean, got: {diags:?}");
}

fn assert_code_v54(src: &str, code: &str) {
    let diags = check_v54(src);
    assert!(
        diags.iter().any(|d| d.starts_with(code)),
        "expected {code}, got: {diags:?}"
    );
}

#[test]
fn refutable_let_enum_variant_is_tpz5026() {
    // An enum with >1 variant: `let A(n) = e` would fault on a `B` value, so it is
    // refutable and rejected. (At runtime `let A(n) = E.B` faults TPZ5001.)
    assert_code_v54(
        "enum E { A(int), B }\nlet e = E.A(5)\nlet A(n) = e\nprint(\"{n}\")",
        "TPZ5026",
    );
}

#[test]
fn refutable_let_list_destructures_are_tpz5026_at_v54() {
    // A fixed-length list pattern is length-refutable: any array with a different
    // length would fault at runtime, so v5.4 requires `if let`.
    assert_code_v54(
        "let xs: Array<int> = [1]\nlet [a, b] = xs\nprint(\"{a}\")",
        "TPZ5026",
    );
    // A rest pattern with a required head is still refutable on an empty array.
    assert_code_v54(
        "let xs: Array<int> = []\nlet [a, ..rest] = xs\nprint(\"{rest.length}\")",
        "TPZ5026",
    );
}

#[test]
fn pure_rest_list_destructure_is_irrefutable_at_v54() {
    assert_clean_v54("let xs: Array<int> = []\nlet [..rest] = xs\nprint(\"{rest.length}\")");
    assert_clean_v54("let xs: Array<int> = [1, 2]\nlet [..] = xs\nprint(\"ok\")");
}

#[test]
fn list_destructuring_let_stays_clean_before_v54() {
    let src = "let xs: Array<int> = []\nlet [a, ..rest] = xs\nprint(\"{rest.length}\")";
    let diags = check_version(src, LangVersion::V5_1);
    assert!(diags.is_empty(), "expected v5.1 clean, got: {diags:?}");
}

#[test]
fn refutable_let_literal_and_range_are_tpz5026() {
    // A literal pattern only matches one value → refutable.
    assert_code_v54("let 5 = 5\nprint(\"ok\")", "TPZ5026");
    // A range pattern only matches a sub-interval → refutable.
    assert_code_v54("let 0..10 = 5\nprint(\"ok\")", "TPZ5026");
}

#[test]
fn refutable_let_nominal_record_with_refutable_field_is_tpz5026() {
    // A nominal record whose field subpattern is a literal is refutable even though
    // the record shape is fixed (the `x: 5` constraint can fail).
    assert_code_v54(
        "record P { x: int, y: int }\nlet p = P { x: 1, y: 2 }\nlet P { x: 5, y } = p\nprint(\"{y}\")",
        "TPZ5026",
    );
}

#[test]
fn irrefutable_let_destructures_stay_clean() {
    // A SINGLE-variant enum covers its whole type → irrefutable.
    assert_clean_v54("enum Wrap { W(int) }\nlet w = Wrap.W(5)\nlet W(n) = w\nprint(\"{n}\")");
    // A NOMINAL record with all-irrefutable fields → irrefutable.
    assert_clean_v54(
        "record P { x: int, y: int }\nlet p = P { x: 1, y: 2 }\nlet P { x, y } = p\nprint(\"{x} {y}\")",
    );
    // A STRUCTURAL record with all-irrefutable fields → irrefutable.
    assert_clean_v54("let { x, y } = { x: 1, y: 2 }\nprint(\"{x} {y}\")");
    // A bare binding is always irrefutable.
    assert_clean_v54("let x = 5\nprint(\"{x}\")");
}

#[test]
fn signed_constant_zero_steps_are_static_errors() {
    assert_code("for i in 1..10 by -0 {\n    print(\"{i}\")\n}", "TPZ5001");
}

// ---- review fold (r2) -------------------------------------------------------

#[test]
fn nested_partials_complete_across_branches() {
    // The r2 counterexample: `Some([])` keeps a partial through the
    // join, the sibling arm solves it, and the wrong annotation is
    // rejected downstream.
    assert_clean(
        "let b: bool = true\nlet xs = if b { Some([]) } else { Some([1]) }\nlet good: Option<Array<int>> = xs",
    );
    assert_code(
        "let b: bool = true\nlet xs = if b { Some([]) } else { Some([1]) }\nlet bad: Option<Array<string>> = xs",
        "TPZ5001",
    );
}

#[test]
fn calling_an_optional_property_is_tpz5005() {
    assert_code(
        "let xs: Option<Array<int>> = Some([1])\nlet n = xs?.length()\nprint(\"{n}\")",
        "TPZ5005",
    );
    assert_code(
        "let xs: Option<Array<int>> = Some([1])\nlet n = 0 |> xs?.length()\nprint(\"{n}\")",
        "TPZ5005",
    );
    // Plain property access stays fine.
    assert_clean("let xs: Option<Array<int>> = Some([1])\nlet n: Option<int> = xs?.length");
}

// ---- review fold (r3) -------------------------------------------------------

#[test]
fn incompatible_partial_siblings_do_not_erase_shapes() {
    // The r3 counterexample: `Some([])` must keep its array shape,
    // so an int sibling cannot silently solve it away.
    let diags = check(
        "let b: bool = true\nlet xs = if b { Some([]) } else { Some(1) }\nlet bad: Option<int> = xs",
    );
    assert!(!diags.is_empty(), "expected a diagnostic, got clean");
}

// ---- review fold (r4) -------------------------------------------------------

#[test]
fn chained_partial_bindings_solve_to_a_fixed_point() {
    // The r4 counterexample: nested partials chain through the join
    // substitution (`v0 := Array<v1>`, `v1 := Array<v2>`, …) and
    // must solve transitively.
    assert_clean(
        "let n: int = 0\nlet xs = match n {\n    case 0 => Some([])\n    case 1 => Some([[]])\n    case 2 => Some([[[]]])\n    case _ => Some([[[1]]])\n}\nlet good: Option<Array<Array<Array<int>>>> = xs",
    );
    assert_code(
        "let n: int = 0\nlet xs = match n {\n    case 0 => Some([])\n    case 1 => Some([[]])\n    case 2 => Some([[[1]]])\n    case _ => Some([[[1]]])\n}\nlet bad: Option<Array<int>> = xs",
        "TPZ5001",
    );
}

// ---- C-8 audit fold ---------------------------------------------------------

#[test]
fn spec_5_argument_ordering() {
    // §5: named arguments FOLLOW all positional and spread
    // arguments — named-after-spread is the required order.
    assert_clean(
        "function f(tag: string = \"x\", ...xs: int) -> int {\n    return xs.length\n}\nlet n: int = f(...[1, 2], tag: \"t\")",
    );
    // A named argument preceding a spread is a static error.
    assert_code(
        "function f(tag: string = \"x\", ...xs: int) -> int {\n    return xs.length\n}\nlet n = f(tag: \"t\", ...[1, 2])\nprint(\"{n}\")",
        "TPZ5004",
    );
    // A spread cannot skip an unsatisfied (required) fixed
    // parameter — not even with a later named argument.
    assert_code(
        "function g(a: int, ...xs: int) -> int {\n    return a\n}\nlet n = g(...[1, 2], a: 0)\nprint(\"{n}\")",
        "TPZ5004",
    );
}

#[test]
fn spread_arguments_type_their_elements() {
    assert_clean(
        "function sum(...xs: int) -> int {\n    return reduce(xs, 0, (a: int, b: int) => a + b)\n}\nlet n: int = sum(...Array.of(1, 2), 3)",
    );
    assert_code(
        "function sum(...xs: int) -> int {\n    return reduce(xs, 0, (a: int, b: int) => a + b)\n}\nlet n = sum(...Array.of(\"a\"))\nprint(\"{n}\")",
        "TPZ5001",
    );
}

#[test]
fn named_arguments_type_user_functions() {
    assert_clean(
        "function greet(name: string, suffix: string = \"!\") -> string {\n    return \"{name}{suffix}\"\n}\nlet s: string = greet(\"T\", suffix: \"?\")",
    );
    assert_code(
        "function greet(name: string, suffix: string = \"!\") -> string {\n    return \"{name}{suffix}\"\n}\nlet s = greet(\"T\", suffix: 1)\nprint(s)",
        "TPZ5001",
    );
    assert_code(
        "function greet(name: string) -> string {\n    return name\n}\nlet s = greet(nope: \"T\")\nprint(s)",
        "TPZ5004",
    );
}

// ---- member/index assignment targets (§4/§5/§9) ---------------------------

#[test]
fn record_member_assignment_requires_a_mut_root() {
    // A pure record-member chain writes via functional update +
    // root rebind, so the root binding must be `let mut`.
    assert_clean("let mut r = { a: 1 }\nr.a = 2");
    assert_code("let r = { a: 1 }\nr.a = 2", "TPZ5003");
    assert_clean("let mut r = { a: 1, b: { c: 2 } }\nr.b.c += 1");
    assert_code("let r = { a: 1, b: { c: 2 } }\nr.b.c += 1", "TPZ5003");
}

#[test]
fn array_cell_assignment_requires_mut() {
    // §9: in-place collection mutation — index assignment included —
    // requires a mutable root binding.
    assert_code("let xs = [1, 2, 3]\nxs[1] = 20", "TPZ5003");
    assert_code("let xs = [1, 2, 3]\nxs[2] += 5", "TPZ5003");
    assert_clean("let mut xs = [1, 2, 3]\nxs[1] = 20");
    assert_clean("let mut xs = [1, 2, 3]\nxs[2] += 5");
    // A record path THROUGH a cell roots at the same binding.
    assert_code("let rs = [{ n: 1 }]\nrs[0].n = 9", "TPZ5003");
    assert_clean("let mut rs = [{ n: 1 }]\nrs[0].n = 9");
}

#[test]
fn member_assignment_types_the_field() {
    assert_code("let mut r = { a: 1 }\nr.a = \"s\"", "TPZ5001");
    assert_code("let mut r = { a: 1 }\nr.nope = 2", "TPZ5006");
    assert_code("let xs = [1, 2]\nxs[0] = \"s\"", "TPZ5001");
}

#[test]
fn compound_assignment_types_the_operation() {
    assert_clean("let mut s = \"a\"\ns += \"b\"");
    assert_code("let mut n = 1\nn += \"s\"", "TPZ5001");
    assert_code("let mut r = { a: 1 }\nr.a -= \"s\"", "TPZ5001");
}

#[test]
fn map_slots_are_not_index_assignable() {
    assert_code("let m: Map<string, int> = Map()\nm[\"k\"] = 1", "TPZ5001");
}

#[test]
fn spread_cannot_skip_a_non_leading_required_parameter() {
    // §5: "required" is per slot, not a leading prefix — at the
    // spread, every positionally unfilled fixed slot needs a default.
    assert_code(
        "function f(a: int = 0, b: int, ...xs: int) -> int {\n    return a + b + xs.length\n}\nlet n = f(0, ...[5], b: 2)\nprint(\"{n}\")",
        "TPZ5004",
    );
}

#[test]
fn named_arguments_cannot_bind_a_variadic_parameter() {
    // The name table is authoritative even when it is empty: a
    // variadic-only function has no nameable parameters.
    assert_code(
        "function f(...xs: int) -> int {\n    return xs.length\n}\nlet n = f(...[1], xs: 2)\nprint(\"{n}\")",
        "TPZ5004",
    );
}

#[test]
fn map_index_assignment_is_caught_through_member_paths() {
    // The write anchors at the chain's LAST index segment; a member
    // suffix does not hide the Map slot.
    assert_code(
        "let m: Map<string, { x: int }> = Map.new()\nm[\"k\"].x = 1",
        "TPZ5001",
    );
}

#[test]
fn shadowing_bindings_hide_callable_metadata() {
    // §4: a lambda shadowing a declared function must not inherit
    // the declaration's defaulted-parameter metadata.
    assert_code(
        "function f(a: int, b: int = 0) -> int {\n    return a + b\n}\nlet n: int = {\n    let f = (x: int, y: int) => x + y\n    f(1)\n}\nprint(\"{n}\")",
        "TPZ5004",
    );
    // The outer declaration's metadata survives past the block.
    assert_clean(
        "function f(a: int, b: int = 0) -> int {\n    return a + b\n}\nlet m: int = {\n    let f = (x: int, y: int) => x + y\n    f(1, 2)\n}\nlet n: int = f(1)",
    );
}

#[test]
fn mutator_methods_require_a_mut_root() {
    // §9: in-place collection mutators need a mutable root binding.
    assert_code("let xs = [1, 2, 3]\nxs.push(4)", "TPZ5003");
    assert_clean("let mut xs = [1, 2, 3]\nxs.push(4)");
    assert_code(
        "let m: Map<string, int> = Map.new()\nm.insert(\"k\", 1)",
        "TPZ5003",
    );
    assert_clean("let mut m: Map<string, int> = Map.new()\nm.insert(\"k\", 1)");
    assert_code("let s = Set.of(1)\ns.add(2)", "TPZ5003");
    assert_clean("let mut s = Set.of(1)\ns.add(2)");
    // Mutability is per binding: an immutable alias of a shared
    // collection still cannot mutate through itself.
    assert_code("let mut s = Set.of(1)\nlet t = s\nt.remove(1)", "TPZ5003");
    // A record field that happens to be named like a mutator is not
    // a collection mutation and needs no mut.
    assert_clean("let r = { remove: (x: int) => x }\nlet n: int = r.remove(5)");
}

#[test]
fn byte_buffer_surface_and_exclusions_are_checked() {
    assert_clean(
        "let mut b = ByteBuffer.allocate(8)\nb.set(0, 255)\nb.fill(1, 3, 7)\nb.copy(b, 0, 4, 4)\nlet n: int = b.get(0)\nlet out: Bytes = b.toBytes()\nlet mut c = ByteBuffer.fromBytes(out)",
    );
    assert_code("let b = ByteBuffer.allocate(1)\nb.set(0, 1)", "TPZ5003");
    assert_code(
        "let mut b = ByteBuffer.allocate(1)\nlet alias = b\nalias.fill(0, 1, 2)",
        "TPZ5003",
    );
    assert_code(
        "let a = ByteBuffer.allocate(1)\nlet b = ByteBuffer.allocate(1)\nlet same = a == b",
        "TPZ5007",
    );
    assert_code(
        "let a = ByteBuffer.allocate(1)\nlet b = ByteBuffer.allocate(1)\nlet ordered = a < b",
        "TPZ5007",
    );
    assert_code(
        "let b = ByteBuffer.allocate(1)\nlet m: Map<ByteBuffer, int> = Map.new()",
        "TPZ5007",
    );
    assert_code(
        "let b = ByteBuffer.allocate(1)\nlet s: Set<ByteBuffer> = Set.of(b)",
        "TPZ5007",
    );
    assert_code(
        "let b = ByteBuffer.allocate(1)\nlet text = JSON.stringify(b)",
        "TPZ5533",
    );
    assert_code(
        "let b = ByteBuffer.allocate(1)\nlet text = \"{b}\"",
        "TPZ5001",
    );
    assert_code(
        "let b = ByteBuffer.allocate(1)\nlet query = sql\"select {b}\"",
        "TPZ5001",
    );
    assert_code(
        "let p = { data: ByteBuffer.allocate(1) }\nlet text = \"{p}\"",
        "TPZ5001",
    );
}

#[test]
fn mutator_root_check_sees_through_all_forms() {
    // §9 regression folds: parens, first-class method values, optional
    // access, and record-stored handles all key off the true root.
    assert_code("let xs = [1]\n(xs).push(2)", "TPZ5003");
    assert_code("let ys = [1, 2]\n(ys)[0] = 9", "TPZ5003");
    assert_clean("let mut ys = [1, 2]\n(ys)[0] = 9");
    // A mutator taken as a first-class value is checked at the
    // point of acquisition.
    assert_code("let xs = [1]\nlet f = xs.push", "TPZ5003");
    assert_clean("let mut xs = [1]\nlet f = xs.push\nlet g = f");
    // Optional access is value-dependent (`?.` short-circuits on
    // None), so the checker does NOT statically reject a mutator
    // through an immutable optional root — the runtime enforces §9
    // on the Some branch instead.
    assert_clean("let xs: Option<Array<int>> = Some([1])\nxs?.push(2)");
    // A record-stored handle is checked at the collection, not the
    // record field.
    assert_code("let xs = [1]\nlet r = { push: xs.push }", "TPZ5003");
}

#[test]
fn pipe_field_mutator_requires_a_mut_root() {
    // §9: `coll |> .push` obtains a mutator handle — same rule.
    assert_code("let xs = [1]\nlet f = xs |> .push", "TPZ5003");
    assert_clean("let mut xs = [1]\nlet f = xs |> .push\nlet g = f");
}

#[test]
fn assignment_through_optional_access_is_rejected() {
    // §4: `?.` is conditional and not an assignable target. An
    // index target whose object routes through `?.` parses (the
    // outer target is an index access) but must be rejected.
    assert_code(
        "let mut r: Option<{ xs: Array<int> }> = Some({ xs: [1] })\nr?.xs[0] = 1",
        "TPZ5001",
    );
}

#[test]
fn pipe_named_argument_into_the_lead_slot_is_rejected() {
    // §11: the piped value already supplies slot 0; naming it too is
    // a duplicate (matches the runtime's GUARD_ARITY).
    assert_code(
        "function id(a: int) -> int {\n    return a\n}\nlet n = 1 |> id(a: 2)\nprint(\"{n}\")",
        "TPZ5004",
    );
    // The placeholder form binds slot 0 to `_`, leaving the named
    // argument free — clean.
    assert_clean(
        "function add(a: int, b: int) -> int {\n    return a + b\n}\nlet n: int = 1 |> add(_, b: 2)",
    );
}

#[test]
fn placeholder_as_pipe_callee_is_a_static_error() {
    // §11: a placeholder is valid only in the argument list.
    assert_code(
        "function echo(s: int) -> int {\n    return s\n}\nlet n = echo |> _(1)\nprint(\"{n}\")",
        "TPZ5001",
    );
}

#[test]
fn expression_for_is_not_a_bare_loop_control_target() {
    assert_code_v54(
        "let xs = for x in [1] { break }\nprint(\"done\")",
        "TPZ5001",
    );
    assert_code_v54(
        "let xs = for x in [1] { continue }\nprint(\"done\")",
        "TPZ5001",
    );
}

#[test]
fn expression_for_collects_a_widened_array_type() {
    assert!(
        check_version(
            "let doubled: Array<int> = for x in [1, 2, 3] { x * 2 }",
            LangVersion::V5_1,
        )
        .is_empty(),
        "the locked v5.1 value-collecting example must type as Array<int>",
    );
    assert_clean_v54(
        "let doubled: Array<int> = for x in [1, 2, 3] { x * 2 }\nlet first: int = doubled[0]\nprint(\"{first}\")",
    );
    assert_clean_v54(
        "let units: Array<()> = for x in [1, 2] { print(\"{x}\") }\nprint(\"{units.length}\")",
    );
    assert_code_v54(
        "let wrong: () = for x in [1, 2] { x }\nprint(\"{wrong}\")",
        "TPZ5001",
    );
}

#[test]
fn statement_for_and_loop_control_stay_clean() {
    assert_clean_v54(
        "let mut s = 0\nfor x in [1, 2, 3] { if x == 2 { break }\ns = s + x }\nprint(\"{s}\")",
    );
    assert_clean_v54("let x = loop { break 5 }\nprint(\"{x}\")");
}

#[test]
fn if_let_result_scope_and_else_chain_follow_match_rules() {
    assert_clean_v54(
        "let a: Option<int> = None\nlet b: Option<int> = Some(2)\nlet out: int = if let Some(n) = a { n } else if let Some(n) = b { n } else { 0 }\nprint(\"{out}\")",
    );
    assert_code_v54(
        "let value: Option<int> = None\nlet out: int = if let Some(n) = value { n }\nprint(\"{out}\")",
        "TPZ5001",
    );
    assert_clean_v54(
        "let n = 9\nlet value: Option<int> = Some(1)\nlet inner = if let Some(n) = value { n } else { 0 }\nlet outer: int = n\nprint(\"{inner}:{outer}\")",
    );
}

#[test]
fn while_let_bindings_are_immutable_and_iteration_scoped() {
    assert_code_v54(
        "let mut value: Option<int> = Some(1)\nwhile let Some(n) = value { n = 2\nvalue = None }",
        "TPZ5003",
    );
    assert_clean_v54(
        "let n = 9\nlet mut value: Option<int> = Some(1)\nwhile let Some(n) = value { print(\"{n}\")\nvalue = None }\nlet outer: int = n\nprint(\"{outer}\")",
    );
}

#[test]
fn while_let_scrutinee_control_targets_the_desugared_loop() {
    assert_clean_v54(
        "let mut attempts = 0\nlet mut total = 0\nwhile let Some(n) = {\n  attempts = attempts + 1\n  if attempts == 1 { continue }\n  if attempts == 3 { break }\n  Some(attempts)\n} {\n  total = total + n\n}\nprint(\"{attempts}:{total}\")",
    );
}

#[test]
fn labeled_break_can_cross_expression_for_to_outer_loop() {
    assert_clean_v54(
        "let r = loop 'outer {\n  let xs = for x in [1] {\n    if x == 1 { break 'outer 9 }\n    0\n  }\n  break 0\n}\nprint(\"{r}\")",
    );
}
