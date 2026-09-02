//! Receiver-implementation authority witnesses.

use topaz_check::check_program_with_version;
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

#[test]
fn monomorphic_nominals_admit_direct_receiver_calls() {
    assert_clean(
        r#"
record Point { x: int }
impl Point {}
impl Point {
    function plus(self, n: int = 1) -> int { self.x + n }
}
impl Point {
    export function total(self, ...xs: int) -> int {
        let mut value = self.x
        for x in xs { value = value + x }
        value
    }
}
let p: Point = Point { x: 3 }
let a: int = p.plus(n: 2)
let b: int = p.total(...[4, 5])
print("{a}:{b}")
"#,
    );

    assert_clean(
        r#"
enum Color { Red, Blue }
impl Color {
    function label(self) -> string {
        match self {
            case Red => "red"
            case Blue => "blue"
        }
    }
    function Red(self) -> bool { true }
}
newtype Count = int
impl Count {
    function next(self) -> Count { Count(self.value() + 1) }
}
let color: Color = Color.Red
let count: Count = Count(1).next()
print("{color.label()}:{color.Red()}:{count.value()}")
"#,
    );
}

#[test]
fn receiver_impls_and_nominal_heads_are_module_hoisted() {
    assert_clean(
        r#"
impl Point {
    function shifted(self) -> Point { Point { x: self.x + offset } }
}
record Point { x: int }
let offset = 2
let p: Point = Point { x: 1 }.shifted()
print("{p.x}")
"#,
    );

    assert_clean(
        r#"
record Point { x: int }
impl Point { function shifted(self) -> int { self.x + offset } }
Point { x: 1 }.shifted()
let offset = 2
0
"#,
    );
}

#[test]
fn omitted_method_return_patches_later_call_metadata() {
    assert_clean(
        r#"
record Point { x: int }
impl Point {
    function doubled(self) { self.x * 2 }
}
let p: Point = Point { x: 2 }
let value: int = p.doubled()
print("{value}")
"#,
    );
    assert_code_and_message(
        r#"
record Point { x: int }
impl Point {
    function doubled(self) { self.x * 2 }
}
let p: Point = Point { x: 2 }
let value: string = p.doubled()
print(value)
"#,
        "TPZ5001",
        "expected `string`, found `int`",
    );
}

#[test]
fn receiver_slot_must_be_bare_first_self() {
    for (source, needle) in [
        (
            "record P { x: int }\nimpl P { function f() -> int { 0 } }\n0",
            "must take `self` as its first parameter",
        ),
        (
            "record P { x: int }\nimpl P { function f(x: int, self: P) -> int { x } }\n0",
            "must take bare `self`",
        ),
        (
            "record P { x: int }\nimpl P { function f(self: P) -> int { self.x } }\n0",
            "must take bare `self`",
        ),
        (
            "record P { x: int }\nimpl P { function f(self = 1) -> int { 0 } }\n0",
            "must take bare `self`",
        ),
        (
            "record P { x: int }\nimpl P { function f(...self: P) -> int { 0 } }\n0",
            "must take bare `self`",
        ),
    ] {
        assert_code_and_message(source, "TPZ5022", needle);
    }
}

#[test]
fn generic_receiver_heads_and_generic_methods_are_closed() {
    for source in [
        "record Box<T> { value: T }\nimpl Box { function get(self) -> int { 0 } }\n0",
        "enum Maybe<T> { None, Some(T) }\nimpl Maybe { function tag(self) -> int { 0 } }\n0",
        "newtype Id<T> = T\nimpl Id { function tag(self) -> int { 0 } }\n0",
    ] {
        assert_code_and_message(source, "TPZ5022", "generic nominal");
    }
    assert_code_and_message(
        "record P { x: int }\nimpl P { function id<T>(self, value: T) -> T { value } }\n0",
        "TPZ5022",
        "generic methods are not supported yet",
    );
}

#[test]
fn impl_blocks_are_local_module_top_level_declarations() {
    assert_code_and_message(
        "record P { x: int }\nfunction f() -> int { impl P { function x(self) -> int { 0 } }\n0 }\nf()",
        "TPZ5022",
        "impl declarations are module-top-level only",
    );
    assert_code_and_message(
        "impl int { function x(self) -> int { 0 } }\n0",
        "TPZ5022",
        "cannot define methods on `int`",
    );
}

#[test]
fn method_identity_is_unique_and_cannot_shadow_fields_or_builtins() {
    assert_code_and_message(
        "record P { x: int }\nimpl P { function f(self) -> int { 0 } }\nimpl P { function f(self) -> int { 1 } }\n0",
        "TPZ5008",
        "already defined for `P`",
    );
    assert_code_and_message(
        "record P { x: int }\nimpl P { function x(self) -> int { 0 } }\n0",
        "TPZ5022",
        "collides with a field",
    );
    assert_code_and_message(
        "newtype Id = int\nimpl Id { function value(self) -> int { 0 } }\n0",
        "TPZ5022",
        "collides with a builtin member",
    );
}

#[test]
fn user_methods_exist_only_as_exact_nominal_direct_calls() {
    let prefix = r#"
record P { x: int }
impl P { function plus(self) -> int { self.x + 1 } }
let p: P = P { x: 1 }
"#;
    assert_code_and_message(
        &format!("{prefix}\nlet method = p.plus\n0"),
        "TPZ5006",
        "has no field `plus`",
    );
    assert_code_and_message(
        &format!("{prefix}\nlet maybe: P | null = p\nmaybe?.plus()\n0"),
        "TPZ5006",
        "has no field `plus`",
    );
    assert_code_and_message(
        &format!("{prefix}\nlet method = p |> .plus\n0"),
        "TPZ5006",
        "has no field `plus`",
    );
}

#[test]
fn method_defaults_cannot_capture_self_and_self_is_contextual_elsewhere() {
    assert_code_and_message(
        "record P { x: int }\nimpl P { function plus(self, n: int = self.x) -> int { n } }\n0",
        "TPZ5001",
        "must be constant expressions",
    );
    assert_clean("let self = 1\nlet impl = 2\nprint(\"{self}:{impl}\")");
}

#[test]
fn an_impl_block_cannot_be_exported_as_a_whole() {
    let src = "record P { x: int }\nexport impl P { function x(self) -> int { 0 } }\n0";
    let parsed = parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: LangVersion::V5_5,
        },
    );
    assert!(
        parsed
            .diagnostics
            .iter()
            .any(|diag| diag.code.as_str() == "TPZ2001"),
        "export impl must stay outside the export grammar: {:?}",
        parsed.diagnostics
    );
}
