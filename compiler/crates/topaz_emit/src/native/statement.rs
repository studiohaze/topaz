use super::*;

/// Split a top-level item list into (non-function leading statements, optional
/// tail expression). Function declarations are filtered out (lowered separately
/// to Rust `fn`s); a trailing `Expr` statement becomes the program value.
pub(super) fn split_top(items: &[Stmt]) -> (Vec<&Stmt>, Option<&Expr>) {
    let non_fn: Vec<&Stmt> = items
        .iter()
        .filter(|s| !matches!(s.kind, StmtKind::Function(_)))
        .collect();
    if let Some(last) = non_fn.last()
        && let StmtKind::Expr(expr) = &last.kind
    {
        let head = non_fn[..non_fn.len() - 1].to_vec();
        return (head, Some(expr));
    }
    (non_fn, None)
}

// ----------------------------------------------------------------------------
// Statements.
// ----------------------------------------------------------------------------

/// Lower one statement into `out`, extending `scope` with any binding it
/// introduces. `in_loop` enables `break`/`continue`.
pub(super) fn emit_stmt(
    stmt: &Stmt,
    ctx: &mut Ctx<'_>,
    scope: &mut Vec<NativeLocal>,
    out: &mut String,
    in_loop: bool,
) -> Result<(), EmitError> {
    match &stmt.kind {
        StmtKind::Let {
            mutable,
            pattern,
            ty,
            value,
        } => {
            let (name, name_span) = simple_binding(pattern, ctx.src)
                .ok_or_else(|| decline("a non-simple let binding").at(stmt.span))?;
            // Same-scope redeclaration refuses (the boxed backend owns the
            // diagnostic; native declines so the program falls back).
            if scope.iter().any(|l| l.name == name) {
                return Err(decline("a same-scope redeclaration").at(stmt.span));
            }
            // (1) A boxed `Array<scalar>` boundary local: `let arr: Array<E> = [..]`
            // with E a concrete scalar and a scalar-array LITERAL initializer. The
            // array stays a boxed `Value::Array`; native reads (`arr[i]`/`.length`)
            // unbox elements at the boundary, and mutable locals can write direct
            // cells through the shared `index_slot` leaf.
            if let Some(elem) = typed_scalar_array(pattern, ty.as_ref(), ctx.src) {
                let arr_rs = emit_boxed_scalar_array(value, elem, ctx, scope)?;
                // The typed HIR records the array local as `Boxed` (the soundness
                // cross-check: the checker agrees this local is non-scalar/boxed).
                ctx.confirm_local(name, name_span, MonoTy::Boxed)
                    .map_err(|e| e.at(stmt.span))?;
                out.push_str(&format!("    let {} = {arr_rs};\n", mangle(name)));
                scope.push(NativeLocal {
                    name: name.to_string(),
                    kind: LocalKind::ScalarArray(elem),
                    mutable: *mutable,
                });
                return Ok(());
            }
            // (2) A checker-proven direct byte-field projection from an eligible
            // read-only record parameter. The record and handle both retain the
            // boxed runtime `Value`; only later byte leaves dispatch directly.
            if let Some(proof) = ctx.byte_projection(name, name_span, value, scope) {
                let boxed = emit_boxed_boundary_expr(value, ctx, scope)
                    .map_err(|inner| inner.at(stmt.span))?;
                ctx.confirm_local(name, name_span, proof.mono)
                    .map_err(|inner| inner.at(stmt.span))?;
                out.push_str(&format!(
                    "    let {}{} = {boxed};\n",
                    if *mutable { "mut " } else { "" },
                    mangle(name)
                ));
                scope.push(NativeLocal {
                    name: name.to_string(),
                    kind: LocalKind::ByteHandle(proof.mono),
                    mutable: *mutable,
                });
                return Ok(());
            }
            // (3) Any checker-exact byte local keeps the boxed runtime carrier
            // but may use direct byte leaves. The clean typed HIR owns the type
            // fact; this is distinct from a record projection, which still
            // requires the separate proof above.
            if let Some(mono) = ctx.hir_locals.get(name, name_span)
                && mono.is_byte_handle()
            {
                let boxed = emit_boxed_boundary_expr(value, ctx, scope)
                    .map_err(|inner| inner.at(stmt.span))?;
                ctx.confirm_local(name, name_span, mono)
                    .map_err(|inner| inner.at(stmt.span))?;
                out.push_str(&format!(
                    "    let {}{} = {boxed};\n",
                    if *mutable { "mut " } else { "" },
                    mangle(name)
                ));
                scope.push(NativeLocal {
                    name: name.to_string(),
                    kind: LocalKind::ByteHandle(mono),
                    mutable: *mutable,
                });
                return Ok(());
            }
            // (4) An ordinary native SCALAR local.
            match emit_expr(value, ctx, scope) {
                Ok(low) => {
                    // SOUNDNESS: the typed HIR must confirm this local at the BINDING
                    // NAME's span (exactly where the checker records it) as the scalar we
                    // inferred — so a native register can never rest on an untyped fact.
                    ctx.confirm_local(name, name_span, low.ty.mono())
                        .map_err(|e| e.at(stmt.span))?;
                    out.push_str(&format!(
                        "    let {}{} = {};\n",
                        if *mutable { "mut " } else { "" },
                        mangle(name),
                        low.rs
                    ));
                    scope.push(NativeLocal {
                        name: name.to_string(),
                        kind: LocalKind::Scalar(low.ty),
                        mutable: *mutable,
                    });
                    Ok(())
                }
                Err(e) if e.is_native_decline() && !*mutable => {
                    let boxed = emit_boxed_boundary_expr(value, ctx, scope)
                        .map_err(|inner| inner.at(stmt.span))?;
                    ctx.confirm_local(name, name_span, MonoTy::Boxed)
                        .map_err(|inner| inner.at(stmt.span))?;
                    out.push_str(&format!("    let {} = {boxed};\n", mangle(name)));
                    scope.push(NativeLocal {
                        name: name.to_string(),
                        kind: LocalKind::BoxedValue,
                        mutable: false,
                    });
                    Ok(())
                }
                Err(e) if e.is_native_decline() => {
                    Err(decline("a mutable boxed boundary local").at(stmt.span))
                }
                Err(e) => Err(e),
            }
        }
        StmtKind::Import(imp) if std_math_namespace_alias(imp, ctx.src).is_some() => Ok(()),
        StmtKind::Import(_) => Err(decline("an unsupported import").at(stmt.span)),
        StmtKind::Assign { target, op, value } => {
            emit_assign(target, *op, value, stmt.span, ctx, scope, out)
        }
        StmtKind::Expr(expr) if matches!(expr.kind, ExprKind::For { .. }) => {
            emit_for_stmt(expr, ctx, scope, out)
        }
        StmtKind::While { cond, body } => {
            let cond_low = emit_expr(cond, ctx, scope)?;
            if cond_low.ty != NativeTy::Bool {
                return Err(decline("a non-bool while condition").at(cond.span));
            }
            let body_rs = emit_loop_body(body, ctx, scope)?;
            // KEEP the back-edge checkpoint (no elision this slice). The
            // condition is a bare `bool` (the checker proved it), so no
            // `condition_bool` guard is needed — it would never fault.
            out.push_str(&format!("    while {} {body_rs}\n", cond_low.rs));
            Ok(())
        }
        // The native scalar island supports only the bare, unlabeled,
        // value-less `break`/`continue` of a `while`/`for`. A labeled or value
        // `break`/`continue` (which only targets a `loop` EXPRESSION) is declined
        // here — `loop` itself is declined below — and the boxed backend (which
        // IS run≡build-pinned for these) lowers it. A native decline never reaches
        // the user; it always falls back to boxed.
        StmtKind::Break {
            label: None,
            value: None,
        } if in_loop => {
            out.push_str("    break;\n");
            Ok(())
        }
        StmtKind::Continue { label: None } if in_loop => {
            out.push_str("    continue;\n");
            Ok(())
        }
        StmtKind::Break { label: Some(_), .. } | StmtKind::Continue { label: Some(_) } => {
            Err(decline("a labeled break/continue").at(stmt.span))
        }
        StmtKind::Break { value: Some(_), .. } => {
            Err(decline("a break with a value").at(stmt.span))
        }
        StmtKind::Break { .. } | StmtKind::Continue { .. } => {
            Err(decline("loop control outside a loop").at(stmt.span))
        }
        // An `if` in STATEMENT position: its arms are statement blocks (they may
        // `let`/`break`/`continue` and need not yield a scalar — the value is
        // discarded). Lower to a Rust `if` whose bodies are statement sequences,
        // NOT the expression-position `if` (which requires both arms to yield one
        // scalar type). The condition is a bare `bool` (checker-proved).
        StmtKind::Expr(expr) if matches!(expr.kind, ExprKind::If { .. }) => {
            emit_if_stmt(expr, ctx, scope, out, in_loop)
        }
        // A bare block in statement position: its own scope, value discarded.
        StmtKind::Expr(expr) if matches!(expr.kind, ExprKind::Block(_)) => {
            let ExprKind::Block(block) = &expr.kind else {
                unreachable!()
            };
            let mut child = scope.to_vec();
            let mut body = String::new();
            for s in &block.stmts {
                emit_stmt(s, ctx, &mut child, &mut body, in_loop)?;
            }
            if let Some(tail) = block.tail.as_deref() {
                emit_discard_tail(tail, ctx, &child, &mut body, in_loop)?;
            }
            out.push_str(&format!("    {{ {body}}}\n"));
            Ok(())
        }
        // An expression statement: lower for effect. A scalar expression with no
        // side effect is harmless (`let _ = …;` keeps any fault/effect).
        StmtKind::Expr(expr) => {
            let low = emit_expr(expr, ctx, scope)?;
            out.push_str(&format!("    let _ = {};\n", low.rs));
            Ok(())
        }
        // Everything else (const, return, defer, import, export, type, enum,
        // nested function) is not part of the scalar island this slice.
        _ => Err(decline("an unsupported statement").at(stmt.span)),
    }
}

/// Lower an `if` in STATEMENT position (value discarded): a Rust `if` whose arms
/// are statement sequences. An `else if` chains naturally (the else branch is
/// itself an `If` expression statement); a plain `else { … }` block lowers as a
/// statement block; a missing `else` emits no `else`. `break`/`continue` are
/// legal in the arms when `in_loop`.
pub(super) fn emit_if_stmt(
    expr: &Expr,
    ctx: &mut Ctx<'_>,
    scope: &[NativeLocal],
    out: &mut String,
    in_loop: bool,
) -> Result<(), EmitError> {
    let ExprKind::If {
        cond,
        then_block,
        else_branch,
    } = &expr.kind
    else {
        unreachable!("emit_if_stmt called on a non-if")
    };
    let cond_low = emit_expr(cond, ctx, scope)?;
    if cond_low.ty != NativeTy::Bool {
        return Err(decline("a non-bool if condition").at(cond.span));
    }
    let then_rs = emit_stmt_block(then_block, ctx, scope, in_loop)?;
    out.push_str(&format!("    if {} {then_rs}", cond_low.rs));
    match else_branch.as_deref() {
        // `else if …`: chain by lowering the nested `if` AS a statement into the
        // `else` arm.
        Some(branch) if matches!(branch.kind, ExprKind::If { .. }) => {
            out.push_str(" else ");
            let mut inner = String::new();
            emit_if_stmt(branch, ctx, scope, &mut inner, in_loop)?;
            // `emit_if_stmt` writes a leading 4-space indent + trailing newline;
            // trim to inline after `else`.
            out.push_str(inner.trim_start());
        }
        // `else { … }`: a plain block.
        Some(branch) => match &branch.kind {
            ExprKind::Block(block) => {
                let else_rs = emit_stmt_block(block, ctx, scope, in_loop)?;
                out.push_str(&format!(" else {else_rs}\n"));
            }
            // A non-block, non-if else (rare in statement position) — discard its
            // scalar value.
            _ => {
                let low = emit_expr(branch, ctx, scope)?;
                out.push_str(&format!(" else {{ let _ = {}; }}\n", low.rs));
            }
        },
        None => out.push('\n'),
    }
    Ok(())
}

/// Lower a `Block` as a STATEMENT sequence (its value discarded) into a Rust
/// block string `{ … }`. Used by statement-position `if` arms and bare blocks.
pub(super) fn emit_stmt_block(
    block: &Block,
    ctx: &mut Ctx<'_>,
    scope: &[NativeLocal],
    in_loop: bool,
) -> Result<String, EmitError> {
    let mut child = scope.to_vec();
    let mut body = String::new();
    for stmt in &block.stmts {
        emit_stmt(stmt, ctx, &mut child, &mut body, in_loop)?;
    }
    if let Some(tail) = block.tail.as_deref() {
        emit_discard_tail(tail, ctx, &child, &mut body, in_loop)?;
    }
    Ok(format!("{{ {body}}}"))
}

/// Lower an assignment to a scalar `let mut` local (`x = e`, `x += e`, etc.) or
/// to a direct mutable scalar-array cell (`arr[i] = e`). Broader member/index
/// paths remain boxed aggregate concerns and decline.
pub(super) fn emit_assign(
    target: &Expr,
    op: AssignOp,
    value: &Expr,
    stmt_span: Span,
    ctx: &mut Ctx<'_>,
    scope: &mut [NativeLocal],
    out: &mut String,
) -> Result<(), EmitError> {
    if matches!(target.kind, ExprKind::Index { .. }) {
        return emit_array_index_assign(target, op, value, stmt_span, ctx, scope, out);
    }

    let ExprKind::Ident = &target.kind else {
        return Err(decline("a non-identifier assignment target").at(target.span));
    };
    let name = text(ctx.src, target.span);
    let local = scope
        .iter()
        .rev()
        .find(|l| l.name == name)
        .ok_or_else(|| decline("an assignment to a non-scalar binding").at(target.span))?;
    if !local.mutable {
        return Err(decline("an assignment to an immutable binding").at(target.span));
    }
    // A bare mutable target must be scalar; array locals only mutate through the
    // direct index-assignment branch above.
    let target_ty = local
        .scalar_ty()
        .ok_or_else(|| decline("an assignment to an array-boundary local").at(target.span))?;
    let rhs = emit_expr(value, ctx, scope)?;
    match op {
        AssignOp::Assign => {
            if rhs.ty != target_ty {
                return Err(decline("an assignment whose value type differs").at(value.span));
            }
            out.push_str(&format!("    {} = {};\n", mangle(name), rhs.rs));
            Ok(())
        }
        // Compound assignment `x op= e` desugars to `x = x op e` THROUGH the
        // shared arith leaf at the COMPOUND-ASSIGN span (the interpreter's
        // op-span convention: the whole assignment statement's span), so an
        // overflow/div0 faults byte-identically.
        AssignOp::Add | AssignOp::Sub | AssignOp::Mul | AssignOp::Div | AssignOp::Rem => {
            let bin = match op {
                AssignOp::Add => BinaryOp::Add,
                AssignOp::Sub => BinaryOp::Sub,
                AssignOp::Mul => BinaryOp::Mul,
                AssignOp::Div => BinaryOp::Div,
                AssignOp::Rem => BinaryOp::Rem,
                _ => unreachable!(),
            };
            let lhs_read = Lowered {
                rs: mangle(name),
                ty: target_ty,
            };
            // The compound-assign fault span is the WHOLE assignment statement's
            // span — exactly what the boxed backend passes to `binary_value`, so
            // a compound-assign overflow/div0 faults byte-identically.
            let combined = lower_binary(bin, &lhs_read, &rhs, stmt_span)?;
            if combined.ty != target_ty {
                return Err(decline("a compound assignment that changes type").at(value.span));
            }
            out.push_str(&format!("    {} = {};\n", mangle(name), combined.rs));
            Ok(())
        }
        AssignOp::Coalesce => Err(decline("a `??=` assignment").at(target.span)),
    }
}

/// Lower `arr[i] (op)= value` for a mutable scalar-array boundary local. The
/// generated order mirrors the boxed emitter and interpreter: array root, index,
/// `index_slot` validation/fault, then RHS evaluation and write-back.
pub(super) fn emit_array_index_assign(
    target: &Expr,
    op: AssignOp,
    value: &Expr,
    stmt_span: Span,
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
    out: &mut String,
) -> Result<(), EmitError> {
    let ExprKind::Index { object, index } = &target.kind else {
        return Err(decline("a non-index array assignment").at(target.span));
    };
    let ExprKind::Ident = &object.kind else {
        return Err(decline("an index assignment to a non-local array").at(object.span));
    };
    let name = text(ctx.src, object.span);
    let local = scope
        .iter()
        .rev()
        .find(|l| l.name == name)
        .ok_or_else(|| decline("an index assignment to a non-array binding").at(object.span))?;
    if !local.mutable {
        return Err(decline("an index assignment to an immutable array").at(target.span));
    }
    let elem = local
        .array_elem()
        .ok_or_else(|| decline("an index assignment to a non-array binding").at(object.span))?;
    let idx = emit_expr(index, ctx, scope)?;
    if idx.ty != NativeTy::I64 {
        return Err(decline("a non-int array index assignment").at(index.span));
    }
    let rhs = emit_expr(value, ctx, scope)?;
    if rhs.ty != elem {
        return Err(decline("an array assignment whose value type differs").at(value.span));
    }

    let sp = emit_span(stmt_span);
    let root = mangle(name);
    match op {
        AssignOp::Assign => {
            let boxed_rhs = elem.box_expr("__nia_v");
            out.push_str(&format!(
                "    {{ let __nia_base = {root}.clone(); let __nia_i = {}; let __nia_idx = Value::Int(__nia_i); let (__nia_store, __nia_k) = index_slot(&__nia_base, &__nia_idx, {sp})?; let __nia_v = {}; __nia_store.borrow_mut()[__nia_k] = {boxed_rhs}; }}\n",
                idx.rs, rhs.rs
            ));
            Ok(())
        }
        AssignOp::Add | AssignOp::Sub | AssignOp::Mul | AssignOp::Div | AssignOp::Rem => {
            let bop = match op {
                AssignOp::Add => "Add",
                AssignOp::Sub => "Sub",
                AssignOp::Mul => "Mul",
                AssignOp::Div => "Div",
                AssignOp::Rem => "Rem",
                AssignOp::Assign | AssignOp::Coalesce => unreachable!("handled by match arms"),
            };
            let boxed_rhs = elem.box_expr("__nia_v");
            out.push_str(&format!(
                "    {{ let __nia_base = {root}.clone(); let __nia_i = {}; let __nia_idx = Value::Int(__nia_i); let (__nia_store, __nia_k) = index_slot(&__nia_base, &__nia_idx, {sp})?; let __nia_cur = __nia_store.borrow()[__nia_k].clone(); let __nia_v = {}; let __nia_rhs = {boxed_rhs}; let __nia_new = binary_value(BinaryOp::{bop}, __nia_cur, __nia_rhs, {sp})?; __nia_store.borrow_mut()[__nia_k] = __nia_new; }}\n",
                idx.rs, rhs.rs
            ));
            Ok(())
        }
        AssignOp::Coalesce => Err(decline("a `??=` array assignment").at(target.span)),
    }
}

/// Lower a tail expression in DISCARD position (its value is thrown away): an
/// `if`/block tail routes through the statement path (so its arms may
/// `break`/`continue`/`let`); any other tail discards through `let _ = …;` (its
/// effects/faults still run). Shared by every statement-position block tail.
pub(super) fn emit_discard_tail(
    tail: &Expr,
    ctx: &mut Ctx<'_>,
    scope: &[NativeLocal],
    out: &mut String,
    in_loop: bool,
) -> Result<(), EmitError> {
    match &tail.kind {
        ExprKind::If { .. } => emit_if_stmt(tail, ctx, scope, out, in_loop),
        ExprKind::Block(block) => {
            let rs = emit_stmt_block(block, ctx, scope, in_loop)?;
            out.push_str(&format!("    {rs}\n"));
            Ok(())
        }
        _ => {
            let low = emit_expr(tail, ctx, scope)?;
            out.push_str(&format!("    let _ = {};\n", low.rs));
            Ok(())
        }
    }
}

/// Lower a statement-position `for` loop over a native scalar iterable. The
/// iterable itself is still produced through the shared boxed leaves
/// (`make_range`/`for_items` or a boxed scalar array), then each item is unboxed
/// to the checker-confirmed scalar loop binding.
pub(super) fn emit_for_stmt(
    expr: &Expr,
    ctx: &mut Ctx<'_>,
    scope: &mut [NativeLocal],
    out: &mut String,
) -> Result<(), EmitError> {
    let ExprKind::For {
        pattern,
        iter,
        body,
    } = &expr.kind
    else {
        unreachable!("emit_for_stmt called on a non-for")
    };

    let (iter_call, elem) = emit_for_items(iter, ctx, scope, expr.span)?;
    let item_span = emit_span(expr.span);
    let mut body_scope = scope.to_vec();
    let bind_rs = match native_for_binding(pattern, ctx.src)? {
        Some((name, name_span)) => {
            if body_scope.iter().any(|local| local.name == name) {
                return Err(decline("a loop binding redeclaration").at(pattern.span));
            }
            ctx.confirm_local(name, name_span, elem.mono())
                .map_err(|e| e.at(pattern.span))?;
            let helper = native_unbox_helper(elem).ok_or_else(|| {
                decline("a non-unboxable native `for` loop element").at(pattern.span)
            })?;
            body_scope.push(NativeLocal {
                name: name.to_string(),
                kind: LocalKind::Scalar(elem),
                mutable: false,
            });
            format!(
                "let {} = {helper}(__native_item, {item_span})?; ",
                mangle(name)
            )
        }
        None => "let _ = __native_item; ".to_string(),
    };
    let body_rs = emit_loop_body(body, ctx, &body_scope)?;
    out.push_str(&format!(
        "    for __native_item in {iter_call} {{ {bind_rs}{body_rs} }}\n"
    ));
    Ok(())
}

pub(super) fn native_for_binding<'a>(
    pattern: &'a Pattern,
    src: &'a LoweredText,
) -> Result<Option<(&'a str, Span)>, EmitError> {
    match &pattern.kind {
        PatternKind::Wildcard => Ok(None),
        PatternKind::Binding(_) | PatternKind::Typed { .. } => Ok(simple_binding(pattern, src)),
        _ => Err(decline("a non-simple native `for` pattern").at(pattern.span)),
    }
}

pub(super) fn emit_for_items(
    iter: &Expr,
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
    for_span: Span,
) -> Result<(String, NativeTy), EmitError> {
    let for_span_rs = emit_span(for_span);
    match &iter.kind {
        ExprKind::Ident => {
            let name = text(ctx.src, iter.span);
            let local = scope
                .iter()
                .rev()
                .find(|local| local.name == name)
                .ok_or_else(|| decline("a native `for` over a non-array binding").at(iter.span))?;
            let elem = local
                .array_elem()
                .ok_or_else(|| decline("a native `for` over a non-array binding").at(iter.span))?;
            Ok((
                format!("for_items(&{}, {for_span_rs})?", mangle(name)),
                elem,
            ))
        }
        ExprKind::Array(_) => {
            let (arr_rs, elem) = emit_boxed_scalar_array_inferred(iter, ctx, scope)?;
            Ok((format!("for_items(&({arr_rs}), {for_span_rs})?"), elem))
        }
        ExprKind::Range {
            lo,
            hi,
            inclusive,
            step,
        } => {
            let lo_low = emit_expr(lo, ctx, scope)?;
            if lo_low.ty != NativeTy::I64 {
                return Err(decline("a non-int native range lower bound").at(lo.span));
            }
            let hi_low = emit_expr(hi, ctx, scope)?;
            if hi_low.ty != NativeTy::I64 {
                return Err(decline("a non-int native range upper bound").at(hi.span));
            }
            let step_rs = match step {
                Some(step) => {
                    let step_low = emit_expr(step, ctx, scope)?;
                    if step_low.ty != NativeTy::I64 {
                        return Err(decline("a non-int native range step").at(step.span));
                    }
                    format!("Some(Value::Int({}))", step_low.rs)
                }
                None => "None".to_string(),
            };
            let range_span = emit_span(iter.span);
            let range_rs = format!(
                "make_range(Value::Int({}), Value::Int({}), {inclusive}, {step_rs}, {range_span})?",
                lo_low.rs, hi_low.rs
            );
            Ok((
                format!("for_items(&({range_rs}), {for_span_rs})?"),
                NativeTy::I64,
            ))
        }
        _ => Err(decline("a native `for` over a non-scalar iterable").at(iter.span)),
    }
}

/// Lower a `while`/`for` loop BODY to a Rust block evaluating to `()`. The body
/// has its OWN scope (a copy of the visible scalars), fresh per iteration like
/// the interpreter's per-iteration child env.
///
/// CHECKPOINT ELISION (the v5.4 perf-unlock): when the unit has no `concurrent`
/// (`ctx.elide_checkpoints`), the back-edge `checkpoint().await` is DROPPED — the
/// loop is a plain Rust loop with no per-iteration async suspension. Otherwise the
/// checkpoint is kept at the body start (reached every iteration, including after
/// `continue`), identical to the boxed backend, so a `while`-spinning `concurrent`
/// arm still yields to the round-robin scheduler. Either way the loop's results,
/// termination, and faults are byte-identical (the checkpoint enforces no budget;
/// `block_on` treats `Pending` as a transparent re-poll).
pub(super) fn emit_loop_body(
    block: &Block,
    ctx: &mut Ctx<'_>,
    scope: &[NativeLocal],
) -> Result<String, EmitError> {
    let elide = ctx.elide_checkpoints;
    let mut body_scope = scope.to_vec();
    let mut body = String::new();
    for stmt in &block.stmts {
        emit_stmt(stmt, ctx, &mut body_scope, &mut body, true)?;
    }
    // A loop body is a STATEMENT: its tail value is discarded (effects still
    // run). An `if`/block tail lowers via the statement path so its arms may
    // `break`/`continue`/`let`; any other tail discards through `let _ = …;`.
    if let Some(tail) = block.tail.as_deref() {
        emit_discard_tail(tail, ctx, &body_scope, &mut body, true)?;
    }
    if elide {
        Ok(format!("{{ {body}}}"))
    } else {
        Ok(format!("{{ checkpoint().await; {body}}}"))
    }
}
