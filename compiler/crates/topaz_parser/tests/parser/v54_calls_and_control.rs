use super::*;

// ---- §3 (v5.4) explicit call-site type arguments ---------------------

#[test]
fn nominal_record_spread_is_one_leading_v54_element() {
    let e = one_expr_v54("Box { ...base, }");
    let ExprKind::RecordUpdate {
        base,
        spread,
        fields,
    } = e.kind
    else {
        panic!("expected a nominal record spread-update, got {e:?}");
    };
    assert!(matches!(base.kind, ExprKind::Ident));
    assert!(spread.is_some());
    assert!(fields.is_empty());

    for src in [
        "Box { ...base, ...other }",
        "Box { value: 1, ...base }",
        "Box<int> { ...base }",
        "schema.Box { ...base }",
    ] {
        let out = parse_with_options(
            FileId(0),
            src,
            ParseOptions {
                language_version: LangVersion::V5_4,
            },
        );
        assert!(
            out.diagnostics
                .iter()
                .any(|diag| diag.code.as_str() == "TPZ2001"),
            "{src:?} must reject with TPZ2001: {:?}",
            out.diagnostics
        );
    }
}

#[test]
fn nominal_record_spread_is_v54_and_later_only() {
    let src = "Box { ...base, }";
    for version in [LangVersion::V5_1, LangVersion::V5_2, LangVersion::V5_3] {
        let out = parse_with_options(
            FileId(0),
            src,
            ParseOptions {
                language_version: version,
            },
        );
        assert!(
            out.diagnostics
                .iter()
                .any(|diag| diag.code.as_str() == "TPZ2001"),
            "{version:?} must reject nominal record spread: {:?}",
            out.diagnostics
        );
    }
    parse_ok_v54(src);
}

#[test]
fn nominal_record_pattern_is_nonempty_and_v54_only() {
    let e = one_expr_v54("match user { case User { name, age: 36, } => name }");
    let ExprKind::Match { cases, .. } = e.kind else {
        panic!("expected a match expression, got {e:?}");
    };
    let PatternKind::NominalRecord { name, fields } = &cases[0].pattern.kind else {
        panic!("expected a nominal record pattern");
    };
    assert_eq!(fields.len(), 2);
    assert!(fields[0].pattern.is_none());
    assert!(fields[1].pattern.is_some());
    assert_eq!(name.span.hi - name.span.lo, 4);

    let empty = parse_with_options(
        FileId(0),
        "match user { case User {} => 1 case _ => 0 }",
        ParseOptions {
            language_version: LangVersion::V5_4,
        },
    );
    assert!(
        empty.diagnostics.iter().any(|diag| {
            diag.code.as_str() == "TPZ2001"
                && diag
                    .message
                    .contains("a record pattern requires at least one field")
        }),
        "empty nominal record pattern must reject: {:?}",
        empty.diagnostics
    );

    let src = "match user { case User { name } => name }";
    for version in [LangVersion::V5_1, LangVersion::V5_2, LangVersion::V5_3] {
        let out = parse_with_options(
            FileId(0),
            src,
            ParseOptions {
                language_version: version,
            },
        );
        assert!(
            out.diagnostics
                .iter()
                .any(|diag| diag.code.as_str() == "TPZ2001"),
            "{version:?} must reject nominal record patterns: {:?}",
            out.diagnostics
        );
    }
}

#[test]
fn call_type_args_bare_function() {
    // `f<int>(x)`: the callee is an Ident, the `>` is adjacent to `(`.
    let e = one_expr_v54("first<int>(xs)");
    let ExprKind::Call {
        callee, type_args, ..
    } = e.kind
    else {
        panic!("expected a call, got {e:?}");
    };
    assert!(matches!(callee.kind, ExprKind::Ident));
    assert_eq!(type_args.len(), 1);
    assert!(matches!(type_args[0].kind, TypeKind::Named { .. }));
}

#[test]
fn call_type_args_static_member() {
    // `Map.new<string, int>()`: a Member callee, a two-element type list.
    let e = one_expr_v54("Map.new<string, int>()");
    let ExprKind::Call {
        callee, type_args, ..
    } = e.kind
    else {
        panic!("expected a call, got {e:?}");
    };
    assert!(matches!(callee.kind, ExprKind::Member { .. }));
    assert_eq!(type_args.len(), 2);
}

#[test]
fn call_type_args_optional_member_reach_the_checker() {
    let e = one_expr_v54("xs?.get<int>(0)");
    let ExprKind::Call {
        callee, type_args, ..
    } = e.kind
    else {
        panic!("expected a call, got {e:?}");
    };
    assert!(matches!(callee.kind, ExprKind::OptionalAccess { .. }));
    assert_eq!(type_args.len(), 1);
}

#[test]
fn call_type_args_nested_gtgt_split() {
    // `f<Array<int>>(xs)`: the closing `>>` splits — the type list parses,
    // and its second `>` is adjacent to `(`.
    let e = one_expr_v54("f<Array<int>>(xs)");
    let ExprKind::Call { type_args, .. } = e.kind else {
        panic!("expected a call, got {e:?}");
    };
    assert_eq!(type_args.len(), 1);
    let TypeKind::Named { args, .. } = &type_args[0].kind else {
        panic!("expected a named type");
    };
    assert_eq!(args.len(), 1, "Array<int> carries one type argument");
}

#[test]
fn comparison_with_space_before_paren_is_not_type_args() {
    // `x < int > (y)`: a SPACE before `(` means the closing `>` is not
    // adjacent — this stays a comparison, never an explicit type-arg call.
    let e = one_expr_v54("x < int > (y)");
    assert!(
        matches!(
            e.kind,
            ExprKind::Binary {
                op: BinaryOp::Gt,
                ..
            }
        ),
        "expected a `>` comparison, got {e:?}"
    );
}

#[test]
fn plain_comparison_still_parses() {
    // A bare `a < b` with no following `(` is untouched by the scan.
    let e = one_expr_v54("a < b");
    assert!(matches!(
        e.kind,
        ExprKind::Binary {
            op: BinaryOp::Lt,
            ..
        }
    ));
}

#[test]
fn call_type_args_are_v54_only() {
    // At v5.3 the SAME adjacent `f<int>(x)` is NOT a type-arg call: the
    // postfix arm is gated `>= V5_4`, so the `<` is a comparison operator.
    let out = parse_with_options(
        FileId(0),
        "f<int>(x)",
        ParseOptions {
            language_version: LangVersion::V5_3,
        },
    );
    let StmtKind::Expr(e) = &out.program.items[0].kind else {
        panic!("expected an expression statement");
    };
    // `(f < int) > (x)` — the outermost node is the `>` comparison, with NO
    // type_args on any Call. The v5.4 form would be a single `Call`.
    assert!(
        matches!(
            e.kind,
            ExprKind::Binary {
                op: BinaryOp::Gt,
                ..
            }
        ),
        "v5.3 must parse `f<int>(x)` as a comparison, got {e:?}"
    );
}

// ---- §7.4 (v5.4) `if let` / `while let` — PARSER DESUGAR to `match` ----------

#[test]
fn if_let_desugars_to_a_two_arm_match() {
    // `if let P = E { T } else { F }` → `match E { case P => T\n case _ => F }`:
    // the parser emits a `Match` whose first arm is the user pattern and whose
    // second arm is a wildcard taking the `else` value. No new AST node.
    let e = one_expr_v54("if let Some(n) = opt { n } else { 0 }");
    let ExprKind::Match { cases, .. } = &e.kind else {
        panic!("expected a desugared match, got {e:?}");
    };
    assert_eq!(cases.len(), 2, "if-let desugars to exactly two arms");
    assert!(
        matches!(cases[0].pattern.kind, PatternKind::Constructor { .. }),
        "first arm carries the user pattern"
    );
    assert!(
        matches!(cases[1].pattern.kind, PatternKind::Wildcard),
        "second arm is the wildcard else"
    );
    assert!(cases[0].guard.is_none() && cases[1].guard.is_none());
}

#[test]
fn if_let_without_else_uses_a_unit_else_arm() {
    // An else-less `if let` mirrors a plain else-less `if`: the wildcard arm yields
    // `()` (Unit), so a statement-position `if let` has no value to mismatch.
    let e = one_expr_v54("if let Some(n) = opt { n }");
    let ExprKind::Match { cases, .. } = &e.kind else {
        panic!("expected a desugared match, got {e:?}");
    };
    assert_eq!(cases.len(), 2);
    let CaseArmBody::Expr(else_expr) = &cases[1].body else {
        panic!("expected an expression arm");
    };
    assert!(
        matches!(else_expr.kind, ExprKind::Unit),
        "the implicit else is Unit, got {else_expr:?}"
    );
}

#[test]
fn while_let_desugars_to_while_true_match_break() {
    // `while let P = E { B }` → `while true { match E { case P => B\n case _ => break } }`.
    let mut program = parse_ok_v54("while let Some(n) = opt { n }\n");
    let stmt = program.items.pop().unwrap();
    let StmtKind::While { cond, body } = stmt.kind else {
        panic!("expected a while statement, got {stmt:?}");
    };
    assert!(
        matches!(cond.kind, ExprKind::Bool(true)),
        "the loop condition is the constant `true`"
    );
    // The body's tail is the desugared match.
    let tail = body.tail.as_ref().expect("while-let body has a tail match");
    let ExprKind::Match { cases, .. } = &tail.kind else {
        panic!("expected a match in the loop body, got {tail:?}");
    };
    assert_eq!(cases.len(), 2);
    assert!(matches!(
        cases[0].pattern.kind,
        PatternKind::Constructor { .. }
    ));
    assert!(matches!(cases[1].pattern.kind, PatternKind::Wildcard));
    // The miss arm `break`s (a block arm whose single statement is `break`).
    let CaseArmBody::Expr(miss) = &cases[1].body else {
        panic!("expected an expression arm");
    };
    let ExprKind::Block(block) = &miss.kind else {
        panic!("expected a block arm, got {miss:?}");
    };
    assert!(
        matches!(
            block.stmts.first().map(|s| &s.kind),
            Some(StmtKind::Break { .. })
        ),
        "the wildcard arm breaks the loop"
    );
}
