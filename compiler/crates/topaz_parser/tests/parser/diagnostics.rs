use super::*;

// ---- diagnostics --------------------------------------------------------

#[test]
fn unknown_template_tag_diagnoses() {
    assert_eq!(codes("let x = foo\"body\""), ["TPZ2002"]);
    // Registry tags pass.
    parse_ok("let p = p\"a/b\"\nlet r = r\"[a-z]+\"\nlet s = sh\"ls\"");
}

#[test]
fn invalid_assignment_target_diagnoses() {
    assert_eq!(codes("f() = 3"), ["TPZ2003"]);
    parse_ok("a.b = 3\nxs[0] = 4");
}

#[test]
fn invalid_defer_body_diagnoses() {
    assert_eq!(codes("defer 5"), ["TPZ2004"]);
}

#[test]
fn concurrent_form_mismatches_diagnose() {
    assert_eq!(
        codes("concurrent(timeout: 3s) {\n    a: f()\n}"),
        ["TPZ2005"]
    );
    assert_eq!(
        codes("concurrent {\n    a: f()\n}\nelse { 0 }"),
        ["TPZ2005"]
    );
}

#[test]
fn malformed_concurrent_timeout_recovers_without_cascade() {
    // A malformed timeout clause must yield one clear diagnostic, not a
    // cascade. The parser recovers (a non-duration value defaults to seconds; a
    // missing `:` is reported but not fatal) so the `)` and the block still parse.
    // With a valid `else`, the timeout problem is the ONLY diagnostic.
    assert_eq!(
        codes("let x = concurrent(timeout: 3) {\n    a: f()\n} else { 0 }"),
        ["TPZ2001"]
    );
    assert_eq!(
        codes("let x = concurrent(timeout 3s) {\n    a: f()\n} else { 0 }"),
        ["TPZ2001"]
    );
    // The recovery is DELIMITER-AWARE: a bad value that opens a balanced form
    // (`(3)`, `[3]`, `{…}`) or spans multiple tokens (`3 + 4`) is skipped up to
    // the clause `)` — never desyncing into a multi-diagnostic cascade.
    for bad in [
        "concurrent(timeout: (3)) { a: f() } else { 0 }",
        "concurrent(timeout: [3]) { a: f() } else { 0 }",
        "concurrent(timeout: { a: f() }) { b: g() } else { 0 }",
        "concurrent(timeout: 3 + 4) { a: f() } else { 0 }",
        "concurrent(timeout: ) { a: f() } else { 0 }",
    ] {
        assert_eq!(
            codes(&format!("let x = {bad}")),
            ["TPZ2001"],
            "input: {bad}"
        );
    }
}

#[test]
fn explicit_type_arguments_get_a_hint_without_a_tuple_cascade() {
    // `Array.of<int>(1, 2)` parses as `(Array.of < int) > (1, 2)`; the
    // `(1, 2)` comma list is not Topaz syntax. Report it ONCE (no cascade) and —
    // because the `(` follows a `callee<type-list>` window — attach a guiding note.
    assert_eq!(codes("let xs = Array.of<int>(1, 2)\nxs"), ["TPZ2001"]);
    let out = parse(FileId(0), "let xs = Array.of<int>(1, 2)\nxs");
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.notes.iter().any(|n| n.contains("infers type arguments"))),
        "want a type-argument note: {:?}",
        out.diagnostics
    );
    // A plain `(a, b)` gets the SAME primary diagnostic but NO type-args note.
    let plain = parse(FileId(0), "let p = (1, 2)\np");
    assert_eq!(
        plain
            .diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>(),
        ["TPZ2001"]
    );
    assert!(
        plain.diagnostics.iter().all(|d| d.notes.is_empty()),
        "a plain tuple must not get the type-args note: {:?}",
        plain.diagnostics
    );
    // A valid comparison `a < b > (c)` is untouched — no diagnostic at all.
    parse_ok("let a = 1\nlet b = 2\nlet c = 3\nlet r = a < b > (c)\nr");
}

#[test]
fn misplaced_mut_suggests_let_mut() {
    // `mut` is only valid immediately after `let`. Misplacement at
    // statement start (`mut let x`) or after the name (`let x mut`) guides to
    // `let mut` instead of a bare "expected an expression" / "expected `=`".
    for src in ["mut let x = 1\nx", "let x mut = 1\nx"] {
        let out = parse(FileId(0), src);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.message.contains("let mut")),
            "want a `let mut` hint for {src:?}: {:?}",
            out.diagnostics
        );
    }
    // The correct form is unaffected.
    parse_ok("let mut x = 1\nx = 2\nx");
}

#[test]
fn missing_range_endpoint_points_at_the_dots() {
    // An absent endpoint after `..` is reported once with a specific
    // message, and a synthetic endpoint stops `(1..)` / `[1..]` from cascading.
    for src in [
        "let r = 1..",
        "let r = (1..)\nr",
        "let xs = [1..]\nxs",
        "let r = 1.. by 2\nr",
        // a `..` followed by the case arrow (`=>`) must stop too, not cascade.
        "match x {\n  case n if n in 1.. => 0\n  case _ => 1\n}",
    ] {
        let out = parse(FileId(0), src);
        assert_eq!(
            out.diagnostics.len(),
            1,
            "want exactly one diagnostic for {src:?}: {:?}",
            out.diagnostics
        );
        assert!(
            out.diagnostics[0].message.contains("range endpoint"),
            "want a range-endpoint message for {src:?}: {:?}",
            out.diagnostics
        );
    }
    // Complete ranges (inclusive, exclusive, with `by`) are unaffected.
    parse_ok("let a = 1..5\nlet b = 0..<10\nlet c = 1..10 by 2\na");
    // Expression-starting endpoints stay valid (`if`/`match`/`true` begin an
    // expression, so they are NOT treated as a missing endpoint).
    parse_ok("let a = 1..if true { 5 } else { 3 }\na");
}

#[test]
fn incomplete_range_before_a_statement_keyword_is_diagnosed_at_the_dots() {
    // SPEC §1a: `..` is a binary trailing-continuation operator (like
    // `+`/`*`/…), so `1..\nlet x = 2` lexes as `1 .. let` with no separator between
    // the two statements. The improvement is the PRIMARY diagnostic: it points at
    // the `..` with the range-endpoint hint instead of a bare "expected an
    // expression" pointing at `let`. A secondary statement-separator diagnostic
    // follows from the absent separator — exactly as for any operator's newline
    // continuation; recovering the trailing statement would need general recovery
    // machinery, out of scope here.
    let out = parse(FileId(0), "let r = 1..\nlet x = 2\nr");
    assert!(
        out.diagnostics[0].message.contains("range endpoint"),
        "the primary diagnostic should be the range-endpoint hint, not a vague one: {:?}",
        out.diagnostics
    );
}

#[test]
fn case_guards_do_not_swallow_the_case_arrow() {
    // `Guard ::= "if" Expression` is ambiguous against the case
    // arrow when the guard ends in an identifier: `day in weekend =>`
    // must read `=>` as the case arrow, not a lambda. Grouping
    // delimiters re-allow lambdas inside guards.
    let e = one_expr("match d {\n    case day if day in weekend => 1\n    case _ => 0\n}");
    let ExprKind::Match { cases, .. } = e.kind else {
        panic!("expected match");
    };
    assert!(cases[0].guard.is_some());

    parse_ok("match xs {\n    case s if s.all(x => x >= 80) => 1\n    case _ => 0\n}");
    parse_ok("match xs {\n    case s if f([y => y]) => 1\n    case _ => 0\n}");
    // Lambdas outside guards are unaffected.
    parse_ok("let f = x => x + 1");

    // ERR-001 shape pin: `y => y => 1` at guard root yields guard
    // `y` (never a lambda) with the first arrow owning the clause;
    // the body may then be a lambda.
    let e = one_expr("match d {\n    case x if y => y => 1\n    case _ => 0\n}");
    let ExprKind::Match { cases, .. } = e.kind else {
        panic!("expected match");
    };
    assert!(matches!(
        cases[0].guard.as_ref().expect("guard").kind,
        ExprKind::Ident
    ));
    assert!(matches!(
        cases[0].body,
        CaseArmBody::Expr(Expr {
            kind: ExprKind::Lambda { .. },
            ..
        })
    ));

    // Grouping re-allows a lambda at guard root (typing is
    // checker-era; the parse is valid).
    parse_ok("match x {\n    case y if (a => a) => 1\n    case _ => 0\n}");

    // A nested match owns its case braces, so its arm body does not inherit
    // the outer guard's naked-lambda restriction.
    parse_ok(concat!(
        "match d {\n",
        "  case outer if match 1 { case inner => item => true } => outer\n",
        "  case _ => 0\n",
        "}",
    ));
}

#[test]
fn duration_literals_only_in_concurrent_timeout() {
    // SPEC §15: a duration literal is not a §1 literal; it exists
    // only in the timeout clause.
    assert_eq!(codes("let x = 3s"), ["TPZ2001"]);
    parse_ok("let d = concurrent(timeout: 250ms) {\n    a: f()\n} else { 0 }");
}

#[test]
fn recovery_reports_then_continues() {
    let out = parse(FileId(0), "let a = ; let b = 2");
    assert!(!out.diagnostics.is_empty());
    assert_eq!(out.program.items.len(), 1);
    assert!(matches!(out.program.items[0].kind, StmtKind::Let { .. }));
}

#[test]
fn module_syntax_fails_by_absence() {
    // ADR-071: `import` lexes as an identifier; the statement then
    // fails in the grammar, not the lexer.
    let out = parse(FileId(0), "import os");
    assert!(!out.diagnostics.is_empty());
}

#[test]
fn full_program_smoke() {
    parse_ok(
        "function 인사하기(이름: string) -> string {\n    return \"안녕하세요, {이름}님!\"\n}\n\nlet 결과 = [\"세계\", \"topaz\"]\n    |> map(인사하기)\n    |> .length\n\nmatch 결과 {\n    case 0 => print(\"empty\")\n    case _ => print(\"ok\")\n}",
    );
}
