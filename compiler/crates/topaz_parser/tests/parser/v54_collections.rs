use super::*;

// ---- §6 (v5.4) contextual set/map literals ---------------------------

#[test]
fn set_literal_is_recognized_contextually() {
    let e = one_expr_v54("set { 1, 2, 3 }");
    let ExprKind::SetLiteral(elems) = e.kind else {
        panic!("expected a set literal, got {e:?}");
    };
    assert_eq!(elems.len(), 3);
}

#[test]
fn empty_set_literal_parses() {
    let e = one_expr_v54("set {}");
    let ExprKind::SetLiteral(elems) = e.kind else {
        panic!("expected a set literal, got {e:?}");
    };
    assert!(elems.is_empty());
}

#[test]
fn set_literal_allows_trailing_comma() {
    let e = one_expr_v54("set { 1, 2, }");
    let ExprKind::SetLiteral(elems) = e.kind else {
        panic!("expected a set literal, got {e:?}");
    };
    assert_eq!(elems.len(), 2);
}

#[test]
fn map_literal_is_recognized_contextually() {
    let e = one_expr_v54("map { \"a\": 1, \"b\": 2 }");
    let ExprKind::MapLiteral(entries) = e.kind else {
        panic!("expected a map literal, got {e:?}");
    };
    assert_eq!(entries.len(), 2);
}

#[test]
fn empty_map_literal_parses() {
    let e = one_expr_v54("map {}");
    let ExprKind::MapLiteral(entries) = e.kind else {
        panic!("expected a map literal, got {e:?}");
    };
    assert!(entries.is_empty());
}

#[test]
fn map_literal_allows_trailing_comma() {
    let e = one_expr_v54("map { 1: 10, 2: 20, }");
    let ExprKind::MapLiteral(entries) = e.kind else {
        panic!("expected a map literal, got {e:?}");
    };
    assert_eq!(entries.len(), 2);
}

#[test]
fn map_free_function_call_is_not_a_literal() {
    // `map(xs, f)` — the §22 HOF. `map` is followed by `(`, NOT `{`, so the
    // contextual rule does not fire and `map` stays an ordinary identifier callee.
    let e = one_expr_v54("map(xs, f)");
    let ExprKind::Call { callee, args, .. } = e.kind else {
        panic!("expected a call, got {e:?}");
    };
    assert!(matches!(callee.kind, ExprKind::Ident));
    assert_eq!(args.len(), 2);
}

#[test]
fn set_and_map_stay_identifiers_when_not_before_brace() {
    // `let map = 1` then `map + 1`: `map` is a plain identifier (binding + use).
    let v = let_value("let map = 1");
    assert!(matches!(v.kind, ExprKind::Int));
    let e = one_expr_v54("map + 1");
    let ExprKind::Binary { lhs, .. } = e.kind else {
        panic!("expected a binary expression, got {e:?}");
    };
    assert!(matches!(lhs.kind, ExprKind::Ident));

    // `set` used as a bare identifier expression.
    let e = one_expr_v54("set");
    assert!(matches!(e.kind, ExprKind::Ident));
}

// ---- §6.4 (v5.4) comprehensions -------------------------------------

#[test]
fn array_comprehension_is_recognized_by_leading_for() {
    let e = one_expr_v54("[ for x in xs => x * x ]");
    let ExprKind::Comprehension {
        kind,
        clauses,
        body,
    } = e.kind
    else {
        panic!("expected an array comprehension, got {e:?}");
    };
    assert_eq!(kind, CompKind::Array);
    assert_eq!(clauses.len(), 1);
    assert!(matches!(clauses[0], CompClause::For { .. }));
    assert!(matches!(body.as_ref(), CompBody::Elem(_)));
}

#[test]
fn array_literal_without_leading_for_is_still_a_literal() {
    // The disambiguation is a LEADING `for` — a normal `[ … ]` stays an array literal.
    let e = one_expr_v54("[1, 2, 3]");
    assert!(matches!(e.kind, ExprKind::Array(_)));
}

#[test]
fn filtered_and_nested_array_comprehension_clauses_parse() {
    let e = one_expr_v54("[ for x in xs for y in ys if x != y => x * y ]");
    let ExprKind::Comprehension { clauses, .. } = e.kind else {
        panic!("expected a comprehension, got {e:?}");
    };
    // Two `for` clauses then one `if` clause, in source order.
    assert_eq!(clauses.len(), 3);
    assert!(matches!(clauses[0], CompClause::For { .. }));
    assert!(matches!(clauses[1], CompClause::For { .. }));
    assert!(matches!(clauses[2], CompClause::If(_)));
}

#[test]
fn set_comprehension_is_recognized_by_leading_for() {
    let e = one_expr_v54("set { for x in xs => x }");
    let ExprKind::Comprehension { kind, body, .. } = e.kind else {
        panic!("expected a set comprehension, got {e:?}");
    };
    assert_eq!(kind, CompKind::Set);
    assert!(matches!(body.as_ref(), CompBody::Elem(_)));
}

#[test]
fn set_literal_without_leading_for_is_still_a_literal() {
    let e = one_expr_v54("set { 1, 2 }");
    assert!(matches!(e.kind, ExprKind::SetLiteral(_)));
}

#[test]
fn map_comprehension_is_recognized_by_leading_for_and_keeps_key_value_body() {
    let e = one_expr_v54("map { for u in users => u.id: u }");
    let ExprKind::Comprehension { kind, body, .. } = e.kind else {
        panic!("expected a map comprehension, got {e:?}");
    };
    assert_eq!(kind, CompKind::Map);
    assert!(matches!(body.as_ref(), CompBody::Entry { .. }));
}

#[test]
fn map_literal_without_leading_for_is_still_a_literal() {
    let e = one_expr_v54("map { 1: 2 }");
    assert!(matches!(e.kind, ExprKind::MapLiteral(_)));
}

#[test]
fn comprehension_for_iter_does_not_swallow_the_clause_arrow_as_a_lambda() {
    // `for x in xs => body`: the iter `xs` must NOT parse `xs => body` as a naked
    // lambda — the `=>` ends the clause list (the body is `x`).
    let e = one_expr_v54("[ for x in xs => x ]");
    let ExprKind::Comprehension { clauses, body, .. } = e.kind else {
        panic!("expected a comprehension, got {e:?}");
    };
    assert_eq!(clauses.len(), 1);
    let CompClause::For { iter, .. } = &clauses[0] else {
        panic!("expected a for clause");
    };
    assert!(matches!(iter.kind, ExprKind::Ident), "iter must be `xs`");
    assert!(matches!(body.as_ref(), CompBody::Elem(_)));
}

#[test]
fn comprehensions_are_v54_only() {
    // At v5.3 a leading `for` in `[ … ]` is not a comprehension (the array literal
    // grammar is unchanged), so it does not parse as `ExprKind::Comprehension`.
    let out = parse_with_options(
        FileId(0),
        "[ for x in xs => x ]",
        ParseOptions {
            language_version: LangVersion::V5_3,
        },
    );
    let has_comp = out.program.items.iter().any(|item| {
        matches!(
            &item.kind,
            StmtKind::Expr(e) if matches!(e.kind, ExprKind::Comprehension { .. })
        )
    });
    assert!(!has_comp, "v5.3 must not parse a comprehension");
}

#[test]
fn set_and_map_literals_are_v54_only() {
    // At v5.3 `set`/`map` are ordinary identifiers, so `set { … }` / `map { … }`
    // is `<ident> <block/record>` and does NOT parse as a literal — the identifier
    // is a bare statement and the brace begins a new construct. Recognizing the
    // literal only at v5.4 keeps the older grammar untouched.
    for src in ["set { 1, 2 }", "map { 1: 2 }"] {
        let out = parse_with_options(
            FileId(0),
            src,
            ParseOptions {
                language_version: LangVersion::V5_3,
            },
        );
        let has_literal = out.program.items.iter().any(|item| {
            matches!(
                &item.kind,
                StmtKind::Expr(e)
                    if matches!(e.kind, ExprKind::SetLiteral(_) | ExprKind::MapLiteral(_))
            )
        });
        assert!(!has_literal, "v5.3 must not parse `{src}` as a literal");
    }
}

#[test]
fn record_named_map_or_set_is_reserved_at_v54() {
    // The contextual `map { … }` / `set { … }` literal would shadow brace
    // CONSTRUCTION of a record named `map`/`set`, so the DECLARATION is rejected
    // with a clear TPZ2012 at v5.4.
    for src in ["record map { x: int }", "record set { x: int }"] {
        let out = parse_with_options(
            FileId(0),
            src,
            ParseOptions {
                language_version: LangVersion::V5_4,
            },
        );
        assert!(
            out.diagnostics.iter().any(|d| d.code.as_str() == "TPZ2012"),
            "v5.4 must reserve `{src}`, got {:?}",
            out.diagnostics
        );
    }
}

#[test]
fn record_named_map_or_set_reservation_is_v54_only() {
    // The reservation is a v5.4 concern: records (and the literal that motivates
    // the reservation) are v5.4-only, so v5.3 never produces the TPZ2012
    // reserved-name diagnostic for `record map`/`record set`.
    for src in ["record map { x: int }", "record set { x: int }"] {
        let out = parse_with_options(
            FileId(0),
            src,
            ParseOptions {
                language_version: LangVersion::V5_3,
            },
        );
        assert!(
            !out.diagnostics.iter().any(|d| d.code.as_str() == "TPZ2012"),
            "v5.3 must not raise the reserved-name diagnostic for `{src}`, got {:?}",
            out.diagnostics
        );
    }
}

#[test]
fn if_let_and_while_let_are_v54_only() {
    // At v5.3 `let` after `if`/`while` is NOT special — the parser reports
    // "expected an expression" (TPZ2001) where the `let` keyword appears, leaving
    // the older grammar untouched.
    for src in [
        "if let Some(n) = opt { n } else { 0 }",
        "while let Some(n) = opt { n }",
    ] {
        let out = parse_with_options(
            FileId(0),
            src,
            ParseOptions {
                language_version: LangVersion::V5_3,
            },
        );
        assert!(
            out.diagnostics.iter().any(|d| d.code.as_str() == "TPZ2001"),
            "v5.3 must reject `{src}`, got {:?}",
            out.diagnostics
        );
    }
}

#[test]
fn construct_owned_braces_are_not_postfix_record_updates() {
    for src in [
        "if ready {}",
        "while ready {}",
        "for item in items {}",
        "if let Some(item) = current {}",
        "while let Some(item) = current {}",
        "using file = resource {}",
    ] {
        let out = parse_with_options(
            FileId(0),
            src,
            ParseOptions {
                language_version: LangVersion::CURRENT,
            },
        );
        assert!(
            out.diagnostics.is_empty(),
            "construct body must own the brace in {src:?}: {:?}",
            out.diagnostics
        );
    }

    let matched = parse_with_options(
        FileId(0),
        "match value {}",
        ParseOptions {
            language_version: LangVersion::CURRENT,
        },
    );
    assert_eq!(matched.diagnostics.len(), 1, "{:?}", matched.diagnostics);
    assert_eq!(
        matched.diagnostics[0].message,
        "`match` requires at least one `case`"
    );
}

#[test]
fn construct_head_delimiters_reallow_nested_record_updates() {
    let src = concat!(
        "let value = { active: false }\n",
        "if (value { active: true }).active {}\n",
        "if choose(value { active: true }) {}\n",
    );
    let out = parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: LangVersion::CURRENT,
        },
    );
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn empty_nominal_form_requires_a_bare_identifier() {
    let accepted = parse_with_options(
        FileId(0),
        "let value = Record {}",
        ParseOptions {
            language_version: LangVersion::CURRENT,
        },
    );
    assert!(
        accepted.diagnostics.is_empty(),
        "{:?}",
        accepted.diagnostics
    );

    let rejected = parse_with_options(
        FileId(0),
        "let value = make() {}",
        ParseOptions {
            language_version: LangVersion::CURRENT,
        },
    );
    assert!(
        rejected
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "expected a statement separator"),
        "call results must not become empty nominal forms: {:?}",
        rejected.diagnostics
    );
}

#[test]
fn using_resource_block_is_v54_contextual_statement() {
    let src = "using file = open(\"input.txt\")? { file.read() }";
    let out = parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: LangVersion::V5_4,
        },
    );
    assert!(
        out.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        out.diagnostics
    );
    assert_eq!(out.program.items.len(), 1);
    let StmtKind::Using { name, value, body } = &out.program.items[0].kind else {
        panic!(
            "expected using statement, got {:?}",
            out.program.items[0].kind
        );
    };
    assert_eq!(&src[name.span.lo as usize..name.span.hi as usize], "file");
    assert!(matches!(value.kind, ExprKind::Try(_)));
    assert!(body.tail.is_some(), "using body keeps its block tail");

    let old = parse_with_options(
        FileId(0),
        "using file = open(\"input.txt\")? { file.read() }",
        ParseOptions {
            language_version: LangVersion::V5_3,
        },
    );
    assert!(
        old.diagnostics.iter().any(|d| d.code.as_str() == "TPZ2001"),
        "v5.3 must not recognize using syntax: {:?}",
        old.diagnostics
    );
}

#[test]
fn loop_expression_is_v54_contextual_and_preserves_identifier_history() {
    let src = "let answer = loop 'search { break 'search 42 }\nanswer";
    let out = parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: LangVersion::V5_4,
        },
    );
    assert!(
        out.diagnostics.is_empty(),
        "unexpected v5.4 loop diagnostics: {:?}",
        out.diagnostics
    );
    let StmtKind::Let { value, .. } = &out.program.items[0].kind else {
        panic!("expected loop-valued let")
    };
    assert!(matches!(value.kind, ExprKind::Loop { label: Some(_), .. }));

    for version in [
        LangVersion::V5_1,
        LangVersion::V5_2,
        LangVersion::V5_3,
        LangVersion::V5_4,
        LangVersion::V5_5,
    ] {
        let ident = parse_with_options(
            FileId(0),
            "let loop = 3\nloop + 1",
            ParseOptions {
                language_version: version,
            },
        );
        assert!(
            ident.diagnostics.is_empty(),
            "loop must remain an identifier in {version:?}: {:?}",
            ident.diagnostics
        );
    }

    for version in [LangVersion::V5_1, LangVersion::V5_2, LangVersion::V5_3] {
        let old = parse_with_options(
            FileId(0),
            "let answer = loop { break 42 }",
            ParseOptions {
                language_version: version,
            },
        );
        assert!(
            old.diagnostics.iter().any(|d| d.code.as_str() == "TPZ2001"),
            "{version:?} must reject the loop expression: {:?}",
            old.diagnostics
        );
    }
}
