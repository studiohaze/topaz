use super::*;

// ----------------------------------------------------------------------------
// Expressions.
// ----------------------------------------------------------------------------

pub(super) fn emit_plain_string_literal(
    lit: &ast::StringLit,
    src: &LoweredText,
) -> Result<String, EmitError> {
    if lit
        .parts
        .iter()
        .any(|part| matches!(part, StringPart::Interpolation(_)))
    {
        return Err(decline("an interpolated native string"));
    }
    let mut decoded = String::new();
    for part in &lit.parts {
        if let StringPart::Text(span) = part {
            decode_escapes(text(src, *span), &mut decoded, *span)
                .map_err(|_| EmitError::malformed_literal("string escape"))?;
        }
    }
    Ok(format!("{decoded:?}.to_string()"))
}

/// Lower an expression to a native scalar, attaching any refusal's span at the
/// innermost node (first-wins, like the boxed `emit_expr`).
pub(super) fn emit_expr(
    expr: &Expr,
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
) -> Result<Lowered, EmitError> {
    emit_expr_inner(expr, ctx, scope).map_err(|e| e.at(expr.span))
}

pub(super) fn emit_expr_inner(
    expr: &Expr,
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
) -> Result<Lowered, EmitError> {
    match &expr.kind {
        ExprKind::Int => {
            let n: i64 = text(ctx.src, expr.span)
                .parse()
                .map_err(|_| EmitError::malformed_literal("integer"))?;
            // Emit as a typed `i64` literal so the inferred type is unambiguous.
            Ok(Lowered {
                rs: format!("{n}i64"),
                ty: NativeTy::I64,
            })
        }
        ExprKind::Float => {
            let x: f64 = text(ctx.src, expr.span)
                .parse()
                .map_err(|_| EmitError::malformed_literal("float"))?;
            // `{:?}` is the shortest round-trippable form; a lexer-valid oversized
            // literal parses to +inf in BOTH engines (a bare float token is
            // unsigned and never NaN), so +inf is the only non-finite case.
            let rs = if x.is_finite() {
                format!("{x:?}f64")
            } else {
                "f64::INFINITY".to_string()
            };
            Ok(Lowered {
                rs,
                ty: NativeTy::F64,
            })
        }
        ExprKind::Bool(b) => Ok(Lowered {
            rs: format!("{b}"),
            ty: NativeTy::Bool,
        }),
        ExprKind::String(lit) if lit.tag.is_none() => Ok(Lowered {
            rs: emit_plain_string_literal(lit, ctx.src)?,
            ty: NativeTy::Str,
        }),
        ExprKind::Unit => Ok(Lowered {
            rs: "()".to_string(),
            ty: NativeTy::Unit,
        }),
        ExprKind::Ident => {
            let name = text(ctx.src, expr.span);
            let local = scope
                .iter()
                .rev()
                .find(|l| l.name == name)
                .ok_or_else(|| decline("a non-scalar or free identifier"))?;
            // Only a SCALAR local reads directly. A bare array-boundary local
            // reference (`let xs = arr`) would need boxing and is declined — only
            // `arr[i]` / `arr.length` lower (handled in the Index/Member arms).
            let ty = local
                .scalar_ty()
                .ok_or_else(|| decline("a bare array-boundary local reference"))?;
            // Numeric/bool/unit locals are `Copy`; strings clone on read so a
            // later use cannot observe a Rust move.
            let rs = if ty == NativeTy::Str {
                format!("{}.clone()", mangle(name))
            } else {
                mangle(name)
            };
            Ok(Lowered { rs, ty })
        }
        // `arr[i]` — a native read from a boxed scalar-array boundary local. The
        // object must be such a local (a bare `Ident`); the index a native `int`.
        // Routes through the shared `index_value` leaf (byte-identical OOB fault)
        // then unboxes the element to a native scalar.
        ExprKind::Index { object, index } => emit_array_index(object, index, ctx, scope, expr.span),
        // `arr.length` — a native `i64` from a boxed scalar-array boundary local.
        ExprKind::Member { object, field } => {
            emit_array_member(object, field, ctx, scope, expr.span)
        }
        ExprKind::Paren(inner) => emit_expr(inner, ctx, scope),
        ExprKind::Unary { op, operand } => {
            let v = emit_expr(operand, ctx, scope)?;
            lower_unary(*op, &v, expr.span)
        }
        ExprKind::Binary { op, lhs, rhs } => {
            // Short-circuit `&&`/`||` over native bools: lower both sides and use
            // Rust's own short-circuit operators (identical observable order —
            // the RHS runs only when the LHS does not decide). `??` is a boxed
            // Option/nullable concern — refuse.
            if matches!(op, BinaryOp::Coalesce) {
                return Err(decline("a `??` operator"));
            }
            let l = emit_expr(lhs, ctx, scope)?;
            if matches!(op, BinaryOp::And | BinaryOp::Or) {
                if l.ty != NativeTy::Bool {
                    return Err(decline("a non-bool logical operand").at(lhs.span));
                }
                let r = emit_expr(rhs, ctx, scope)?;
                if r.ty != NativeTy::Bool {
                    return Err(decline("a non-bool logical operand").at(rhs.span));
                }
                let rust_op = if matches!(op, BinaryOp::And) {
                    "&&"
                } else {
                    "||"
                };
                return Ok(Lowered {
                    rs: format!("({} {rust_op} {})", l.rs, r.rs),
                    ty: NativeTy::Bool,
                });
            }
            let r = emit_expr(rhs, ctx, scope)?;
            lower_binary(*op, &l, &r, expr.span)
        }
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            let cond_low = emit_expr(cond, ctx, scope)?;
            if cond_low.ty != NativeTy::Bool {
                return Err(decline("a non-bool if condition").at(cond.span));
            }
            let then_low = emit_block(then_block, ctx, scope)?;
            // A native `if` must yield a scalar of a single type, so BOTH arms
            // must be present and agree (a missing `else` yields `Unit`, which
            // only matches a `Unit` then-arm).
            let else_low = match else_branch {
                Some(branch) => emit_expr(branch, ctx, scope)?,
                None => Lowered {
                    rs: "()".to_string(),
                    ty: NativeTy::Unit,
                },
            };
            if then_low.ty != else_low.ty {
                return Err(decline("an `if` whose arms have different scalar types"));
            }
            // Both arms are Rust BLOCK expressions (braced) — an `if` expression
            // requires block arms.
            Ok(Lowered {
                rs: format!(
                    "if {} {{ {} }} else {{ {} }}",
                    cond_low.rs, then_low.rs, else_low.rs
                ),
                ty: then_low.ty,
            })
        }
        ExprKind::Match { scrutinee, cases } => {
            emit_match_expr(scrutinee, cases, ctx, scope, expr.span)
        }
        ExprKind::Block(block) => emit_block(block, ctx, scope),
        ExprKind::Call { callee, args, .. } => emit_call(callee, args, ctx, scope, expr.span),
        // Everything else is a boxed-value construct (string/array/record/enum/
        // Option/Result/range/lambda/match/concurrent/member/index/…): refuse.
        _ => Err(decline("a non-scalar expression")),
    }
}

/// Lower a block expression to a Rust block yielding a scalar. The block is its
/// own scope; its value is the tail expression (`Unit` when there is none).
pub(super) fn emit_block(
    block: &Block,
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
) -> Result<Lowered, EmitError> {
    // A block in expression position cannot introduce bindings the native scope
    // model threads outward, but it CAN have its own inner `let`s used by its
    // tail. Lower with a child scope into a Rust block expression.
    let mut child = scope.to_vec();
    let mut out = String::new();
    // We need a mutable Ctx to lower statements (fn defs accumulate), but block
    // statements in this slice never declare functions; lower with a local
    // shadow. To keep `emit_stmt`'s signature, route through a temporary.
    //
    // NOTE: `emit_block` takes `&Ctx` for expressions, but statements need
    // `&mut Ctx`. A block with statements is therefore lowered via the
    // statement path only at the top level / loop body (which DO hold `&mut`);
    // an expression-position block with statements refuses here to avoid an
    // unsound shortcut. A bare tail-only block lowers cleanly.
    if !block.stmts.is_empty() {
        return Err(decline("a block expression with statements"));
    }
    let _ = (&mut child, &mut out);
    match block.tail.as_deref() {
        Some(tail) => emit_expr(tail, ctx, scope),
        None => Ok(Lowered {
            rs: "()".to_string(),
            ty: NativeTy::Unit,
        }),
    }
}

/// Lower a narrow scalar `match` expression: scalar scrutinee, scalar literal
/// patterns, `_`, optional scalar-bool guards, and expression bodies. Binding
/// patterns, destructuring, return arms, and non-scalar values remain boxed-only.
pub(super) fn emit_match_expr(
    scrutinee: &Expr,
    cases: &[ast::CaseClause],
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
    span: Span,
) -> Result<Lowered, EmitError> {
    let scrut = emit_expr(scrutinee, ctx, scope)?;
    let mut arms = String::new();
    let mut result_ty = None;
    let mut closed = false;
    for case in cases {
        let mut arm_scope_storage;
        let mut body_prefix = String::new();
        let (arm_scope, guard, closes) = match &case.pattern.kind {
            PatternKind::Literal(lit) => {
                let lit_low = emit_expr(lit, ctx, scope)?;
                if lit_low.ty != scrut.ty {
                    return Err(decline("a native `match` literal whose type differs").at(lit.span));
                }
                let guard = lower_binary(
                    BinaryOp::Eq,
                    &Lowered {
                        rs: "__scrut".to_string(),
                        ty: scrut.ty,
                    },
                    &lit_low,
                    span,
                )?
                .rs;
                (scope, guard, false)
            }
            PatternKind::Wildcard => (scope, "true".to_string(), case.guard.is_none()),
            PatternKind::Binding(name) => {
                let binding = text(ctx.src, name.span);
                if scope.iter().any(|local| local.name == binding) {
                    return Err(decline("a native `match` binding redeclaration").at(name.span));
                }
                ctx.confirm_local(binding, name.span, scrut.ty.mono())
                    .map_err(|e| e.at(name.span))?;
                arm_scope_storage = scope.to_vec();
                arm_scope_storage.push(NativeLocal {
                    name: binding.to_string(),
                    kind: LocalKind::Scalar(scrut.ty),
                    mutable: false,
                });
                let bind_value = if scrut.ty == NativeTy::Str {
                    "__scrut.clone()"
                } else {
                    "__scrut"
                };
                let bind_rs = format!("let {} = {bind_value}; ", mangle(binding));
                body_prefix = bind_rs.clone();
                (
                    &arm_scope_storage[..],
                    "true".to_string(),
                    case.guard.is_none(),
                )
            }
            _ => return Err(decline("a non-scalar native `match` pattern").at(case.pattern.span)),
        };

        let guard = if let Some(g) = &case.guard {
            let g_low = emit_expr(g, ctx, arm_scope)?;
            if g_low.ty != NativeTy::Bool {
                return Err(decline("a non-bool native `match` guard").at(g.span));
            }
            if matches!(case.pattern.kind, PatternKind::Binding(_)) {
                format!("{{ {body_prefix} {} }}", g_low.rs)
            } else {
                format!("({guard}) && ({})", g_low.rs)
            }
        } else {
            guard
        };
        let body = emit_match_body(&case.body, ctx, arm_scope)?;
        match result_ty {
            Some(ty) if ty != body.ty => {
                return Err(decline("a native `match` with mismatched arm types").at(case.span));
            }
            Some(_) => {}
            None => result_ty = Some(body.ty),
        }

        if closes {
            arms.push_str(&format!("{{ {body_prefix}{} }}", body.rs));
            closed = true;
            break;
        }
        arms.push_str(&format!("if {guard} {{ {body_prefix}{} }} else ", body.rs));
    }

    let ty = result_ty.ok_or_else(|| decline("an empty native `match`").at(span))?;
    if !closed {
        arms.push_str(&format!(
            "{{ return Err(fault(codes::FAULT_MATCH_MISS, {:?}, {})); }}",
            "no `case` matched and no catch-all exists (§5)",
            emit_span(span)
        ));
    }
    Ok(Lowered {
        rs: format!("{{ let __scrut = {}; {arms} }}", scrut.rs),
        ty,
    })
}

pub(super) fn emit_match_body(
    body: &ast::CaseArmBody,
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
) -> Result<Lowered, EmitError> {
    match body {
        ast::CaseArmBody::Expr(expr) => emit_expr(expr, ctx, scope),
        ast::CaseArmBody::Return { .. } => Err(decline("a native `match` return arm")),
    }
}

/// Lower an argument for a native `Array<scalar>` parameter. A boundary local is
/// cloned across the call boundary (matching boxed `Value` semantics); a direct
/// array literal is boxed inline exactly like a boundary `let` initializer.
pub(super) fn emit_array_arg(
    arg: &Expr,
    elem: NativeTy,
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
) -> Result<String, EmitError> {
    if let ExprKind::Ident = &arg.kind {
        let name = text(ctx.src, arg.span);
        let local =
            scope.iter().rev().find(|l| l.name == name).ok_or_else(|| {
                decline("an array argument from a non-array binding").at(arg.span)
            })?;
        let local_elem = local
            .array_elem()
            .ok_or_else(|| decline("an array argument from a non-array binding").at(arg.span))?;
        if local_elem != elem {
            return Err(decline("an array argument whose element type differs").at(arg.span));
        }
        return Ok(format!("{}.clone()", mangle(name)));
    }
    if matches!(arg.kind, ExprKind::Array(_)) {
        return emit_boxed_scalar_array(arg, elem, ctx, scope);
    }
    Err(decline("a non-array argument").at(arg.span))
}

/// Lower `arr[i]` — a native read from a boxed scalar-array boundary local. The
/// object must be a bare `Ident` bound to a `ScalarArray` local; the index a
/// native `int`. Routes through the shared `native_index_<E>` runtime helper,
/// which calls the SHARED `index_value` leaf (byte-identical OOB `FAULT_INDEX` at
/// the index expression's span) then unboxes the element to the native scalar.
pub(super) fn emit_array_index(
    object: &Expr,
    index: &Expr,
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
    span: Span,
) -> Result<Lowered, EmitError> {
    let ExprKind::Ident = &object.kind else {
        return Err(decline("an index on a non-local-array object").at(object.span));
    };
    let oname = text(ctx.src, object.span);
    let local = scope
        .iter()
        .rev()
        .find(|l| l.name == oname)
        .ok_or_else(|| decline("an index on a non-array binding").at(object.span))?;
    let elem = local
        .array_elem()
        .ok_or_else(|| decline("an index on a non-array binding").at(object.span))?;
    // The index must be a native `int`.
    let idx = emit_expr(index, ctx, scope)?;
    if idx.ty != NativeTy::I64 {
        return Err(decline("a non-int array index").at(index.span));
    }
    // The OOB fault span is the INDEX expression's span — exactly the span the
    // interpreter threads to `index_value` (its `KIndexApply` carries the index
    // site). (The boxed emitter also routes `arr[i]` through `index_value` at this
    // span, so all three engines fault identically.)
    let helper = match elem {
        NativeTy::I64 => "native_index_int",
        NativeTy::F64 => "native_index_float",
        NativeTy::Bool => "native_index_bool",
        NativeTy::Str => "native_index_string",
        NativeTy::Unit => {
            return Err(decline("a non-unboxable array index").at(span));
        }
    };
    Ok(Lowered {
        rs: format!(
            "{helper}(&{}, {}, {})?",
            mangle(oname),
            idx.rs,
            emit_span(span)
        ),
        ty: elem,
    })
}

/// Lower `arr.length` — a native `i64` from a boxed scalar-array boundary local,
/// via the shared `native_array_len` (the `Array.length` arm of `member_value`,
/// so the value matches the interpreter exactly). Only `.length` on an array
/// boundary local lowers; any other member declines.
pub(super) fn emit_array_member(
    object: &Expr,
    field: &topaz_hir::emission::Ident,
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
    span: Span,
) -> Result<Lowered, EmitError> {
    let ExprKind::Ident = &object.kind else {
        return Err(decline("a member on a non-local-array object").at(object.span));
    };
    let oname = text(ctx.src, object.span);
    let local = scope
        .iter()
        .rev()
        .find(|l| l.name == oname)
        .ok_or_else(|| decline("a member on a non-array binding").at(object.span))?;
    // Confirm it is an array boundary local (the element type is unused for length
    // but its presence proves the local is a `ScalarArray`).
    local
        .array_elem()
        .ok_or_else(|| decline("a member on a non-array binding").at(object.span))?;
    if text(ctx.src, field.span) != "length" {
        return Err(decline("an array member other than `.length`").at(span));
    }
    Ok(Lowered {
        rs: format!("native_array_len(&{}, {})?", mangle(oname), emit_span(span)),
        ty: NativeTy::I64,
    })
}
