use crate::*;

pub(super) fn emit_statement_lowered_pipe_receiver_static_call_to_target(
    object: &Expr,
    args: &[CallArg],
    call: PipeStaticCall<'_>,
    target: StatementTarget<'_, '_, '_, '_>,
    render_body: impl FnOnce(&str, &[String]) -> String,
) -> Result<bool, PyEmitError> {
    emit_statement_lowered_pipe_receiver_static_call_to_target_with_bound(
        object,
        args,
        call,
        target,
        |recv, bound| render_body(recv, &bound.slots),
    )
}

pub(super) fn emit_statement_lowered_pipe_receiver_static_call_to_target_with_bound(
    object: &Expr,
    args: &[CallArg],
    call: PipeStaticCall<'_>,
    target: StatementTarget<'_, '_, '_, '_>,
    render_body: impl FnOnce(&str, &BoundStaticArgs) -> String,
) -> Result<bool, PyEmitError> {
    let StatementTarget {
        target_py,
        ctx,
        indent,
        out,
    } = target;
    let pad = " ".repeat(indent);
    let recv = emit_statement_lowered_expr_value(object, ctx, indent, out)?;
    let bound = bind_statement_lowered_pipe_static_call_args(args, call, ctx, indent, out)?;
    let call = render_body(&recv, &bound);
    writeln!(out, "{pad}{target_py} = {call}").expect("write to string");
    Ok(true)
}

pub(super) fn emit_statement_lowered_pipe_optional_receiver_static_call_to_target(
    object: &Expr,
    args: &[CallArg],
    call: PipeStaticCall<'_>,
    wrap: StatementLoweredOptionalWrap,
    target: StatementTarget<'_, '_, '_, '_>,
    render_body: impl Fn(&str, &[String]) -> String,
) -> Result<(), PyEmitError> {
    emit_statement_lowered_pipe_optional_receiver_static_call_to_target_with_bound(
        object,
        args,
        call,
        wrap,
        target,
        |recv, bound| render_body(recv, &bound.slots),
    )
}

pub(super) fn emit_statement_lowered_pipe_optional_receiver_static_call_to_target_with_bound(
    object: &Expr,
    args: &[CallArg],
    call: PipeStaticCall<'_>,
    wrap: StatementLoweredOptionalWrap,
    target: StatementTarget<'_, '_, '_, '_>,
    render_body: impl Fn(&str, &BoundStaticArgs) -> String,
) -> Result<(), PyEmitError> {
    let StatementTarget {
        target_py,
        ctx,
        indent,
        out,
    } = target;
    let has_spread = args.iter().any(|arg| matches!(arg, CallArg::Spread(_)));
    let pad = " ".repeat(indent);
    let receiver_value = emit_statement_lowered_expr_value(object, ctx, indent, out)?;
    let receiver_tmp = ctx.fresh_temp("optional_receiver");
    writeln!(out, "{pad}{receiver_tmp} = {receiver_value}").expect("write to string");
    writeln!(out, "{pad}if {receiver_tmp} is None:").expect("write to string");
    writeln!(out, "{pad}    {target_py} = None").expect("write to string");
    writeln!(out, "{pad}else:").expect("write to string");
    writeln!(out, "{pad}    if isinstance({receiver_tmp}, Some):").expect("write to string");
    ctx.with_metadata_control_flow(|ctx| {
        let some_call = if has_spread {
            bind_statement_lowered_pipe_static_spread_fault_expr(args, call, ctx, indent + 8, out)?
        } else {
            let some_bound =
                bind_statement_lowered_pipe_static_call_args(args, call, ctx, indent + 8, out)?;
            render_body(&format!("{receiver_tmp}.value"), &some_bound)
        };
        emit_statement_lowered_optional_wrapped_assignment(
            target_py,
            wrap,
            &some_call,
            ctx,
            indent + 8,
            out,
        )
    })?;
    writeln!(out, "{pad}    else:").expect("write to string");
    ctx.with_metadata_control_flow(|ctx| {
        let direct_call = if has_spread {
            bind_statement_lowered_pipe_static_spread_fault_expr(args, call, ctx, indent + 8, out)?
        } else {
            let direct_bound =
                bind_statement_lowered_pipe_static_call_args(args, call, ctx, indent + 8, out)?;
            render_body(&receiver_tmp, &direct_bound)
        };
        writeln!(out, "{pad}        {target_py} = {direct_call}").expect("write to string");
        Ok(())
    })
}

pub(super) fn bind_statement_lowered_pipe_static_spread_fault_expr(
    args: &[CallArg],
    call: PipeStaticCall<'_>,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<String, PyEmitError> {
    let PipeStaticCall {
        lhs,
        callback_arities,
        piped,
        span,
        ..
    } = call;
    if positional_after_named_py(args)
        || args.iter().any(|arg| matches!(arg, CallArg::Named { .. }))
    {
        return Err(PyEmitError::unsupported("call argument shape").at(span));
    }
    let first_spread = args
        .iter()
        .position(|arg| matches!(arg, CallArg::Spread(_)))
        .ok_or_else(|| PyEmitError::unsupported("call argument shape").at(span))?;
    let has_placeholder = args.iter().any(|arg| {
        matches!(arg, CallArg::Positional(expr) if matches!(&expr.kind, ExprKind::Placeholder))
    });
    let mut positional_index = 0usize;
    let mut prefix = Vec::new();
    if !has_placeholder {
        prefix.push(
            emit_statement_lowered_pipe_leading_arg(
                lhs,
                piped,
                positional_index,
                callback_arities,
                ctx,
            )?
            .py,
        );
        positional_index += 1;
    }

    for arg in &args[..first_spread] {
        let CallArg::Positional(expr) = arg else {
            return Err(PyEmitError::unsupported("call argument shape").at(call_arg_span(arg)));
        };
        let value = emit_statement_lowered_pipe_static_arg_expr(
            expr,
            positional_index,
            call,
            ctx,
            indent,
            out,
        )?;
        prefix.push(value.py);
        positional_index += 1;
    }

    let mut tail = Vec::new();
    let pad = " ".repeat(indent);
    for arg in &args[first_spread..] {
        match arg {
            CallArg::Positional(expr) => {
                let value = emit_statement_lowered_pipe_static_arg_expr(
                    expr,
                    positional_index,
                    call,
                    ctx,
                    indent,
                    out,
                )?;
                tail.push(value.py);
                positional_index += 1;
            }
            CallArg::Spread(expr) => {
                if contains_placeholder(expr) {
                    return Err(PyEmitError::unsupported("pipe placeholder").at(expr.span));
                }
                let value = emit_statement_lowered_expr_value(expr, ctx, indent, out)?;
                let spread_value = ctx.fresh_temp("call_spread");
                writeln!(
                    out,
                    "{pad}{spread_value} = tpz_spread_values({value}, {})",
                    py_span(expr.span)
                )
                .expect("write to string");
                tail.push(format!("*{spread_value}"));
            }
            CallArg::Named { .. } => {
                return Err(PyEmitError::unsupported("call argument shape").at(call_arg_span(arg)));
            }
        }
    }

    Ok(format!(
        "tpz_nonvariadic_static_spread_call([{}], [{}], [], {})",
        prefix.join(", "),
        tail.join(", "),
        py_span(span)
    ))
}

pub(super) fn bind_statement_lowered_pipe_static_call_args(
    args: &[CallArg],
    call: PipeStaticCall<'_>,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<BoundStaticArgs, PyEmitError> {
    let PipeStaticCall {
        lhs,
        params,
        callback_arities,
        piped,
        span,
    } = call;
    let has_placeholder = args.iter().any(|arg| {
        matches!(arg, CallArg::Positional(expr) if contains_placeholder(expr))
            || matches!(arg, CallArg::Named { value, .. } if contains_placeholder(value))
    });
    let mut slots = vec![None; params.len()];
    let mut cooperative_callback_slots = vec![false; params.len()];
    let mut ordered = Vec::with_capacity(args.len() + usize::from(!has_placeholder));
    let mut positional_index = 0usize;
    let mut saw_named = false;

    if !has_placeholder {
        let value = materialize_statement_lowered_pipe_arg(
            emit_statement_lowered_pipe_leading_arg(
                lhs,
                piped,
                positional_index,
                callback_arities,
                ctx,
            )?,
            ctx,
            indent,
            out,
        );
        fill_pipe_static_slot(
            &mut slots,
            &mut cooperative_callback_slots,
            &mut ordered,
            positional_index,
            value,
            span,
        )?;
        positional_index += 1;
    }

    for arg in args {
        match arg {
            CallArg::Positional(expr) if saw_named => {
                return Err(PyEmitError::unsupported("call argument shape").at(expr.span));
            }
            CallArg::Positional(expr) => {
                let value = emit_statement_lowered_pipe_static_arg_expr(
                    expr,
                    positional_index,
                    call,
                    ctx,
                    indent,
                    out,
                )?;
                let value = materialize_statement_lowered_pipe_arg(value, ctx, indent, out);
                fill_pipe_static_slot(
                    &mut slots,
                    &mut cooperative_callback_slots,
                    &mut ordered,
                    positional_index,
                    value,
                    expr.span,
                )?;
                positional_index += 1;
            }
            CallArg::Named { name, value } => {
                saw_named = true;
                let source_name = ctx.text(name.span);
                let Some(param_index) = params.iter().position(|param| *param == source_name)
                else {
                    return Err(PyEmitError::unsupported("call argument shape").at(name.span));
                };
                let value = emit_statement_lowered_pipe_static_arg_expr(
                    value,
                    param_index,
                    call,
                    ctx,
                    indent,
                    out,
                )?;
                let value = materialize_statement_lowered_pipe_arg(value, ctx, indent, out);
                fill_pipe_static_slot(
                    &mut slots,
                    &mut cooperative_callback_slots,
                    &mut ordered,
                    param_index,
                    value,
                    name.span,
                )?;
            }
            CallArg::Spread(expr) => {
                return Err(PyEmitError::unsupported("pipe stage argument").at(expr.span));
            }
        }
    }

    let slots = slots
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| PyEmitError::unsupported("call argument shape").at(span))?;
    Ok(BoundStaticArgs {
        slots,
        ordered,
        cooperative_callback_slots,
        spread_fault: None,
    })
}

pub(super) fn materialize_statement_lowered_pipe_arg(
    value: StaticArgValue,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> StaticArgValue {
    let temp = ctx.fresh_temp("pipe_arg");
    writeln!(out, "{}{temp} = {}", " ".repeat(indent), value.py).expect("write to string");
    StaticArgValue {
        py: temp,
        cooperative_callback: value.cooperative_callback,
    }
}

pub(super) fn emit_statement_lowered_pipe_leading_arg(
    lhs: &Expr,
    piped: &str,
    param_index: usize,
    callback_arities: &[(usize, usize)],
    ctx: &Ctx<'_>,
) -> Result<StaticArgValue, PyEmitError> {
    if let Some((_, arity)) = callback_arities
        .iter()
        .find(|(callback_index, _)| *callback_index == param_index)
    {
        return emit_pipe_leading_callback(lhs, piped, *arity, ctx);
    }
    Ok(StaticArgValue::plain(piped.to_string()))
}

pub(super) fn emit_statement_lowered_pipe_positional_call_to_target(
    callee: &Expr,
    args: &[CallArg],
    piped: &str,
    span: Span,
    target: StatementTarget<'_, '_, '_, '_>,
) -> Result<(), PyEmitError> {
    let StatementTarget {
        target_py,
        ctx,
        indent,
        out,
    } = target;
    let mut values = Vec::with_capacity(args.len() + 1);
    let mut uses_placeholder = false;
    for arg in args {
        match arg {
            CallArg::Positional(expr) => {
                if contains_placeholder(expr) {
                    uses_placeholder = true;
                }
                values.push(emit_statement_lowered_pipe_arg_expr(
                    expr, piped, ctx, indent, out,
                )?);
            }
            CallArg::Named { name, .. } => {
                return Err(PyEmitError::unsupported("pipe stage argument").at(name.span));
            }
            CallArg::Spread(expr) => {
                return Err(PyEmitError::unsupported("pipe stage argument").at(expr.span));
            }
        }
    }
    if !uses_placeholder {
        values.insert(0, piped.to_string());
    }
    emit_statement_lowered_pipe_callable_call_to_target(
        callee, values, span, target_py, ctx, indent, out,
    )
}

pub(super) fn emit_statement_lowered_pipe_named_call_to_target(
    callee: &Expr,
    args: &[CallArg],
    piped: &str,
    span: Span,
    target: StatementTarget<'_, '_, '_, '_>,
) -> Result<(), PyEmitError> {
    let StatementTarget {
        target_py,
        ctx,
        indent,
        out,
    } = target;
    let info = pipe_known_callable_info(callee, ctx)?;
    if info.params.last().is_some_and(|param| param.variadic) {
        return Err(PyEmitError::unsupported("pipe stage call target").at(span));
    }
    let has_placeholder = args.iter().any(|arg| {
        matches!(arg, CallArg::Positional(expr) if contains_placeholder(expr))
            || matches!(arg, CallArg::Named { value, .. } if contains_placeholder(value))
    });
    let mut call_args = if info.needs_host {
        vec!["host".to_string()]
    } else {
        Vec::new()
    };
    let mut filled = vec![false; info.params.len()];
    let mut positional_index = 0usize;
    let mut saw_named = false;

    if !has_placeholder {
        if info.params.is_empty() {
            return Err(PyEmitError::unsupported("call argument shape").at(span));
        }
        filled[0] = true;
        positional_index = 1;
        call_args.push(piped.to_string());
    }

    for arg in args {
        match arg {
            CallArg::Positional(_) if saw_named => {
                return Err(PyEmitError::unsupported("call argument shape").at(call_arg_span(arg)));
            }
            CallArg::Positional(expr) => {
                if positional_index >= info.params.len() || filled[positional_index] {
                    return Err(PyEmitError::unsupported("call argument shape").at(expr.span));
                }
                filled[positional_index] = true;
                positional_index += 1;
                call_args.push(emit_statement_lowered_pipe_arg_expr(
                    expr, piped, ctx, indent, out,
                )?);
            }
            CallArg::Named { name, value } => {
                saw_named = true;
                let source_name = ctx.text(name.span);
                let Some(param_index) = named_callable_param_index(&info.params, source_name)
                else {
                    return Err(PyEmitError::unsupported("call argument shape").at(name.span));
                };
                if filled[param_index] {
                    return Err(PyEmitError::unsupported("call argument shape").at(name.span));
                }
                filled[param_index] = true;
                let value_py =
                    emit_statement_lowered_pipe_arg_expr(value, piped, ctx, indent, out)?;
                call_args.push(format!("{}={value_py}", info.params[param_index].py_name));
            }
            CallArg::Spread(expr) => {
                return Err(PyEmitError::unsupported("pipe stage argument").at(expr.span));
            }
        }
    }
    for (param, is_filled) in info.params.iter().zip(filled) {
        if !is_filled && !param.has_default {
            return Err(PyEmitError::unsupported("call argument shape").at(span));
        }
    }
    write_known_function_call_to_target(out, indent, target_py, &info, &call_args, ctx);
    Ok(())
}

pub(super) fn emit_statement_lowered_pipe_spread_call_to_target(
    callee: &Expr,
    args: &[CallArg],
    piped: &str,
    span: Span,
    target: StatementTarget<'_, '_, '_, '_>,
) -> Result<(), PyEmitError> {
    let StatementTarget {
        target_py,
        ctx,
        indent,
        out,
    } = target;
    let info = pipe_known_callable_info(callee, ctx)?;
    if !info.params.last().is_some_and(|param| param.variadic) {
        return emit_statement_lowered_nonvariadic_known_function_pipe_spread_call_to_target(
            &info,
            args,
            piped,
            span,
            StatementTarget::new(target_py, ctx, indent, out),
        );
    }
    emit_statement_lowered_variadic_known_function_pipe_call_to_target(
        &info,
        args,
        piped,
        span,
        StatementTarget::new(target_py, ctx, indent, out),
    )
}

pub(super) fn emit_statement_lowered_nonvariadic_known_function_pipe_spread_call_to_target(
    info: &FunctionInfo,
    args: &[CallArg],
    piped: &str,
    span: Span,
    target: StatementTarget<'_, '_, '_, '_>,
) -> Result<(), PyEmitError> {
    let StatementTarget {
        target_py,
        ctx,
        indent,
        out,
    } = target;
    if positional_after_named_py(args) {
        return emit_statement_lowered_pipe_call_order_fault_to_target(
            args, piped, span, target_py, ctx, indent, out,
        );
    }
    let first_spread = args
        .iter()
        .position(|arg| matches!(arg, CallArg::Spread(_)))
        .ok_or_else(|| PyEmitError::unsupported("pipe stage argument").at(span))?;
    let has_placeholder = pipe_args_contain_placeholder(args);
    let mut prefix = Vec::new();
    if !has_placeholder {
        prefix.push(piped.to_string());
    }
    for arg in &args[..first_spread] {
        let CallArg::Positional(expr) = arg else {
            return Err(PyEmitError::unsupported("pipe stage argument").at(call_arg_span(arg)));
        };
        prefix.push(bind_statement_lowered_pipe_call_arg_expr(
            expr, piped, ctx, indent, out,
        )?);
    }

    let region_end = args[first_spread..]
        .iter()
        .position(|arg| matches!(arg, CallArg::Named { .. }))
        .map(|idx| idx + first_spread)
        .unwrap_or(args.len());
    let pad = " ".repeat(indent);
    let mut tail = Vec::new();
    for arg in &args[first_spread..region_end] {
        match arg {
            CallArg::Positional(expr) => {
                tail.push(bind_statement_lowered_pipe_call_arg_expr(
                    expr, piped, ctx, indent, out,
                )?);
            }
            CallArg::Spread(expr) => {
                let value =
                    bind_statement_lowered_pipe_call_arg_expr(expr, piped, ctx, indent, out)?;
                let spread_value = ctx.fresh_temp("call_spread");
                writeln!(
                    out,
                    "{pad}{spread_value} = tpz_spread_values({value}, {})",
                    py_span(expr.span)
                )
                .expect("write to string");
                tail.push(format!("*{spread_value}"));
            }
            CallArg::Named { .. } => unreachable!("region_end stops at the first named arg"),
        }
    }

    let mut named = Vec::new();
    for arg in &args[region_end..] {
        let CallArg::Named { name, value } = arg else {
            return Err(PyEmitError::unsupported("pipe stage argument").at(call_arg_span(arg)));
        };
        let value = bind_statement_lowered_pipe_call_arg_expr(value, piped, ctx, indent, out)?;
        named.push(format!("({}, {value})", py_string(ctx.text(name.span))));
    }

    writeln!(
        out,
        "{pad}{target_py} = tpz_nonvariadic_spread_call([{}], [{}], [{}], {}, {})",
        prefix.join(", "),
        tail.join(", "),
        named.join(", "),
        info.params.len(),
        py_span(span)
    )
    .expect("write to string");
    Ok(())
}

pub(super) fn emit_statement_lowered_pipe_callable_call_to_target(
    callee: &Expr,
    values: Vec<String>,
    span: Span,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let call = match &callee.kind {
        ExprKind::Ident | ExprKind::Paren(_) => match pipe_known_callable_info(callee, ctx) {
            Ok(info) => emit_known_function_positional_values(&info, values, span, ctx)?,
            Err(_) if lambda_callee(callee) => {
                format!("{}({})", emit_expr(callee, ctx)?, values.join(", "))
            }
            Err(err) => return Err(err),
        },
        _ if lambda_callee(callee) => format!("{}({})", emit_expr(callee, ctx)?, values.join(", ")),
        _ => return Err(PyEmitError::unsupported("pipe stage call target").at(callee.span)),
    };
    writeln!(out, "{}{target_py} = {call}", " ".repeat(indent)).expect("write to string");
    Ok(())
}

pub(super) fn emit_statement_lowered_pipe_arg_expr(
    expr: &Expr,
    piped: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<String, PyEmitError> {
    if matches!(&expr.kind, ExprKind::Placeholder) {
        return Ok(piped.to_string());
    }
    if contains_placeholder(expr) {
        return emit_statement_lowered_expr_value_with_pipe_placeholder(
            expr, piped, ctx, indent, out,
        );
    }
    emit_statement_lowered_expr_value(expr, ctx, indent, out)
}

pub(super) fn emit_statement_lowered_pipe_static_arg_expr(
    expr: &Expr,
    param_index: usize,
    call: PipeStaticCall<'_>,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<StaticArgValue, PyEmitError> {
    let PipeStaticCall {
        lhs,
        callback_arities,
        piped,
        ..
    } = call;
    if matches!(&expr.kind, ExprKind::Placeholder) {
        return emit_statement_lowered_pipe_leading_arg(
            lhs,
            piped,
            param_index,
            callback_arities,
            ctx,
        );
    }
    if contains_placeholder(expr) {
        if pipe_param_is_callback(param_index, callback_arities) {
            return Err(PyEmitError::unsupported("pipe placeholder").at(expr.span));
        }
        return emit_statement_lowered_expr_value_with_pipe_placeholder(
            expr, piped, ctx, indent, out,
        )
        .map(StaticArgValue::plain);
    }
    emit_statement_lowered_static_arg_expr(expr, param_index, callback_arities, ctx, indent, out)
}

pub(super) fn pipe_known_callable_info(
    callee: &Expr,
    ctx: &Ctx<'_>,
) -> Result<FunctionInfo, PyEmitError> {
    match &callee.kind {
        ExprKind::Ident => {
            let name = ctx.text(callee.span);
            if let Some(info) = ctx.function_info(name)
                && !ctx.binding_is_bound(name)
            {
                return Ok(info.clone());
            }
            if ctx.binding_is_bound(name) {
                return ctx
                    .binding_callable_info_at(name, callee.span)
                    .ok_or_else(|| {
                        PyEmitError::unsupported("pipe stage call target").at(callee.span)
                    });
            }
            Err(PyEmitError::unsupported("pipe stage call target").at(callee.span))
        }
        ExprKind::Paren(inner) => pipe_known_callable_info(inner, ctx),
        _ => Err(PyEmitError::unsupported("pipe stage call target").at(callee.span)),
    }
}

pub(super) fn emit_statement_lowered_variadic_known_function_pipe_call_to_target(
    info: &FunctionInfo,
    args: &[CallArg],
    piped: &str,
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
    let fixed_count = info.params.len().saturating_sub(1);
    let variadic_param = info
        .params
        .last()
        .expect("variadic call has at least one parameter");
    let mut fixed_slots = vec![None; fixed_count];
    let mut tail = Vec::new();
    let has_placeholder = pipe_args_contain_placeholder(args);
    let mut positional_index = if has_placeholder { 0usize } else { 1usize };
    let mut saw_named = false;
    let mut saw_spread = false;
    let mut skipped_required_by_spread = false;
    let mut evaluated_args = Vec::new();

    if !has_placeholder {
        if fixed_count == 0 {
            tail.push(VariadicTailPiece::Value(piped.to_string()));
        } else {
            fixed_slots[0] = Some(piped.to_string());
        }
    }

    for arg in args {
        match arg {
            CallArg::Positional(expr) if saw_named => {
                let value =
                    bind_statement_lowered_pipe_call_arg_expr(expr, piped, ctx, indent, out)?;
                evaluated_args.push(value);
                writeln!(
                    out,
                    "{pad}{target_py} = tpz_call_order_fault([{}], {}, {})",
                    evaluated_args.join(", "),
                    py_string("positional arguments may not follow named arguments (§5)"),
                    py_span(span)
                )
                .expect("write to string");
                return Ok(());
            }
            CallArg::Positional(expr) => {
                let value =
                    bind_statement_lowered_pipe_call_arg_expr(expr, piped, ctx, indent, out)?;
                evaluated_args.push(value.clone());
                if !saw_spread && positional_index < fixed_count {
                    if fixed_slots[positional_index].is_some() {
                        return Err(PyEmitError::unsupported("call argument shape").at(expr.span));
                    }
                    fixed_slots[positional_index] = Some(value);
                    positional_index += 1;
                } else {
                    tail.push(VariadicTailPiece::Value(value));
                    positional_index += 1;
                }
            }
            CallArg::Spread(expr) => {
                let value =
                    bind_statement_lowered_pipe_call_arg_expr(expr, piped, ctx, indent, out)?;
                if saw_named {
                    evaluated_args.push(value);
                    writeln!(
                        out,
                        "{pad}{target_py} = tpz_call_order_fault([{}], {}, {})",
                        evaluated_args.join(", "),
                        py_string("named arguments must follow spread arguments (§5)"),
                        py_span(span)
                    )
                    .expect("write to string");
                    return Ok(());
                }
                if !saw_spread
                    && (positional_index.min(fixed_count)..fixed_count)
                        .any(|idx| fixed_slots[idx].is_none() && !info.params[idx].has_default)
                {
                    skipped_required_by_spread = true;
                }
                let spread_value = ctx.fresh_temp("call_spread");
                writeln!(
                    out,
                    "{pad}{spread_value} = tpz_spread_values({value}, {})",
                    py_span(expr.span)
                )
                .expect("write to string");
                evaluated_args.push(spread_value.clone());
                tail.push(VariadicTailPiece::Spread(spread_value));
                saw_spread = true;
            }
            CallArg::Named { name, value } => {
                saw_named = true;
                let source_name = ctx.text(name.span);
                let Some(param_index) = named_callable_param_index(&info.params, source_name)
                else {
                    return Err(PyEmitError::unsupported("call argument shape").at(name.span));
                };
                if param_index == fixed_count {
                    return Err(PyEmitError::unsupported("call argument shape").at(name.span));
                }
                if fixed_slots[param_index].is_some() {
                    return Err(PyEmitError::unsupported("call argument shape").at(name.span));
                }
                let value =
                    bind_statement_lowered_pipe_call_arg_expr(value, piped, ctx, indent, out)?;
                evaluated_args.push(value.clone());
                fixed_slots[param_index] = Some(value);
            }
        }
    }

    if skipped_required_by_spread {
        writeln!(
            out,
            "{pad}{target_py} = tpz_call_order_fault([{}], {}, {})",
            evaluated_args.join(", "),
            py_string("a spread argument cannot skip an unsatisfied fixed parameter (§5)"),
            py_span(span)
        )
        .expect("write to string");
        return Ok(());
    }

    let mut call_args = if info.needs_host {
        vec!["host".to_string()]
    } else {
        Vec::new()
    };
    for (param, value) in info.params[..fixed_count].iter().zip(fixed_slots) {
        match value {
            Some(value) => push_known_variadic_fixed_arg(&mut call_args, param, value),
            None if param.has_default => {}
            None => return Err(PyEmitError::unsupported("call argument shape").at(span)),
        }
    }
    let tail_expr = render_variadic_tail(&tail);
    call_args.push(format!("{}={tail_expr}", variadic_param.py_name));
    write_known_function_call_to_target(out, indent, target_py, info, &call_args, ctx);
    Ok(())
}

pub(super) fn emit_statement_lowered_pipe_static_callable_value_call_to_target(
    callee: &Expr,
    args: &[CallArg],
    params: &[FunctionParamInfo],
    piped: &str,
    span: Span,
    target: StatementTarget<'_, '_, '_, '_>,
) -> Result<(), PyEmitError> {
    let StatementTarget {
        target_py,
        ctx,
        indent,
        out,
    } = target;
    if params.last().is_some_and(|param| param.variadic) {
        return emit_statement_lowered_pipe_variadic_static_callable_value_call_to_target(
            callee,
            args,
            params,
            piped,
            span,
            StatementTarget::new(target_py, ctx, indent, out),
        );
    }
    if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        return emit_statement_lowered_pipe_nonvariadic_static_callable_value_spread_fault_to_target(
            args,
            params.len(),
            piped,
            span,
            StatementTarget::new(target_py, ctx, indent, out),
        );
    }

    let pad = " ".repeat(indent);
    let callee_py = if args.iter().any(call_arg_contains_statement_lowered_expr)
        && !expr_contains_statement_lowered_expr(callee)
    {
        bind_statement_lowered_expr_value(callee, "call_callee", ctx, indent, out)?
    } else {
        emit_statement_lowered_expr_value(callee, ctx, indent, out)?
    };
    let has_placeholder = pipe_args_contain_placeholder(args);
    let mut positional = Vec::new();
    let mut kwargs = Vec::new();
    let mut filled = vec![false; params.len()];
    let mut positional_index = 0usize;
    let mut saw_named = false;

    if !has_placeholder {
        if params.is_empty() {
            return Err(PyEmitError::unsupported("call argument shape").at(span));
        }
        filled[0] = true;
        positional_index = 1;
        positional.push(piped.to_string());
    }

    for arg in args {
        match arg {
            CallArg::Positional(_) if saw_named => {
                return emit_statement_lowered_pipe_call_order_fault_to_target(
                    args, piped, span, target_py, ctx, indent, out,
                );
            }
            CallArg::Positional(expr) => {
                if positional_index >= params.len() || filled[positional_index] {
                    return Err(PyEmitError::unsupported("call argument shape").at(expr.span));
                }
                filled[positional_index] = true;
                positional_index += 1;
                positional.push(emit_statement_lowered_pipe_arg_expr(
                    expr, piped, ctx, indent, out,
                )?);
            }
            CallArg::Named { name, value } => {
                saw_named = true;
                let source_name = ctx.text(name.span);
                let Some(param_index) = named_callable_param_index(params, source_name) else {
                    return Err(PyEmitError::unsupported("call argument shape").at(name.span));
                };
                if filled[param_index] {
                    return Err(PyEmitError::unsupported("call argument shape").at(name.span));
                }
                filled[param_index] = true;
                let value_py =
                    emit_statement_lowered_pipe_arg_expr(value, piped, ctx, indent, out)?;
                kwargs.push(format!(
                    "{}: {value_py}",
                    py_string(&params[param_index].py_name)
                ));
            }
            CallArg::Spread(expr) => {
                return Err(PyEmitError::unsupported("pipe stage argument").at(expr.span));
            }
        }
    }
    for (param, is_filled) in params.iter().zip(filled) {
        if !is_filled && !param.has_default {
            return Err(PyEmitError::unsupported("call argument shape").at(span));
        }
    }
    writeln!(
        out,
        "{pad}{target_py} = tpz_call({callee_py}, {}, {{{}}}, {})",
        py_tuple(positional),
        kwargs.join(", "),
        py_span(span)
    )
    .expect("write to string");
    Ok(())
}

pub(super) fn emit_statement_lowered_pipe_nonvariadic_static_callable_value_spread_fault_to_target(
    args: &[CallArg],
    param_count: usize,
    piped: &str,
    span: Span,
    target: StatementTarget<'_, '_, '_, '_>,
) -> Result<(), PyEmitError> {
    let StatementTarget {
        target_py,
        ctx,
        indent,
        out,
    } = target;
    if positional_after_named_py(args) {
        return emit_statement_lowered_pipe_call_order_fault_to_target(
            args, piped, span, target_py, ctx, indent, out,
        );
    }
    let first_spread = args
        .iter()
        .position(|arg| matches!(arg, CallArg::Spread(_)))
        .ok_or_else(|| PyEmitError::unsupported("pipe stage argument").at(span))?;
    let has_placeholder = pipe_args_contain_placeholder(args);
    let mut prefix = Vec::new();
    if !has_placeholder {
        prefix.push(piped.to_string());
    }
    for arg in &args[..first_spread] {
        let CallArg::Positional(expr) = arg else {
            return Err(PyEmitError::unsupported("pipe stage argument").at(call_arg_span(arg)));
        };
        prefix.push(bind_statement_lowered_pipe_call_arg_expr(
            expr, piped, ctx, indent, out,
        )?);
    }

    let region_end = args[first_spread..]
        .iter()
        .position(|arg| matches!(arg, CallArg::Named { .. }))
        .map(|idx| idx + first_spread)
        .unwrap_or(args.len());
    let pad = " ".repeat(indent);
    let mut tail = Vec::new();
    for arg in &args[first_spread..region_end] {
        match arg {
            CallArg::Positional(expr) => tail.push(bind_statement_lowered_pipe_call_arg_expr(
                expr, piped, ctx, indent, out,
            )?),
            CallArg::Spread(expr) => {
                let value =
                    bind_statement_lowered_pipe_call_arg_expr(expr, piped, ctx, indent, out)?;
                let spread_value = ctx.fresh_temp("call_spread");
                writeln!(
                    out,
                    "{pad}{spread_value} = tpz_spread_values({value}, {})",
                    py_span(expr.span)
                )
                .expect("write to string");
                tail.push(format!("*{spread_value}"));
            }
            CallArg::Named { .. } => unreachable!("region_end stops at the first named arg"),
        }
    }

    let mut named = Vec::new();
    for arg in &args[region_end..] {
        let CallArg::Named { name, value } = arg else {
            return Err(PyEmitError::unsupported("pipe stage argument").at(call_arg_span(arg)));
        };
        let value = bind_statement_lowered_pipe_call_arg_expr(value, piped, ctx, indent, out)?;
        named.push(format!("({}, {value})", py_string(ctx.text(name.span))));
    }

    writeln!(
        out,
        "{pad}{target_py} = tpz_nonvariadic_spread_call([{}], [{}], [{}], {param_count}, {})",
        prefix.join(", "),
        tail.join(", "),
        named.join(", "),
        py_span(span)
    )
    .expect("write to string");
    Ok(())
}

pub(super) fn emit_statement_lowered_pipe_variadic_static_callable_value_call_to_target(
    callee: &Expr,
    args: &[CallArg],
    params: &[FunctionParamInfo],
    piped: &str,
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
    let fixed_count = params.len().saturating_sub(1);
    let variadic_param = params
        .last()
        .expect("variadic static callable pipe call has at least one parameter");
    let callee_py = bind_statement_lowered_expr_value(callee, "call_callee", ctx, indent, out)?;
    let mut fixed_slots = vec![None; fixed_count];
    let mut tail = Vec::new();
    let has_placeholder = pipe_args_contain_placeholder(args);
    let mut positional_index = if has_placeholder { 0usize } else { 1usize };
    let mut saw_named = false;
    let mut saw_spread = false;
    let mut skipped_required_by_spread = false;
    let mut evaluated_args = Vec::new();

    if !has_placeholder {
        if fixed_count == 0 {
            tail.push(VariadicTailPiece::Value(piped.to_string()));
        } else {
            fixed_slots[0] = Some(StaticVariadicFixedArg::Positional(piped.to_string()));
        }
    }

    for arg in args {
        match arg {
            CallArg::Positional(expr) if saw_named => {
                let value =
                    bind_statement_lowered_pipe_call_arg_expr(expr, piped, ctx, indent, out)?;
                evaluated_args.push(value);
                writeln!(
                    out,
                    "{pad}{target_py} = tpz_call_order_fault([{}], {}, {})",
                    evaluated_args.join(", "),
                    py_string("positional arguments may not follow named arguments (§5)"),
                    py_span(span)
                )
                .expect("write to string");
                return Ok(());
            }
            CallArg::Positional(expr) => {
                let value =
                    bind_statement_lowered_pipe_call_arg_expr(expr, piped, ctx, indent, out)?;
                evaluated_args.push(value.clone());
                if !saw_spread && positional_index < fixed_count {
                    if fixed_slots[positional_index].is_some() {
                        return Err(PyEmitError::unsupported("call argument shape").at(expr.span));
                    }
                    fixed_slots[positional_index] = Some(StaticVariadicFixedArg::Positional(value));
                    positional_index += 1;
                } else {
                    tail.push(VariadicTailPiece::Value(value));
                    positional_index += 1;
                }
            }
            CallArg::Spread(expr) => {
                let value =
                    bind_statement_lowered_pipe_call_arg_expr(expr, piped, ctx, indent, out)?;
                if saw_named {
                    evaluated_args.push(value);
                    writeln!(
                        out,
                        "{pad}{target_py} = tpz_call_order_fault([{}], {}, {})",
                        evaluated_args.join(", "),
                        py_string("named arguments must follow spread arguments (§5)"),
                        py_span(span)
                    )
                    .expect("write to string");
                    return Ok(());
                }
                if !saw_spread
                    && (positional_index.min(fixed_count)..fixed_count)
                        .any(|idx| fixed_slots[idx].is_none() && !params[idx].has_default)
                {
                    skipped_required_by_spread = true;
                }
                let spread_value = ctx.fresh_temp("call_spread");
                writeln!(
                    out,
                    "{pad}{spread_value} = tpz_spread_values({value}, {})",
                    py_span(expr.span)
                )
                .expect("write to string");
                evaluated_args.push(spread_value.clone());
                tail.push(VariadicTailPiece::Spread(spread_value));
                saw_spread = true;
            }
            CallArg::Named { name, value } => {
                saw_named = true;
                let source_name = ctx.text(name.span);
                let Some(param_index) = named_callable_param_index(params, source_name) else {
                    return Err(PyEmitError::unsupported("call argument shape").at(name.span));
                };
                if param_index == fixed_count {
                    return Err(PyEmitError::unsupported("call argument shape").at(name.span));
                }
                if fixed_slots[param_index].is_some() {
                    return Err(PyEmitError::unsupported("call argument shape").at(name.span));
                }
                let value =
                    bind_statement_lowered_pipe_call_arg_expr(value, piped, ctx, indent, out)?;
                evaluated_args.push(value.clone());
                fixed_slots[param_index] = Some(StaticVariadicFixedArg::Named(value));
            }
        }
    }

    if skipped_required_by_spread {
        writeln!(
            out,
            "{pad}{target_py} = tpz_call_order_fault([{}], {}, {})",
            evaluated_args.join(", "),
            py_string("a spread argument cannot skip an unsatisfied fixed parameter (§5)"),
            py_span(span)
        )
        .expect("write to string");
        return Ok(());
    }

    let mut positional = Vec::new();
    let mut kwargs = Vec::new();
    for (param, value) in params[..fixed_count].iter().zip(fixed_slots) {
        match value {
            Some(StaticVariadicFixedArg::Positional(value)) => positional.push(value),
            Some(StaticVariadicFixedArg::Named(value)) => {
                kwargs.push(format!("{}: {value}", py_string(&param.py_name)));
            }
            None if param.has_default => {}
            None => return Err(PyEmitError::unsupported("call argument shape").at(span)),
        }
    }
    let tail_expr = render_variadic_tail(&tail);
    kwargs.push(format!(
        "{}: {tail_expr}",
        py_string(&variadic_param.py_name)
    ));
    writeln!(
        out,
        "{pad}{target_py} = tpz_call({callee_py}, {}, {{{}}}, {})",
        py_tuple(positional),
        kwargs.join(", "),
        py_span(span)
    )
    .expect("write to string");
    Ok(())
}
