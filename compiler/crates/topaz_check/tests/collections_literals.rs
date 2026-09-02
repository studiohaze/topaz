//! §6 (v5.4) set/map LITERAL check witnesses (Slice C):
//!
//!   1. a non-empty `set { … }` / `map { … }` types as `Set<T>` / `Map<K, V>`,
//!      its element/key/value types inferred and unified.
//!   2. an EMPTY `set {}` / `map {}` demands a contextual type (TPZ5020) — an
//!      annotation resolves it, a bare binding does not (the empty-`[]` rule).
//!   3. a statically-obvious DUPLICATE constant key in a map literal is a CHECK
//!      error (TPZ5602); a runtime-valued duplicate is left to the TPZ4601 fault.
//!   4. every statically-known key type rejected by runtime `freeze` is rejected
//!      at CHECK time too (TPZ5007): nominal enum/record/newtype keyability follows
//!      payload/field/base keyability, including nested union/structural keys.

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

// ---- well-typed literals -----------------------------------------------

#[test]
fn non_empty_set_literal_types_as_set() {
    assert_clean("let s: Set<int> = set { 1, 2, 3 }\nprint(\"{s.length}\")");
    assert_clean("let s = set { \"a\", \"b\" }\nprint(\"{s.length}\")");
}

#[test]
fn non_empty_map_literal_types_as_map() {
    assert_clean("let m: Map<string, int> = map { \"a\": 1, \"b\": 2 }\nprint(\"{m.length}\")");
    assert_clean("let m = map { 1: \"one\", 2: \"two\" }\nprint(\"{m.length}\")");
}

#[test]
fn set_literal_element_type_must_unify_with_annotation() {
    // A `string` element against a `Set<int>` annotation is a mismatch.
    assert_code("let s: Set<int> = set { 1, \"two\" }", "TPZ5001");
}

#[test]
fn map_literal_value_type_must_unify_with_annotation() {
    assert_code(
        "let m: Map<string, int> = map { \"a\": 1, \"b\": \"two\" }",
        "TPZ5001",
    );
}

// ---- empty literals need a contextual type -----------------------------

#[test]
fn empty_set_literal_with_annotation_is_clean() {
    assert_clean("let s: Set<int> = set {}\nprint(\"{s.length}\")");
}

#[test]
fn empty_map_literal_with_annotation_is_clean() {
    assert_clean("let m: Map<string, int> = map {}\nprint(\"{m.length}\")");
}

#[test]
fn bare_empty_set_literal_needs_a_type() {
    assert_code("let s = set {}", "TPZ5020");
}

#[test]
fn bare_empty_map_literal_needs_a_type() {
    assert_code("let m = map {}", "TPZ5020");
}

// ---- static duplicate constant keys (TPZ5602) --------------------------

#[test]
fn duplicate_constant_string_key_is_tpz5602() {
    assert_code("let m = map { \"a\": 1, \"a\": 2 }", "TPZ5602");
}

#[test]
fn duplicate_constant_int_key_is_tpz5602() {
    assert_code("let m = map { 1: \"x\", 1: \"y\" }", "TPZ5602");
}

#[test]
fn distinct_constant_keys_are_clean() {
    assert_clean("let m = map { \"a\": 1, \"b\": 2 }\nprint(\"{m.length}\")");
}

#[test]
fn runtime_valued_duplicate_keys_are_not_a_static_error() {
    // Two bindings that happen to be equal at runtime are NOT a static dup — that
    // is the TPZ4601 runtime fault's job, not TPZ5602. Check must pass here.
    assert_clean(
        "let k1 = \"a\"\nlet k2 = \"a\"\nlet m = map { k1: 1, k2: 2 }\nprint(\"{m.length}\")",
    );
}

// ---- keyability follows runtime freeze -------------------------------

#[test]
fn newtype_set_literal_element_is_clean_when_base_is_keyable() {
    assert_clean("newtype UserId = int\nlet s = set { UserId(1), UserId(2) }");
}

#[test]
fn newtype_map_literal_key_is_clean_when_base_is_keyable() {
    assert_clean("newtype UserId = int\nlet m = map { UserId(1): \"a\" }");
}

#[test]
fn newtype_over_non_keyable_base_literal_key_is_tpz5007() {
    assert_code(
        "newtype Bad = Map<string, int>\nlet m = map { Bad(Map.new()): \"a\" }",
        "TPZ5007",
    );
}

#[test]
fn enum_set_literal_element_is_clean_when_payloads_are_keyable() {
    assert_clean("enum Color { Red, Blue }\nlet s = set { Color.Red, Color.Blue }");
    assert_clean("enum Lookup { Hit(int), Miss }\nlet s = set { Lookup.Hit(1), Lookup.Miss }");
}

#[test]
fn enum_with_non_keyable_payload_literal_key_is_tpz5007() {
    assert_code(
        "enum Bad { Box(Map<string, int>) }\nlet b = Bad.Box(Map.new())\nlet m = map { b: \"bad\" }",
        "TPZ5007",
    );
}

#[test]
fn nominal_record_map_literal_key_is_clean_when_fields_are_keyable() {
    assert_clean("record User { id: int }\nlet u = User { id: 1 }\nlet m = map { u: \"Ada\" }");
}

#[test]
fn nominal_record_map_literal_key_rejects_non_keyable_field() {
    assert_code(
        "record Bad { m: Map<string, int> }\nlet b = Bad { m: Map.new() }\nlet m = map { b: \"bad\" }",
        "TPZ5007",
    );
}

#[test]
fn newtype_hidden_in_union_set_literal_element_is_clean_when_base_is_keyable() {
    assert_clean("newtype UserId = int\nlet s = set { UserId(1), 2 }");
}

#[test]
fn non_keyable_nested_in_structural_key_is_tpz5007() {
    assert_code(
        "enum Bad { Box(Map<string, int>) }\nlet s = set { { bad: Bad.Box(Map.new()) } }",
        "TPZ5007",
    );
}

#[test]
fn structural_keyable_literal_keys_stay_clean() {
    assert_clean("let s = set { [1, 2], [3, 4] }\nprint(\"{s.length}\")");
    assert_clean("let m = map { { x: 1 }: \"one\", { x: 2 }: \"two\" }\nprint(\"{m.length}\")");
}

#[test]
fn annotated_enum_and_record_keys_follow_payloads_and_fields() {
    assert_clean("enum Color { Red, Blue }\nlet s: Set<Color> = set {}\nprint(\"{s.length}\")");
    assert_code(
        "enum Bad { Box(Map<string, int>) }\nlet s: Set<Bad> = set {}",
        "TPZ5007",
    );
    assert_clean(
        "record User { id: int }\nlet m: Map<User, string> = map {}\nprint(\"{m.length}\")",
    );
    assert_code(
        "record Bad { m: Map<string, int> }\nlet m: Map<Bad, string> = map {}",
        "TPZ5007",
    );
}

#[test]
fn inferred_set_of_enum_key_is_clean_when_payloads_are_keyable() {
    assert_clean("enum Color { Red, Blue }\nlet s = Set.of(Color.Red)");
}
