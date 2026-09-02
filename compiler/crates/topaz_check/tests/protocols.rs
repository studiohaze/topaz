//! Protocol, conformance, derive, and static-dispatch authority witnesses.

use topaz_check::{UnitModule, check_program_with_version, check_unit_with_version};
use topaz_diag::FileId;
use topaz_parser::{ParseOptions, parse_with_options};
use topaz_syntax::LangVersion;

fn check_at(src: &str, version: LangVersion) -> Vec<String> {
    let parsed = parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: version,
        },
    );
    assert!(
        parsed.diagnostics.is_empty(),
        "test source must parse: {src}\n{:?}",
        parsed.diagnostics
    );
    check_program_with_version(src, &parsed.program, version)
        .diagnostics
        .iter()
        .map(|diag| format!("{} {}", diag.code.as_str(), diag.message))
        .collect()
}

fn assert_clean(src: &str) {
    let diagnostics = check_at(src, LangVersion::V5_5);
    assert!(
        diagnostics.is_empty(),
        "expected clean, got {diagnostics:?}"
    );
}

fn assert_code_and_message(src: &str, code: &str, message: &str) {
    let diagnostics = check_at(src, LangVersion::V5_5);
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.starts_with(code) && diag.contains(message)),
        "expected {code} containing {message:?}, got {diagnostics:?}"
    );
}

fn check_closed_unit(src: &str) -> Vec<String> {
    let parsed = parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: LangVersion::V5_5,
        },
    );
    assert!(
        parsed.diagnostics.is_empty(),
        "test source must parse: {src}"
    );
    check_unit_with_version(
        &[UnitModule {
            identity: "main".to_string(),
            is_entry: true,
            is_extern: false,
            is_generated_std: false,
            extern_replay_error: None,
            src,
            program: &parsed.program,
        }],
        LangVersion::V5_5,
    )
    .diagnostics
    .iter()
    .map(|diag| format!("{} {}", diag.code.as_str(), diag.message))
    .collect()
}

#[test]
fn implicit_self_and_one_explicit_standin_are_equivalent() {
    assert_clean(
        r#"
protocol Merge {
    function merge(value: Self, other: Option<Self>) -> Self
}
record Item { value: int }
impl Merge<Item> {
    function merge(value: Item, other: Option<Item>) -> Item { value }
}
let item: Item = Merge.merge(Item { value: 1 }, None)
print("{item.value}")
"#,
    );
    assert_clean(
        r#"
type T = int
type Verdict = bool
protocol Same<T> {
    function same(value: Self, other: T) -> Verdict
}
record Item { value: int }
impl Same<Item> {
    function same(value: Item, other: Item) -> bool { value == other }
}
let ok: bool = Same.same(Item { value: 1 }, Item { value: 1 })
print("{ok}")
"#,
    );
}

#[test]
fn marker_protocols_are_empty_but_have_no_dispatch_members() {
    assert_clean("protocol Marker {}\nrecord Item {}\nimpl Marker<Item> {}\n0");
    assert_code_and_message(
        "protocol Marker {}\nrecord Item {}\nimpl Marker<Item> {}\nMarker.anything(Item {})",
        "TPZ5522",
        "has no method `anything`",
    );
}

#[test]
fn protocol_signatures_are_explicit_static_receiver_signatures() {
    for (source, message) in [
        (
            "protocol P { function make() -> int }\n0",
            "must take the conforming value as its first parameter",
        ),
        (
            "protocol P { function f(value: int) -> int }\n0",
            "must use `Self` or the protocol's type parameter",
        ),
        (
            "protocol P { function f(value: Self) }\n0",
            "requires an explicit return type",
        ),
        (
            "protocol P { function f<T>(value: Self) -> Self }\n0",
            "cannot be generic",
        ),
        (
            "protocol P { function f(value: Self = 1) -> Self }\n0",
            "cannot declare parameter defaults",
        ),
        (
            "protocol P { function f(...value: Self) -> Self }\n0",
            "cannot be variadic",
        ),
    ] {
        assert_code_and_message(source, "TPZ5022", message);
    }
    assert_clean(
        "protocol Sink { function write(value: Self) -> () }\nrecord Item {}\nimpl Sink<Item> { function write(value: Item) -> () {} }\nSink.write(Item {})",
    );
}

#[test]
fn protocols_take_at_most_one_conforming_type_parameter() {
    assert_code_and_message(
        "protocol Pair<A, B> { function first(value: A) -> A }\n0",
        "TPZ5022",
        "at most one conforming-type parameter",
    );
    assert_code_and_message(
        "type P = int\nprotocol P { function get(value: Self) -> int }\n0",
        "TPZ5022",
        "is already a type and cannot also be a protocol",
    );
}

#[test]
fn protocol_declarations_are_module_top_level_only() {
    assert_code_and_message(
        "function f() -> int { protocol P { function get(value: Self) -> int }\n0 }\nf()",
        "TPZ5022",
        "protocol declarations are module-top-level only",
    );
}

#[test]
fn manual_impl_signatures_match_after_alias_formation_without_widening() {
    assert_clean(
        "type Verdict = bool\nprotocol P { function test(value: Self) -> Verdict }\nrecord R {}\nimpl P<R> { function test(value: R) -> bool { true } }\nlet ok: bool = P.test(R {})\nprint(\"{ok}\")",
    );
    assert_code_and_message(
        "protocol P { function test(value: Self) -> bool }\nrecord R {}\nimpl P<R> { function test(value: R) -> bool | string { true } }\n0",
        "TPZ5022",
        "must match the declared signature exactly",
    );
    assert_code_and_message(
        "protocol P { function test(value: Self, n: int) -> bool }\nrecord R {}\nimpl P<R> { function test(value: R, n: string) -> bool { true } }\n0",
        "TPZ5022",
        "must match the declared signature exactly",
    );
}

#[test]
fn manual_protocol_methods_have_no_generic_default_variadic_or_export_surface() {
    for (source, message) in [
        (
            "protocol P { function f(value: Self) -> int }\nrecord R {}\nimpl P<R> { export function f(value: R) -> int { 1 } }\n0",
            "is module-local and cannot be exported",
        ),
        (
            "protocol P { function f(value: Self) -> int }\nrecord R {}\nimpl P<R> { function f<T>(value: R) -> int { 1 } }\n0",
            "cannot be generic",
        ),
        (
            "protocol P { function f(value: Self) -> int }\nrecord R {}\nimpl P<R> { function f(value: R = R {}) -> int { 1 } }\n0",
            "cannot declare parameter defaults",
        ),
        (
            "protocol P { function f(value: Self) -> int }\nrecord R {}\nimpl P<R> { function f(...value: R) -> int { 1 } }\n0",
            "cannot be variadic",
        ),
        (
            "protocol P { function f(value: Self) -> int }\nrecord R {}\nimpl P<R> { function f(value: R) { 1 } }\n0",
            "requires an explicit return type",
        ),
    ] {
        assert_code_and_message(source, "TPZ5022", message);
    }
}

#[test]
fn generic_derive_and_manual_conformance_shells_reject_at_declaration() {
    for source in [
        "record Box<T> derives Show { value: T }\n0",
        "enum Maybe<T> derives Eq { None, Some(T) }\n0",
        "record Box<T> { value: T }\nimpl Show<Box> { function show(value: Box<int>) -> string { \"box\" } }\n0",
    ] {
        assert_code_and_message(source, "TPZ5022", "generic nominal");
    }
}

#[test]
fn derive_capabilities_expand_nested_generic_nominals_exactly() {
    assert_clean(
        "record Box<T> { value: T }\nrecord Good derives Eq, Order, JSON { value: Box<int> }\n0",
    );
    assert_code_and_message(
        "record Box<T> { value: T }\nrecord Bad derives Eq { value: Box<(int) -> int> }\n0",
        "TPZ5530",
        "non-comparable",
    );
    assert_code_and_message(
        "record Box<T> { value: T }\nrecord Bad derives JSON { value: Box<float> }\n0",
        "TPZ5530",
        "cannot round-trip through JSON",
    );
}

#[test]
fn order_derive_requires_a_true_total_member_order() {
    for source in [
        "record R derives Order { value: float }\n0",
        "enum E derives Order { F(float) }\n0",
        "record Inner { value: float }\nrecord Outer derives Order { inner: Inner }\n0",
    ] {
        assert_code_and_message(source, "TPZ5530", "not totally orderable");
    }
    assert_clean(
        "enum E { A, B(int, string) }\nrecord R derives Order { value: E }\nlet n: int = Order.compare(R { value: E.A }, R { value: E.B(1, \"x\") })\nprint(\"{n}\")",
    );
}

#[test]
fn monomorphic_newtypes_over_instantiated_generics_are_exact_nominal_receivers() {
    assert_clean(
        "newtype Ids = Array<int>\nprotocol Size { function size(value: Self) -> int }\nimpl Size<Ids> { function size(value: Ids) -> int { value.value().length } }\nlet n: int = Size.size(Ids([1, 2]))\nprint(\"{n}\")",
    );
    assert_code_and_message(
        "newtype Ids = Array<int>\nprotocol Size { function size(value: Self) -> int }\nimpl Size<Ids> { function size(value: Array<int>) -> int { value.length } }\n0",
        "TPZ5022",
        "must match the declared signature exactly",
    );
}

#[test]
fn protocol_heads_are_direct_static_calls_not_first_class_values() {
    for source in [
        "record R derives Show {}\nlet head = Show\n0",
        "record R derives Show {}\nlet method = Show.show\n0",
    ] {
        let diagnostics = check_closed_unit(source);
        assert!(
            diagnostics
                .iter()
                .any(|diag| diag.starts_with("TPZ5002") && diag.contains("not bound")),
            "expected closed-unit protocol-head rejection, got {diagnostics:?}"
        );
    }
    assert_clean("let Show = { show: (x) => x + 1 }\nlet n: int = Show.show(1)\nprint(\"{n}\")");
}
