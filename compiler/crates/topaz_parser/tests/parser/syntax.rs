use super::*;

// ---- CDR-001 §7 AST-shape smoke suite --------------------------------

#[test]
fn pipe_field_sugar() {
    let e = one_expr("value |> .name");
    let ExprKind::Pipe { rhs, .. } = e.kind else {
        panic!("expected pipe: {e:?}");
    };
    assert!(matches!(rhs.as_ref(), PipeRhs::Field(_)));
}

#[test]
fn chained_pipe_field_sugar_is_left_associative() {
    let e = one_expr("x |> .a |> .b");
    let ExprKind::Pipe { lhs, rhs } = e.kind else {
        panic!("expected pipe");
    };
    assert!(matches!(rhs.as_ref(), PipeRhs::Field(_)));
    assert!(matches!(lhs.kind, ExprKind::Pipe { .. }));
}

#[test]
fn optional_access_vs_parenthesized_try() {
    let e = one_expr("expr?.field");
    assert!(matches!(e.kind, ExprKind::OptionalAccess { .. }));

    let e = one_expr("(expr?).field");
    let ExprKind::Member { object, .. } = e.kind else {
        panic!("expected member access");
    };
    let ExprKind::Paren(inner) = &object.kind else {
        panic!("expected parenthesized object");
    };
    assert!(matches!(inner.kind, ExprKind::Try(_)));
}

#[test]
fn record_literal_vs_block() {
    let v = let_value("let p = { x: 1 }");
    assert!(matches!(v.kind, ExprKind::RecordLiteral { .. }));

    let v = let_value("let b = { f() }");
    let ExprKind::Block(block) = v.kind else {
        panic!("expected block");
    };
    assert!(block.tail.is_some());
}

#[test]
fn record_update_is_postfix() {
    let v = let_value("let q = p { x: 3, y: 4 }");
    let ExprKind::RecordUpdate {
        base,
        spread,
        fields,
    } = v.kind
    else {
        panic!("expected record update");
    };
    assert!(matches!(base.kind, ExprKind::Ident));
    assert!(spread.is_none());
    assert_eq!(fields.len(), 2);
}

#[test]
fn call_spread_in_variadic_tail() {
    let e = one_expr("f(a, ...rest, b, label: 1)");
    let ExprKind::Call { args, .. } = e.kind else {
        panic!("expected call");
    };
    assert!(matches!(args[0], CallArg::Positional(_)));
    assert!(matches!(args[1], CallArg::Spread(_)));
    assert!(matches!(args[2], CallArg::Positional(_)));
    assert!(matches!(args[3], CallArg::Named { .. }));
}

#[test]
fn multiline_sql_template_with_interpolation() {
    let src = "let report = sql\"\"\"\n    SELECT *\n    WHERE region = {region}\n    \"\"\"";
    let v = let_value(src);
    let ExprKind::String(lit) = v.kind else {
        panic!("expected template");
    };
    assert!(lit.multiline);
    let tag = lit.tag.expect("tagged");
    assert_eq!(&src[tag.lo as usize..tag.hi as usize], "sql");
    assert!(matches!(
        lit.parts[..],
        [
            StringPart::Text(_),
            StringPart::Interpolation(_),
            StringPart::Text(_)
        ]
    ));
}

#[test]
fn empty_interpolation_reports_once_and_recovers_inside_a_block() {
    let src = concat!(
        "function jsonAlias(value: JSONValue) -> () {\n",
        "  let parsed = value.parseAs<string>(\"{}\")\n",
        "}\n",
    );
    let out = parse(FileId(0), src);
    assert_eq!(out.diagnostics.len(), 1, "{:?}", out.diagnostics);
    assert_eq!(out.diagnostics[0].code.as_str(), "TPZ2001");
    assert_eq!(out.diagnostics[0].message, "expected an expression");
    assert_eq!(out.program.items.len(), 1);
}

// ---- types and the GtGt split rule ------------------------------------

#[test]
fn gtgt_splits_in_nested_generic_closings() {
    let mut program = parse_ok("type T = Array<Array<int>>");
    let StmtKind::TypeAlias(alias) = program.items.pop().unwrap().kind else {
        panic!("expected type alias");
    };
    let TypeKind::Named { args, .. } = &alias.ty.kind else {
        panic!("expected named type");
    };
    assert_eq!(args.len(), 1);
    let TypeKind::Named { args, .. } = &args[0].kind else {
        panic!("expected nested named type");
    };
    assert_eq!(args.len(), 1);

    // Three levels: `>>` then `>`.
    parse_ok("type U = Array<Array<Array<int>>>");
    // Mixed arguments after a split.
    parse_ok("let m: Map<string, Array<Array<int>>> = f()");
}

#[test]
fn gtgt_in_expressions_is_composition() {
    let e = one_expr("f >> g >> h");
    let ExprKind::Compose { lhs, rhs } = e.kind else {
        panic!("expected composition");
    };
    // Right-associative: f >> (g >> h).
    assert!(matches!(lhs.kind, ExprKind::Ident));
    assert!(matches!(rhs.kind, ExprKind::Compose { .. }));
}

#[test]
fn function_and_union_and_literal_types() {
    parse_ok("type Handler = (int, ...string) -> Result<int, string>");
    parse_ok("type Mode = \"fast\" | \"slow\" | null");
    parse_ok("type Point = { x: int, y: int }");
    parse_ok("type Pair<A, B> = { first: A, second: B }");
    parse_ok("function f(cb: () -> ()) -> () { cb() }");
}

// ---- precedence -------------------------------------------------------

#[test]
fn arithmetic_precedence() {
    let e = one_expr("1 + 2 * 3");
    let ExprKind::Binary { op, rhs, .. } = e.kind else {
        panic!("expected binary");
    };
    assert_eq!(op, BinaryOp::Add);
    assert!(matches!(
        rhs.kind,
        ExprKind::Binary {
            op: BinaryOp::Mul,
            ..
        }
    ));
}

#[test]
fn power_binds_tighter_than_unary() {
    let e = one_expr("-2 ** 2");
    let ExprKind::Unary { op, operand } = e.kind else {
        panic!("expected unary");
    };
    assert_eq!(op, UnaryOp::Minus);
    assert!(matches!(
        operand.kind,
        ExprKind::Binary {
            op: BinaryOp::Pow,
            ..
        }
    ));
}

#[test]
fn coalesce_is_left_associative() {
    let e = one_expr("a ?? b ?? c");
    let ExprKind::Binary { op, lhs, .. } = e.kind else {
        panic!("expected binary");
    };
    assert_eq!(op, BinaryOp::Coalesce);
    assert!(matches!(
        lhs.kind,
        ExprKind::Binary {
            op: BinaryOp::Coalesce,
            ..
        }
    ));
}

#[test]
fn range_with_step() {
    let e = one_expr("1..10 by 2");
    let ExprKind::Range {
        inclusive, step, ..
    } = e.kind
    else {
        panic!("expected range");
    };
    assert!(inclusive);
    assert!(step.is_some());

    let e = one_expr("0..<n");
    assert!(matches!(
        e.kind,
        ExprKind::Range {
            inclusive: false,
            step: None,
            ..
        }
    ));
}

#[test]
fn placeholder_in_pipeline_call() {
    let e = one_expr("xs |> filter(_)");
    let ExprKind::Pipe { rhs, .. } = e.kind else {
        panic!("expected pipe");
    };
    let PipeRhs::Expr(rhs) = rhs.as_ref() else {
        panic!("expected expression rhs");
    };
    let ExprKind::Call { args, .. } = &rhs.kind else {
        panic!("expected call");
    };
    assert!(
        matches!(&args[..], [CallArg::Positional(p)] if matches!(p.kind, ExprKind::Placeholder))
    );
}

// ---- statements and declarations ---------------------------------------

#[test]
fn function_declaration_full_form() {
    let mut program = parse_ok(
        "function map<T, U>(xs: Array<T>, f: (T) -> U) -> Array<U> {\n    return f(xs)\n}",
    );
    let StmtKind::Function(decl) = program.items.pop().unwrap().kind else {
        panic!("expected function");
    };
    assert_eq!(decl.type_params.len(), 2);
    assert_eq!(decl.params.len(), 2);
    assert!(decl.return_type.is_some());
    assert!(matches!(decl.body.stmts[0].kind, StmtKind::Return(Some(_))));
}

#[test]
fn function_type_parameter_protocol_bounds() {
    let mut program = parse_ok_v54(
        "function render<T: Show + Eq>(value: T) -> string { return Show.show(value) }",
    );
    let StmtKind::Function(decl) = program.items.pop().unwrap().kind else {
        panic!("expected function");
    };
    assert_eq!(decl.type_params.len(), 1);
    assert_eq!(decl.type_param_bounds.len(), 1);
    assert_eq!(decl.type_param_bounds[0].len(), 2);
}

#[test]
fn function_protocol_bounds_are_v54_only() {
    let src = "function render<T: Show>(value: T) -> T { return value }";
    for version in [LangVersion::V5_1, LangVersion::V5_2, LangVersion::V5_3] {
        let out = parse_with_options(
            FileId(0),
            src,
            ParseOptions {
                language_version: version,
            },
        );
        assert!(
            out.diagnostics.iter().any(|diag| {
                diag.code.as_str() == "TPZ2001"
                    && diag.message.contains("generic protocol bounds need v5.4")
            }),
            "{version:?} must reject a bound clause: {:?}",
            out.diagnostics
        );
    }
    let v54 = parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: LangVersion::V5_4,
        },
    );
    assert!(v54.diagnostics.is_empty(), "v5.4 must retain bounds");
}

#[test]
fn variadic_parameter_and_default() {
    let mut program =
        parse_ok("function log(level: int = 1, ...parts: string) -> () { print(level) }");
    let StmtKind::Function(decl) = program.items.pop().unwrap().kind else {
        panic!("expected function");
    };
    assert!(decl.params[0].default.is_some());
    assert!(decl.params[1].variadic);
}

#[test]
fn bindings_and_assignments() {
    let program = parse_ok("let mut n = 0\nn += 1\nn ??= 2\nconst MAX: int = 10");
    assert!(matches!(
        program.items[0].kind,
        StmtKind::Let { mutable: true, .. }
    ));
    assert!(matches!(
        program.items[1].kind,
        StmtKind::Assign {
            op: AssignOp::Add,
            ..
        }
    ));
    assert!(matches!(
        program.items[2].kind,
        StmtKind::Assign {
            op: AssignOp::Coalesce,
            ..
        }
    ));
    assert!(matches!(program.items[3].kind, StmtKind::Const { .. }));
}

#[test]
fn let_destructures_record_pattern() {
    let mut program = parse_ok("let { x, y } = point");
    let StmtKind::Let { pattern, .. } = program.items.pop().unwrap().kind else {
        panic!("expected let");
    };
    let PatternKind::Record(fields) = &pattern.kind else {
        panic!("expected record pattern");
    };
    assert_eq!(fields.len(), 2);
    assert!(fields[0].pattern.is_none()); // shorthand binds the name
}

#[test]
fn while_break_continue_and_defer() {
    parse_ok("while n < 3 {\n    n += 1\n    if done { break } else { continue }\n}");
    parse_ok("defer close(file)");
    parse_ok("defer {\n    cleanup()\n}");
}

#[test]
fn block_value_vs_statement_list() {
    let v = let_value("let a = { f(); }");
    let ExprKind::Block(block) = v.kind else {
        panic!("expected block");
    };
    assert_eq!(block.stmts.len(), 1);
    assert!(block.tail.is_none());

    let v = let_value("let b = { f(); g() }");
    let ExprKind::Block(block) = v.kind else {
        panic!("expected block");
    };
    assert_eq!(block.stmts.len(), 1);
    assert!(block.tail.is_some());
}

#[test]
fn if_else_chain_and_for_value() {
    let v = let_value("let v = if a { 1 } else if b { 2 } else { 3 }");
    let ExprKind::If { else_branch, .. } = v.kind else {
        panic!("expected if");
    };
    let else_if = else_branch.expect("else");
    assert!(matches!(else_if.kind, ExprKind::If { .. }));

    let v = let_value("let xs = for x in 1..3 { x * 2 }");
    assert!(matches!(v.kind, ExprKind::For { .. }));
}

#[test]
fn lambdas() {
    let v = let_value("let id = x => x");
    let ExprKind::Lambda { params, .. } = v.kind else {
        panic!("expected lambda");
    };
    assert_eq!(params.len(), 1);

    let v = let_value("let add = (a: int, b: int) => a + b");
    let ExprKind::Lambda { params, .. } = v.kind else {
        panic!("expected lambda");
    };
    assert_eq!(params.len(), 2);
    assert!(params[0].ty.is_some());

    let v = let_value("let unit = () => 1");
    assert!(matches!(v.kind, ExprKind::Lambda { ref params, .. } if params.is_empty()));

    let v = let_value("let u = ()");
    assert!(matches!(v.kind, ExprKind::Unit));
}

#[test]
fn match_patterns_and_guard() {
    let e = one_expr(
        "match v {\n    case Some({ x }) if x > 0 => x\n    case [first, ..rest] => first\n    case 1..5 => 1\n    case n: int => n\n    case _ => 0\n}",
    );
    let ExprKind::Match { cases, .. } = e.kind else {
        panic!("expected match");
    };
    assert_eq!(cases.len(), 5);
    assert!(matches!(
        cases[0].pattern.kind,
        PatternKind::Constructor { .. }
    ));
    assert!(cases[0].guard.is_some());
    let PatternKind::List(ref elems) = cases[1].pattern.kind else {
        panic!("expected list pattern");
    };
    assert!(matches!(elems[1], ListPatternElem::Rest(Some(_))));
    assert!(matches!(
        cases[2].pattern.kind,
        PatternKind::Range {
            inclusive: true,
            ..
        }
    ));
    assert!(matches!(cases[3].pattern.kind, PatternKind::Typed { .. }));
    assert!(matches!(cases[4].pattern.kind, PatternKind::Wildcard));
}

#[test]
fn concurrent_forms() {
    let e = one_expr("concurrent {\n    a: f()\n    b: g()\n}");
    let ExprKind::Concurrent {
        timeout,
        arms,
        else_block,
    } = e.kind
    else {
        panic!("expected concurrent");
    };
    assert!(timeout.is_none());
    assert_eq!(arms.len(), 2);
    assert!(else_block.is_none());

    let e = one_expr("concurrent(timeout: 3s) {\n    a: f()\n} else { 0 }");
    let ExprKind::Concurrent {
        timeout,
        else_block,
        ..
    } = e.kind
    else {
        panic!("expected concurrent");
    };
    assert!(timeout.is_some());
    assert!(else_block.is_some());
}

#[test]
fn dashboard_sample_end_to_end() {
    // EXAMPLES H.1 shape: timeout form whose else block's value is a
    // record literal.
    let src = "let dashboard = concurrent(timeout: 3s) {\n    user: loadUser(userId)\n    posts: loadPosts(userId)\n} else {\n    {\n        user: None,\n        posts: []\n    }\n}";
    let v = let_value(src);
    let ExprKind::Concurrent { else_block, .. } = v.kind else {
        panic!("expected concurrent");
    };
    let block = else_block.expect("else block");
    let tail = block.tail.as_ref().expect("record literal tail");
    assert!(matches!(tail.kind, ExprKind::RecordLiteral { .. }));
}

#[test]
fn assert_parses_as_an_ordinary_call() {
    let e = one_expr("assert(x > 0, \"message\")");
    assert!(matches!(e.kind, ExprKind::Call { .. }));
}
