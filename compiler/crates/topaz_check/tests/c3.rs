//! Phase C-3 witnesses: the §22 builtin signature table, call typing
//! with rank-1 instantiation, the §22.1 contextual-typing rule, and
//! the TPZ5004/TPZ5005/TPZ5020 graduations.

use topaz_check::check_program;
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

fn check_v54(src: &str) -> Vec<String> {
    let out = parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: LangVersion::V5_4,
        },
    );
    assert!(out.diagnostics.is_empty(), "test source must parse: {src}");
    topaz_check::check_program_with_version(src, &out.program, LangVersion::V5_4)
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

fn check_v55(src: &str) -> Vec<String> {
    let out = parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: LangVersion::V5_5,
        },
    );
    assert!(out.diagnostics.is_empty(), "test source must parse: {src}");
    topaz_check::check_program_with_version(src, &out.program, LangVersion::V5_5)
        .diagnostics
        .iter()
        .map(|d| format!("{} {}", d.code.as_str(), d.message))
        .collect()
}

fn assert_code_message_v55(src: &str, code: &str, message: &str) {
    let diags = check_v55(src);
    assert!(
        diags
            .iter()
            .any(|d| d.starts_with(code) && d.contains(message)),
        "expected {code} containing {message:?}, got: {diags:?}"
    );
}

// ---- TPZ5004 graduation: arity --------------------------------------

#[test]
fn builtin_and_user_arity_is_static_tpz5004() {
    assert_code("print(\"a\", \"b\")", "TPZ5004");
    assert_code("toInt()", "TPZ5004");
    assert_code(
        "function f(a: int, b: int) -> int {\n    return a + b\n}\nlet x: int = f(1)",
        "TPZ5004",
    );
    assert_clean("function f(a: int, b: int) -> int {\n    return a + b\n}\nlet x: int = f(1, 2)");
}

#[test]
fn default_parameters_relax_required_arity() {
    assert_clean(
        "function greet(name: string, greeting: string = \"Hi\") -> string {\n    return \"{greeting}, {name}\"\n}\nprint(greet(\"T\"))",
    );
}

// ---- TPZ5005 graduation: not callable ---------------------------------

#[test]
fn calling_a_non_function_is_tpz5005() {
    assert_code("let n = 1\nlet x = n(2)", "TPZ5005");
    assert_code("let xs: Array<int> = [1]\nlet n = xs.length()", "TPZ5005");
    assert_code(
        "let mode = RoundingMode.HalfEven\nlet x = mode()",
        "TPZ5005",
    );
}

// ---- builtin signatures ------------------------------------------------

#[test]
fn builtin_argument_types_check() {
    assert_code("print(42)", "TPZ5001");
    assert_clean("print(\"42\")");
    assert_code("toInt(7)", "TPZ5001");
    assert_clean("let n: Option<int> = toInt(\"7\")");
    assert_clean(
        "match BigInt.parse(\"9223372036854775808\", 10) { case Some(a) => { let b = BigInt.fromInt(2)\nlet c: BigInt = (a + b) * b\nlet s: string = c.toString(10)\nlet i: Option<int> = c.toInt()\nlet q: Result<BigInt, string> = c.div(b)\nlet r: Result<BigInt, string> = c.mod(b)\nlet ordered: bool = a < c\ns }\ncase None => \"none\" }",
    );
    assert_code(
        "let a = BigInt.fromInt(5)\nlet b = BigInt.fromInt(2)\nlet c = a / b",
        "TPZ5001",
    );
    assert_clean(
        "match Decimal.parse(\"12.3400\") { case Some(a) => { let b = Decimal.fromInt(2)\nlet c: Decimal = (a + b) * b\nlet s: string = c.toString()\nlet sc: int = c.scale()\nlet i: Option<int> = c.toInt()\nlet rounded: Decimal = c.round(1)\nlet down: Decimal = c.round(scale: 1, mode: RoundingMode.Down)\nlet q: Result<Decimal, string> = c.div(b, 2)\nlet ordered: bool = a < c\ns }\ncase None => \"none\" }",
    );
    assert_code(
        "let a = Decimal.fromInt(5)\nlet b = Decimal.fromInt(2)\nlet c = a / b",
        "TPZ5001",
    );
    assert_clean_v54(
        "match URL.parse(\"https://example.com/b\") { case Ok(b) => match URL.parse(\"https://example.com/a\") { case Ok(a) => { let ordered: bool = a < b\nlet mut m: Map<URL, int> = Map.new()\nm.insert(b, 7)\nlet got: int = m.getOr(b, 0)\nlet first = [b, a].sorted().get(0)\n\"ok\" }\ncase Err(e) => e }\ncase Err(e) => e }",
    );
}

#[test]
fn using_resource_block_binds_file_and_rejects_non_file_initializer() {
    assert_clean_v54(
        "function read(path: string) -> Result<string, string> {\n    using file = open(path)? {\n        return file.read()\n    }\n}",
    );
    assert_code_v54("using file = 1 {\n    file.close()\n}", "TPZ5001");
}

#[test]
fn receiver_members_type_from_the_receiver() {
    assert_clean(
        "let mut xs: Array<int> = [1, 2]\nxs.push(3)\nlet n: Option<int> = xs.get(0)\nlet len: int = xs.length",
    );
    assert_code("let mut xs: Array<int> = [1]\nxs.push(\"two\")", "TPZ5001");
    assert_clean("let xs = [1]\nlet n: Option<int> = xs.get(i: 0)");
    assert_code("let xs = [1]\nlet n = xs.get(k: 0)", "TPZ5004");
    assert_clean(
        "let mut m: Map<string, int> = Map.new()\nm.insert(\"k\", 1)\nlet v: Option<int> = m.get(\"k\")\nlet ks: Array<string> = m.keys",
    );
    assert_code(
        "let mut m: Map<string, int> = Map.new()\nm.insert(1, 1)",
        "TPZ5001",
    );
    // §6 (v5.4) `pop` is now a real array mutator, so an IMMUTABLE-root `pop` faults
    // the mut-root gate (TPZ5003), not "unknown member". A genuinely unknown member
    // still graduates to TPZ5006.
    assert_code("let xs: Array<int> = [1]\nlet y = xs.pop()", "TPZ5003");
    assert_code("let xs: Array<int> = [1]\nlet y = xs.nope()", "TPZ5006");
    // The v5.4 array mutators type-check on a `let mut` root and yield their result type.
    assert_clean(
        "let mut xs: Array<int> = [1, 2, 3]\nlet last: Option<int> = xs.pop()\nxs.sort()\nxs.sortBy((x: int) => 0 - x)\nxs.insert(0, 9)\nlet got: Option<int> = xs.removeAt(0)\nxs.reverse()\nxs.retain((x: int) => x > 0)\nxs.clear()",
    );
}

#[test]
fn file_surface_types() {
    assert_clean(
        "function read(path: string) -> Result<string, string> {\n    let file = open(path)?\n    defer file.close()\n    return file.read()\n}",
    );
}

// ---- rank-1 generics + lambdas from context ----------------------------

#[test]
fn map_filter_reduce_solve_from_the_iterable() {
    assert_clean("let xs: Array<int> = [1, 2, 3]\nlet ys: Array<int> = map(xs, (x: int) => x * x)");
    // The lambda parameter type flows from the iterable: no
    // annotation needed.
    assert_clean("let xs: Array<int> = [1, 2, 3]\nlet ys: Array<int> = map(xs, x => x * 2)");
    assert_clean(
        "let xs: Array<int> = [1, 2, 3]\nlet total: int = reduce(xs, 0, (acc: int, x: int) => acc + x)",
    );
    // A lambda body that misuses the contextual parameter type.
    assert_code(
        "let xs: Array<int> = [1, 2, 3]\nlet ys = map(xs, x => x + \"!\")",
        "TPZ5001",
    );
}

#[test]
fn shadowed_type_params_stay_distinct() {
    // The reviewer's counterexample: inner T must not collapse into
    // outer T.
    assert_code(
        "function outer<T>(x: T) -> () {\n    function inner<T>(y: T) -> T {\n        return x\n    }\n    let n: int = inner(1)\n    print(\"{n}\")\n}",
        "TPZ5001",
    );
    assert_clean(
        "function outer<T>(x: T) -> () {\n    function inner<T>(y: T) -> T {\n        return y\n    }\n    let n: int = inner(1)\n    print(\"{n}\")\n}",
    );
}

#[test]
fn local_aliases_see_enclosing_type_params() {
    // The reviewer's counterexample: a block-local alias inside a
    // generic function resolves the function's rigid T.
    assert_clean(
        "function f<T>(x: T) -> () {\n    type A = T\n    let y: A = x\n    print(\"{y}\")\n}",
    );
    assert_code(
        "function f<T>(x: T) -> () {\n    type A = T\n    let y: A = 1\n    print(\"{y}\")\n}",
        "TPZ5001",
    );
}

#[test]
fn local_alias_chains_see_enclosing_type_params() {
    // The reviewer's chain counterexample: A -> B -> T must resolve
    // T to the function's skolem even when B is validated after A.
    assert_clean(
        "function f<T>(x: T) -> () {
    type A = B
    type B = T
    let y: A = x
    print(\"{y}\")
}",
    );
    assert_code(
        "function f<T>(x: T) -> () {
    type A = B
    type B = T
    let y: A = 1
    print(\"{y}\")
}",
        "TPZ5001",
    );
}

#[test]
fn user_generics_instantiate_per_call() {
    assert_clean(
        "function first<T>(xs: Array<T>) -> Option<T> {\n    return xs.get(0)\n}\nlet x: Option<int> = first([1, 2])",
    );
    assert_code(
        "function first<T>(xs: Array<T>) -> Option<T> {\n    return xs.get(0)\n}\nlet x: Option<string> = first([1, 2])",
        "TPZ5001",
    );
}

// ---- §22.1 contextual rule (TPZ5020) ------------------------------------

#[test]
fn unsolved_without_context_is_tpz5020() {
    assert_code("let m = Map.new()", "TPZ5020");
    assert_code("let xs = []", "TPZ5020");
    assert_code("let n = None", "TPZ5020");
}

#[test]
fn context_solves_the_variables() {
    assert_clean("let m: Map<string, int> = Map.new()");
    assert_clean(
        "let entries: Array<{ key: string, value: int }> = [{ key: \"a\", value: 1 }, { key: \"a\", value: 9 }]\nlet m = Map.ofEntries(entries)\nlet got: int = m.getOr(\"a\", 0)",
    );
    assert_clean(
        "let entries: Array<{ key: string, value: int }> = []\nlet m = Map.ofEntries(entries: entries)\nlet got: int = m.length",
    );
    assert_clean("let xs: Array<int> = []");
    assert_clean("let n: Option<int> = None");
    assert_clean("let r: Result<int, string> = Ok(1)");
    assert_clean("let r: Result<int, string> = Err(\"boom\")");
    assert_clean("function f() -> Result<int, string> {\n    return Err(\"boom\")\n}");
}

#[test]
fn contextual_literals_are_preserved_through_calls() {
    // The declared return type reaches the record argument, so the
    // literal-union field accepts the literal (no premature widening).
    assert_clean(
        "type Kind = \"a\" | \"b\"\nfunction f() -> Result<int, { kind: Kind }> {\n    return Err({ kind: \"a\" })\n}",
    );
}

#[test]
fn review_fold_witnesses() {
    // Missing required slot cannot be satisfied by a named optional.
    assert_code("assert(message: \"boom\")", "TPZ5004");
    // Positional after named (SPEC §5).
    assert_code("assert(message: \"boom\", true)", "TPZ5004");
    // Context exists but cannot solve: §22.1 fires at the site.
    assert_code(
        "let r: Result<int, string> | Result<bool, string> = Err(1)",
        "TPZ5020",
    );
    // Contextual literals survive into bindings (no widening).
    assert_clean("let x: Option<\"open\"> = Some(\"open\")");
    // Direct Test.assertEq gates known non-comparable values at check time.
    assert_code(
        "let left: Map<string, int> = Map.new()\nlet right: Map<string, int> = Map.new()\nTest.assertEq(left, right)",
        "TPZ5007",
    );
    // Spread needs a variadic tail.
    assert_code("print(...[\"x\"])", "TPZ5004");
    assert_clean("let xs: Array<int> = Array.of(...[1, 2])");
    assert_code(
        "function id(x: int) -> int { x }\nlet entries = [{ key: id, value: 1 }]\nlet m = Map.ofEntries(entries)",
        "TPZ5007",
    );
    // Block-tail initializers still report §22.1.
    assert_code("let x = {\n    Map.new()\n}", "TPZ5020");
    // A known non-function in pipe-stage position.
    assert_code(
        "let notAFunction = 1\nlet y = 1 |> notAFunction(2)\nprint(\"{y}\")",
        "TPZ5005",
    );
}

#[test]
fn dynamic_index_function_value_order_faults_are_checker_rejected() {
    let prefix = r#"
function add(a: int = 0, b: int = 0, ...xs: int) -> int {
    a + b + xs.length
}
function addRequired(a: int, b: int, ...xs: int) -> int {
    a + b + xs.length
}
function pick(label: string) -> int {
    0
}
"#;
    let direct_positional_after_named = format!(
        "{prefix}
function main() -> int {{
    let arr = [add]
    arr[pick(\"k\")](a: 1, 2)
}}
"
    );
    let direct_named_before_spread = format!(
        "{prefix}
function main() -> int {{
    let arr = [add]
    arr[pick(\"k\")](a: 1, ...[2])
}}
"
    );
    let direct_spread_skips_required = format!(
        "{prefix}
function main() -> int {{
    let arr = [addRequired]
    arr[pick(\"k\")](...[1, 2])
}}
"
    );
    let pipe_positional_after_named = format!(
        "{prefix}
function main() -> int {{
    let arr = [add]
    1 |> arr[pick(\"k\")](a: _, 2)
}}
"
    );
    let pipe_named_before_spread = format!(
        "{prefix}
function main() -> int {{
    let arr = [add]
    1 |> arr[pick(\"k\")](a: _, ...[2])
}}
"
    );
    let pipe_spread_skips_required = format!(
        "{prefix}
function main() -> int {{
    let arr = [addRequired]
    1 |> arr[pick(\"k\")](...[2])
}}
"
    );

    for src in [direct_positional_after_named, pipe_positional_after_named] {
        assert_code_message_v55(
            &src,
            "TPZ5004",
            "positional arguments may not follow named arguments",
        );
    }
    for src in [direct_named_before_spread, pipe_named_before_spread] {
        assert_code_message_v55(
            &src,
            "TPZ5004",
            "named arguments must follow spread arguments",
        );
    }
    for src in [direct_spread_skips_required, pipe_spread_skips_required] {
        assert_code_message_v55(
            &src,
            "TPZ5004",
            "a spread argument cannot skip an unsatisfied fixed parameter",
        );
    }
}

// ---- staged silence (pipes, ambient, spread) -----------------------------

#[test]
fn pipeline_stages_are_not_standalone_calls() {
    assert_clean(
        "function double(xs: Array<int>) -> Array<int> {\n    return map(xs, (x: int) => x * 2)\n}\nlet ys = [1, 2] |> double\nprint(\"{ys}\")",
    );
    assert_clean("let ys = [1, 2, 3] |> map(_, (x: int) => x + 1)\nprint(\"{ys}\")");
    assert_clean(
        "let total = [1, 2] |> reduce(_, 0, (a: int, b: int) => a + b)\nprint(\"{total}\")",
    );
}

#[test]
fn ambient_calls_stay_silent() {
    assert_clean("let user = loadUser(42)\nprint(\"{user}\")");
}

#[test]
fn for_loops_type_their_element() {
    assert_code(
        "let xs: Array<int> = [1, 2]\nfor x in xs {\n    let s: string = x\n    print(s)\n}",
        "TPZ5001",
    );
    assert_clean(
        "let xs: Array<int> = [1, 2]\nfor x in xs {\n    let n: int = x\n    print(\"{n}\")\n}",
    );
}

// ---- C3: int/float/bool scalar receivers expose no members ----------

#[test]
fn int_float_bool_reject_unknown_member_access() {
    // A non-string scalar exposes no members, so an unknown member is a static
    // error — in both the widened (`Prim`) and literal forms — rather than
    // silently accepted and left to diverge from the interpreter's fault (C3).
    assert_code("let n = 5\nn.bogus", "TPZ5006");
    assert_code("(5).bogus", "TPZ5006");
    assert_code("let x = 1.5\nx.bogus", "TPZ5006");
    assert_code("let b = true\nb.bogus", "TPZ5006");
    assert_code("true.bogus", "TPZ5006");
}

#[test]
fn int_float_bool_reject_unknown_method_call() {
    // The member-CALL path rejects unknown scalar methods identically.
    assert_code("let n = 5\nn.bogus()", "TPZ5006");
    assert_code("(5).round()", "TPZ5006");
    assert_code("let x = 1.5\nx.floor()", "TPZ5006");
    assert_code("let b = true\nb.toggle()", "TPZ5006");
}

#[test]
fn string_method_calls_to_unknown_methods_reject() {
    // Methods outside the v5.4 string surface still reject statically (TPZ5006),
    // matching the interpreter. The accepted string-method matrix lives in c7.rs.
    assert_code(
        "let s = \"hello\"\nlet keep = s.toUpper()\nprint(\"{keep}\")",
        "TPZ5006",
    );
    assert_code(
        "let s = \"hello\"\nlet keep = s.replaceFirst(\"l\", \"L\")\nprint(\"{keep}\")",
        "TPZ5006",
    );
}
