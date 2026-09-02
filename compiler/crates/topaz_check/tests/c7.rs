//! §22.2 `Option`→`Result` bridge method typing (`okOr` / `okOrElse`).
//! `T` flows from the Option receiver; the error type `E` is the lone
//! scheme variable, solved from the `error` argument (eager) or the
//! callback's return type (lazy), and the result is `Result<T, E>`.

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

// ---- okOr / okOrElse type as Result<T, E> ----------------------------

#[test]
fn ok_or_yields_result_t_e() {
    // `toInt(_)` is `Option<int>`; `.okOr("e")` fixes `E = string`, so the
    // annotation `Result<int, string>` conforms.
    assert_clean("let r: Result<int, string> = toInt(\"5\").okOr(\"not a number\")");
    // The error type tracks the argument: a bool error makes it Result<int, bool>.
    assert_clean("let r: Result<int, bool> = toInt(\"5\").okOr(false)");
}

#[test]
fn ok_or_else_yields_result_t_e() {
    // The callback's return type fixes `E`; `() -> string` ⇒ Result<int, string>.
    assert_clean("let r: Result<int, string> = toInt(\"5\").okOrElse(() => \"e\")");
    assert_clean("let r: Result<int, int> = toInt(\"5\").okOrElse(() => 0)");
}

#[test]
fn ok_or_threads_element_type_through_some() {
    // `T` comes straight from the receiver: Some(5) is Option<int>.
    assert_clean("let r: Result<int, string> = Some(5).okOr(\"e\")");
    assert_clean("let r: Result<string, int> = Some(\"hi\").okOrElse(() => 0)");
}

#[test]
fn ok_or_wrong_error_type_is_tpz5001() {
    // The annotation pins `E = string`, but the argument is an int.
    assert_code(
        "let r: Result<int, string> = toInt(\"5\").okOr(7)",
        "TPZ5001",
    );
}

#[test]
fn ok_or_else_wrong_callback_return_is_tpz5001() {
    // Annotation pins `E = string`; the callback returns an int.
    assert_code(
        "let r: Result<int, string> = toInt(\"5\").okOrElse(() => 7)",
        "TPZ5001",
    );
}

#[test]
fn ok_or_else_non_function_argument_is_tpz5001() {
    // `okOrElse` wants a `() -> E`, not a bare value. With `E` pinned by the
    // annotation the function-type expectation is fully solved, so a bare
    // value is the `() -> string` vs `"e"` mismatch — exactly how `map`/`filter`
    // reject a non-function once their slot type is known. (Unannotated, `E`
    // is still open and it surfaces as the §22.1 contextual-type request,
    // identical to `map([1,2], 5)`.)
    assert_code(
        "let r: Result<int, string> = toInt(\"5\").okOrElse(\"e\")",
        "TPZ5001",
    );
}

#[test]
fn ok_or_wrong_arity_is_tpz5004() {
    // `okOr` takes exactly one argument.
    assert_code("let r = toInt(\"5\").okOr()", "TPZ5004");
    assert_code("let r = toInt(\"5\").okOr(\"a\", \"b\")", "TPZ5004");
}

#[test]
fn ok_or_else_callback_return_drives_e_when_annotated() {
    // The callback body's type fixes `E`; a `() -> string` lambda conforms to
    // a `Result<int, string>` annotation. (A wrong RETURN type is the TPZ5001
    // covered by `ok_or_else_wrong_callback_return_is_tpz5001`. A surplus
    // lambda PARAMETER is not rejected here — the rank-1 engine accepts a wider
    // lambda against an inferred `() -> E` slot exactly as `map`/`filter` do
    // when their element/param type is still being solved; this is the shared
    // generic-argument behavior, not specific to the bridge.)
    assert_clean("let r: Result<int, string> = toInt(\"5\").okOrElse(() => \"e\")");
}

#[test]
fn unknown_option_member_is_tpz5006() {
    // A non-existent Option member faults NO_FIELD (and may suggest a real one).
    assert_code("let r = toInt(\"5\").okOrNope(\"e\")", "TPZ5006");
}

// ---- bare `None` receiver: the bridge args are CHECKED, not staged ----
//
// A bare `None` infers as `Option<_>` (the element var unsolved, lowered to
// `Unknown`), NOT a shapeless `Unknown`. So a member call on it RESOLVES the
// bridge scheme and type-checks the argument exactly as `Some(_)`/`toInt(_)`
// receivers do — the receiver shape no longer collapses and silently swallows
// every argument as `Unknown`. These pin the arms that were the static hole.

#[test]
fn none_receiver_ok_or_else_non_function_is_rejected() {
    // `okOrElse` wants `() -> E`; a bare value (or a non-callback) is a mismatch
    // even when the receiver is a bare `None` and `E` is pinned by the annotation.
    assert_code("let r: Result<int, string> = (None).okOrElse(5)", "TPZ5001");
}

#[test]
fn none_receiver_bridge_arity_is_tpz5004() {
    // The bridge methods take exactly one argument, on a bare `None` too.
    assert_code("let r: Result<int, string> = (None).okOrElse()", "TPZ5004");
    assert_code("let r: Result<int, string> = (None).okOr()", "TPZ5004");
    assert_code(
        "let r: Result<int, string> = (None).okOr(\"a\", \"b\")",
        "TPZ5004",
    );
}

#[test]
fn none_receiver_ok_or_else_callback_arity_is_tpz5004() {
    // `okOrElse`'s callback is `() -> E`; a lambda with parameters cannot satisfy
    // the zero-arg shape — the arity is caught on a bare `None` receiver as well.
    assert_code(
        "let r: Result<int, int> = (None).okOrElse((x: int) => x)",
        "TPZ5004",
    );
    assert_code(
        "let r: Result<int, int> = (None).okOrElse((a, b) => a)",
        "TPZ5004",
    );
}

#[test]
fn none_receiver_unknown_member_is_tpz5006() {
    // A bare `None` keeps its `Option<_>` shape, so an unknown member on it
    // faults NO_FIELD rather than silently typing as `Unknown`.
    assert_code(
        "let r: Result<int, string> = (None).bogusMember(99)",
        "TPZ5006",
    );
}

#[test]
fn none_receiver_valid_bridge_is_clean() {
    // The fix must not regress the valid bare-`None` bridge calls (the difftest
    // corpus relies on these): a correct `error`/callback still type-checks,
    // annotated or not.
    assert_clean("let r: Result<int, string> = (None).okOr(\"e\")");
    assert_clean("let r: Result<int, string> = (None).okOrElse(() => \"e\")");
    assert_clean("let b = (None).okOr(\"e\")");
    assert_clean("let d = (None).okOrElse(() => \"f\")");
}

// ---- bare `None` keeps Option shape ⇒ match is exhaustive over the TYPE ----
//
// The same shape-preservation that lets a bridge call resolve also gives a bare
// `None` scrutinee its `Option<Unknown>` type. So `match None { … }` is now
// checked for EXHAUSTIVENESS over the Option type — both constructors, not just
// the one constructor literally written. This is a DELIBERATE, type-consistent
// consequence (a match on an `Option` should cover the `Option`); it is the more
// correct behavior, the check-corpus is green under it, and no real program
// relies on the old `Unknown`-scrutinee (a bare `Unknown` is unmatchable-exhaust
// in the same way a typed value is). These pin both directions of the rule.

#[test]
fn match_on_bare_none_requires_some_arm_tpz5021() {
    // A lone `None` arm is NON-exhaustive over `Option<Unknown>`: the `Some`
    // constructor is uncovered, so the match faults TPZ5021. (On `main` the
    // bare-`Unknown` scrutinee made this types-ok; the shape fix corrects it.)
    assert_code("let x = match None { case None => 0 }", "TPZ5021");
}

#[test]
fn match_on_bare_none_with_both_arms_is_clean() {
    // Covering BOTH constructors of the `Option` type is exhaustive ⇒ clean.
    assert_clean("let x = match None {\n  case None => 0\n  case Some(v) => v\n}");
}

// ---- S1 (v5.4 §7.1) MATCH GUARDS do not count toward coverage --------------
// A guarded arm (`case P if cond =>`) may be skipped at runtime, so its pattern
// does NOT cover its case for exhaustiveness. The checker already merges only
// UNGUARDED arms into the coverage; these pin that a guarded arm leaves its case
// uncovered, while an unguarded sibling restores exhaustiveness.

#[test]
fn guarded_arm_does_not_cover_for_exhaustiveness_tpz5021() {
    // §7.1: `Some(n) if n > 0` is GUARDED, so it does not cover `Some` — with only
    // `None` unguarded, the `Some` constructor is missing and the match is
    // NON-exhaustive (TPZ5021). The guarded arm's pattern must NOT make it look
    // exhaustive (the inverse would silently drop the uncovered-`Some` value).
    assert_code(
        "let opt: Option<int> = Some(5)\nlet r = match opt {\n  case Some(n) if n > 0 => n\n  case None => 0\n}\nprint(\"{r}\")",
        "TPZ5021",
    );
}

#[test]
fn unguarded_sibling_restores_exhaustiveness() {
    // Adding an UNGUARDED `Some(_)` (or a `_`) arm covers the case the guarded arm
    // could miss, so the match is exhaustive ⇒ clean.
    assert_clean(
        "let opt: Option<int> = Some(5)\nlet r = match opt {\n  case Some(n) if n > 0 => n\n  case Some(_) => -1\n  case None => 0\n}\nprint(\"{r}\")",
    );
}

// ---- C7: the v5.4 string method surface --------------------------------

#[test]
fn unknown_string_method_calls_reject() {
    // The interpreter has no OTHER string methods and faults them at runtime; the
    // checker rejects every such call statically (TPZ5006), in both widened
    // (`Prim::String`) and bare-literal (`Type::Literal(Lit::Str)`) receiver forms.
    for m in [
        "replaceFirst(\"a\", \"b\")",
        "toUpper()",
        "toLowerCase()",
        "length()",
    ] {
        assert_code(
            &format!("let s = \"hello\"\nlet _ = s.{m}\nprint(\"ok\")"),
            "TPZ5006",
        );
        // bare string literal receiver (no `let` widening) — same fault
        assert_code(&format!("let _ = \"hello\".{m}\nprint(\"ok\")"), "TPZ5006");
    }
}

#[test]
fn string_stdlib_methods_type_clean() {
    // The string stdlib must type clean on both the widened and bare-literal
    // receiver forms.
    for (m, ret) in [
        ("startsWith(\"he\")", "bool"),
        ("endsWith(\"lo\")", "bool"),
        ("contains(\"ell\")", "bool"),
        ("indexOf(\"l\")", "opt"),
        ("lastIndexOf(\"l\")", "opt"),
        ("trim()", "str"),
        ("trimStart()", "str"),
        ("trimEnd()", "str"),
        ("byteLength()", "int"),
        ("split(\",\")", "arr"),
        ("replace(\"l\", \"L\")", "str"),
    ] {
        let _ = ret;
        assert_clean(&format!("let s = \"hello\"\nlet _ = s.{m}\nprint(\"ok\")"));
        assert_clean(&format!("let _ = \"hello\".{m}\nprint(\"ok\")"));
    }
}

#[test]
fn unknown_string_method_hint_suggests_scalars() {
    // The one real method is offered as the did-you-mean.
    let diags = check("let s = \"hello\"\nlet _ = s.scalar()\nprint(\"ok\")");
    assert!(
        diags
            .iter()
            .any(|d| d.starts_with("TPZ5006") && d.contains("scalars")),
        "expected a `scalars` suggestion, got: {diags:?}"
    );
}

#[test]
fn the_real_string_method_scalars_stays_clean() {
    // `s.scalars()` (alongside the string stdlib methods) must still type clean.
    assert_clean("let s = \"hello\"\nlet cs = s.scalars()\nprint(\"{cs}\")");
    assert_clean("let cs = \"hello\".scalars()\nprint(\"{cs}\")");
}

#[test]
fn unknown_string_member_access_still_rejects() {
    // Member ACCESS of an unknown string member was already rejected (C3); C7
    // does not change that — only adds the symmetric CALL rejection.
    assert_code(
        "let s = \"hello\"\nlet _ = s.bogus\nprint(\"ok\")",
        "TPZ5006",
    );
}

#[test]
fn string_method_on_an_opaque_receiver_stays_staged() {
    // A generic/`Var`/`Unknown` receiver is NOT member-closed, so an unknown
    // method on it stays STAGED (the checker cannot decide it) — the C7
    // rejection is scoped to CONCRETE string receivers only.
    assert_clean(
        "function f<T>(x: T) -> () {\n    let _ = x.contains(\"y\")\n    print(\"ok\")\n}",
    );
}
