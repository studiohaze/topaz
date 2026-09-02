use crate::*;

pub(super) fn emit_block_as_function(
    block: &Block,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    if block.stmts.is_empty() && block.tail.is_none() {
        writeln!(out, "{}__tpz_result = TPZ_UNIT", " ".repeat(indent)).expect("write to string");
        return Ok(());
    }
    ctx.push_scope();
    let snapshot = match pre_register_nested_functions(block, ctx, indent, out) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            ctx.pop_scope();
            return Err(error);
        }
    };
    let result = (|| -> Result<(), PyEmitError> {
        for stmt in &block.stmts {
            emit_stmt(stmt, ctx, indent, out)?;
        }
        match block.tail.as_deref() {
            Some(tail) => {
                if !emit_expr_to_target_if_needed(tail, "__tpz_result", ctx, indent, out)? {
                    let tail_py = emit_expr(tail, ctx)?;
                    writeln!(out, "{}__tpz_result = {tail_py}", " ".repeat(indent))
                        .expect("write to string");
                }
            }
            None => writeln!(out, "{}__tpz_result = TPZ_UNIT", " ".repeat(indent))
                .expect("write to string"),
        }
        Ok(())
    })();
    snapshot.restore(ctx);
    ctx.pop_scope();
    result
}

pub(super) fn emit_expr_to_target_if_needed(
    expr: &Expr,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<bool, PyEmitError> {
    match &expr.kind {
        ExprKind::Loop { label, body } => {
            emit_loop_expr_to_target(*label, body, target_py, ctx, indent, out)?;
            Ok(true)
        }
        ExprKind::Concurrent {
            timeout,
            arms,
            else_block,
        } => {
            emit_concurrent_expr_to_target(
                timeout.as_deref(),
                arms,
                else_block.as_deref(),
                expr.span,
                StatementTarget::new(target_py, ctx, indent, out),
            )?;
            Ok(true)
        }
        _ if expr_needs_statement_lowering(expr, ctx) => {
            emit_statement_lowered_expr_to_target(expr, target_py, ctx, indent, out)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub(super) fn expr_needs_statement_lowering(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    expr_contains_statement_lowered_expr(expr)
        || expr_contains_cooperative_statement_lowering(expr, ctx)
        || expr_contains_eager_outer_mutation(expr, ctx)
}

pub(super) fn expr_contains_eager_outer_mutation(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    let mut scope = MutationScope::default();
    let mut names = BTreeSet::new();
    scope.push_frame();
    collect_mutated_outer_roots_in_expr(expr, ctx, &mut scope, &mut names);
    scope.pop_frame();
    !names.is_empty()
}

pub(super) fn expr_contains_statement_lowered_expr(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Loop { .. } | ExprKind::For { .. } => true,
        ExprKind::Paren(inner)
        | ExprKind::Unary { operand: inner, .. }
        | ExprKind::Try(inner)
        | ExprKind::Member { object: inner, .. }
        | ExprKind::OptionalAccess { object: inner, .. } => {
            expr_contains_statement_lowered_expr(inner)
        }
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Compose { lhs, rhs } => {
            expr_contains_statement_lowered_expr(lhs) || expr_contains_statement_lowered_expr(rhs)
        }
        ExprKind::String(lit) => lit.parts.iter().any(|part| match part {
            StringPart::Text(_) => false,
            StringPart::Interpolation(expr) => expr_contains_statement_lowered_expr(expr),
        }),
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            expr_contains_statement_lowered_expr(cond)
                || block_contains_statement_lowered_expr(then_block)
                || else_branch
                    .as_deref()
                    .is_some_and(expr_contains_statement_lowered_expr)
        }
        ExprKind::Match { scrutinee, cases } => {
            expr_contains_statement_lowered_expr(scrutinee)
                || cases.iter().any(|case| {
                    case.guard
                        .as_ref()
                        .is_some_and(expr_contains_statement_lowered_expr)
                        || match &case.body {
                            CaseArmBody::Expr(expr) => expr_contains_statement_lowered_expr(expr),
                            CaseArmBody::Return { value, .. } => value
                                .as_ref()
                                .is_some_and(expr_contains_statement_lowered_expr),
                        }
                })
        }
        ExprKind::Call { callee, args, .. } => {
            expr_contains_statement_lowered_expr(callee)
                || args.iter().any(|arg| match arg {
                    CallArg::Positional(expr) | CallArg::Spread(expr) => {
                        expr_contains_statement_lowered_expr(expr)
                    }
                    CallArg::Named { value, .. } => expr_contains_statement_lowered_expr(value),
                })
        }
        ExprKind::Index { object, index } => {
            expr_contains_statement_lowered_expr(object)
                || expr_contains_statement_lowered_expr(index)
        }
        ExprKind::Array(elements) => elements.iter().any(|element| match element {
            ArrayElement::Expr(expr) | ArrayElement::Spread(expr) => {
                expr_contains_statement_lowered_expr(expr)
            }
        }),
        ExprKind::SetLiteral(elements) => elements.iter().any(expr_contains_statement_lowered_expr),
        ExprKind::MapLiteral(entries) => entries.iter().any(|(key, value)| {
            expr_contains_statement_lowered_expr(key)
                || expr_contains_statement_lowered_expr(value)
                || expr_is_lambda_literal(value)
        }),
        ExprKind::Block(block) => block_contains_statement_lowered_expr(block),
        ExprKind::Range { lo, hi, step, .. } => {
            expr_contains_statement_lowered_expr(lo)
                || expr_contains_statement_lowered_expr(hi)
                || step
                    .as_deref()
                    .is_some_and(expr_contains_statement_lowered_expr)
        }
        ExprKind::Comprehension { clauses, body, .. } => {
            clauses.iter().any(|clause| match clause {
                CompClause::For { iter, .. } => expr_contains_statement_lowered_expr(iter),
                CompClause::If(cond) => expr_contains_statement_lowered_expr(cond),
            }) || match body.as_ref() {
                CompBody::Elem(expr) => expr_contains_statement_lowered_expr(expr),
                CompBody::Entry { key, value } => {
                    expr_contains_statement_lowered_expr(key)
                        || expr_contains_statement_lowered_expr(value)
                }
            }
        }
        ExprKind::RecordLiteral { fields } => fields
            .iter()
            .any(|field| expr_contains_statement_lowered_expr(&field.value)),
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            expr_contains_statement_lowered_expr(base)
                || spread
                    .as_deref()
                    .is_some_and(expr_contains_statement_lowered_expr)
                || fields
                    .iter()
                    .any(|field| expr_contains_statement_lowered_expr(&field.value))
        }
        ExprKind::Pipe { lhs, rhs } => {
            expr_contains_statement_lowered_expr(lhs)
                || matches!(rhs.as_ref(), PipeRhs::Expr(stage) if expr_contains_statement_lowered_expr(stage))
        }
        ExprKind::Float
        | ExprKind::Bool(_)
        | ExprKind::Int
        | ExprKind::Null
        | ExprKind::Unit
        | ExprKind::Ident
        | ExprKind::Duration(_)
        | ExprKind::Placeholder => false,
        ExprKind::Lambda { body, .. } => expr_contains_statement_lowered_expr(body),
        ExprKind::Concurrent { .. } => true,
    }
}

pub(super) fn expr_contains_cooperative_statement_lowering(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    if !ctx.cooperative_yields {
        return false;
    }
    match &expr.kind {
        ExprKind::Call { callee, args, .. } => {
            callee_is_cooperative_optional_receiver_hof(callee, ctx)
                || expr_contains_cooperative_statement_lowering(callee, ctx)
                || args
                    .iter()
                    .any(|arg| call_arg_contains_cooperative_statement_lowering(arg, ctx))
        }
        ExprKind::Paren(inner)
        | ExprKind::Unary { operand: inner, .. }
        | ExprKind::Try(inner)
        | ExprKind::Member { object: inner, .. }
        | ExprKind::OptionalAccess { object: inner, .. } => {
            expr_contains_cooperative_statement_lowering(inner, ctx)
        }
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Compose { lhs, rhs } => {
            expr_contains_cooperative_statement_lowering(lhs, ctx)
                || expr_contains_cooperative_statement_lowering(rhs, ctx)
        }
        ExprKind::String(lit) => lit.parts.iter().any(|part| match part {
            StringPart::Text(_) => false,
            StringPart::Interpolation(expr) => {
                expr_contains_cooperative_statement_lowering(expr, ctx)
            }
        }),
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            expr_contains_cooperative_statement_lowering(cond, ctx)
                || block_contains_cooperative_statement_lowering(then_block, ctx)
                || else_branch
                    .as_deref()
                    .is_some_and(|expr| expr_contains_cooperative_statement_lowering(expr, ctx))
        }
        ExprKind::Match { scrutinee, cases } => {
            expr_contains_cooperative_statement_lowering(scrutinee, ctx)
                || cases.iter().any(|case| {
                    case.guard
                        .as_ref()
                        .is_some_and(|expr| expr_contains_cooperative_statement_lowering(expr, ctx))
                        || match &case.body {
                            CaseArmBody::Expr(expr) => {
                                expr_contains_cooperative_statement_lowering(expr, ctx)
                            }
                            CaseArmBody::Return { value, .. } => {
                                value.as_ref().is_some_and(|expr| {
                                    expr_contains_cooperative_statement_lowering(expr, ctx)
                                })
                            }
                        }
                })
        }
        ExprKind::Index { object, index } => {
            expr_contains_cooperative_statement_lowering(object, ctx)
                || expr_contains_cooperative_statement_lowering(index, ctx)
        }
        ExprKind::Array(elements) => elements.iter().any(|element| match element {
            ArrayElement::Expr(expr) | ArrayElement::Spread(expr) => {
                expr_contains_cooperative_statement_lowering(expr, ctx)
            }
        }),
        ExprKind::SetLiteral(elements) => elements
            .iter()
            .any(|expr| expr_contains_cooperative_statement_lowering(expr, ctx)),
        ExprKind::MapLiteral(entries) => entries.iter().any(|(key, value)| {
            expr_contains_cooperative_statement_lowering(key, ctx)
                || expr_contains_cooperative_statement_lowering(value, ctx)
        }),
        ExprKind::Block(block) => block_contains_cooperative_statement_lowering(block, ctx),
        ExprKind::Range { lo, hi, step, .. } => {
            expr_contains_cooperative_statement_lowering(lo, ctx)
                || expr_contains_cooperative_statement_lowering(hi, ctx)
                || step
                    .as_deref()
                    .is_some_and(|expr| expr_contains_cooperative_statement_lowering(expr, ctx))
        }
        ExprKind::Comprehension { .. } => true,
        ExprKind::RecordLiteral { fields } => fields
            .iter()
            .any(|field| expr_contains_cooperative_statement_lowering(&field.value, ctx)),
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            nominal_record_construct_has_dynamic_default(base, ctx)
                || expr_contains_cooperative_statement_lowering(base, ctx)
                || spread
                    .as_deref()
                    .is_some_and(|expr| expr_contains_cooperative_statement_lowering(expr, ctx))
                || fields
                    .iter()
                    .any(|field| expr_contains_cooperative_statement_lowering(&field.value, ctx))
        }
        ExprKind::Pipe { lhs, rhs } => {
            expr_contains_cooperative_statement_lowering(lhs, ctx)
                || matches!(rhs.as_ref(), PipeRhs::Expr(stage) if expr_contains_cooperative_statement_lowering(stage, ctx))
        }
        ExprKind::Int
        | ExprKind::Float
        | ExprKind::Duration(_)
        | ExprKind::Bool(_)
        | ExprKind::Null
        | ExprKind::Unit
        | ExprKind::Ident
        | ExprKind::Placeholder
        | ExprKind::For { .. }
        | ExprKind::Lambda { .. }
        | ExprKind::Concurrent { .. }
        | ExprKind::Loop { .. } => false,
    }
}

pub(super) fn nominal_record_construct_has_dynamic_default(base: &Expr, ctx: &Ctx<'_>) -> bool {
    nominal_record_for_construct_base(base, ctx).is_some_and(|record| {
        record.fields.iter().any(|field| {
            field
                .default
                .as_ref()
                .is_some_and(|default| default.helper_py_names.is_some())
        })
    })
}

pub(super) fn block_contains_cooperative_statement_lowering(block: &Block, ctx: &Ctx<'_>) -> bool {
    block
        .tail
        .as_deref()
        .is_some_and(|expr| expr_contains_cooperative_statement_lowering(expr, ctx))
}

pub(super) fn call_arg_contains_cooperative_statement_lowering(
    arg: &CallArg,
    ctx: &Ctx<'_>,
) -> bool {
    match arg {
        CallArg::Positional(expr) | CallArg::Spread(expr) => {
            expr_contains_cooperative_statement_lowering(expr, ctx)
        }
        CallArg::Named { value, .. } => expr_contains_cooperative_statement_lowering(value, ctx),
    }
}

pub(super) fn callee_is_cooperative_optional_receiver_hof(callee: &Expr, ctx: &Ctx<'_>) -> bool {
    match &callee.kind {
        ExprKind::OptionalAccess { object, field } => {
            let method = ctx.text(field.span);
            optional_receiver_inner_shape(object, ctx)
                .is_some_and(|shape| optional_receiver_hof_uses_cooperative_driver(method, shape))
        }
        ExprKind::Paren(inner) => callee_is_cooperative_optional_receiver_hof(inner, ctx),
        _ => false,
    }
}

pub(super) fn optional_receiver_hof_uses_cooperative_driver(
    method: &str,
    shape: ReceiverShape,
) -> bool {
    matches!(
        (method, shape),
        (
            "map",
            ReceiverShape::Array | ReceiverShape::Option | ReceiverShape::Result
        ) | ("filter", ReceiverShape::Array | ReceiverShape::Map)
            | ("reduce", ReceiverShape::Array)
            | ("sortedBy", ReceiverShape::Array)
            | ("sortBy", ReceiverShape::Array)
            | ("retain", ReceiverShape::Array)
            | ("flatMap", ReceiverShape::Option | ReceiverShape::Result)
            | ("okOrElse", ReceiverShape::Option)
            | ("mapValues", ReceiverShape::Map)
            | ("update", ReceiverShape::Map)
    )
}

pub(super) fn expr_is_lambda_literal(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Lambda { .. } => true,
        ExprKind::Paren(inner) => expr_is_lambda_literal(inner),
        _ => false,
    }
}

pub(super) fn block_contains_statement_lowered_expr(block: &Block) -> bool {
    if !block.stmts.is_empty() {
        return true;
    }
    block
        .tail
        .as_deref()
        .is_some_and(expr_contains_statement_lowered_expr)
}

pub(super) fn call_arg_contains_statement_lowered_expr(arg: &CallArg) -> bool {
    match arg {
        CallArg::Positional(expr) | CallArg::Spread(expr) => {
            expr_contains_statement_lowered_expr(expr)
        }
        CallArg::Named { value, .. } => expr_contains_statement_lowered_expr(value),
    }
}

pub(super) fn emit_statement_lowered_expr_value(
    expr: &Expr,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<String, PyEmitError> {
    if expr_needs_statement_lowering(expr, ctx) {
        let tmp = ctx.fresh_temp("expr_value");
        emit_statement_lowered_expr_to_target(expr, &tmp, ctx, indent, out)?;
        Ok(tmp)
    } else {
        emit_expr(expr, ctx)
    }
}

pub(super) fn emit_statement_lowered_map_value(
    value: &Expr,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<String, PyEmitError> {
    if map_value_needs_cooperative_callable(value, ctx) {
        let regular = ctx.with_cooperative_yields(false, |ctx| emit_expr(value, ctx))?;
        let cooperative = ctx.with_cooperative_yields(true, |ctx| emit_expr(value, ctx))?;
        return Ok(format!(
            "tpz_cooperative_callable({regular}, {cooperative})"
        ));
    }
    emit_statement_lowered_expr_value(value, ctx, indent, out)
}

pub(super) fn map_value_needs_cooperative_callable(value: &Expr, ctx: &Ctx<'_>) -> bool {
    match &value.kind {
        ExprKind::Lambda { body, .. } => expr_contains_cooperative_known_call(body, ctx),
        ExprKind::Paren(inner) => map_value_needs_cooperative_callable(inner, ctx),
        _ => false,
    }
}

pub(super) fn bind_statement_lowered_expr_value(
    expr: &Expr,
    tmp_hint: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<String, PyEmitError> {
    let tmp = ctx.fresh_temp(tmp_hint);
    if expr_needs_statement_lowering(expr, ctx) {
        emit_statement_lowered_expr_to_target(expr, &tmp, ctx, indent, out)?;
    } else {
        let pad = " ".repeat(indent);
        let value_py = emit_expr(expr, ctx)?;
        writeln!(out, "{pad}{tmp} = {value_py}").expect("write to string");
    }
    Ok(tmp)
}

pub(super) fn emit_statement_lowered_expr_to_target(
    expr: &Expr,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    match &expr.kind {
        ExprKind::Loop { label, body } => {
            emit_loop_expr_to_target(*label, body, target_py, ctx, indent, out)
        }
        ExprKind::Concurrent {
            timeout,
            arms,
            else_block,
        } => emit_concurrent_expr_to_target(
            timeout.as_deref(),
            arms,
            else_block.as_deref(),
            expr.span,
            StatementTarget::new(target_py, ctx, indent, out),
        ),
        ExprKind::For {
            pattern,
            iter,
            body,
        } => emit_for_expr_to_target(
            pattern,
            iter,
            body,
            expr.span,
            StatementTarget::new(target_py, ctx, indent, out),
        ),
        ExprKind::Comprehension {
            kind,
            clauses,
            body,
        } => emit_statement_lowered_comprehension_to_target(
            *kind,
            clauses,
            body,
            expr.span,
            StatementTarget::new(target_py, ctx, indent, out),
        ),
        ExprKind::Lambda { params, body } => {
            emit_statement_lowered_lambda_to_target(params, body, None, target_py, ctx, indent, out)
        }
        ExprKind::Paren(inner) => {
            let value_py = emit_statement_lowered_expr_value(inner, ctx, indent, out)?;
            writeln!(out, "{pad}{target_py} = {value_py}").expect("write to string");
            Ok(())
        }
        ExprKind::Unary { op, operand } => {
            let operand_py = emit_statement_lowered_expr_value(operand, ctx, indent, out)?;
            let value_py = emit_unary_from_py(*op, &operand_py, expr.span);
            writeln!(out, "{pad}{target_py} = {value_py}").expect("write to string");
            Ok(())
        }
        ExprKind::Binary { op, lhs, rhs } => {
            match op {
                BinaryOp::And | BinaryOp::Or => {
                    return emit_statement_lowered_lazy_bool_to_target(
                        *op,
                        lhs,
                        rhs,
                        expr.span,
                        StatementTarget {
                            target_py,
                            ctx,
                            indent,
                            out,
                        },
                    );
                }
                BinaryOp::Coalesce => {
                    return emit_statement_lowered_coalesce_to_target(
                        lhs, rhs, target_py, ctx, indent, out,
                    );
                }
                _ => {}
            }
            let lhs_py = emit_statement_lowered_expr_value(lhs, ctx, indent, out)?;
            let rhs_py = emit_statement_lowered_expr_value(rhs, ctx, indent, out)?;
            let value_py = emit_binary_from_py(*op, &lhs_py, &rhs_py, expr.span)?;
            writeln!(out, "{pad}{target_py} = {value_py}").expect("write to string");
            Ok(())
        }
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => emit_statement_lowered_if_to_target(
            cond,
            then_block,
            else_branch.as_deref(),
            expr.span,
            StatementTarget {
                target_py,
                ctx,
                indent,
                out,
            },
        ),
        ExprKind::Match { scrutinee, cases } => emit_statement_lowered_match_to_target(
            scrutinee, cases, target_py, expr.span, ctx, indent, out,
        ),
        ExprKind::Block(block) => {
            emit_statement_lowered_block_expr_to_target(block, target_py, ctx, indent, out)
        }
        ExprKind::String(lit) if lit.tag.is_none() => {
            let value_py = emit_string_with_statement_values(lit, ctx, indent, out)?;
            writeln!(out, "{pad}{target_py} = {value_py}").expect("write to string");
            Ok(())
        }
        ExprKind::String(lit) => {
            let value_py = emit_template_with_statement_values(lit, ctx, indent, out)?;
            writeln!(out, "{pad}{target_py} = {value_py}").expect("write to string");
            Ok(())
        }
        ExprKind::Call {
            callee,
            args,
            type_args,
        } => {
            if let Some((params, body)) = immediate_lambda_callee(callee) {
                let mutated_outer_roots =
                    immediate_lambda_mutated_outer_roots(params, body, args, ctx);
                emit_statement_lowered_immediate_lambda_call_to_target(
                    callee, args, target_py, ctx, indent, out,
                )?;
                for root in mutated_outer_roots {
                    ctx.clear_collection_alias_value_metadata(&root);
                }
                return Ok(());
            }
            if collection_mutation_root_for_call(callee, ctx).is_some() {
                emit_statement_lowered_call_to_target(
                    callee, args, expr.span, target_py, ctx, indent, out,
                )?;
                apply_collection_mutation_metadata(callee, args, ctx);
                return Ok(());
            }
            let known_function_mutated_roots =
                known_function_call_mutated_outer_roots(callee, args, ctx);
            if !known_function_mutated_roots.is_empty() {
                emit_statement_lowered_call_to_target(
                    callee, args, expr.span, target_py, ctx, indent, out,
                )?;
                for root in known_function_mutated_roots {
                    ctx.clear_collection_alias_value_metadata(&root);
                }
                return Ok(());
            }
            if !type_args.is_empty()
                && let Some(method) = typed_json_call_method(callee, ctx)
            {
                if type_args.len() != 1 {
                    return Err(PyEmitError::unsupported("typed JSON type arguments").at(expr.span));
                }
                let schema = emit_json_schema(&type_args[0], ctx)?;
                let params = if method == "parseAs" {
                    &["text"][..]
                } else {
                    &["value"][..]
                };
                return emit_statement_lowered_static_call_to_target(
                    args,
                    StaticCallSpec::new(params, &[], expr.span),
                    StatementTarget::new(target_py, ctx, indent, out),
                    |slots| render_typed_json_runtime_call(method, &slots[0], &schema, expr.span),
                );
            }
            emit_statement_lowered_call_to_target(
                callee, args, expr.span, target_py, ctx, indent, out,
            )
        }
        ExprKind::Array(elements) => {
            let mut values = Vec::with_capacity(elements.len());
            for element in elements {
                match element {
                    ArrayElement::Expr(expr) => {
                        values.push(emit_statement_lowered_expr_value(expr, ctx, indent, out)?);
                    }
                    ArrayElement::Spread(spread) => {
                        let value = emit_statement_lowered_expr_value(spread, ctx, indent, out)?;
                        values.push(format!(
                            "*tpz_spread_values({value}, {})",
                            py_span(spread.span)
                        ));
                    }
                }
            }
            writeln!(out, "{pad}{target_py} = [{}]", values.join(", ")).expect("write to string");
            Ok(())
        }
        ExprKind::SetLiteral(elements) => {
            let mut values = Vec::with_capacity(elements.len());
            for element in elements {
                values.push(emit_statement_lowered_expr_value(
                    element, ctx, indent, out,
                )?);
            }
            writeln!(
                out,
                "{pad}{target_py} = tpz_set_of([{}], {})",
                values.join(", "),
                py_span(expr.span)
            )
            .expect("write to string");
            Ok(())
        }
        ExprKind::MapLiteral(entries) => {
            let mut values = Vec::with_capacity(entries.len());
            for (key, value) in entries {
                let key_py = emit_statement_lowered_expr_value(key, ctx, indent, out)?;
                let value_py = emit_statement_lowered_map_value(value, ctx, indent, out)?;
                values.push(format!("({key_py}, {value_py})"));
            }
            writeln!(
                out,
                "{pad}{target_py} = tpz_map_of([{}], {})",
                values.join(", "),
                py_span(expr.span)
            )
            .expect("write to string");
            Ok(())
        }
        ExprKind::RecordLiteral { fields } => {
            let shape = record_shape(fields, ctx.map);
            let mut values = Vec::with_capacity(fields.len());
            for field in fields {
                values.push(emit_statement_lowered_expr_value(
                    &field.value,
                    ctx,
                    indent,
                    out,
                )?);
            }
            writeln!(
                out,
                "{pad}{target_py} = {}({})",
                record_class_name(&shape),
                values.join(", ")
            )
            .expect("write to string");
            Ok(())
        }
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            if let ExprKind::Ident = &base.kind {
                let head = ctx.text(base.span);
                if !ctx.binding_is_bound(head)
                    && let Some(record) = ctx.records.get(head).cloned()
                {
                    return emit_statement_lowered_nominal_record_construct_to_target(
                        &record,
                        spread.as_deref(),
                        fields,
                        expr.span,
                        StatementTarget::new(target_py, ctx, indent, out),
                    );
                }
            }
            if spread.is_some() {
                return Err(PyEmitError::unsupported("nominal record spread").at(expr.span));
            }
            let base_py = emit_statement_lowered_expr_value(base, ctx, indent, out)?;
            let mut updates = Vec::with_capacity(fields.len());
            for field in fields {
                let source_name = ctx.text(field.name.span);
                let value_py = emit_statement_lowered_expr_value(&field.value, ctx, indent, out)?;
                updates.push(format!(
                    "({}, {}, lambda: {value_py})",
                    py_string(&mangle(source_name)),
                    py_string(source_name)
                ));
            }
            writeln!(
                out,
                "{pad}{target_py} = tpz_record_update({base_py}, [{}], {})",
                updates.join(", "),
                py_span(expr.span)
            )
            .expect("write to string");
            Ok(())
        }
        ExprKind::Member { object, field } => {
            if let Some(value_py) =
                payloadless_enum_member_construct(object, field, expr.span, ctx)?
            {
                writeln!(out, "{pad}{target_py} = {value_py}").expect("write to string");
                return Ok(());
            }
            let object_py = emit_statement_lowered_expr_value(object, ctx, indent, out)?;
            let member = ctx.text(field.span);
            writeln!(
                out,
                "{pad}{target_py} = tpz_member({object_py}, {}, {}, {})",
                py_string(&mangle(member)),
                py_string(member),
                py_span(expr.span)
            )
            .expect("write to string");
            Ok(())
        }
        ExprKind::OptionalAccess { object, field } => {
            let object_py = emit_statement_lowered_expr_value(object, ctx, indent, out)?;
            let member = ctx.text(field.span);
            writeln!(
                out,
                "{pad}{target_py} = tpz_optional_member({object_py}, {}, {}, {})",
                py_string(&mangle(member)),
                py_string(member),
                py_span(expr.span)
            )
            .expect("write to string");
            Ok(())
        }
        ExprKind::Index { object, index } => {
            let object_py = emit_statement_lowered_expr_value(object, ctx, indent, out)?;
            let index_py = emit_statement_lowered_expr_value(index, ctx, indent, out)?;
            writeln!(
                out,
                "{pad}{target_py} = tpz_index({object_py}, {index_py}, {})",
                py_span(expr.span)
            )
            .expect("write to string");
            Ok(())
        }
        ExprKind::Range {
            lo,
            hi,
            inclusive,
            step,
        } => {
            let lo_py = emit_statement_lowered_expr_value(lo, ctx, indent, out)?;
            let hi_py = emit_statement_lowered_expr_value(hi, ctx, indent, out)?;
            let step_py = match step {
                Some(step) => emit_statement_lowered_expr_value(step, ctx, indent, out)?,
                None => "None".to_string(),
            };
            let inclusive_py = if *inclusive { "True" } else { "False" };
            writeln!(
                out,
                "{pad}{target_py} = tpz_range({lo_py}, {hi_py}, {inclusive_py}, {step_py}, {})",
                py_span(expr.span)
            )
            .expect("write to string");
            Ok(())
        }
        ExprKind::Try(inner) => {
            let inner_py = emit_statement_lowered_expr_value(inner, ctx, indent, out)?;
            writeln!(
                out,
                "{pad}{target_py} = tpz_try({inner_py}, {})",
                py_span(expr.span)
            )
            .expect("write to string");
            Ok(())
        }
        ExprKind::Pipe { lhs, rhs } => {
            emit_statement_lowered_pipe_to_target(lhs, rhs, expr.span, target_py, ctx, indent, out)
        }
        _ => {
            if expr_needs_statement_lowering(expr, ctx) {
                Err(PyEmitError::unsupported("statement-lowered expression shape").at(expr.span))
            } else {
                let value_py = emit_expr(expr, ctx)?;
                writeln!(out, "{pad}{target_py} = {value_py}").expect("write to string");
                Ok(())
            }
        }
    }
}

pub(super) fn emit_unary_from_py(op: UnaryOp, operand_py: &str, span: Span) -> String {
    match op {
        UnaryOp::Plus => operand_py.to_string(),
        UnaryOp::Minus => format!("tpz_neg({operand_py}, {})", py_span(span)),
        UnaryOp::Not => format!("not tpz_condition({operand_py}, {})", py_span(span)),
    }
}

pub(super) fn emit_binary_from_py(
    op: BinaryOp,
    lhs_py: &str,
    rhs_py: &str,
    span: Span,
) -> Result<String, PyEmitError> {
    let leaf = match op {
        BinaryOp::Add => "tpz_add",
        BinaryOp::Sub => "tpz_sub",
        BinaryOp::Mul => "tpz_mul",
        BinaryOp::Div => "tpz_div",
        BinaryOp::Rem => "tpz_rem_trunc_i64",
        BinaryOp::Pow => "tpz_pow",
        BinaryOp::Lt => "tpz_lt",
        BinaryOp::Le => "tpz_le",
        BinaryOp::Gt => "tpz_gt",
        BinaryOp::Ge => "tpz_ge",
        BinaryOp::Eq => "tpz_eq",
        BinaryOp::Ne => "tpz_ne",
        BinaryOp::In => "tpz_in",
        BinaryOp::And | BinaryOp::Or | BinaryOp::Coalesce => {
            return Err(PyEmitError::unsupported("binary operator").at(span));
        }
    };
    Ok(format!("{leaf}({lhs_py}, {rhs_py}, {})", py_span(span)))
}

pub(super) struct StatementTarget<'target, 'ctx, 'source, 'out> {
    pub(super) target_py: &'target str,
    pub(super) ctx: &'ctx mut Ctx<'source>,
    pub(super) indent: usize,
    pub(super) out: &'out mut String,
}

pub(super) struct StatementEmission<'ctx, 'source, 'out> {
    pub(super) ctx: &'ctx mut Ctx<'source>,
    pub(super) indent: usize,
    pub(super) out: &'out mut String,
}

impl<'ctx, 'source, 'out> StatementEmission<'ctx, 'source, 'out> {
    pub(super) fn new(ctx: &'ctx mut Ctx<'source>, indent: usize, out: &'out mut String) -> Self {
        Self { ctx, indent, out }
    }
}

impl<'target, 'ctx, 'source, 'out> StatementTarget<'target, 'ctx, 'source, 'out> {
    pub(super) fn new(
        target_py: &'target str,
        ctx: &'ctx mut Ctx<'source>,
        indent: usize,
        out: &'out mut String,
    ) -> Self {
        Self {
            target_py,
            ctx,
            indent,
            out,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct StaticCallSpec<'params> {
    pub(super) params: &'params [&'params str],
    pub(super) callback_arities: &'params [(usize, usize)],
    pub(super) span: Span,
}

#[derive(Clone, Copy)]
pub(super) struct PipeStaticCall<'call> {
    pub(super) lhs: &'call Expr,
    pub(super) params: &'call [&'call str],
    pub(super) callback_arities: &'call [(usize, usize)],
    pub(super) piped: &'call str,
    pub(super) span: Span,
}

impl<'call> PipeStaticCall<'call> {
    pub(super) fn new(
        lhs: &'call Expr,
        params: &'call [&'call str],
        callback_arities: &'call [(usize, usize)],
        piped: &'call str,
        span: Span,
    ) -> Self {
        Self {
            lhs,
            params,
            callback_arities,
            piped,
            span,
        }
    }
}

impl<'params> StaticCallSpec<'params> {
    pub(super) fn new(
        params: &'params [&'params str],
        callback_arities: &'params [(usize, usize)],
        span: Span,
    ) -> Self {
        Self {
            params,
            callback_arities,
            span,
        }
    }
}

pub(super) struct GuardedMatchCase<'case> {
    pub(super) target_py: Option<&'case str>,
    pub(super) matched_py: &'case str,
    pub(super) pattern_metadata: Option<&'case PatternBindingMetadata>,
}

pub(super) fn emit_statement_lowered_lazy_bool_to_target(
    op: BinaryOp,
    lhs: &Expr,
    rhs: &Expr,
    span: Span,
    target: StatementTarget<'_, '_, '_, '_>,
) -> Result<(), PyEmitError> {
    let StatementTarget {
        target_py,
        ctx,
        indent,
        out,
    } = target;
    let pad = " ".repeat(indent);
    let inner = " ".repeat(indent + 4);
    let span_py = py_span(span);
    let lhs_py = bind_statement_lowered_expr_value(lhs, "lazy_lhs", ctx, indent, out)?;
    writeln!(out, "{pad}if tpz_condition({lhs_py}, {span_py}):").expect("write to string");
    match op {
        BinaryOp::And => {
            let rhs_py = ctx.with_metadata_control_flow(|ctx| {
                emit_statement_lowered_expr_value(rhs, ctx, indent + 4, out)
            })?;
            writeln!(
                out,
                "{inner}{target_py} = tpz_condition({rhs_py}, {span_py})"
            )
            .expect("write to string");
            writeln!(out, "{pad}else:").expect("write to string");
            writeln!(out, "{inner}{target_py} = False").expect("write to string");
        }
        BinaryOp::Or => {
            writeln!(out, "{inner}{target_py} = True").expect("write to string");
            writeln!(out, "{pad}else:").expect("write to string");
            let rhs_py = ctx.with_metadata_control_flow(|ctx| {
                emit_statement_lowered_expr_value(rhs, ctx, indent + 4, out)
            })?;
            writeln!(
                out,
                "{inner}{target_py} = tpz_condition({rhs_py}, {span_py})"
            )
            .expect("write to string");
        }
        _ => unreachable!("only lazy boolean operators are lowered here"),
    }
    Ok(())
}

pub(super) fn emit_statement_lowered_coalesce_to_target(
    lhs: &Expr,
    rhs: &Expr,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let inner = " ".repeat(indent + 4);
    let lhs_py = bind_statement_lowered_expr_value(lhs, "coalesce_lhs", ctx, indent, out)?;
    writeln!(out, "{pad}if isinstance({lhs_py}, Some):").expect("write to string");
    writeln!(out, "{inner}{target_py} = {lhs_py}.value").expect("write to string");
    writeln!(out, "{pad}elif {lhs_py} is None or {lhs_py} is TPZ_NULL:").expect("write to string");
    let rhs_py = ctx.with_metadata_control_flow(|ctx| {
        emit_statement_lowered_expr_value(rhs, ctx, indent + 4, out)
    })?;
    writeln!(out, "{inner}{target_py} = {rhs_py}").expect("write to string");
    writeln!(out, "{pad}else:").expect("write to string");
    writeln!(out, "{inner}{target_py} = {lhs_py}").expect("write to string");
    Ok(())
}

pub(super) fn emit_statement_lowered_if_to_target(
    cond: &Expr,
    then_block: &Block,
    else_branch: Option<&Expr>,
    span: Span,
    target: StatementTarget<'_, '_, '_, '_>,
) -> Result<(), PyEmitError> {
    let StatementTarget {
        target_py,
        ctx,
        indent,
        out,
    } = target;
    let pad = " ".repeat(indent);
    let cond_py = emit_statement_lowered_expr_value(cond, ctx, indent, out)?;
    writeln!(out, "{pad}if tpz_condition({cond_py}, {}):", py_span(span)).expect("write to string");
    ctx.with_metadata_control_flow(|ctx| {
        emit_statement_lowered_block_expr_to_target(then_block, target_py, ctx, indent + 4, out)
    })?;
    writeln!(out, "{pad}else:").expect("write to string");
    match else_branch {
        Some(branch) => ctx.with_metadata_control_flow(|ctx| {
            emit_statement_lowered_expr_to_target(branch, target_py, ctx, indent + 4, out)
        }),
        None => {
            writeln!(out, "{}{} = TPZ_UNIT", " ".repeat(indent + 4), target_py)
                .expect("write to string");
            Ok(())
        }
    }
}

pub(super) fn emit_statement_lowered_block_expr_to_target(
    block: &Block,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    if block_has_direct_defer(block) {
        return emit_defer_scoped_block_expr_to_target(block, target_py, ctx, indent, out);
    }
    ctx.push_scope();
    let result =
        emit_statement_lowered_block_expr_contents_to_target(block, target_py, ctx, indent, out);
    ctx.pop_scope();
    result
}

pub(super) fn emit_defer_scoped_block_expr_to_target(
    block: &Block,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let mark = ctx.fresh_temp("defer_mark");
    writeln!(out, "{pad}{mark} = len(__tpz_defers)").expect("write to string");
    writeln!(out, "{pad}try:").expect("write to string");
    ctx.push_scope();
    let result = emit_statement_lowered_block_expr_contents_to_target(
        block,
        target_py,
        ctx,
        indent + 4,
        out,
    );
    ctx.pop_scope();
    result?;
    writeln!(out, "{pad}    __tpz_run_defers_to({mark})").expect("write to string");
    writeln!(
        out,
        "{pad}except (TpzReturn, TpzLoopBreak, TpzLoopContinue):"
    )
    .expect("write to string");
    writeln!(out, "{pad}    __tpz_run_defers_to({mark})").expect("write to string");
    writeln!(out, "{pad}    raise").expect("write to string");
    writeln!(out, "{pad}except TpzFault:").expect("write to string");
    writeln!(out, "{pad}    raise").expect("write to string");
    Ok(())
}

pub(super) fn emit_statement_lowered_block_expr_contents_to_target(
    block: &Block,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let snapshot = pre_register_nested_functions(block, ctx, indent, out)?;
    let result = (|| -> Result<(), PyEmitError> {
        for stmt in &block.stmts {
            emit_stmt(stmt, ctx, indent, out)?;
        }
        match block.tail.as_deref() {
            Some(tail) => emit_statement_lowered_expr_to_target(tail, target_py, ctx, indent, out),
            None => {
                let pad = " ".repeat(indent);
                writeln!(out, "{pad}{target_py} = TPZ_UNIT").expect("write to string");
                Ok(())
            }
        }
    })();
    snapshot.restore(ctx);
    result
}

#[derive(Clone, Debug)]
pub(super) struct PatternBindingMetadata {
    pub(super) name: String,
    pub(super) callable_params: Option<Vec<FunctionParamInfo>>,
    pub(super) receiver_shape: Option<ReceiverShape>,
    pub(super) wrapped_value_metadata: WrappedValueMetadataCatalog,
    pub(super) record_descendants: RecordDescendantCatalog,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct WrappedPatternValueProjection {
    pub(super) callable_params: Option<Vec<FunctionParamInfo>>,
    pub(super) receiver_shape: Option<ReceiverShape>,
    pub(super) wrapped_value_metadata: WrappedValueMetadataCatalog,
    pub(super) record_descendants: RecordDescendantCatalog,
}

impl WrappedPatternValueProjection {
    pub(super) fn from_catalog(
        catalog: &WrappedValueMetadataCatalog,
        wrapper: RecordWrapper,
        record_descendants: RecordDescendantCatalog,
    ) -> Self {
        let root = catalog.root(wrapper);
        Self {
            callable_params: root.callable_params,
            receiver_shape: root.receiver_shape,
            wrapped_value_metadata: catalog.projected(wrapper),
            record_descendants,
        }
    }

    pub(super) fn into_binding_metadata(self, name: String) -> Option<PatternBindingMetadata> {
        if self.callable_params.is_none()
            && self.receiver_shape.is_none()
            && self.wrapped_value_metadata.is_empty()
            && self.record_descendants.is_empty()
        {
            return None;
        }
        Some(PatternBindingMetadata {
            name,
            callable_params: self.callable_params,
            receiver_shape: self.receiver_shape,
            wrapped_value_metadata: self.wrapped_value_metadata,
            record_descendants: self.record_descendants,
        })
    }
}

pub(super) struct MapValuePatternProjection<'a> {
    pub(super) metadata: &'a MapValueMetadata,
    pub(super) observed: Option<StaticMapValueMetadata>,
    pub(super) record_descendants: RecordDescendantCatalog,
}

impl MapValuePatternProjection<'_> {
    pub(super) fn into_binding_metadata(self, name: String) -> Option<PatternBindingMetadata> {
        let callable_params = self
            .observed
            .as_ref()
            .and_then(|metadata| metadata.callable_params.clone())
            .or_else(|| self.metadata.declared_callable_params.clone());
        let receiver_shape = self
            .observed
            .as_ref()
            .and_then(|metadata| metadata.receiver_shape)
            .or(self.metadata.receiver_shape);
        let mut wrapped_value_metadata = self.metadata.wrapped_value_metadata.clone();
        if let Some(observed) = &self.observed {
            wrapped_value_metadata.overlay(observed.wrapped_value_metadata.clone());
        }
        if callable_params.is_none()
            && receiver_shape.is_none()
            && wrapped_value_metadata.is_empty()
            && self.record_descendants.is_empty()
        {
            return None;
        }
        Some(PatternBindingMetadata {
            name,
            callable_params,
            receiver_shape,
            wrapped_value_metadata,
            record_descendants: self.record_descendants,
        })
    }
}

pub(super) fn map_get_parts<'a>(value: &'a Expr, ctx: &Ctx<'_>) -> Option<(&'a Expr, &'a Expr)> {
    match &value.kind {
        ExprKind::Call { callee, args, .. } => {
            let ExprKind::Member { object, field } = &callee.kind else {
                return None;
            };
            if ctx.text(field.span) != "get" || args.len() != 1 {
                return None;
            }
            let key = match &args[0] {
                CallArg::Positional(key) => key,
                CallArg::Named { name, value } if ctx.text(name.span) == "k" => value,
                CallArg::Named { .. } | CallArg::Spread(_) => return None,
            };
            Some((object, key))
        }
        ExprKind::Paren(inner) => map_get_parts(inner, ctx),
        _ => None,
    }
}

pub(super) fn map_get_pattern_binding_metadata(
    scrutinee: &Expr,
    case: &CaseClause,
    ctx: &Ctx<'_>,
) -> Option<PatternBindingMetadata> {
    let name = some_pattern_binding_name(&case.pattern, ctx)?;
    let (object, key) = map_get_parts(scrutinee, ctx)?;
    ctx.map_value_pattern_projection(object, key)?
        .into_binding_metadata(name)
}

pub(super) fn wrapped_pattern_binding_metadata(
    scrutinee: &Expr,
    case: &CaseClause,
    wrapper: RecordWrapper,
    ctx: &Ctx<'_>,
) -> Option<PatternBindingMetadata> {
    let name = match wrapper {
        RecordWrapper::Option => some_pattern_binding_name(&case.pattern, ctx),
        RecordWrapper::ResultOk => ok_pattern_binding_name(&case.pattern, ctx),
        RecordWrapper::MapValue => None,
    }?;
    ctx.wrapped_pattern_value_projection(scrutinee, wrapper)
        .into_binding_metadata(name)
}

pub(super) fn pattern_binding_metadata(
    scrutinee: &Expr,
    case: &CaseClause,
    ctx: &Ctx<'_>,
) -> Option<PatternBindingMetadata> {
    map_get_pattern_binding_metadata(scrutinee, case, ctx)
        .or_else(|| wrapped_pattern_binding_metadata(scrutinee, case, RecordWrapper::Option, ctx))
        .or_else(|| wrapped_pattern_binding_metadata(scrutinee, case, RecordWrapper::ResultOk, ctx))
}

pub(super) fn register_pattern_binding_with_metadata(
    binding: &PatternBinding,
    pattern_metadata: Option<&PatternBindingMetadata>,
    ctx: &mut Ctx<'_>,
) -> String {
    let py_name = ctx.new_binding_py_name(&binding.name);
    let pattern_metadata = pattern_metadata.filter(|metadata| metadata.name == binding.name);
    if let Some(params) = pattern_metadata.and_then(|metadata| metadata.callable_params.as_ref()) {
        ctx.register_binding_with_callable_params(&binding.name, params.clone());
    } else {
        ctx.register_binding(&binding.name, false);
    }
    if let Some(metadata) = pattern_metadata {
        ctx.set_binding_receiver_shape(
            &binding.name,
            metadata.receiver_shape,
            metadata.wrapped_value_metadata.clone(),
        );
        ctx.set_binding_descendant_metadata(&binding.name, metadata.record_descendants.clone());
    }
    ctx.set_binding_py_name(&binding.name, py_name.clone());
    py_name
}

pub(super) fn emit_pattern_bindings_with_metadata(
    bindings: &[PatternBinding],
    pattern_metadata: Option<&PatternBindingMetadata>,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) {
    for binding in bindings {
        let py_name = register_pattern_binding_with_metadata(binding, pattern_metadata, ctx);
        write_pattern_binding_assignment(out, indent, &py_name, binding);
    }
}

pub(super) fn emit_statement_lowered_match_to_target(
    scrutinee: &Expr,
    cases: &[CaseClause],
    target_py: &str,
    span: Span,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    if cases
        .iter()
        .any(|case| case_guard_needs_statement_lowering(case, ctx))
    {
        return emit_statement_lowered_guarded_match_to_target(
            scrutinee, cases, target_py, span, ctx, indent, out,
        );
    }
    let pad = " ".repeat(indent);
    let tmp = bind_statement_lowered_expr_value(scrutinee, "match", ctx, indent, out)?;
    for (idx, case) in cases.iter().enumerate() {
        let head = if idx == 0 { "if" } else { "elif" };
        let pattern_metadata = pattern_binding_metadata(scrutinee, case, ctx);
        let (condition, bindings) = emit_match_stmt_condition(&tmp, &case.pattern, ctx)?;
        let condition = emit_direct_case_condition_with_guard(
            condition,
            &bindings,
            pattern_metadata.as_ref(),
            case.guard.as_ref(),
            ctx,
        )?;
        writeln!(out, "{pad}{head} {condition}:").expect("write to string");
        ctx.push_scope();
        let result = {
            emit_pattern_bindings_with_metadata(
                &bindings,
                pattern_metadata.as_ref(),
                ctx,
                indent + 4,
                out,
            );
            ctx.with_metadata_control_flow(|ctx| {
                emit_statement_lowered_case_arm_body_to_target(
                    &case.body,
                    target_py,
                    ctx,
                    indent + 4,
                    out,
                )
            })
        };
        ctx.pop_scope();
        result?;
    }
    writeln!(out, "{pad}else:").expect("write to string");
    writeln!(
        out,
        "{}{} = tpz_impossible_match({tmp}, {})",
        " ".repeat(indent + 4),
        target_py,
        py_span(span)
    )
    .expect("write to string");
    Ok(())
}

pub(super) fn case_guard_needs_statement_lowering(case: &CaseClause, ctx: &Ctx<'_>) -> bool {
    case.guard
        .as_ref()
        .is_some_and(|guard| expr_needs_statement_lowering(guard, ctx))
}

pub(super) fn emit_direct_case_condition_with_guard(
    condition: String,
    bindings: &[PatternBinding],
    pattern_metadata: Option<&PatternBindingMetadata>,
    guard: Option<&Expr>,
    ctx: &mut Ctx<'_>,
) -> Result<String, PyEmitError> {
    let Some(guard) = guard else {
        return Ok(condition);
    };
    ctx.push_scope();
    let binding_py_names = bindings
        .iter()
        .map(|binding| register_pattern_binding_with_metadata(binding, pattern_metadata, ctx))
        .collect::<Vec<_>>();
    let result = emit_case_condition_with_guard_for_expr(
        condition,
        bindings,
        &binding_py_names,
        Some(guard),
        ctx,
    );
    ctx.pop_scope();
    result
}

pub(super) fn emit_statement_lowered_guarded_match_to_target(
    scrutinee: &Expr,
    cases: &[CaseClause],
    target_py: &str,
    span: Span,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let tmp = bind_statement_lowered_expr_value(scrutinee, "match", ctx, indent, out)?;
    let matched = ctx.fresh_temp("match_done");
    writeln!(out, "{pad}{matched} = False").expect("write to string");
    for case in cases {
        writeln!(out, "{pad}if not {matched}:").expect("write to string");
        let pattern_metadata = pattern_binding_metadata(scrutinee, case, ctx);
        emit_statement_lowered_guarded_match_case(
            &tmp,
            case,
            GuardedMatchCase {
                target_py: Some(target_py),
                matched_py: &matched,
                pattern_metadata: pattern_metadata.as_ref(),
            },
            ctx,
            indent + 4,
            out,
        )?;
    }
    writeln!(out, "{pad}if not {matched}:").expect("write to string");
    writeln!(
        out,
        "{}{} = tpz_impossible_match({tmp}, {})",
        " ".repeat(indent + 4),
        target_py,
        py_span(span)
    )
    .expect("write to string");
    Ok(())
}

pub(super) fn emit_statement_lowered_guarded_match_case(
    tmp: &str,
    case: &CaseClause,
    output: GuardedMatchCase<'_>,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let GuardedMatchCase {
        target_py,
        matched_py,
        pattern_metadata,
    } = output;
    let pad = " ".repeat(indent);
    let inner = " ".repeat(indent + 4);
    let (condition, bindings) = emit_match_stmt_condition(tmp, &case.pattern, ctx)?;
    writeln!(out, "{pad}if {condition}:").expect("write to string");
    ctx.push_scope();
    let result = (|| -> Result<(), PyEmitError> {
        emit_pattern_bindings_with_metadata(&bindings, pattern_metadata, ctx, indent + 4, out);
        let body_indent = if let Some(guard) = case.guard.as_ref() {
            let guard_py = emit_statement_lowered_expr_value(guard, ctx, indent + 4, out)?;
            writeln!(
                out,
                "{inner}if tpz_condition({guard_py}, {}):",
                py_span(guard.span)
            )
            .expect("write to string");
            indent + 8
        } else {
            indent + 4
        };
        writeln!(out, "{}{matched_py} = True", " ".repeat(body_indent)).expect("write to string");
        ctx.with_metadata_control_flow(|ctx| match target_py {
            Some(target) => emit_statement_lowered_case_arm_body_to_target(
                &case.body,
                target,
                ctx,
                body_indent,
                out,
            ),
            None => emit_case_arm_body_as_stmt(&case.body, ctx, body_indent, out),
        })
    })();
    ctx.pop_scope();
    result
}

pub(super) fn emit_statement_lowered_case_arm_body_to_target(
    body: &CaseArmBody,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    match body {
        CaseArmBody::Expr(expr) => match &expr.kind {
            ExprKind::Block(block) => {
                emit_statement_lowered_block_expr_to_target(block, target_py, ctx, indent, out)
            }
            _ => emit_statement_lowered_expr_to_target(expr, target_py, ctx, indent, out),
        },
        CaseArmBody::Return {
            value: Some(value), ..
        } => {
            let value_py = emit_statement_lowered_expr_value(value, ctx, indent, out)?;
            writeln!(out, "{pad}{target_py} = tpz_return({value_py})").expect("write to string");
            Ok(())
        }
        CaseArmBody::Return { value: None, .. } => {
            writeln!(out, "{pad}{target_py} = tpz_return(TPZ_UNIT)").expect("write to string");
            Ok(())
        }
    }
}

pub(super) fn emit_string_with_statement_values(
    lit: &StringLit,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<String, PyEmitError> {
    let mut parts = Vec::new();
    for part in &lit.parts {
        match part {
            StringPart::Text(span) => {
                let mut decoded = String::new();
                decode_escapes(text_in_map(ctx.map, *span), &mut decoded, *span)
                    .map_err(|_| PyEmitError::malformed_literal("string escape"))?;
                parts.push(py_string(&decoded));
            }
            StringPart::Interpolation(expr) => {
                let value_py = emit_statement_lowered_expr_value(expr, ctx, indent, out)?;
                parts.push(format!("tpz_render({value_py})"));
            }
        }
    }
    Ok(format!("''.join([{}])", parts.join(", ")))
}

pub(super) fn emit_template_with_statement_values(
    lit: &StringLit,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<String, PyEmitError> {
    let tag_span = lit.tag.expect("untagged string handled before template");
    let tag = py_string(text_in_map(ctx.map, tag_span));
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut values = Vec::new();
    for part in &lit.parts {
        match part {
            StringPart::Text(span) => {
                decode_escapes(text_in_map(ctx.map, *span), &mut current, *span)
                    .map_err(|_| PyEmitError::malformed_literal("string escape"))?;
            }
            StringPart::Interpolation(expr) => {
                parts.push(py_string(&std::mem::take(&mut current)));
                // §16 interpolations evaluate ONCE each, left-to-right, and a
                // fault aborts before any later interpolation runs. Bind EVERY
                // value to a fresh temp (not just the statement-lowered ones):
                // an inline value after a hoisted statementful one would
                // otherwise evaluate after that hoisted value's side effects.
                values.push(bind_statement_lowered_expr_value(
                    expr,
                    "template_value",
                    ctx,
                    indent,
                    out,
                )?);
            }
        }
    }
    parts.push(py_string(&current));
    Ok(format!(
        "tpz_make_template({tag}, [{}], [{}])",
        parts.join(", "),
        values.join(", ")
    ))
}

pub(super) fn emit_template_expr(lit: &StringLit, ctx: &Ctx<'_>) -> Result<String, PyEmitError> {
    let tag_span = lit.tag.expect("untagged string handled before template");
    let tag = py_string(text_in_map(ctx.map, tag_span));
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut values = Vec::new();
    for part in &lit.parts {
        match part {
            StringPart::Text(span) => {
                decode_escapes(text_in_map(ctx.map, *span), &mut current, *span)
                    .map_err(|_| PyEmitError::malformed_literal("string escape"))?;
            }
            StringPart::Interpolation(expr) => {
                parts.push(py_string(&std::mem::take(&mut current)));
                values.push(emit_expr(expr, ctx)?);
            }
        }
    }
    parts.push(py_string(&current));
    Ok(format!(
        "tpz_make_template({tag}, [{}], [{}])",
        parts.join(", "),
        values.join(", ")
    ))
}
