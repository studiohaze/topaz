use crate::*;

pub(crate) fn emit_concurrent_expr(
    expr: &Expr,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprKind::Concurrent {
        timeout,
        arms,
        else_block,
    } = &expr.kind
    else {
        unreachable!("concurrent helper received another expression kind");
    };
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let lowered = {
        let timeout_route = match (timeout.as_deref(), else_block.as_deref()) {
            (None, None) => None,
            (Some(duration), Some(else_block)) => {
                Some((concurrent_timeout_ms(duration, src)?, else_block))
            }
            _ => return Err(EmitError::unsupported("concurrent timeout without else")),
        };
        let all_arms_are_instant = arms
            .iter()
            .all(|arm| expr_is_instant_concurrent_arm(&arm.value));
        let zero_timeout = timeout_route.is_some_and(|(ms, _)| ms == 0);
        let zero_timeout_single = zero_timeout && arms.len() == 1;
        let zero_timeout_single_instant = zero_timeout_single && all_arms_are_instant;
        let zero_timeout_multi_instant_else =
            zero_timeout && arms.len() > 1 && all_arms_are_instant;

        if let Some((ms, else_blk)) = timeout_route {
            let nonzero_timeout_all_instant = ms > 0 && all_arms_are_instant;
            if block_has_bare_return(else_blk)
                && !zero_timeout_single_instant
                && !zero_timeout_multi_instant_else
                && !nonzero_timeout_all_instant
            {
                return Err(EmitError::unsupported("`return`/`?` in a concurrent else"));
            }
            if zero_timeout_multi_instant_else {
                return emit_block(else_blk, src, aliases, locals, in_loop);
            }
            if zero_timeout_single_instant && !block_has_try_expr(else_blk) {
                if expr_has_bare_return(&arms[0].value) {
                    return Err(EmitError::unsupported("`return`/`?` in a concurrent arm"));
                }
                return emit_zero_timeout_single_instant_concurrent_expr(
                    &arms[0], else_blk, src, aliases, locals,
                );
            }
        }

        let mut entries: Vec<String> = Vec::with_capacity(arms.len());
        for arm in arms {
            // An arm lowers as a no-parameter CLOSURE, but the interpreter runs it with
            // NO function boundary: a bare `return` faults ("return outside a function"),
            // and a `?` on its `Err` branch faults the same at top level. Emitting the
            // closure would instead `return`/`?` out of the closure — an observable
            // boundary mismatch — so refuse it (control-flow-aware lowering is a later
            // step). A `return` nested in a deeper lambda is its own scope (not flagged).
            if expr_has_bare_return(&arm.value) {
                return Err(EmitError::unsupported("`return`/`?` in a concurrent arm"));
            }
            let name = text(src, arm.name.span);
            let captures = lambda_captures(&arm.value, &[], locals, src)?;
            let mut body_locals: Vec<(String, Bind)> = Vec::new();
            push_capture_locals(&captures, locals, &mut body_locals)?;
            // §17 an arm body is a capture-pruned closure (like a lambda): a
            // type-annotation head is not captured into `body_locals`, so refuse a
            // qualified type in it rather than risk resolving past a shadow the
            // interpreter's live arm env sees (`in_nested`).
            let arm_aliases = aliases.with_body(&[], true);
            // §14 a concurrent arm is its own closure — reset the flow (its block
            // defers + early exits are the arm's own, not the enclosing scope's).
            let body_rs = with_reset_flow(&arm_aliases, |a| {
                emit_expr(&arm.value, src, a, &body_locals, false)
            })?;
            let closure = emit_closure_value(ClosureEmission {
                param_names: &[],
                captures: &captures,
                defaults: &[],
                variadic: None,
                variadic_guard: None,
                param_guards: "",
                body: &body_rs,
                return_guard: None,
                has_defers: false,
            });
            // §4/§15 the arm wrapper is a synthetic zero-arg closure — run it
            // UNCOUNTED (so the arm body starts at the ambient depth, like the
            // interpreter's raw-eval arm, not one level deep) inside a per-arm
            // `depth_scoped` so interleaved/abandoned arms keep ISOLATED recursion
            // counters (no accumulation, no leak across the shared `cx`).
            entries.push(format!(
            "({name:?}.to_string(), depth_scoped(call_value_uncounted({closure}, vec![], cx.clone(), {}), cx.clone()))",
            emit_span(arm.span)
        ));
        }
        let arms_rs = entries.join(", ");
        match timeout_route {
            None => format!("concurrent_join(vec![{arms_rs}]).await?"),
            Some((ms, else_blk)) => {
                // The `else` block lowers like a `function` body (§7): its own captures,
                // then the statement sequence as the closure body — a no-parameter thunk
                // the executor drives only when the deadline fires.
                let else_captures = closure_captures_block(else_blk, &[], locals, src)?;
                let mut else_scope: Vec<(String, Bind)> = Vec::new();
                push_capture_locals(&else_captures, locals, &mut else_scope)?;
                let else_base = else_scope.len();
                // §17 the else thunk is a capture-pruned closure too — refuse a
                // qualified type in it (`in_nested`), same as a lambda/arm body.
                let else_aliases = aliases.with_body(&[], true);
                // §14 the timeout `else` is its OWN closure thunk — reset the flow
                // (save/clear/restore) so its block defers + any drain are the else's
                // own, never the enclosing scope's. (`return`/`?` are refused in the
                // else and `break`/`continue` lower not-in-loop, so this is also
                // future-proofing the invariant the other closure bodies hold.)
                let saved_else_flow = {
                    let mut f = else_aliases.flow.borrow_mut();
                    let s = (
                        std::mem::take(&mut f.stacks),
                        std::mem::take(&mut f.loop_markers),
                        f.fn_base,
                    );
                    f.fn_base = 0;
                    s
                };
                let else_seq = emit_stmt_seq(StatementSequenceEmission {
                    stmts: &else_blk.stmts,
                    tail: else_blk.tail.as_deref(),
                    src,
                    aliases: &else_aliases,
                    locals: &mut else_scope,
                    base: else_base,
                    in_loop: false,
                    defer_scope: false,
                    at_module_top: false,
                });
                {
                    let mut f = else_aliases.flow.borrow_mut();
                    f.stacks = saved_else_flow.0;
                    f.loop_markers = saved_else_flow.1;
                    f.fn_base = saved_else_flow.2;
                }
                let (else_lines, else_tail) = else_seq?;
                let else_body = format!("{{ {else_lines}{else_tail} }}");
                let else_closure = emit_closure_value(ClosureEmission {
                    param_names: &[],
                    captures: &else_captures,
                    defaults: &[],
                    variadic: None,
                    variadic_guard: None,
                    param_guards: "",
                    body: &else_body,
                    return_guard: None,
                    has_defers: false,
                });
                // §4 the timeout `else` runs ONCE after the arms are abandoned — the
                // interpreter pushes it as a NORMAL block at the ambient depth (not an
                // isolated arm), so run it UNCOUNTED (its synthetic wrapper must not
                // consume a level); its body then counts from the ambient like the
                // interpreter's `push_block`.
                let else_fut = format!(
                    "call_value_uncounted({else_closure}, vec![], cx.clone(), {})",
                    emit_span(else_blk.span)
                );
                format!(
                    "concurrent_join_timeout(cx.clone(), {ms}, vec![{arms_rs}], {else_fut}).await?"
                )
            }
        }
    };
    Ok(lowered)
}

pub(crate) fn emit_zero_timeout_single_instant_concurrent_expr(
    arm: &ConcurrentArm,
    else_blk: &Block,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    locals: &[(String, Bind)],
) -> Result<String, EmitError> {
    let name = text(src, arm.name.span);
    let value_rs = emit_expr(&arm.value, src, aliases, locals, false)?;
    let else_rs = emit_block(else_blk, src, aliases, locals, false)?;
    Ok(format!(
        "{{ match (async {{ Ok::<Value, RtError>({value_rs}) }}).await {{ \
         Ok(__v) => Value::record([({name:?}.to_string(), __v)]), \
         Err(_) => {else_rs}, }} }}"
    ))
}

pub(crate) fn text(src: &LoweredText, span: Span) -> &str {
    src.get(span)
        .expect("Lowered IR omitted a lexical atom required by Rust emission")
}

/// The §15 `concurrent(timeout: d)` duration literal in MILLISECONDS, parsed exactly as the
/// interpreter's `start_concurrent`: the leading ASCII digits of the literal's source text
/// scaled by the unit (`ms`/`s`/`m`). A non-`Duration` timeout is refused — the parser only
/// admits a duration literal there, so this is a belt-and-braces guard.
pub(crate) fn concurrent_timeout_ms(dur: &Expr, src: &LoweredText) -> Result<u64, EmitError> {
    match &dur.kind {
        ExprKind::Duration(_) => {}
        _ => {
            return Err(EmitError::unsupported(
                "concurrent timeout must be a duration literal",
            ));
        }
    }
    parse_duration_milliseconds(text(src, dur.span)).ok_or(EmitError::unsupported(
        "concurrent timeout duration overflows u64 milliseconds",
    ))
}

pub(crate) fn expr_is_instant_concurrent_arm(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Float
        | ExprKind::Bool(_)
        | ExprKind::Int
        | ExprKind::Null
        | ExprKind::Unit
        | ExprKind::Ident => true,
        ExprKind::Paren(inner) | ExprKind::Unary { operand: inner, .. } => {
            expr_is_instant_concurrent_arm(inner)
        }
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Compose { lhs, rhs } => {
            expr_is_instant_concurrent_arm(lhs) && expr_is_instant_concurrent_arm(rhs)
        }
        ExprKind::Range { lo, hi, step, .. } => {
            expr_is_instant_concurrent_arm(lo)
                && expr_is_instant_concurrent_arm(hi)
                && step.as_deref().is_none_or(expr_is_instant_concurrent_arm)
        }
        ExprKind::Array(elements) => elements.iter().all(|element| match element {
            ArrayElement::Expr(expr) | ArrayElement::Spread(expr) => {
                expr_is_instant_concurrent_arm(expr)
            }
        }),
        ExprKind::SetLiteral(elements) => elements.iter().all(expr_is_instant_concurrent_arm),
        ExprKind::MapLiteral(entries) => entries.iter().all(|(key, value)| {
            expr_is_instant_concurrent_arm(key) && expr_is_instant_concurrent_arm(value)
        }),
        ExprKind::RecordLiteral { fields } => fields
            .iter()
            .all(|field| expr_is_instant_concurrent_arm(&field.value)),
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            expr_is_instant_concurrent_arm(base)
                && spread
                    .as_ref()
                    .is_none_or(|expr| expr_is_instant_concurrent_arm(expr))
                && fields
                    .iter()
                    .all(|field| expr_is_instant_concurrent_arm(&field.value))
        }
        ExprKind::String(lit) => lit.parts.iter().all(|part| match part {
            StringPart::Text(_) => true,
            StringPart::Interpolation(expr) => expr_is_instant_concurrent_arm(expr),
        }),
        ExprKind::Block(block) => block_is_instant_concurrent_arm(block),
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            expr_is_instant_concurrent_arm(cond)
                && block_is_instant_concurrent_arm(then_block)
                && else_branch
                    .as_deref()
                    .is_none_or(expr_is_instant_concurrent_arm)
        }
        ExprKind::Match { scrutinee, cases } => {
            expr_is_instant_concurrent_arm(scrutinee)
                && cases.iter().all(|case| {
                    case.guard
                        .as_ref()
                        .is_none_or(expr_is_instant_concurrent_arm)
                        && match &case.body {
                            CaseArmBody::Expr(expr) => expr_is_instant_concurrent_arm(expr),
                            CaseArmBody::Return { .. } => false,
                        }
                })
        }
        ExprKind::Member { object, .. } | ExprKind::OptionalAccess { object, .. } => {
            expr_is_instant_concurrent_arm(object)
        }
        ExprKind::Index { object, index } => {
            expr_is_instant_concurrent_arm(object) && expr_is_instant_concurrent_arm(index)
        }
        ExprKind::Try(_)
        | ExprKind::Duration(_)
        | ExprKind::Placeholder
        | ExprKind::For { .. }
        | ExprKind::Loop { .. }
        | ExprKind::Concurrent { .. }
        | ExprKind::Call { .. }
        | ExprKind::Pipe { .. }
        | ExprKind::Comprehension { .. }
        | ExprKind::Lambda { .. } => false,
    }
}

pub(crate) fn block_is_instant_concurrent_arm(block: &Block) -> bool {
    block.stmts.iter().all(|stmt| match &stmt.kind {
        StmtKind::Let { value, .. } | StmtKind::Const { value, .. } | StmtKind::Expr(value) => {
            expr_is_instant_concurrent_arm(value)
        }
        _ => false,
    }) && block
        .tail
        .as_deref()
        .is_none_or(expr_is_instant_concurrent_arm)
}
