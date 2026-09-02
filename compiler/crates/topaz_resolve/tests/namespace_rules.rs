//! Namespace usage rules (SPEC v5.2 §17) that need precise sources
//! the corpus fixture shape does not isolate: Form-A member writes,
//! exported type aliases in expression position, qualified-type head
//! proof, and walker coverage of lambda/pattern/block-local types.

use topaz_resolve::{InMemoryProvider, ResolvedReferenceRole, resolve, resolve_with_version};
use topaz_syntax::LangVersion;

const LIB: &str = "export function trim(s: string) -> string { s }\n\
                   export let user = { name: \"u\" }\n\
                   export type User = { id: int }\n";

fn primary_code(entry_src: &str) -> Option<String> {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", entry_src);
    provider.add_file("utils/strings.tpz", LIB);
    let output = resolve(&provider, "main.tpz", None);
    output
        .diagnostics
        .first()
        .map(|d| d.code.as_str().to_string())
}

fn primary_message(entry_src: &str) -> Option<String> {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", entry_src);
    provider.add_file("utils/strings.tpz", LIB);
    resolve(&provider, "main.tpz", None)
        .diagnostics
        .first()
        .map(|d| d.message.clone())
}

#[test]
fn not_exported_typo_suggests_the_export() {
    // A not-exported member that is a plausible typo of a real export
    // gets a "; did you mean …?" hint. `Usre` is one transposition from the
    // exported type alias `User`.
    let msg = primary_message("import utils.strings\nlet f = (x: strings.Usre) => x\n");
    assert!(
        msg.as_deref()
            .is_some_and(|m| m.contains("did you mean `User`?")),
        "want a User suggestion, got: {msg:?}"
    );
    // An unrelated name offers nothing (a wrong suggestion is worse than none).
    let none = primary_message("import utils.strings\nlet f = (x: strings.Zzzz) => x\n");
    assert!(
        none.as_deref().is_some_and(|m| !m.contains("did you mean")),
        "want no suggestion, got: {none:?}"
    );
}

#[test]
fn namespace_member_assignment_is_readonly() {
    let code = primary_code("import utils.strings\nstrings.user = 2\n");
    assert_eq!(code.as_deref(), Some("TPZ3015"));
}

#[test]
fn exported_type_alias_is_not_a_value() {
    let code = primary_code("import utils.strings\nlet x = strings.User\n");
    assert_eq!(code.as_deref(), Some("TPZ3013"));
}

#[test]
fn qualified_type_head_must_be_a_namespace() {
    let code = primary_code("import utils.strings\nlet n = 1\nlet u: n.User = make()\n");
    assert_eq!(code.as_deref(), Some("TPZ3013"));
}

#[test]
fn qualified_type_head_shadowed_by_local_is_rejected() {
    let code = primary_code(
        "import utils.strings\n\
         function f(strings: int) -> () {\n\
         \tlet u: strings.User = make()\n\
         \t()\n\
         }\n",
    );
    assert_eq!(code.as_deref(), Some("TPZ3013"));
}

#[test]
fn lambda_parameter_types_are_walked() {
    let code = primary_code("import utils.strings\nlet f = (x: strings.Nope) => x\n");
    assert_eq!(code.as_deref(), Some("TPZ3009"));
}

#[test]
fn match_typed_patterns_are_walked() {
    let code = primary_code(
        "import utils.strings\n\
         let r = match 1 {\n\
         \tcase x: strings.Nope => x\n\
         \tcase _ => 0\n\
         }\n",
    );
    assert_eq!(code.as_deref(), Some("TPZ3009"));
}

#[test]
fn block_local_const_shadows_namespace() {
    let code = primary_code(
        "import utils.strings\n\
         function f() -> int {\n\
         \tconst strings = 1\n\
         \tstrings\n\
         }\n",
    );
    assert_eq!(code, None);
}

#[test]
fn valid_namespace_use_stays_clean() {
    let code = primary_code(
        "import utils.strings\n\
         let t = strings.trim(\"x\")\n\
         let u: strings.User = make()\n",
    );
    assert_eq!(code, None);
}

#[test]
fn top_level_same_binding_or_introduces_one_module_name() {
    let mut provider = InMemoryProvider::new();
    provider.add_file(
        "main.tpz",
        "let value: Result<int, int> = Err(7)\n\
         let Ok(x) | Err(x) = value\n\
         print(\"{x}\")\n",
    );
    let output = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_5);
    assert!(
        output
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code.as_str() != "TPZ3008"),
        "agreement-equivalent alternatives must introduce `x` once: {:?}",
        output.diagnostics
    );
}

#[test]
fn top_level_binding_or_keeps_duplicate_within_one_alternative_loud() {
    let mut provider = InMemoryProvider::new();
    provider.add_file(
        "main.tpz",
        "let value = { left: 1, right: 2 }\n\
         let { left: x, right: x } | { left: x, right: x } = value\n",
    );
    let output = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_5);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == "TPZ3008"),
        "a duplicate inside one alternative must remain a module collision: {:?}",
        output.diagnostics
    );
}

#[test]
fn imported_initializer_nominal_record_binding_shadows_later_module_name() {
    let mut provider = InMemoryProvider::new();
    provider.add_file(
        "main.tpz",
        "import model { observed }\nprint(\"{observed}\")\n",
    );
    provider.add_file(
        "model.tpz",
        "record Point { x: int }\n\
         export let observed = {\n\
         \tlet Point { x } = Point { x: 1 }\n\
         \tx\n\
         }\n\
         let x = 2\n",
    );
    let output = resolve_with_version(&provider, "main.tpz", None, LangVersion::CURRENT);
    assert!(
        output.diagnostics.is_empty(),
        "the block-local nominal-record binding must not reach the later module binding: {:?}",
        output.diagnostics
    );
}

#[test]
fn imported_initializer_delays_short_circuit_rhs_and_optional_call_arguments() {
    let mut provider = InMemoryProvider::new();
    provider.add_file(
        "main.tpz",
        "import model { andValue, orValue, coalesced, optionalCall }\n",
    );
    provider.add_file(
        "model.tpz",
        "export let andValue: bool = false && laterBool\n\
         export let orValue: bool = true || laterBool\n\
         let present: Option<int> = Some(1)\n\
         export let coalesced: int = present ?? laterInt\n\
         let receiver: Option<string> = None\n\
         export let optionalCall: string = receiver?.replace(laterText, \"y\") ?? \"skipped\"\n\
         let laterBool: bool = true\n\
         let laterInt: int = 2\n\
         let laterText: string = \"x\"\n",
    );
    let output = resolve_with_version(&provider, "main.tpz", None, LangVersion::CURRENT);
    assert!(
        output.diagnostics.is_empty(),
        "conditionally skipped initializer references must not be TPZ3018: {:?}",
        output.diagnostics
    );
}

#[test]
fn nominal_record_typed_pattern_resolves_its_type_reference() {
    let source = "type User = { id: int }\n\
                  record Point { x: User }\n\
                  function reveal(point: Point) -> User {\n\
                  \tlet Point { x: value: User } = point\n\
                  \tvalue\n\
                  }\n";
    let pattern_type_lo = source.rfind("User").expect("pattern annotation") as u32;
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", source);
    let output = resolve_with_version(&provider, "main.tpz", None, LangVersion::CURRENT);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let reference = output
        .name_facts
        .references
        .iter()
        .find(|reference| reference.span.lo == pattern_type_lo)
        .expect("typed pattern type reference");
    assert_eq!(reference.role, ResolvedReferenceRole::Type);
    assert_eq!(reference.target_module.as_deref(), Some("main"));
    assert_eq!(reference.target_name.as_deref(), Some("User"));
}

#[test]
fn imported_receiver_impl_is_a_valid_module_item() {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", "import model\n0\n");
    provider.add_file(
        "model.tpz",
        "export record Point { x: int }\n\
         impl Point { export function coordinate(self) -> int { self.x } }\n",
    );
    let output = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_5);
    assert!(
        output
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code.as_str() != "TPZ3007"),
        "an imported module may define methods on its own nominal: {:?}",
        output.diagnostics
    );
}

#[test]
fn imported_module_may_keep_nominal_declarations_private() {
    let mut provider = InMemoryProvider::new();
    provider.add_file(
        "main.tpz",
        "import model { answer }\nlet value = answer()\n",
    );
    provider.add_file(
        "model.tpz",
        "record Point { x: int }\n\
         enum Choice { First, Second }\n\
         newtype Count = int\n\
         function point() -> Point { Point { x: 1 } }\n\
         export function answer() -> int { point().x }\n",
    );
    let output = resolve_with_version(&provider, "main.tpz", None, LangVersion::CURRENT);
    assert!(
        output.diagnostics.is_empty(),
        "private nominal declarations are valid imported-module implementation details: {:?}",
        output.diagnostics
    );
}

#[test]
fn std_root_resolves_as_a_v5_4_virtual_module() {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", "import std.math\nlet x = 1\n");
    let output = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_4);
    assert!(
        output.diagnostics.is_empty(),
        "std.math should resolve virtually: {:?}",
        output.diagnostics
    );
    assert!(
        output.modules.iter().any(|m| m.identity == "std.math"),
        "virtual std.math module must be in the closure: {:?}",
        output.modules
    );
}

#[test]
fn package_generated_std_module_has_explicit_provenance() {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", "import std.lispex\nlet x = 1\n");
    provider.add_generated_std_module(
        "std.lispex",
        "std/lispex.tpz",
        "export function available() -> bool { true }\n",
    );
    let denied_current = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_17);
    assert_eq!(
        denied_current
            .diagnostics
            .first()
            .map(|diagnostic| diagnostic.code.as_str()),
        Some("TPZ3001")
    );
    assert!(
        denied_current
            .modules
            .iter()
            .all(|module| module.identity != "std.lispex")
    );

    let output = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_18);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let module = output
        .modules
        .iter()
        .find(|module| module.identity == "std.lispex")
        .expect("generated std module");
    assert!(module.is_generated_std);

    let mut ordinary = InMemoryProvider::new();
    ordinary.add_file("main.tpz", "import std.lispex\nlet x = 1\n");
    ordinary.add_file(
        "std/lispex.tpz",
        "export function available() -> bool { true }\n",
    );
    let denied = resolve_with_version(&ordinary, "main.tpz", None, LangVersion::V5_17);
    assert_eq!(
        denied
            .diagnostics
            .first()
            .map(|diagnostic| diagnostic.code.as_str()),
        Some("TPZ3001")
    );
}

#[test]
fn std_root_is_still_reserved_before_v5_4() {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", "import std.math\nlet x = 1\n");
    let output = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_3);
    let code = output.diagnostics.first().map(|d| d.code.as_str());
    assert_eq!(code, Some("TPZ3016"));
}

#[test]
fn topaz_root_remains_reserved_in_v5_4() {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", "import topaz.runtime\nlet x = 1\n");
    let output = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_4);
    let code = output.diagnostics.first().map(|d| d.code.as_str());
    assert_eq!(code, Some("TPZ3016"));
}

#[test]
fn unknown_std_module_reports_unresolved() {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", "import std.net\nlet x = 1\n");
    let output = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_4);
    let diag = output.diagnostics.first().expect("unresolved std.net");
    assert_eq!(diag.code.as_str(), "TPZ3001");
    assert!(diag.message.contains("virtual `std/net.tpz`"));
}
