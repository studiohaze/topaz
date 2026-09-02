use crate::*;

pub(super) fn emit_pipe_stage(
    lhs: &Expr,
    stage: &Expr,
    piped: &str,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    match &stage.kind {
        ExprKind::Call {
            callee,
            args,
            type_args,
        } => {
            if !type_args.is_empty()
                && let Some(method) = typed_json_call_method(callee, ctx)
            {
                if type_args.len() != 1 {
                    return Err(
                        PyEmitError::unsupported("typed JSON type arguments").at(stage.span)
                    );
                }
                let schema = emit_json_schema(&type_args[0], ctx)?;
                let params = if method == "parseAs" {
                    &["text"][..]
                } else {
                    &["value"][..]
                };
                let bound = bind_fixed_pipe_static_call_args(
                    args,
                    params,
                    &[],
                    lhs,
                    piped,
                    stage.span,
                    ctx,
                )?;
                return Ok(render_bound_static_call(&bound, |slots| {
                    render_typed_json_runtime_call(method, &slots[0], &schema, stage.span)
                }));
            }
            if let ExprKind::Member { object, field } = &callee.kind {
                let method = ctx.text(field.span);
                if let ExprKind::Ident = &object.kind {
                    let namespace = ctx.text(object.span);
                    if !ctx.binding_is_bound(namespace)
                        && let Some(call) = emit_pipe_namespace_builtin_call(
                            lhs, namespace, method, args, stage.span, piped, ctx,
                        )?
                    {
                        return Ok(call);
                    }
                }
                if let Some(call) = emit_pipe_receiver_callback_builtin_call(
                    lhs, object, method, args, stage.span, ctx,
                )? {
                    return Ok(call);
                }
            }
            if let ExprKind::OptionalAccess { object, field } = &callee.kind {
                let method = ctx.text(field.span);
                if let Some(call) = emit_pipe_optional_receiver_callback_builtin_call(
                    lhs, object, method, args, stage.span, ctx,
                )? {
                    return Ok(call);
                }
            }
            if let Some(params) = pipe_static_callable_value_params(callee, ctx) {
                return emit_pipe_static_callable_value_call(
                    lhs, callee, args, &params, piped, stage.span, ctx,
                );
            }
            if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
                return emit_pipe_callable_spread_call(callee, args, piped, stage.span, ctx);
            }
            if args.iter().any(|arg| matches!(arg, CallArg::Named { .. })) {
                return emit_pipe_callable_named_call(lhs, callee, args, piped, stage.span, ctx);
            }
            let (mut values, uses_placeholder) = emit_pipe_stage_args(args, piped, ctx)?;
            if !uses_placeholder {
                values.insert(0, piped.to_string());
            }
            emit_pipe_callable_call(callee, values, stage.span, ctx)
        }
        ExprKind::Paren(inner) => emit_pipe_stage(lhs, inner, piped, ctx),
        _ => emit_pipe_callable_call(stage, vec![piped.to_string()], stage.span, ctx),
    }
}

/// Ordinary explicit call-site type arguments are checker-only and erase before
/// Python execution. Typed JSON is the exception because its target type must
/// be materialized as a runtime schema; recognize only the unshadowed static
/// heads so that this residual remains decidable.
pub(super) fn typed_json_call_method(callee: &Expr, ctx: &Ctx<'_>) -> Option<&'static str> {
    if let ExprKind::Member { object, field } = &callee.kind
        && let ExprKind::Ident = &object.kind
        && ctx.text(object.span) == "JSON"
        && matches!(ctx.text(field.span), "parseAs" | "decode")
        && !ctx.binding_is_bound("JSON")
    {
        match ctx.text(field.span) {
            "parseAs" => Some("parseAs"),
            "decode" => Some("decode"),
            _ => None,
        }
    } else {
        None
    }
}

pub(super) fn render_typed_json_runtime_call(
    method: &str,
    value: &str,
    schema: &str,
    span: Span,
) -> String {
    if method == "parseAs" {
        format!("tpz_json_parse_as({value}, {schema}, {})", py_span(span))
    } else {
        format!("tpz_json_decode({value}, {schema}, {})", py_span(span))
    }
}

pub(super) fn emit_pipe_namespace_builtin_call(
    lhs: &Expr,
    namespace: &str,
    method: &str,
    args: &[CallArg],
    span: Span,
    piped: &str,
    ctx: &Ctx<'_>,
) -> Result<Option<String>, PyEmitError> {
    if namespace == "Codec" && method == "deflateFixedCompress" {
        return Err(PyEmitError::unsupported("codec fixed DEFLATE on the Python target").at(span));
    }
    if namespace == "Codec" && method == "zlibFixedCompress" {
        return Err(PyEmitError::unsupported("codec fixed zlib on the Python target").at(span));
    }
    if namespace == "Codec" && method == "reedSolomon255223Protect" {
        return Err(
            PyEmitError::unsupported("Reed-Solomon protection on the Python target").at(span),
        );
    }
    match (namespace, method) {
        ("Map", "ofEntries") => {
            let bound =
                bind_fixed_pipe_static_call_args(args, &["entries"], &[], lhs, piped, span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_map_of_entries({}, {})", slots[0], py_span(span))
            })))
        }
        _ => Ok(None),
    }
}

pub(super) fn emit_pipe_callable_named_call(
    lhs: &Expr,
    callee: &Expr,
    args: &[CallArg],
    piped: &str,
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        return Err(PyEmitError::unsupported("pipe stage argument").at(span));
    }
    let name = match &callee.kind {
        ExprKind::Ident => ctx.text(callee.span),
        ExprKind::Paren(inner) => {
            return emit_pipe_callable_named_call(lhs, inner, args, piped, span, ctx);
        }
        _ => return Err(PyEmitError::unsupported("pipe stage call target").at(callee.span)),
    };
    let info = if !ctx.binding_is_bound(name) {
        ctx.function_info(name).cloned()
    } else {
        ctx.binding_callable_info_at(name, callee.span)
    }
    .ok_or_else(|| PyEmitError::unsupported("pipe stage call target").at(callee.span))?;
    emit_known_function_pipe_named_call(&info, lhs, args, piped, span, ctx)
}

pub(super) fn emit_pipe_callable_spread_call(
    callee: &Expr,
    args: &[CallArg],
    piped: &str,
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let name = match &callee.kind {
        ExprKind::Ident => ctx.text(callee.span),
        ExprKind::Paren(inner) => {
            return emit_pipe_callable_spread_call(inner, args, piped, span, ctx);
        }
        _ => return Err(PyEmitError::unsupported("pipe stage call target").at(callee.span)),
    };
    let info = if !ctx.binding_is_bound(name) {
        ctx.function_info(name).cloned()
    } else {
        ctx.binding_callable_info_at(name, callee.span)
    }
    .ok_or_else(|| PyEmitError::unsupported("pipe stage call target").at(callee.span))?;
    if !info.params.last().is_some_and(|param| param.variadic) {
        return emit_nonvariadic_known_function_pipe_spread_call(&info, args, piped, span, ctx);
    }
    emit_variadic_known_function_pipe_call(&info, args, piped, span, ctx)
}

pub(super) fn emit_nonvariadic_known_function_pipe_spread_call(
    info: &FunctionInfo,
    args: &[CallArg],
    piped: &str,
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    if positional_after_named_py(args) {
        return emit_pipe_call_order_fault_py(args, piped, span, ctx);
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
        prefix.push(emit_expr_with_pipe_placeholder(expr, piped, ctx)?);
    }

    let region_end = args[first_spread..]
        .iter()
        .position(|arg| matches!(arg, CallArg::Named { .. }))
        .map(|idx| idx + first_spread)
        .unwrap_or(args.len());
    let mut tail = Vec::new();
    for arg in &args[first_spread..region_end] {
        match arg {
            CallArg::Positional(expr) => {
                tail.push(emit_expr_with_pipe_placeholder(expr, piped, ctx)?)
            }
            CallArg::Spread(expr) => tail.push(format!(
                "*tpz_spread_values({}, {})",
                emit_expr_with_pipe_placeholder(expr, piped, ctx)?,
                py_span(expr.span)
            )),
            CallArg::Named { .. } => unreachable!("region_end stops at the first named arg"),
        }
    }

    let mut named = Vec::new();
    for arg in &args[region_end..] {
        let CallArg::Named { name, value } = arg else {
            return Err(PyEmitError::unsupported("pipe stage argument").at(call_arg_span(arg)));
        };
        named.push(format!(
            "({}, {})",
            py_string(ctx.text(name.span)),
            emit_expr_with_pipe_placeholder(value, piped, ctx)?
        ));
    }

    Ok(format!(
        "tpz_nonvariadic_spread_call([{}], [{}], [{}], {}, {})",
        prefix.join(", "),
        tail.join(", "),
        named.join(", "),
        info.params.len(),
        py_span(span)
    ))
}

pub(super) fn pipe_args_contain_placeholder(args: &[CallArg]) -> bool {
    args.iter().any(|arg| match arg {
        CallArg::Positional(expr) | CallArg::Spread(expr) => contains_placeholder(expr),
        CallArg::Named { value, .. } => contains_placeholder(value),
    })
}

pub(super) fn pipe_static_callable_value_params(
    callee: &Expr,
    ctx: &Ctx<'_>,
) -> Option<Vec<FunctionParamInfo>> {
    match &callee.kind {
        ExprKind::Member { object, field } => {
            ctx.record_member_field_projection(object, field)
                .callable_params
        }
        ExprKind::Index { object, index } => {
            ctx.array_element_callable_params_for_index(object, index)
        }
        ExprKind::Paren(inner) => pipe_static_callable_value_params(inner, ctx),
        _ => None,
    }
}

pub(super) fn emit_pipe_static_callable_value_call(
    lhs: &Expr,
    callee: &Expr,
    args: &[CallArg],
    params: &[FunctionParamInfo],
    piped: &str,
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    if params.last().is_some_and(|param| param.variadic) {
        return emit_pipe_variadic_static_callable_value_call(
            lhs, callee, args, params, piped, span, ctx,
        );
    }
    if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        return emit_pipe_nonvariadic_static_callable_value_spread_fault(
            lhs,
            args,
            params.len(),
            piped,
            span,
            ctx,
        );
    }

    let callee_value = emit_callable_value_expr(callee, ctx)?;
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
                return emit_pipe_call_order_fault_py(args, piped, span, ctx);
            }
            CallArg::Positional(expr) => {
                if positional_index >= params.len() || filled[positional_index] {
                    return Err(PyEmitError::unsupported("call argument shape").at(expr.span));
                }
                filled[positional_index] = true;
                positional_index += 1;
                positional.push(emit_pipe_arg_expr(expr, lhs, piped, ctx)?);
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
                kwargs.push(format!(
                    "{}: {}",
                    py_string(&params[param_index].py_name),
                    emit_pipe_arg_expr(value, lhs, piped, ctx)?
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
    Ok(format!(
        "tpz_call({callee_value}, {}, {{{}}}, {})",
        py_tuple(positional),
        kwargs.join(", "),
        py_span(span)
    ))
}

pub(super) fn emit_pipe_nonvariadic_static_callable_value_spread_fault(
    lhs: &Expr,
    args: &[CallArg],
    param_count: usize,
    piped: &str,
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    if positional_after_named_py(args) {
        return emit_pipe_call_order_fault_py(args, piped, span, ctx);
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
        prefix.push(emit_pipe_arg_expr(expr, lhs, piped, ctx)?);
    }

    let region_end = args[first_spread..]
        .iter()
        .position(|arg| matches!(arg, CallArg::Named { .. }))
        .map(|idx| idx + first_spread)
        .unwrap_or(args.len());
    let mut tail = Vec::new();
    for arg in &args[first_spread..region_end] {
        match arg {
            CallArg::Positional(expr) => tail.push(emit_pipe_arg_expr(expr, lhs, piped, ctx)?),
            CallArg::Spread(expr) => tail.push(format!(
                "*tpz_spread_values({}, {})",
                emit_expr_with_pipe_placeholder(expr, piped, ctx)?,
                py_span(expr.span)
            )),
            CallArg::Named { .. } => unreachable!("region_end stops at the first named arg"),
        }
    }

    let mut named = Vec::new();
    for arg in &args[region_end..] {
        let CallArg::Named { name, value } = arg else {
            return Err(PyEmitError::unsupported("pipe stage argument").at(call_arg_span(arg)));
        };
        named.push(format!(
            "({}, {})",
            py_string(ctx.text(name.span)),
            emit_pipe_arg_expr(value, lhs, piped, ctx)?
        ));
    }

    Ok(format!(
        "tpz_nonvariadic_spread_call([{}], [{}], [{}], {param_count}, {})",
        prefix.join(", "),
        tail.join(", "),
        named.join(", "),
        py_span(span)
    ))
}

pub(super) fn emit_pipe_variadic_static_callable_value_call(
    lhs: &Expr,
    callee: &Expr,
    args: &[CallArg],
    params: &[FunctionParamInfo],
    piped: &str,
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let fixed_count = params.len().saturating_sub(1);
    let variadic_param = params
        .last()
        .expect("variadic static callable pipe call has at least one parameter");
    let mut fixed_slots = vec![None; fixed_count];
    let mut tail = Vec::new();
    let mut evals = Vec::new();
    let callee_value = push_variadic_call_eval(&mut evals, emit_callable_value_expr(callee, ctx)?);
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
                    push_variadic_call_eval(&mut evals, emit_pipe_arg_expr(expr, lhs, piped, ctx)?);
                evaluated_args.push(value);
                return Ok(render_variadic_call_with_evals(
                    &evals,
                    format!(
                        "tpz_call_order_fault([{}], {}, {})",
                        evaluated_args.join(", "),
                        py_string("positional arguments may not follow named arguments (§5)"),
                        py_span(span)
                    ),
                ));
            }
            CallArg::Positional(expr) => {
                let value =
                    push_variadic_call_eval(&mut evals, emit_pipe_arg_expr(expr, lhs, piped, ctx)?);
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
                let raw = emit_expr_with_pipe_placeholder(expr, piped, ctx)?;
                if saw_named {
                    let value = push_variadic_call_eval(&mut evals, raw);
                    evaluated_args.push(value);
                    return Ok(render_variadic_call_with_evals(
                        &evals,
                        format!(
                            "tpz_call_order_fault([{}], {}, {})",
                            evaluated_args.join(", "),
                            py_string("named arguments must follow spread arguments (§5)"),
                            py_span(span)
                        ),
                    ));
                }
                if !saw_spread
                    && (positional_index.min(fixed_count)..fixed_count)
                        .any(|idx| fixed_slots[idx].is_none() && !params[idx].has_default)
                {
                    skipped_required_by_spread = true;
                }
                let value = format!("tpz_spread_values({}, {})", raw, py_span(expr.span));
                let value = push_variadic_call_eval(&mut evals, value);
                evaluated_args.push(value.clone());
                tail.push(VariadicTailPiece::Spread(value));
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
                let value = push_variadic_call_eval(
                    &mut evals,
                    emit_pipe_arg_expr(value, lhs, piped, ctx)?,
                );
                evaluated_args.push(value.clone());
                fixed_slots[param_index] = Some(StaticVariadicFixedArg::Named(value));
            }
        }
    }

    if skipped_required_by_spread {
        return Ok(render_variadic_call_with_evals(
            &evals,
            format!(
                "tpz_call_order_fault([{}], {}, {})",
                evaluated_args.join(", "),
                py_string("a spread argument cannot skip an unsatisfied fixed parameter (§5)"),
                py_span(span)
            ),
        ));
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
    let call = format!(
        "tpz_call({callee_value}, {}, {{{}}}, {})",
        py_tuple(positional),
        kwargs.join(", "),
        py_span(span)
    );
    Ok(render_variadic_call_with_evals(&evals, call))
}

pub(super) fn emit_pipe_call_order_fault_py(
    args: &[CallArg],
    piped: &str,
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let mut saw_named = false;
    let mut evaluated = Vec::new();
    for arg in args {
        match arg {
            CallArg::Named { value, .. } => {
                saw_named = true;
                evaluated.push(emit_expr_with_pipe_placeholder(value, piped, ctx)?);
            }
            CallArg::Positional(expr) if saw_named => {
                evaluated.push(emit_expr_with_pipe_placeholder(expr, piped, ctx)?);
                return Ok(format!(
                    "tpz_call_order_fault([{}], {}, {})",
                    evaluated.join(", "),
                    py_string("positional arguments may not follow named arguments (§5)"),
                    py_span(span)
                ));
            }
            CallArg::Positional(expr) => {
                evaluated.push(emit_expr_with_pipe_placeholder(expr, piped, ctx)?);
            }
            CallArg::Spread(expr) if saw_named => {
                evaluated.push(emit_expr_with_pipe_placeholder(expr, piped, ctx)?);
                return Ok(format!(
                    "tpz_call_order_fault([{}], {}, {})",
                    evaluated.join(", "),
                    py_string("named arguments must follow spread arguments (§5)"),
                    py_span(span)
                ));
            }
            CallArg::Spread(expr) => evaluated.push(format!(
                "*tpz_spread_values({}, {})",
                emit_expr_with_pipe_placeholder(expr, piped, ctx)?,
                py_span(expr.span)
            )),
        }
    }
    Err(PyEmitError::unsupported("call argument shape").at(span))
}

pub(super) fn emit_known_function_pipe_named_call(
    info: &FunctionInfo,
    lhs: &Expr,
    args: &[CallArg],
    piped: &str,
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    if info.params.last().is_some_and(|param| param.variadic) {
        return Err(PyEmitError::unsupported("pipe stage call target").at(span));
    }
    let has_placeholder = args
        .iter()
        .any(|arg| matches!(arg, CallArg::Positional(expr) | CallArg::Named { value: expr, .. } if contains_placeholder(expr)));
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
                return emit_call_order_fault_py(args, span, ctx);
            }
            CallArg::Positional(expr) => {
                if positional_index >= info.params.len() || filled[positional_index] {
                    return Err(PyEmitError::unsupported("call argument shape").at(expr.span));
                }
                filled[positional_index] = true;
                positional_index += 1;
                call_args.push(emit_pipe_arg_expr(expr, lhs, piped, ctx)?);
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
                call_args.push(format!(
                    "{}={}",
                    info.params[param_index].py_name,
                    emit_pipe_arg_expr(value, lhs, piped, ctx)?
                ));
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
    Ok(known_function_call_expr(info, &call_args, ctx))
}

pub(super) fn emit_pipe_arg_expr(
    expr: &Expr,
    lhs: &Expr,
    piped: &str,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    if matches!(&expr.kind, ExprKind::Placeholder) {
        return emit_pipe_leading_arg(lhs, piped, 0, &[], ctx).map(|value| value.py);
    }
    if contains_placeholder(expr) {
        return emit_expr_with_pipe_placeholder(expr, piped, ctx);
    }
    emit_expr(expr, ctx)
}

pub(super) fn emit_pipe_stage_args(
    args: &[CallArg],
    piped: &str,
    ctx: &Ctx<'_>,
) -> Result<(Vec<String>, bool), PyEmitError> {
    let mut values = Vec::with_capacity(args.len());
    let mut uses_placeholder = false;
    for arg in args {
        match arg {
            CallArg::Positional(expr) => {
                if contains_placeholder(expr) {
                    uses_placeholder = true;
                }
                values.push(emit_expr_with_pipe_placeholder(expr, piped, ctx)?);
            }
            CallArg::Named { name, .. } => {
                return Err(PyEmitError::unsupported("pipe stage argument").at(name.span));
            }
            CallArg::Spread(expr) => {
                return Err(PyEmitError::unsupported("pipe stage argument").at(expr.span));
            }
        }
    }
    Ok((values, uses_placeholder))
}

pub(super) fn emit_pipe_callable_call(
    callee: &Expr,
    values: Vec<String>,
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    match &callee.kind {
        ExprKind::Ident => {
            let name = ctx.text(callee.span);
            if let Some(info) = ctx.function_info(name)
                && !ctx.binding_is_bound(name)
            {
                return emit_known_function_positional_values(info, values, span, ctx);
            }
            if ctx.binding_is_bound(name) {
                if let Some(info) = ctx.binding_callable_info_at(name, callee.span) {
                    return emit_known_function_positional_values(&info, values, span, ctx);
                }
                return Err(PyEmitError::unsupported("pipe stage call target").at(callee.span));
            }
            Err(PyEmitError::unsupported("pipe stage call target").at(callee.span))
        }
        ExprKind::Paren(inner) => emit_pipe_callable_call(inner, values, span, ctx),
        _ if lambda_callee(callee) => Ok(format!(
            "{}({})",
            emit_expr(callee, ctx)?,
            values.join(", ")
        )),
        _ => Err(PyEmitError::unsupported("pipe stage call target").at(callee.span)),
    }
}

pub(super) fn emit_known_function_positional_values(
    info: &FunctionInfo,
    values: Vec<String>,
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    if info.params.last().is_some_and(|param| param.variadic) {
        return Err(PyEmitError::unsupported("pipe stage call target").at(span));
    }
    let value_count = values.len();
    if value_count > info.params.len() {
        return Err(PyEmitError::unsupported("call argument shape").at(span));
    }
    // This underflow is reachable through checker-accepted pipe placeholder
    // calls; direct calls with missing required arguments are rejected earlier.
    for param in info.params.iter().skip(value_count) {
        if !param.has_default {
            return Ok(format!(
                "tpz_call_order_fault([{}], {}, {})",
                values.join(", "),
                py_string(&format!(
                    "missing argument for parameter `{}` (§5)",
                    param.source_name
                )),
                py_span(span)
            ));
        }
    }
    let mut call_args = if info.needs_host {
        vec!["host".to_string()]
    } else {
        Vec::new()
    };
    call_args.extend(values);
    Ok(known_function_call_expr(info, &call_args, ctx))
}

pub(super) fn emit_variadic_known_function_pipe_call(
    info: &FunctionInfo,
    args: &[CallArg],
    piped: &str,
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let fixed_count = info.params.len().saturating_sub(1);
    let variadic_param = info
        .params
        .last()
        .expect("variadic call has at least one parameter");
    let mut fixed_slots = vec![None; fixed_count];
    let mut tail = Vec::new();
    let mut evals = Vec::new();
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
                let value = push_variadic_call_eval(
                    &mut evals,
                    emit_expr_with_pipe_placeholder(expr, piped, ctx)?,
                );
                evaluated_args.push(value);
                return Ok(render_variadic_call_with_evals(
                    &evals,
                    format!(
                        "tpz_call_order_fault([{}], {}, {})",
                        evaluated_args.join(", "),
                        py_string("positional arguments may not follow named arguments (§5)"),
                        py_span(span)
                    ),
                ));
            }
            CallArg::Positional(expr) => {
                let value = push_variadic_call_eval(
                    &mut evals,
                    emit_expr_with_pipe_placeholder(expr, piped, ctx)?,
                );
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
                let raw = emit_expr_with_pipe_placeholder(expr, piped, ctx)?;
                if saw_named {
                    let value = push_variadic_call_eval(&mut evals, raw);
                    evaluated_args.push(value);
                    return Ok(render_variadic_call_with_evals(
                        &evals,
                        format!(
                            "tpz_call_order_fault([{}], {}, {})",
                            evaluated_args.join(", "),
                            py_string("named arguments must follow spread arguments (§5)"),
                            py_span(span)
                        ),
                    ));
                }
                if !saw_spread
                    && (positional_index.min(fixed_count)..fixed_count)
                        .any(|idx| fixed_slots[idx].is_none() && !info.params[idx].has_default)
                {
                    skipped_required_by_spread = true;
                }
                let value = format!("tpz_spread_values({}, {})", raw, py_span(expr.span));
                let value = push_variadic_call_eval(&mut evals, value);
                evaluated_args.push(value.clone());
                tail.push(VariadicTailPiece::Spread(value));
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
                let value = push_variadic_call_eval(
                    &mut evals,
                    emit_expr_with_pipe_placeholder(value, piped, ctx)?,
                );
                evaluated_args.push(value.clone());
                fixed_slots[param_index] = Some(value);
            }
        }
    }

    if skipped_required_by_spread {
        return Ok(render_variadic_call_with_evals(
            &evals,
            format!(
                "tpz_call_order_fault([{}], {}, {})",
                evaluated_args.join(", "),
                py_string("a spread argument cannot skip an unsatisfied fixed parameter (§5)"),
                py_span(span)
            ),
        ));
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
    let call = known_function_call_expr(info, &call_args, ctx);
    Ok(render_variadic_call_with_evals(&evals, call))
}

pub(super) fn render_variadic_call_with_evals(evals: &[(String, String)], call: String) -> String {
    if evals.is_empty() {
        return call;
    }
    let params = evals
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let values = evals
        .iter()
        .map(|(_, value)| value.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!("(lambda {params}: {call})({values})")
}
