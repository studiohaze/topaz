//! §6.4 (v5.4) collection COMPREHENSION check witnesses (Slice D):
//!
//!   1. an array/set/map comprehension types as `Array<T>` / `Set<T>` /
//!      `Map<K, V>`, the element/key/value type inferred from the body under the
//!      `for`-clause bindings; `if`-clause conditions must be `bool`.
//!   2. an EMPTY/unconstrained comprehension demands a contextual type (TPZ5612) —
//!      an annotation resolves it, a bare binding does not (the empty-`[]` rule).
//!   3. the `for`-clause loop variables are visible in later clauses and the body
//!      (scoping; the unbound-after-comprehension case is a resolver concern); a body
//!      variable named `acc` is an ordinary reference (the accumulator is engine-side,
//!      hygiene).
//!   4. a NEWTYPE set element / map key follows base keyability, consistent with
//!      literals and `Set.of`.

use topaz_check::check_program_with_version;
use topaz_diag::FileId;
use topaz_parser::{ParseOptions, parse_with_options};
use topaz_syntax::LangVersion;

fn check(src: &str) -> Vec<String> {
    let out = parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: LangVersion::V5_4,
        },
    );
    assert!(out.diagnostics.is_empty(), "test source must parse: {src}");
    check_program_with_version(src, &out.program, LangVersion::V5_4)
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

// ---- well-typed comprehensions -----------------------------------------

#[test]
fn array_comprehension_types_as_array() {
    assert_clean("let xs = [ for x in [1, 2, 3] => x * x ]\nprint(\"{xs.length}\")");
}

#[test]
fn filtered_array_comprehension_types() {
    assert_clean("let xs = [ for x in [1, 2, 3] if x > 1 => x ]\nprint(\"{xs.length}\")");
}

#[test]
fn nested_array_comprehension_types() {
    assert_clean("let xs = [ for x in [1, 2] for y in [3, 4] => x * y ]\nprint(\"{xs.length}\")");
}

#[test]
fn set_comprehension_types_as_set() {
    assert_clean("let s = set { for x in [1, 1, 2] => x }\nprint(\"{s.length}\")");
}

#[test]
fn map_comprehension_types_as_map() {
    assert_clean("let m = map { for x in [1, 2] => x: x * 10 }\nprint(\"{m.length}\")");
}

#[test]
fn comprehension_element_type_flows_to_use() {
    // The element type is `int`, so the `Some(n)` arm binds `n: int` and `+ 1` is
    // well-typed (a string body would make the arm/`+` ill-typed). This pins that the
    // body type is inferred into the resulting `Array<int>`.
    assert_clean(
        "let xs = [ for x in [1, 2, 3] => x * 2 ]\nlet n = match xs.get(0) { case Some(v) => v\ncase None => 0 } + 1\nprint(\"{n}\")",
    );
}

#[test]
fn body_may_reference_an_outer_binding_named_acc() {
    // HYGIENE: `acc` is an ordinary outer binding — the accumulator is engine-side,
    // never a Topaz binding, so the body's `acc` reference resolves to the `let`.
    assert_clean("let acc = 100\nlet xs = [ for x in [1, 2] => x + acc ]\nprint(\"{xs.length}\")");
}

#[test]
fn loop_variable_is_visible_in_a_later_clause() {
    // A `for`-clause binding is in scope for a LATER clause's iterable/condition.
    assert_clean("let xs = [ for x in [1, 2] for y in [x, x + 1] => y ]\nprint(\"{xs.length}\")");
}

#[test]
fn nominal_record_for_pattern_binds_in_body() {
    assert_clean(
        "record Point { x: int, y: int }\nlet pts = [Point { x: 1, y: 2 }, Point { x: 3, y: 4 }]\nlet sums = [ for Point { x, y } in pts => x + y ]\nprint(\"{sums.length}\")",
    );
}

#[test]
fn range_for_pattern_is_accepted() {
    assert_clean("let xs = [ for 1..3 in [1, 2, 3] => 1 ]\nprint(\"{xs.length}\")");
}

#[test]
fn or_for_pattern_is_accepted() {
    assert_clean("let xs = [ for 1 | 2 in [1, 2] => 1 ]\nprint(\"{xs.length}\")");
}

#[test]
fn lambda_in_body_captures_comprehension_loop_variable() {
    assert_clean(
        "let x = 99\nlet fs = [ for x in [1, 2, 3] => (() => x) ]\nlet ys = [fs[0](), fs[1](), fs[2]()] \nprint(\"{ys.length}\")",
    );
}

// ---- empty / unconstrained needs a contextual type ----------------------

#[test]
fn empty_array_comprehension_needs_a_type() {
    assert_code(
        "let xs = [ for x in [] => x ]\nprint(\"{xs.length}\")",
        "TPZ5612",
    );
}

#[test]
fn empty_array_comprehension_annotated_is_clean() {
    assert_clean("let xs: Array<int> = [ for x in [] => x ]\nprint(\"{xs.length}\")");
}

#[test]
fn empty_map_comprehension_annotated_is_clean() {
    assert_clean("let m: Map<int, int> = map { for x in [] => x: x }\nprint(\"{m.length}\")");
}

// ---- diagnostics --------------------------------------------------------

#[test]
fn non_bool_filter_condition_is_a_type_error() {
    // An `if`-clause condition must be `bool` (the §5 condition rule).
    assert_code(
        "let xs = [ for x in [1, 2] if x => x ]\nprint(\"{xs.length}\")",
        "TPZ5001",
    );
}

#[test]
fn newtype_set_comprehension_element_is_clean_when_base_is_keyable() {
    // A keyable-base newtype is a valid Set key.
    assert_clean(
        "newtype UserId = int\nlet s = set { for n in [1, 2] => UserId(n) }\nprint(\"{s.length}\")",
    );
}

#[test]
fn newtype_set_comprehension_element_rejects_non_keyable_base() {
    assert_code(
        "newtype Bad = Map<string, int>\nlet s = set { for n in [1] => Bad(Map.new()) }\nprint(\"{s.length}\")",
        "TPZ5007",
    );
}

#[test]
fn comprehension_body_is_not_a_bare_loop_control_target() {
    assert_code(
        "let xs = [ for x in [1] => { break } ]\nprint(\"{xs.length}\")",
        "TPZ5001",
    );
    assert_code(
        "let xs = [ for x in [1] => { continue } ]\nprint(\"{xs.length}\")",
        "TPZ5001",
    );
}

#[test]
fn comprehension_filter_is_not_a_bare_loop_control_target() {
    assert_code(
        "let xs = [ for x in [1] if { continue\ntrue } => x ]\nprint(\"{xs.length}\")",
        "TPZ5001",
    );
}

#[test]
fn nested_statement_loop_inside_comprehension_keeps_its_own_target() {
    assert_clean(
        "let xs = [ for x in [1] => { for y in [1] { break }\nx } ]\nprint(\"{xs.length}\")",
    );
}
