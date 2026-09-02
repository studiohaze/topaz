use crate::*;

pub(super) fn emit_receiver_mutating_spread_builtin_call(
    object: &Expr,
    method: &str,
    args: &[CallArg],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<Option<String>, PyEmitError> {
    if !args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        return Ok(None);
    }
    if let ExprKind::Ident = &object.kind {
        let name = ctx.text(object.span);
        if !ctx.binding_is_bound(name) {
            return Ok(None);
        }
    }
    let Some((params, render_body)): Option<ReceiverMutatingSpreadSpec> = (match method {
        "push" => Some((&["x"][..], |recv, slots, span| {
            format!("tpz_array_push({recv}, {}, {})", slots[0], py_span(span))
        })),
        "insert" if receiver_is_array_value(object, ctx) => {
            Some((&["index", "value"][..], |recv, slots, span| {
                format!(
                    "tpz_array_insert({recv}, {}, {}, {})",
                    slots[0],
                    slots[1],
                    py_span(span)
                )
            }))
        }
        "insert" => Some((&["k", "v"][..], |recv, slots, span| {
            format!(
                "tpz_map_insert({recv}, {}, {}, {})",
                slots[0],
                slots[1],
                py_span(span)
            )
        })),
        "remove" if receiver_is_map_value(object, ctx) => {
            Some((&["k"][..], |recv, slots, span| {
                format!("tpz_remove({recv}, {}, {})", slots[0], py_span(span))
            }))
        }
        "remove" => Some((&["value"][..], |recv, slots, span| {
            format!("tpz_remove({recv}, {}, {})", slots[0], py_span(span))
        })),
        "clear" => Some((&[][..], |recv, _slots, span| {
            format!("tpz_clear({recv}, {})", py_span(span))
        })),
        "add" => Some((&["value"][..], |recv, slots, span| {
            format!("tpz_set_add({recv}, {}, {})", slots[0], py_span(span))
        })),
        _ => None,
    }) else {
        return Ok(None);
    };
    let bound = bind_fixed_static_call_args(args, params, &[], span, ctx)?;
    let recv = emit_expr(object, ctx)?;
    Ok(Some(render_bound_receiver_static_call(
        recv,
        &bound,
        |recv, slots| render_body(recv, slots, span),
    )))
}

pub(super) fn optional_receiver_inner_shape(expr: &Expr, ctx: &Ctx<'_>) -> Option<ReceiverShape> {
    match &expr.kind {
        ExprKind::OptionalAccess { object, field } => {
            ctx.option_record_field_projection(object, field)
                .receiver_shape
        }
        ExprKind::Paren(inner) => optional_receiver_inner_shape(inner, ctx),
        _ => {
            ctx.wrapped_pattern_value_projection(expr, RecordWrapper::Option)
                .receiver_shape
        }
    }
}

pub(super) fn render_optional_receiver_callback_call(
    object: &Expr,
    bound: BoundStaticArgs,
    _span: Span,
    ctx: &Ctx<'_>,
    render_body: impl Fn(&str, &[String]) -> String,
) -> Result<String, PyEmitError> {
    render_optional_receiver_static_call(object, bound, ctx, render_body)
}

pub(super) fn render_optional_receiver_static_call(
    object: &Expr,
    bound: BoundStaticArgs,
    ctx: &Ctx<'_>,
    render_body: impl Fn(&str, &[String]) -> String,
) -> Result<String, PyEmitError> {
    let object_py = emit_expr(object, ctx)?;
    let some_call =
        render_bound_receiver_static_call("__tpz_obj.value".to_string(), &bound, &render_body);
    let direct_call =
        render_bound_receiver_static_call("__tpz_obj".to_string(), &bound, render_body);
    Ok(format!(
        "(lambda __tpz_obj: None if __tpz_obj is None else (tpz_wrap_optional({some_call}) if isinstance(__tpz_obj, Some) else {direct_call}))({object_py})"
    ))
}

pub(super) fn reject_yield_from_inside_optional_receiver_unit_lambda(
    call: &str,
) -> Result<(), PyEmitError> {
    if contains_yield_from_outside_strings(call) {
        return Err(PyEmitError::unsupported(
            "optional receiver unit call yield",
        ));
    }
    Ok(())
}

pub(super) fn render_optional_receiver_unit_call(
    object: &Expr,
    bound: BoundStaticArgs,
    ctx: &Ctx<'_>,
    render_body: impl Fn(&str, &[String]) -> String,
) -> Result<String, PyEmitError> {
    let object_py = emit_expr(object, ctx)?;
    let some_call =
        render_bound_receiver_static_call("__tpz_obj.value".to_string(), &bound, &render_body);
    let direct_call =
        render_bound_receiver_static_call("__tpz_obj".to_string(), &bound, render_body);
    reject_yield_from_inside_optional_receiver_unit_lambda(&some_call)?;
    reject_yield_from_inside_optional_receiver_unit_lambda(&direct_call)?;
    Ok(format!(
        "(lambda __tpz_obj: None if __tpz_obj is None else (tpz_wrap_optional_unit({some_call}) if isinstance(__tpz_obj, Some) else {direct_call}))({object_py})"
    ))
}

pub(super) fn emit_nonvariadic_static_spread_fault(
    args: &[CallArg],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    if positional_after_named_py(args) {
        return emit_call_order_fault_py(args, span, ctx);
    }
    let first_spread = args
        .iter()
        .position(|arg| matches!(arg, CallArg::Spread(_)))
        .expect("caller checked spread is present");
    let mut prefix = Vec::new();
    for arg in &args[..first_spread] {
        let CallArg::Positional(expr) = arg else {
            return Err(PyEmitError::unsupported("call argument shape").at(call_arg_span(arg)));
        };
        prefix.push(emit_expr(expr, ctx)?);
    }

    let region_end = args[first_spread..]
        .iter()
        .position(|arg| matches!(arg, CallArg::Named { .. }))
        .map(|idx| idx + first_spread)
        .unwrap_or(args.len());
    let mut tail = Vec::new();
    for arg in &args[first_spread..region_end] {
        match arg {
            CallArg::Positional(expr) => tail.push(emit_expr(expr, ctx)?),
            CallArg::Spread(expr) => tail.push(format!(
                "*tpz_spread_values({}, {})",
                emit_expr(expr, ctx)?,
                py_span(expr.span)
            )),
            CallArg::Named { .. } => unreachable!("region_end stops at the first named arg"),
        }
    }

    let mut named = Vec::new();
    for arg in &args[region_end..] {
        let CallArg::Named { name, value } = arg else {
            return Err(PyEmitError::unsupported("call argument shape").at(call_arg_span(arg)));
        };
        named.push(format!(
            "({}, {})",
            py_string(ctx.text(name.span)),
            emit_expr(value, ctx)?
        ));
    }

    Ok(format!(
        "tpz_nonvariadic_static_spread_call([{}], [{}], [{}], {})",
        prefix.join(", "),
        tail.join(", "),
        named.join(", "),
        py_span(span)
    ))
}

pub(super) fn bind_static_call_args(
    args: &[CallArg],
    params: &[&str],
    callback_arities: &[(usize, usize)],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<BoundStaticArgs, PyEmitError> {
    bind_static_call_args_with_spread_policy(args, params, callback_arities, span, ctx, false)
}

pub(super) fn bind_fixed_static_call_args(
    args: &[CallArg],
    params: &[&str],
    callback_arities: &[(usize, usize)],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<BoundStaticArgs, PyEmitError> {
    bind_static_call_args_with_spread_policy(args, params, callback_arities, span, ctx, true)
}

pub(super) fn bind_static_call_args_with_spread_policy(
    args: &[CallArg],
    params: &[&str],
    callback_arities: &[(usize, usize)],
    span: Span,
    ctx: &Ctx<'_>,
    emit_spread_fault: bool,
) -> Result<BoundStaticArgs, PyEmitError> {
    if emit_spread_fault && args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        return Ok(BoundStaticArgs {
            slots: Vec::new(),
            ordered: Vec::new(),
            cooperative_callback_slots: Vec::new(),
            spread_fault: Some(emit_nonvariadic_static_spread_fault(args, span, ctx)?),
        });
    }

    let mut slots = vec![None; params.len()];
    let mut cooperative_callback_slots = vec![false; params.len()];
    let mut ordered = Vec::with_capacity(args.len());
    let mut positional_index = 0usize;
    let mut saw_named = false;
    for arg in args {
        match arg {
            CallArg::Positional(_) if saw_named => {
                return Ok(BoundStaticArgs {
                    slots: Vec::new(),
                    ordered: Vec::new(),
                    cooperative_callback_slots: Vec::new(),
                    spread_fault: Some(emit_call_order_fault_py(args, span, ctx)?),
                });
            }
            CallArg::Positional(expr) => {
                if positional_index >= params.len() || slots[positional_index].is_some() {
                    return Err(PyEmitError::unsupported("call argument shape").at(expr.span));
                }
                let emitted = emit_static_arg_expr(expr, positional_index, callback_arities, ctx)?;
                cooperative_callback_slots[positional_index] = emitted.cooperative_callback;
                slots[positional_index] = Some(emitted.py.clone());
                ordered.push((positional_index, emitted.py));
                positional_index += 1;
            }
            CallArg::Named { name, value } => {
                saw_named = true;
                let source_name = ctx.text(name.span);
                let Some(param_index) = params.iter().position(|param| *param == source_name)
                else {
                    return Err(PyEmitError::unsupported("call argument shape").at(name.span));
                };
                if slots[param_index].is_some() {
                    return Err(PyEmitError::unsupported("call argument shape").at(name.span));
                }
                let emitted = emit_static_arg_expr(value, param_index, callback_arities, ctx)?;
                cooperative_callback_slots[param_index] = emitted.cooperative_callback;
                slots[param_index] = Some(emitted.py.clone());
                ordered.push((param_index, emitted.py));
            }
            CallArg::Spread(expr) => {
                return Err(PyEmitError::unsupported("call argument shape").at(expr.span));
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

pub(super) fn bind_pipe_static_call_args(
    args: &[CallArg],
    params: &[&str],
    callback_arities: &[(usize, usize)],
    lhs: &Expr,
    piped: &str,
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<BoundStaticArgs, PyEmitError> {
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
        let value = emit_pipe_leading_arg(lhs, piped, positional_index, callback_arities, ctx)?;
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
                let value = emit_pipe_static_arg_expr(
                    expr,
                    lhs,
                    piped,
                    positional_index,
                    callback_arities,
                    ctx,
                )?;
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
                let value = emit_pipe_static_arg_expr(
                    value,
                    lhs,
                    piped,
                    param_index,
                    callback_arities,
                    ctx,
                )?;
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

pub(super) fn bind_fixed_pipe_static_call_args(
    args: &[CallArg],
    params: &[&str],
    callback_arities: &[(usize, usize)],
    lhs: &Expr,
    piped: &str,
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<BoundStaticArgs, PyEmitError> {
    if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        return Ok(BoundStaticArgs {
            slots: Vec::new(),
            ordered: Vec::new(),
            cooperative_callback_slots: Vec::new(),
            spread_fault: Some(emit_nonvariadic_pipe_static_spread_fault(
                args,
                lhs,
                piped,
                callback_arities,
                span,
                ctx,
            )?),
        });
    }
    bind_pipe_static_call_args(args, params, callback_arities, lhs, piped, span, ctx)
}

pub(super) fn emit_nonvariadic_pipe_static_spread_fault(
    args: &[CallArg],
    lhs: &Expr,
    piped: &str,
    callback_arities: &[(usize, usize)],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    if positional_after_named_py(args) {
        return Err(PyEmitError::unsupported("call argument shape").at(span));
    }
    if args.iter().any(|arg| matches!(arg, CallArg::Named { .. })) {
        return Err(PyEmitError::unsupported("call argument shape").at(span));
    }
    let first_spread = args
        .iter()
        .position(|arg| matches!(arg, CallArg::Spread(_)))
        .expect("caller checked spread is present");
    let has_placeholder = args.iter().any(|arg| {
        matches!(arg, CallArg::Positional(expr) if matches!(&expr.kind, ExprKind::Placeholder))
    });
    let mut positional_index = 0usize;
    let mut prefix = Vec::new();
    if !has_placeholder {
        prefix.push(emit_pipe_leading_arg(lhs, piped, positional_index, callback_arities, ctx)?.py);
        positional_index += 1;
    }
    for arg in &args[..first_spread] {
        let CallArg::Positional(expr) = arg else {
            return Err(PyEmitError::unsupported("call argument shape").at(call_arg_span(arg)));
        };
        let value =
            emit_pipe_static_arg_expr(expr, lhs, piped, positional_index, callback_arities, ctx)?;
        prefix.push(value.py);
        positional_index += 1;
    }

    let mut tail = Vec::new();
    for arg in &args[first_spread..] {
        match arg {
            CallArg::Positional(expr) => {
                let value = emit_pipe_static_arg_expr(
                    expr,
                    lhs,
                    piped,
                    positional_index,
                    callback_arities,
                    ctx,
                )?;
                tail.push(value.py);
                positional_index += 1;
            }
            CallArg::Spread(expr) => {
                if contains_placeholder(expr) {
                    return Err(PyEmitError::unsupported("pipe placeholder").at(expr.span));
                }
                tail.push(format!(
                    "*tpz_spread_values({}, {})",
                    emit_expr(expr, ctx)?,
                    py_span(expr.span)
                ));
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

pub(super) fn fill_pipe_static_slot(
    slots: &mut [Option<String>],
    cooperative_callback_slots: &mut [bool],
    ordered: &mut Vec<(usize, String)>,
    param_index: usize,
    value: StaticArgValue,
    span: Span,
) -> Result<(), PyEmitError> {
    if param_index >= slots.len() || slots[param_index].is_some() {
        return Err(PyEmitError::unsupported("call argument shape").at(span));
    }
    cooperative_callback_slots[param_index] = value.cooperative_callback;
    slots[param_index] = Some(value.py.clone());
    ordered.push((param_index, value.py));
    Ok(())
}

pub(super) fn emit_pipe_leading_arg(
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

pub(super) fn emit_pipe_leading_callback(
    lhs: &Expr,
    piped: &str,
    arity: usize,
    ctx: &Ctx<'_>,
) -> Result<StaticArgValue, PyEmitError> {
    if let ExprKind::Ident = &lhs.kind {
        let name = ctx.text(lhs.span);
        if !ctx.binding_is_bound(name)
            && let Some(py_name) = ctx.function_py_name(name)
        {
            let params = (0..arity)
                .map(|idx| format!("__tpz_cb_{idx}"))
                .collect::<Vec<_>>();
            let mut args = vec!["host".to_string()];
            args.extend(params.iter().cloned());
            return Ok(StaticArgValue::plain(format!(
                "(lambda {}: {}({}))",
                params.join(", "),
                py_name,
                args.join(", ")
            )));
        }
    }
    Ok(StaticArgValue::plain(piped.to_string()))
}

pub(super) fn emit_static_arg_expr(
    expr: &Expr,
    param_index: usize,
    callback_arities: &[(usize, usize)],
    ctx: &Ctx<'_>,
) -> Result<StaticArgValue, PyEmitError> {
    if let Some((_, arity)) = callback_arities
        .iter()
        .find(|(callback_index, _)| *callback_index == param_index)
    {
        emit_callback_expr(expr, *arity, ctx)
    } else {
        emit_expr(expr, ctx).map(StaticArgValue::plain)
    }
}

pub(super) fn pipe_param_is_callback(
    param_index: usize,
    callback_arities: &[(usize, usize)],
) -> bool {
    callback_arities
        .iter()
        .any(|(callback_index, _)| *callback_index == param_index)
}

pub(super) fn emit_pipe_static_arg_expr(
    expr: &Expr,
    lhs: &Expr,
    piped: &str,
    param_index: usize,
    callback_arities: &[(usize, usize)],
    ctx: &Ctx<'_>,
) -> Result<StaticArgValue, PyEmitError> {
    if matches!(&expr.kind, ExprKind::Placeholder) {
        return emit_pipe_leading_arg(lhs, piped, param_index, callback_arities, ctx);
    }
    if contains_placeholder(expr) {
        if pipe_param_is_callback(param_index, callback_arities) {
            return Err(PyEmitError::unsupported("pipe placeholder").at(expr.span));
        }
        return emit_expr_with_pipe_placeholder(expr, piped, ctx).map(StaticArgValue::plain);
    }
    emit_static_arg_expr(expr, param_index, callback_arities, ctx)
}

pub(super) fn render_bound_static_call(
    bound: &BoundStaticArgs,
    render_body: impl FnOnce(&[String]) -> String,
) -> String {
    if let Some(spread_fault) = &bound.spread_fault {
        return spread_fault.clone();
    }
    let already_canonical = bound
        .ordered
        .iter()
        .enumerate()
        .all(|(source_index, (param_index, _))| *param_index == source_index);
    if bound.ordered.len() <= 1 || already_canonical {
        return render_body(&bound.slots);
    }
    let mut slots = vec![String::new(); bound.slots.len()];
    let mut params = Vec::with_capacity(bound.ordered.len());
    let mut args = Vec::with_capacity(bound.ordered.len());
    for (source_index, (param_index, emitted)) in bound.ordered.iter().enumerate() {
        let temp = format!("__tpz_call_arg_{source_index}");
        slots[*param_index] = temp.clone();
        params.push(temp);
        args.push(emitted.clone());
    }
    format!(
        "(lambda {}: {})({})",
        params.join(", "),
        render_body(&slots),
        args.join(", ")
    )
}

pub(super) fn render_bound_receiver_static_call(
    receiver: String,
    bound: &BoundStaticArgs,
    render_body: impl FnOnce(&str, &[String]) -> String,
) -> String {
    if let Some(spread_fault) = &bound.spread_fault {
        return format!("(lambda __tpz_call_recv: {spread_fault})({receiver})");
    }
    let already_canonical = bound
        .ordered
        .iter()
        .enumerate()
        .all(|(source_index, (param_index, _))| *param_index == source_index);
    if bound.ordered.len() <= 1 || already_canonical {
        return render_body(&receiver, &bound.slots);
    }
    let recv_temp = "__tpz_call_recv".to_string();
    let mut slots = vec![String::new(); bound.slots.len()];
    let mut params = Vec::with_capacity(bound.ordered.len() + 1);
    let mut args = Vec::with_capacity(bound.ordered.len() + 1);
    params.push(recv_temp.clone());
    args.push(receiver);
    for (source_index, (param_index, emitted)) in bound.ordered.iter().enumerate() {
        let temp = format!("__tpz_call_arg_{source_index}");
        slots[*param_index] = temp.clone();
        params.push(temp);
        args.push(emitted.clone());
    }
    format!(
        "(lambda {}: {})({})",
        params.join(", "),
        render_body(&recv_temp, &slots),
        args.join(", ")
    )
}
