//! Phase C-2 witnesses: expression-core typing. The graduation
//! witnesses mirror the runtime guards (same violation, now static);
//! the corpus sweep proves the staged checker never false-positives
//! on real programs.

use std::fs;

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

fn check_v54(src: &str) -> Vec<String> {
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

fn assert_code_v54(src: &str, code: &str) {
    let diags = check_v54(src);
    assert!(
        diags.iter().any(|d| d.starts_with(code)),
        "expected {code}, got: {diags:?}"
    );
}

// ---- TPZ5001 graduation: type mismatch -------------------------------

#[test]
fn annotation_mismatch_is_static_tpz5001() {
    assert_code("let port: int = \"eighty\"", "TPZ5001");
    assert_code("let flag: bool = 1", "TPZ5001");
    assert_clean("let port: int = 8080");
    assert_clean("let port: int | null = null");
}

#[test]
fn literal_union_annotations() {
    assert_clean("let state: \"open\" | \"closed\" = \"open\"");
    // CDR-004 §4 widening witness: a plain string variable does not
    // satisfy the literal union…
    assert_code(
        "let s = \"open\"\nlet state: \"open\" | \"closed\" = s",
        "TPZ5001",
    );
    // …while `const` preserves the literal type.
    assert_clean("const s = \"open\"\nlet state: \"open\" | \"closed\" = s");
}

#[test]
fn arithmetic_and_logic_mismatches() {
    assert_code("let x = 1 + \"two\"", "TPZ5001");
    assert_code("let x = 1 + 2.5", "TPZ5001");
    assert_code("let x = true && 1", "TPZ5001");
    assert_code("let x = -\"flip\"", "TPZ5001");
    assert_clean("let x = 1 + 2");
    assert_clean("let x = 1.5 + 2.5");
    assert_clean("let x = \"a\" + \"b\"");
    assert_clean("let x = !true");
}

#[test]
fn condition_must_be_bool() {
    assert_code("if 1 {\n    print(\"x\")\n}", "TPZ5001");
    assert_code("while \"yes\" {\n    print(\"x\")\n}", "TPZ5001");
    assert_clean("if true {\n    print(\"x\")\n}");
}

#[test]
fn assignment_checks_target_type() {
    assert_code("let mut n = 1\nn = \"two\"\nprint(\"{n}\")", "TPZ5001");
    assert_clean("let mut n = 1\nn = 2\nprint(\"{n}\")");
}

#[test]
fn array_indexing_types_element() {
    assert_code("let xs = [1, 2, 3]\nlet y: string = xs[0]", "TPZ5001");
    assert_code("let xs = [1, 2, 3]\nlet y = xs[\"zero\"]", "TPZ5001");
    assert_clean("let xs = [1, 2, 3]\nlet y: int = xs[0]");
}

#[test]
fn default_parameter_must_match_annotation() {
    assert_code(
        "function f(n: int = \"one\") -> () {\n    print(\"{n}\")\n}",
        "TPZ5001",
    );
}

#[test]
fn mismatch_widens_a_literal_found_only_for_prim_expectations() {
    // Against a plain-primitive expectation a literal `found` reads as its
    // base type (the `??` slip `o ?? "default"` shows `string`, not `"default"`)…
    let prim = check("let x: int = \"eighty\"\nprint(\"{x}\")");
    assert!(
        prim.iter().any(|d| d.contains("found `string`")),
        "{prim:?}"
    );
    assert!(
        !prim.iter().any(|d| d.contains("\"eighty\"")),
        "the literal should be widened away: {prim:?}"
    );
    // …but a literal-union expectation keeps the specific literal — it is the point.
    let uni = check("let s: \"open\" | \"closed\" = \"foo\"\nprint(s)");
    assert!(
        uni.iter().any(|d| d.contains("found `\"foo\"`")),
        "the literal should be kept: {uni:?}"
    );
}

#[test]
fn top_level_return_and_try_are_static_tpz5001() {
    // §5/§7: `return` (and `?`, which desugars to a conditional return) outside any
    // function is a static error — the interpreter faults on it, so `check` must gate
    // it too. Previously `check` reported types-ok and the program blew up at `run`.
    assert_code("return 5", "TPZ5001");
    assert_code("if true {\n    return 1\n}", "TPZ5001");
    assert_code(
        "function mk() -> Result<int, string> {\n    return Err(\"x\")\n}\nlet x = mk()?\nprint(\"{x}\")",
        "TPZ5001",
    );
    // a `return` arm of a top-level `match` is also outside any function.
    assert_code(
        "let r = match 1 {\n    case 1 => return 7\n    case _ => 0\n}\nprint(\"{r}\")",
        "TPZ5001",
    );
    // A top-level loop body is still outside any function. Its direct returns
    // and `?` hidden inside a break value must stay statically rejected.
    assert_code_v54("loop {\n    return 1\n}", "TPZ5001");
    assert_code_v54(
        "function fail() -> Result<int, string> {\n    return Err(\"x\")\n}\nloop {\n    break fail()?\n}",
        "TPZ5001",
    );
    // inside a function body all of these are fine (incl. a `return` match arm).
    assert_clean("function f() -> int {\n    return 5\n}\nprint(\"{f()}\")");
    assert_clean(
        "function g(n: int) -> int {\n    let r = match n {\n        case 1 => return 7\n        case _ => 0\n    }\n    return r\n}\nprint(\"{g(2)}\")",
    );
}

#[test]
fn index_read_of_a_non_array_is_static_tpz5001() {
    // §9: only arrays are index-readable; indexing a Map/Set/string/record/int
    // faults at runtime, so `check` rejects it instead of reporting types-ok.
    assert_code(
        "let mut m: Map<string, int> = Map.new()\nlet v = m[\"a\"]\nprint(\"{v}\")",
        "TPZ5001",
    );
    assert_code(
        "let s = Set.of(1, 2)\nlet v = s[0]\nprint(\"{v}\")",
        "TPZ5001",
    );
    assert_code("let t = \"abc\"\nlet c = t[0]\nprint(\"{c}\")", "TPZ5001");
    assert_code("let r = { a: 1 }\nlet v = r[0]\nprint(\"{v}\")", "TPZ5001");
    // arrays still index cleanly.
    assert_clean("let xs = [1, 2, 3]\nlet y: int = xs[0]\nprint(\"{y}\")");
    // a union that could be an array at runtime is NOT rejected (the interpreter
    // indexes it when the value is the array member)…
    assert_clean("let xs: Array<int> | Array<string> = [10]\nlet y = xs[0]\nprint(\"{y}\")");
    // …but the index must still be an int, even for that union.
    assert_code(
        "let xs: Array<int> | Array<string> = [10]\nlet y = xs[\"0\"]\nprint(\"{y}\")",
        "TPZ5001",
    );
    // A mixed union with an array member is value-dependent (it runs when the value
    // is the array), so `check` accepts it and the runtime guard catches the rest.
    assert_clean("let xs: Array<int> | string = [1, 2, 3]\nlet y = xs[0]\nprint(\"{y}\")");
    // A union with NO indexable member always faults, so it IS rejected.
    assert_code(
        "let v: string | int = 5\nlet x = v[0]\nprint(\"{x}\")",
        "TPZ5001",
    );
}

#[test]
fn print_of_a_non_string_suggests_interpolation() {
    // §22.2: `print` is string-only; the most common newcomer slip — `print(n)` —
    // should point at the interpolation form, not a bare "expected `string`" mismatch.
    let diags = check("let n = 42\nprint(n)");
    assert!(
        diags
            .iter()
            .any(|d| d.starts_with("TPZ5001") && d.contains("interpolate")),
        "{diags:?}"
    );
    assert!(
        check("let r = { a: 1 }\nprint(r)")
            .iter()
            .any(|d| d.contains("interpolate")),
        "a non-string record should get the hint too"
    );
    // the interpolation form and a real string stay clean.
    assert_clean("let n = 42\nprint(\"{n}\")");
    assert_clean("print(\"hi\")");
}

// ---- TPZ5006 graduation: no such field -------------------------------

#[test]
fn record_field_access_is_static_tpz5006() {
    assert_code(
        "let user = { name: \"A\", age: 30 }\nprint(\"{user.email}\")",
        "TPZ5006",
    );
    assert_clean("let user = { name: \"A\", age: 30 }\nprint(\"{user.name}\")");
}

#[test]
fn template_exposes_only_tag_and_parts() {
    // §16: a `template` exposes exactly `.tag -> string` and `.parts ->
    // Array<string>`; every other member is a static TPZ5006 — the checker is now
    // the SOUND gate (the runtime faults a bogus member too, so accepting it
    // statically was unsound). `.tag`/`.parts` type correctly.
    assert_clean(
        "let x = 5\nlet q = sql\"a {x} b\"\nprint(q.tag)\nlet n = q.parts.length\nprint(\"{n}\")",
    );
    assert_clean("let x = 5\nlet q = sql\"a {x} b\"\nprint(q.parts[0])");
    assert_code("let q = sql\"x\"\nprint(\"{q.bogus}\")", "TPZ5006");
    // The interpolated VALUES are deliberately NOT reachable (§16) — sql/sh
    // injection safety — so even `.values` is a no-such-member error.
    assert_code("let q = sql\"x\"\nprint(\"{q.values}\")", "TPZ5006");
    // The bogus member must be rejected on the CALL path too (`q.x()` and the pipe
    // call stage `… |> q.x()`), not just plain access — else check accepts what the
    // runtime/native build faults.
    assert_code("let q = sql\"x\"\nq.bogus()", "TPZ5006");
    assert_code("let q = sql\"x\"\nq.values()", "TPZ5006");
    assert_code("let q = sql\"x\"\n0 |> q.bogus()", "TPZ5006");
    // A `template` is NOT a `string` (§16): using one where a string is required
    // is a static type error, not a silent coercion.
    assert_code("let q = sql\"x\"\nlet s: string = q\nprint(s)", "TPZ5001");
}

#[test]
fn record_update_checks_fields() {
    assert_code(
        "let user = { name: \"A\" }\nlet u2 = user { email: \"x\" }\nprint(\"{u2}\")",
        "TPZ5006",
    );
    assert_code(
        "let user = { name: \"A\" }\nlet u2 = user { name: 1 }\nprint(\"{u2}\")",
        "TPZ5001",
    );
    assert_clean("let user = { name: \"A\" }\nlet u2 = user { name: \"B\" }\nprint(\"{u2}\")");
}

#[test]
fn record_update_on_a_generic_or_non_record_is_strict() {
    // A record update on a rigid generic projects `RecordUpdateOf<T>`, so
    // it cannot silently discharge a concrete expectation.
    assert_code(
        "function steal<T>(t: T) -> int {\n    return t { a: 1 }\n}",
        "TPZ5001",
    );
    // A concrete non-record base faults at runtime ("record update needs a
    // record"), so `check` matches `run`.
    assert_code(
        "function f(n: int) -> () {\n    let _ = n { a: 1 }\n}",
        "TPZ5001",
    );
    assert_code(
        "function f(xs: Array<int>) -> () {\n    let _ = xs { a: 1 }\n}",
        "TPZ5001",
    );
    // The projected result leaks if the function publishes it unannotated.
    assert_code("function f<T>(t: T) {\n    return t { a: 1 }\n}", "TPZ5022");
    // A union with a rigid arm projects (rigid result), and a union with a
    // decidably non-record arm faults at runtime, so both reject.
    assert_code(
        "function steal<T>(t: T | { a: int }) -> int {\n    return t { a: 1 }\n}",
        "TPZ5001",
    );
    assert_code(
        "function steal(v: { a: int } | int) -> int {\n    return v { a: 1 }\n}",
        "TPZ5001",
    );
    // A union of records stays updatable.
    assert_clean(
        "function f(v: { a: int } | { a: int, b: int }) -> () {\n    let _ = v { a: 9 }\n}",
    );
}

#[test]
fn unknown_member_and_field_suggest_the_closest_name() {
    // A member/field typo hints the intended name in the TPZ5006 message, at
    // every NO_FIELD site (builtin member ACCESS; a callable member CALL; record
    // field access, functional update, and destructuring pattern).
    let hints = |src: &str| check(src).iter().any(|d| d.contains("did you mean"));
    assert!(
        hints("let xs: Array<int> = [1]\nlet y = xs.lenght"),
        "member"
    );
    // A member CALL hints a callable METHOD typo (`push`); a non-callable
    // property is NOT offered at a call position (C4) — covered in c6.rs.
    assert!(
        hints("let xs: Array<int> = [1]\nlet _ = xs.puhs(1)"),
        "member call"
    );
    assert!(
        hints("let r = { width: 4 }\nprint(\"{r.widht}\")"),
        "field access"
    );
    assert!(
        hints("let r = { width: 4 }\nlet r2 = r { widht: 9 }\nprint(\"{r2}\")"),
        "record update"
    );
    assert!(
        hints("let r = { width: 4 }\nlet { widht } = r"),
        "record pattern"
    );
    // and it names the intended member, not just "some" member.
    assert!(
        check("let xs: Array<int> = [1]\nlet y = xs.lenght")
            .iter()
            .any(|d| d.contains("did you mean `length`?")),
        "names the intended member"
    );
}

#[test]
fn unrelated_or_short_typos_get_no_suggestion() {
    // Still TPZ5006, but no misleading hint: an unrelated name, and a typo whose
    // closest member is only three characters (`set` must not be offered `get`).
    let no_hint = |src: &str| {
        let diags = check(src);
        assert!(
            diags.iter().any(|d| d.starts_with("TPZ5006")),
            "expected TPZ5006: {diags:?}"
        );
        assert!(
            !diags.iter().any(|d| d.contains("did you mean")),
            "unexpected hint: {diags:?}"
        );
    };
    no_hint("let xs: Array<int> = [1]\nlet y = xs.frobnicate");
    no_hint("let xs: Array<int> = [1]\nlet y = xs.set");
}

// ---- TPZ5007 graduation: incomparable values --------------------------

#[test]
fn incomparable_values_are_static_tpz5007() {
    assert_code("let x = 1 == \"one\"", "TPZ5007");
    assert_code("let x = 1 < \"two\"", "TPZ5007");
    assert_clean("let x = 1 == 2");
    assert_clean("let x = \"a\" < \"b\"");
}

#[test]
fn template_values_are_not_comparable() {
    assert_code(
        "let a = sql\"select 1\"\nlet b = sql\"select 1\"\nlet same = a == b",
        "TPZ5007",
    );
    assert_clean("let q = sql\"select {1}\"\nprint(\"{q}\")");
}

// ---- staged-silence guarantees ----------------------------------------

#[test]
fn unknown_forms_stay_silent() {
    // Calls, pipes, match results, `?`-propagation, coalesce: all
    // later phases; none may produce a diagnostic here.
    assert_clean("let n = toInt(\"42\") ?? 0\nprint(\"{n}\")");
    assert_clean(
        "function f(xs: Array<string>) -> Array<string> {\n    return map(xs, (s: string) => s + \"!\")\n}",
    );
    assert_clean(
        "let r = match 1 {\n    case 1 => \"one\"\n    case _ => \"other\"\n}\nprint(\"{r}\")",
    );
}

#[test]
fn duplicate_record_literal_field_is_tpz5022() {
    assert_code("let r = { x: 1, x: 2 }", "TPZ5022");
}

// ---- review-fold witnesses ---------------------------------------------

#[test]
fn literal_unions_are_usable_as_their_primitive() {
    assert_clean(
        "let s: \"a\" | \"b\" = \"a\"
let t = s + \"c\"
print(t)",
    );
    assert_clean(
        "let n: 1 | 2 = 1
let m = n + 1
print(\"{m}\")",
    );
}

#[test]
fn duplicate_record_update_last_wins() {
    assert_clean(
        "let r = { x: 1 }
let r2 = r { x: \"bad\", x: 2 }
print(\"{r2}\")",
    );
    assert_code(
        "let r = { x: 1 }
let r2 = r { x: 2, x: \"bad\" }
print(\"{r2}\")",
        "TPZ5001",
    );
}

#[test]
fn nested_type_aliases_are_lexically_scoped() {
    assert_clean(
        "function f() -> () {
    type Local = int
    let x: Local = 1
    print(\"{x}\")
}",
    );
    assert_code(
        "function f() -> () {
    type Local = int
    let x: Local = \"one\"
    print(\"{x}\")
}",
        "TPZ5001",
    );
}

#[test]
fn rem_is_int_only() {
    assert_code("let x = 1.5 % 2.0", "TPZ5001");
    assert_clean("let x = 5 % 2");
}

#[test]
fn map_membership_and_equality_are_rejected() {
    // The Map type must reach the operator concretely, so it comes
    // from an annotation (Map.new() itself types in C-3).
    assert_code(
        "let m: Map<string, int> = Map.new()
let b = \"k\" in m
print(\"{b}\")",
        "TPZ5001",
    );
    assert_code("let b = \"ab\" in \"abc\"", "TPZ5001");
    assert_code(
        "let a: Map<string, int> = Map.new()
let c: Map<string, int> = Map.new()
let same = a == c",
        "TPZ5007",
    );
}

#[test]
fn match_guards_must_be_bool() {
    // The guard must be concretely typed to diagnose: unbound
    // pattern names stay Unknown until pattern typing (C-4).
    assert_code(
        "let r = match 1 {
    case 1 if 2 + 3 => \"x\"
    case _ => \"y\"
}
print(r)",
        "TPZ5001",
    );
}

#[test]
fn alias_bodies_capture_their_definition_scope() {
    // The reviewer's counterexample: a use-site frame must not
    // re-bind names an outer alias body mentions.
    assert_code(
        "type B = string
type A = B
function f() -> () {
    type B = int
    let x: A = 1
    print(\"{x}\")
}",
        "TPZ5001",
    );
    // The local alias itself still shadows for local USES.
    assert_clean(
        "type B = string
function f() -> () {
    type B = int
    let x: B = 1
    print(\"{x}\")
}",
    );
}

// ---- corpus sweep: zero false positives at scale -----------------------

#[test]
fn every_positive_corpus_row_checks_clean() {
    let root = corpus_extract::repo_root();
    let generated = corpus_extract::generate(&root).expect("corpus generation");
    let mut checked = 0usize;
    let mut failures = Vec::new();
    for row in &generated.rows {
        if row.expect != "parse_ok" {
            continue;
        }
        let file = row.file.as_deref().expect("parse_ok rows carry files");
        // `concepts-modules/10.tpz` is a DELIBERATE "rejected" teaching
        // fragment (its source says so): `export let cache = compute()`
        // immediately calls a `function compute` declared textually
        // later, so the interpreter faults at load (`compute is not
        // bound`) and the §4 init-order check correctly rejects it
        // (TPZ5002). It parses, so it stays a `parse_ok` row, but it is
        // not a clean-checking PROGRAM — exclude it from the
        // no-false-positive sweep across all three locales.
        if file.ends_with("concepts-modules/10.tpz") {
            continue;
        }
        // ADR-106 corrects the historical checker bug that treated an
        // expression-position `for` as Unit. These two v5.1 examples ended a
        // Unit-returning function with that now-value-collecting expression;
        // the v5.6 canonical copies add an explicit `return ()`. Keep the
        // historical extraction byte-identical, but do not misclassify the
        // corrected TPZ5001 as a checker false positive.
        const HISTORICAL_FALSE_UNIT_FOR_TAILS: &[&str] = &[
            "corpus/v5.1/examples/049.tpz",
            "corpus/v5.1/examples/050.tpz",
        ];
        if HISTORICAL_FALSE_UNIT_FOR_TAILS
            .iter()
            .any(|s| file.ends_with(s))
        {
            continue;
        }
        // §22.2-frozen (C7): these v5.1 site fragments call string methods
        // (`contains`/`endsWith`/`split`) that the vendored docs self-label as
        // "illustrative placeholders, not canonical API". The v5.2 string surface
        // is frozen at `scalars()` only, so the checker correctly rejects these
        // (TPZ5006 — the interpreter faults them too); they are not clean-checking
        // v5.2 PROGRAMS. Excluded across all three locales. (The site corpus is
        // regenerated wholesale from the `.mdx`, so a skip-list — not an edit of
        // the generated `.tpz` — is the right mechanism.)
        const STRING_METHOD_PLACEHOLDERS: &[&str] = &[
            "concepts-control-flow/17.tpz",
            "concepts-control-flow/20.tpz",
            "concepts-error-handling/01.tpz",
            "concepts-error-handling/16.tpz",
            "concepts-functions-closures/18.tpz",
        ];
        if STRING_METHOD_PLACEHOLDERS.iter().any(|s| file.ends_with(s)) {
            continue;
        }
        // `concepts-error-handling/15.tpz` illustrates interactive input via a
        // hypothetical `input("prompt")`. The canonical §22 `input()` is ZERO-arg
        // (the host-provided text payload — the WASM editor's textarea, a native
        // binary's piped stdin); the prompt-bearing call is non-canonical doc
        // pseudo-code, like the string-method placeholders above. The site corpus
        // regenerates from the `.mdx`, so a skip-list — not an edit of the generated
        // `.tpz` — is the right mechanism. Excluded across all three locales.
        if file.ends_with("concepts-error-handling/15.tpz") {
            continue;
        }
        // `concepts-error-handling/02.tpz` wraps `Math.sqrt(value)` as if it
        // returned a bare `float` (`return Ok(Math.sqrt(value))`). Since the v5.4
        // `Math` builtin namespace landed, `Math.sqrt` returns `Result<float,
        // string>` (a NEGATIVE argument is a value-level `Err`, never NaN — §8), so
        // the canonical call is `Math.sqrt(value)` propagated, not re-wrapped in
        // `Ok(…)`. The fragment's float-returning assumption is now non-canonical doc
        // pseudo-code, exactly like the `input("prompt")` and string-method cases
        // above; the site corpus regenerates from the `.mdx`, so a skip-list — not an
        // edit of the generated `.tpz` — is the right mechanism. Excluded across all
        // three locales.
        if file.ends_with("concepts-error-handling/02.tpz") {
            continue;
        }
        let src = fs::read_to_string(root.join(file)).expect("committed corpus file");
        let out = parse_with_options(
            FileId(0),
            &src,
            ParseOptions {
                language_version: LangVersion::V5_2,
            },
        );
        if !out.diagnostics.is_empty() {
            continue; // parse gates own this
        }
        checked += 1;
        let result = check_program_with_version(&src, &out.program, LangVersion::V5_2);
        if !result.diagnostics.is_empty() {
            let summary: Vec<String> = result
                .diagnostics
                .iter()
                .map(|d| format!("{} {}", d.code.as_str(), d.message))
                .collect();
            failures.push(format!("{file}: {}", summary.join("; ")));
        }
    }
    // Floor lowered by the two historical false-Unit `for` tails above. The
    // remaining clean sweep must still retain at least 584 checked programs.
    assert!(checked >= 584, "corpus sweep shrank: {checked}");
    assert!(
        failures.is_empty(),
        "{} corpus rows produced checker diagnostics:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---- C2: member access / call on a union receiver -------------------

#[test]
fn union_member_rejects_a_closed_arm_that_lacks_it() {
    // A union member access/call rejects iff a member-CLOSED non-null arm lacks
    // the member (that arm would runtime-fault). Previously the union fell
    // through to staged `Unknown`, silently accepting `(Array|null).bogus`.
    assert_code(
        "function f(x: Array<int> | null) {\n    x.bogus\n}",
        "TPZ5006",
    );
    assert_code(
        "function f(x: Array<int> | null) {\n    let _ = x.bogus()\n}",
        "TPZ5006",
    );
    // Strict typing treats the `null` arm as member-closed, so even a real Array
    // member rejects on the plain path — the
    // value may be `null` and fault at runtime. `?.` (or `??` / a type pattern)
    // is the escape.
    assert_code(
        "function f(x: Array<int> | null) {\n    let _ = x.length\n}",
        "TPZ5006",
    );
    assert_clean("function f(x: Array<int> | null) {\n    let _ = x?.length\n}");
}

#[test]
fn union_member_rejects_when_any_decidable_arm_lacks_it() {
    // A non-null union: `int` is member-closed and exposes no methods, so the
    // call/access rejects even though `Array` has them — the `int` arm would
    // runtime-fault (the sound `ANY` strength).
    assert_code(
        "function f(x: Array<int> | int) {\n    let _ = x.bogus()\n}",
        "TPZ5006",
    );
    assert_code(
        "function f(x: Array<int> | int) {\n    x.bogus\n}",
        "TPZ5006",
    );
}

#[test]
fn union_with_a_string_arm_rejects_unknown_member() {
    // The v5.2 string surface is frozen at `scalars` (C7), so an unknown member
    // CALL or ACCESS through a string-armed union is a decidable absence on the
    // string arm — the union rejects (TPZ5006), matching the interpreter's fault.
    assert_code(
        "function f(x: string | null) {\n    let _ = x.bogus()\n}",
        "TPZ5006",
    );
    assert_code("function f(x: string | null) {\n    x.bogus\n}", "TPZ5006");
    // `scalars` IS the one real string member, but the `null` arm member-closes
    // the union, so even `scalars` rejects on the plain
    // path — the value may be `null`. `?.` is the escape.
    assert_code(
        "function f(x: string | null) {\n    let _ = x.scalars()\n}",
        "TPZ5006",
    );
    assert_clean("function f(x: string | null) {\n    let _ = x?.scalars()\n}");
}

#[test]
fn a_member_on_a_nullable_union_is_strict_with_escapes() {
    // Strict typing rejects a plain member on a `T | null` value because it may
    // be `null` and fault at runtime, exactly
    // like a plain member on `Option<X>`.
    assert_code(
        "function f(v: { x: int } | null) -> int {\n    return v.x\n}",
        "TPZ5006",
    );
    // The three escapes all type-check (and run cleanly): `?.`, `??`, type pattern.
    assert_clean("function f(v: { x: int } | null) -> int {\n    return v?.x ?? 0\n}");
    assert_clean(
        "function f(v: { x: int } | null) -> int {\n    let r = v ?? { x: 0 }\n    return r.x\n}",
    );
    assert_clean(
        "function f(v: { x: int } | null) -> int {\n    return match v {\n        case r: { x: int } => r.x\n        case _ => 0\n    }\n}",
    );
}

#[test]
fn constant_arithmetic_faults_are_static_errors() {
    // §2/§13a: a fault inside a constant expression is a STATIC error — the same
    // outcome `run`/`build` reject through `const_guarded`, not deferred to
    // runtime. Previously `topaz check` reported types-ok for all of these.
    assert_code("const BAD = 1 / 0", "TPZ5001");
    assert_code("const X = 5 % 0", "TPZ5001");
    assert_code("const X = 9223372036854775807 + 1", "TPZ5001");
    assert_code("const X = 2 ** -1", "TPZ5001");
    assert_code("const X = 10 ** 100", "TPZ5001");
    // Valid constant arithmetic, and a non-constant operand, stay clean.
    assert_clean("const OK = 2 + 3 * 4");
    assert_clean("function f(n: int) -> int {\n    return n / 0\n}");
    // A short-circuit operator is not a constant expression, so folding does NOT
    // descend into it — no false inner-arithmetic error (matching runtime, which
    // instead rejects the whole initializer as non-constant).
    assert_clean("const X = (1 / 0 == 0) && true");
    // A bare comparison's operands ARE evaluated, so an inner fault still reports.
    assert_code("const X = 1 / 0 == 0", "TPZ5001");
}

#[test]
fn compose_requires_unary_operands_on_both_sides() {
    // §11: `>>` composes UNARY functions. A multi-argument operand on EITHER side
    // is a static error (TPZ5004), not silently typed as a multi-arg function.
    // The right side was already enforced; this pins the mirrored left-side check.
    assert_code(
        "let addPair = (a: int, b: int) => a + b\nlet inc = (x: int) => x + 1\nlet f = addPair >> inc",
        "TPZ5004",
    );
    assert_code(
        "let inc = (x: int) => x + 1\nlet addPair = (a: int, b: int) => a + b\nlet f = inc >> addPair",
        "TPZ5004",
    );
    // Unary on both sides composes cleanly.
    assert_clean(
        "let inc = (x: int) => x + 1\nlet dbl = (x: int) => x * 2\nlet f = inc >> dbl\nf(5)",
    );
    // A variadic operand is multi-argument too (one fixed param is not enough).
    assert_code(
        "function head(x: int, ...xs: int) -> int {\n    return x\n}\nlet inc = (y: int) => y + 1\nlet f = head >> inc",
        "TPZ5004",
    );
    assert_code(
        "function head(x: int, ...xs: int) -> int {\n    return x\n}\nlet inc = (y: int) => y + 1\nlet f = inc >> head",
        "TPZ5004",
    );
}
