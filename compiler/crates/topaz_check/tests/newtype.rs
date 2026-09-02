//! v5.4 newtype soundness witnesses:
//!
//!   1. `id.value()` is typed as the BASE (not `Unknown`), so a wrong
//!      result-type binding and a wrong arity are CHECK errors — they
//!      must not pass `check` then fault at run.
//!   2. a newtype is a valid Map/Set key iff its base is keyable. The key keeps
//!      nominal identity (`UserId(1)` is not `1`), while a newtype over `Map`
//!      remains a CHECK error (TPZ5007), consistent with runtime `freeze`.

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

// ---- MUST-FIX 1: `.value()` is typed as the base, with arity --------

#[test]
fn value_unwrap_is_typed_as_the_base() {
    // `id.value()` is `int` (the base), so a correct binding type checks.
    assert_clean(
        "newtype UserId = int\nlet id = UserId(1)\nlet n: int = id.value()\nprint(\"{n}\")",
    );
}

#[test]
fn value_unwrap_wrong_result_type_is_a_check_error() {
    // `id.value()` is `int`, NOT `string` — a CHECK type error (TPZ5001),
    // never a check-pass-then-runtime-fault.
    assert_code(
        "newtype UserId = int\nlet id = UserId(1)\nlet s: string = id.value()\nprint(s)",
        "TPZ5001",
    );
}

#[test]
fn value_unwrap_wrong_arity_is_a_check_error() {
    // `.value()` takes ZERO args; `id.value(2)` is a CHECK arity error
    // (TPZ5004), never a runtime TPZ5004.
    assert_code(
        "newtype UserId = int\nlet id = UserId(1)\nprint(\"{id.value(2)}\")",
        "TPZ5004",
    );
}

#[test]
fn unknown_member_is_a_check_error() {
    // The newtype's ONLY member is `.value()`; any other is NO_FIELD.
    assert_code(
        "newtype UserId = int\nlet id = UserId(1)\nprint(\"{id.other()}\")",
        "TPZ5006",
    );
}

// ---- MUST-FIX 2: newtype keyability follows its base ----------------

#[test]
fn inferred_set_of_newtype_key_is_clean_when_base_is_keyable() {
    assert_clean(
        "newtype UserId = int\nlet s = Set.of(UserId(1), UserId(1), UserId(2))\nprint(\"{s.length}\")",
    );
}

#[test]
fn annotated_set_newtype_key_is_clean_when_base_is_keyable() {
    assert_clean("newtype UserId = int\nlet s: Set<UserId> = Set.of(UserId(1))\nprint(\"ok\")");
}

#[test]
fn annotated_map_newtype_key_is_clean_when_base_is_keyable() {
    assert_clean(
        "newtype UserId = int\nlet mut m: Map<UserId, string> = Map.new()\nm.insert(UserId(1), \"a\")\nlet out: Option<string> = m.get(UserId(1))\nprint(\"{out}\")",
    );
}

#[test]
fn newtype_over_non_keyable_base_is_a_check_error() {
    assert_code(
        "newtype Bad = Map<string, int>\nlet s: Set<Bad> = Set.of(Bad(Map.new()))\nprint(\"ok\")",
        "TPZ5007",
    );
}

#[test]
fn newtype_in_map_value_position_is_fine() {
    // Only the KEY/element slot is a key — a newtype in the VALUE slot of a
    // Map is keyless and therefore allowed.
    assert_clean(
        "newtype UserId = int\nlet mut m: Map<string, UserId> = Map.new()\nm.insert(\"k\", UserId(1))\nprint(\"ok\")",
    );
}

#[test]
fn scalar_keyed_collections_still_check() {
    // The fix must not over-reject: int/string keys remain valid.
    assert_clean("let s = Set.of(1, 2, 3)\nprint(\"ok\")");
    assert_clean("let mut m: Map<int, string> = Map.new()\nm.insert(1, \"a\")\nprint(\"ok\")");
}

#[test]
fn recursive_mixed_and_nested_newtype_bases_form_without_infinite_values() {
    assert_clean("newtype Never = Never\n0");
    assert_clean("newtype A = B\nnewtype B = A\n0");
    assert_clean(
        "record Node { next: Link | null }\nnewtype Link = Node\nlet end: Node = Node { next: null }\nlet linked: Link = Link(end)\nprint(\"{linked}\")",
    );
    assert_clean(
        "newtype Inner<T> = T\nnewtype Outer<T> = Inner<T>\nlet wrapped: Outer<int> = Outer(Inner(1))\nlet inner: Inner<int> = wrapped.value()\nlet value: int = inner.value()\nprint(\"{value}\")",
    );
}

#[test]
fn generic_newtype_parameters_are_invariant_even_when_phantom() {
    assert_code(
        "newtype Tag<T> = int\nlet a: Tag<int> = Tag(1)\nlet b: Tag<string> = a",
        "TPZ5001",
    );
    assert_clean(
        "newtype Tag<T> = int\nfunction left() -> Tag<int> | Tag<string> {\n    let x: Tag<int> = Tag(1)\n    x\n}\nfunction right() -> Tag<int> | Tag<string> {\n    let x: Tag<string> = Tag(1)\n    x\n}\nlet same = left() == right()\nprint(\"{same}\")",
    );
}

#[test]
fn generic_value_bridge_preserves_the_rigid_base_type() {
    assert_clean(
        "newtype Id<T> = T\nfunction unwrap<T>(id: Id<T>) -> T { id.value() }\nlet n: int = unwrap(Id(7))\nprint(\"{n}\")",
    );
}

#[test]
fn a_record_base_value_field_does_not_collide_with_the_bridge() {
    assert_clean(
        "record Payload { value: int }\nnewtype Wrapped = Payload\nlet wrapped: Wrapped = Wrapped(Payload { value: 7 })\nlet payload: Payload = wrapped.value()\nlet n: int = payload.value\nprint(\"{n}\")",
    );
}

#[test]
fn value_bridge_can_be_captured_as_a_zero_argument_bound_method() {
    assert_clean(
        "newtype UserId = int\nlet id: UserId = UserId(7)\nlet get: () -> int = id.value\nlet n: int = get()\nprint(\"{n}\")",
    );
}
