use crate::*;

pub(super) fn emit_composed_binding_call(
    source_name: &str,
    args: &[CallArg],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let positional = positional_args(args)?;
    let emitted_args = positional
        .iter()
        .map(|arg| emit_expr(arg, ctx))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!(
        "tpz_call({}, {}, {{}}, {})",
        mangle(source_name),
        py_tuple(emitted_args),
        py_span(span)
    ))
}

pub(super) fn emit_positional_callable_value_call(
    callee: &Expr,
    args: &[CallArg],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let positional = positional_args(args)?;
    let emitted_args = positional
        .iter()
        .map(|arg| emit_expr(arg, ctx))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!(
        "tpz_call({}, {}, {{}}, {})",
        emit_callable_value_expr(callee, ctx)?,
        py_tuple(emitted_args),
        py_span(span)
    ))
}

pub(super) fn emit_static_callable_value_call(
    callee: &Expr,
    args: &[CallArg],
    params: &[FunctionParamInfo],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let callee_py = emit_callable_value_expr(callee, ctx)?;
    emit_static_callable_value_call_with_callee_py(callee_py, args, params, span, ctx)
}

pub(super) fn emit_static_callable_value_call_with_callee_py(
    callee_py: String,
    args: &[CallArg],
    params: &[FunctionParamInfo],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    if params.last().is_some_and(|param| param.variadic) {
        return emit_variadic_static_callable_value_call_with_callee_py(
            callee_py, args, params, span, ctx,
        );
    }
    if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        return emit_nonvariadic_static_spread_fault(args, span, ctx);
    }
    let mut positional = Vec::new();
    let mut kwargs = Vec::new();
    let mut filled = vec![false; params.len()];
    let mut positional_index = 0usize;
    let mut saw_named = false;
    for arg in args {
        match arg {
            CallArg::Positional(_) if saw_named => {
                return emit_call_order_fault_py(args, span, ctx);
            }
            CallArg::Positional(expr) => {
                if positional_index >= params.len() || filled[positional_index] {
                    return Err(PyEmitError::unsupported("call argument shape").at(expr.span));
                }
                filled[positional_index] = true;
                positional_index += 1;
                positional.push(emit_expr(expr, ctx)?);
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
                    emit_expr(value, ctx)?
                ));
            }
            CallArg::Spread(expr) => {
                return Err(PyEmitError::unsupported("call argument shape").at(expr.span));
            }
        }
    }
    for (param, is_filled) in params.iter().zip(filled) {
        if !is_filled && !param.has_default {
            return Err(PyEmitError::unsupported("call argument shape").at(span));
        }
    }
    Ok(format!(
        "tpz_call({}, {}, {{{}}}, {})",
        callee_py,
        py_tuple(positional),
        kwargs.join(", "),
        py_span(span)
    ))
}

pub(super) fn emit_user_receiver_method_call(
    callee_py: String,
    args: &[CallArg],
    params: &[FunctionParamInfo],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    if args.iter().any(|arg| matches!(arg, CallArg::Named { .. }))
        && args.iter().all(|arg| !matches!(arg, CallArg::Spread(_)))
    {
        if positional_after_named_py(args) {
            return emit_call_order_fault_py(args, span, ctx);
        }
        let mut positional = Vec::new();
        let mut kwargs = Vec::new();
        for arg in args {
            match arg {
                CallArg::Positional(expr) => positional.push(emit_expr(expr, ctx)?),
                CallArg::Named { name, value } => kwargs.push(format!(
                    "{}: {}",
                    py_string(&mangle(ctx.text(name.span))),
                    emit_expr(value, ctx)?
                )),
                CallArg::Spread(_) => unreachable!("spread excluded"),
            }
        }
        let call = format!(
            "tpz_call({callee_py}, {}, {{{}}}, {})",
            py_tuple(positional),
            kwargs.join(", "),
            py_span(span)
        );
        return Ok(cooperative_user_method_call(call, ctx.cooperative_yields));
    }
    let call = emit_static_callable_value_call_with_callee_py(callee_py, args, params, span, ctx)?;
    Ok(cooperative_user_method_call(call, ctx.cooperative_yields))
}

pub(super) fn emit_dynamic_user_receiver_method_call(
    callee_py: String,
    args: &[CallArg],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    if positional_after_named_py(args) {
        return emit_call_order_fault_py(args, span, ctx);
    }
    let mut pieces = Vec::with_capacity(args.len());
    for arg in args {
        match arg {
            CallArg::Positional(expr) => {
                pieces.push(format!("(\"pos\", None, {})", emit_expr(expr, ctx)?))
            }
            CallArg::Spread(expr) => pieces.push(format!(
                "(\"spread\", None, tpz_spread_values({}, {}))",
                emit_expr(expr, ctx)?,
                py_span(expr.span)
            )),
            CallArg::Named { name, value } => pieces.push(format!(
                "(\"named\", {}, {})",
                py_string(&mangle(ctx.text(name.span))),
                emit_expr(value, ctx)?
            )),
        }
    }
    let helper = if ctx.cooperative_yields {
        "tpz_user_method_call_cooperative"
    } else {
        "tpz_user_method_call"
    };
    let call = format!(
        "{helper}({callee_py}, [{}], {})",
        pieces.join(", "),
        py_span(span)
    );
    Ok(if ctx.cooperative_yields {
        format!("(yield from {call})")
    } else {
        call
    })
}

pub(super) fn cooperative_user_method_call(call: String, cooperative: bool) -> String {
    if !cooperative {
        return call;
    }
    if let Some(inner) = call
        .strip_prefix("tpz_call(")
        .and_then(|call| call.strip_suffix(')'))
    {
        format!("(yield from tpz_call_cooperative({inner}))")
    } else {
        // Variadic calls can carry an evaluation-order lambda wrapper. That
        // wrapper is intentionally left synchronous until a statement-lowered
        // cooperative variadic method call needs to be admitted.
        call
    }
}

pub(super) fn record_field_callable_callee_py(
    receiver_py: &str,
    field: &Ident,
    span: Span,
    ctx: &Ctx<'_>,
) -> String {
    let member = ctx.text(field.span);
    format!(
        "tpz_member({}, {}, {}, {})",
        receiver_py,
        py_string(&mangle(member)),
        py_string(member),
        py_span(span)
    )
}

pub(super) fn emit_optional_static_callable_value_call(
    object: &Expr,
    field: &Ident,
    args: &[CallArg],
    params: &[FunctionParamInfo],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let object_py = emit_expr(object, ctx)?;
    let some_callee = record_field_callable_callee_py("__tpz_obj.value", field, span, ctx);
    let direct_callee = record_field_callable_callee_py("__tpz_obj", field, span, ctx);
    let some_call =
        emit_static_callable_value_call_with_callee_py(some_callee, args, params, span, ctx)?;
    let direct_call =
        emit_static_callable_value_call_with_callee_py(direct_callee, args, params, span, ctx)?;
    Ok(format!(
        "(lambda __tpz_obj: None if __tpz_obj is None else (tpz_wrap_optional({some_call}) if isinstance(__tpz_obj, Some) else {direct_call}))({object_py})"
    ))
}

pub(super) fn emit_known_function_call(
    info: &FunctionInfo,
    args: &[CallArg],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    if info.params.last().is_some_and(|param| param.variadic) {
        return emit_variadic_known_function_call(info, args, span, ctx);
    }
    if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        return emit_nonvariadic_known_function_spread_fault(info, args, span, ctx);
    }
    let mut call_args = if info.needs_host {
        vec!["host".to_string()]
    } else {
        Vec::new()
    };
    let mut filled = vec![false; info.params.len()];
    let mut positional_index = 0usize;
    let mut saw_named = false;
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
                call_args.push(emit_expr(expr, ctx)?);
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
                    emit_expr(value, ctx)?
                ));
            }
            CallArg::Spread(expr) => {
                return Err(PyEmitError::unsupported("call argument shape").at(expr.span));
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

pub(super) fn emit_nonvariadic_known_function_spread_fault(
    info: &FunctionInfo,
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
        "tpz_nonvariadic_spread_call([{}], [{}], [{}], {}, {})",
        prefix.join(", "),
        tail.join(", "),
        named.join(", "),
        info.params.len(),
        py_span(span)
    ))
}

pub(super) fn positional_after_named_py(args: &[CallArg]) -> bool {
    let mut saw_named = false;
    for arg in args {
        match arg {
            CallArg::Named { .. } => saw_named = true,
            CallArg::Positional(_) | CallArg::Spread(_) if saw_named => return true,
            CallArg::Positional(_) | CallArg::Spread(_) => {}
        }
    }
    false
}

pub(super) fn emit_call_order_fault_py(
    args: &[CallArg],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let mut saw_named = false;
    let mut evaluated = Vec::new();
    for arg in args {
        match arg {
            CallArg::Named { value, .. } => {
                saw_named = true;
                evaluated.push(emit_expr(value, ctx)?);
            }
            CallArg::Positional(expr) if saw_named => {
                evaluated.push(emit_expr(expr, ctx)?);
                return Ok(format!(
                    "tpz_call_order_fault([{}], {}, {})",
                    evaluated.join(", "),
                    py_string("positional arguments may not follow named arguments (§5)"),
                    py_span(span)
                ));
            }
            CallArg::Positional(expr) => evaluated.push(emit_expr(expr, ctx)?),
            CallArg::Spread(expr) if saw_named => {
                evaluated.push(emit_expr(expr, ctx)?);
                return Ok(format!(
                    "tpz_call_order_fault([{}], {}, {})",
                    evaluated.join(", "),
                    py_string("named arguments must follow spread arguments (§5)"),
                    py_span(span)
                ));
            }
            CallArg::Spread(expr) => evaluated.push(format!(
                "*tpz_spread_values({}, {})",
                emit_expr(expr, ctx)?,
                py_span(expr.span)
            )),
        }
    }
    Err(PyEmitError::unsupported("call argument shape").at(span))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum VariadicTailPiece {
    Value(String),
    Spread(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum StaticVariadicFixedArg {
    Positional(String),
    Named(String),
}

pub(super) fn emit_variadic_static_callable_value_call_with_callee_py(
    callee_py: String,
    args: &[CallArg],
    params: &[FunctionParamInfo],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let fixed_count = params.len().saturating_sub(1);
    let variadic_param = params
        .last()
        .expect("variadic static callable call has at least one parameter");
    let mut fixed_slots = vec![None; fixed_count];
    let mut tail = Vec::new();
    let mut evals = Vec::new();
    let callee_value = push_variadic_call_eval(&mut evals, callee_py);
    let mut positional_index = 0usize;
    let mut saw_named = false;
    let mut saw_spread = false;
    let mut skipped_required_by_spread = false;
    let mut evaluated_args = Vec::new();

    for arg in args {
        match arg {
            CallArg::Positional(expr) if saw_named => {
                let value = push_variadic_call_eval(&mut evals, emit_expr(expr, ctx)?);
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
                let value = push_variadic_call_eval(&mut evals, emit_expr(expr, ctx)?);
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
                let raw = emit_expr(expr, ctx)?;
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
                let value = push_variadic_call_eval(&mut evals, emit_expr(value, ctx)?);
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

pub(super) fn emit_variadic_known_function_call(
    info: &FunctionInfo,
    args: &[CallArg],
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
    let mut positional_index = 0usize;
    let mut saw_named = false;
    let mut saw_spread = false;

    for arg in args {
        match arg {
            CallArg::Positional(expr) if saw_named => {
                return Err(PyEmitError::unsupported("call argument shape").at(expr.span));
            }
            CallArg::Positional(expr) => {
                let value = push_variadic_call_eval(&mut evals, emit_expr(expr, ctx)?);
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
                let value = format!(
                    "tpz_spread_values({}, {})",
                    emit_expr(expr, ctx)?,
                    py_span(expr.span)
                );
                let value = push_variadic_call_eval(&mut evals, value);
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
                fixed_slots[param_index] =
                    Some(push_variadic_call_eval(&mut evals, emit_expr(value, ctx)?));
            }
        }
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
    if evals.is_empty() {
        return Ok(call);
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
    Ok(format!("(lambda {params}: {call})({values})"))
}

pub(super) fn push_variadic_call_eval(evals: &mut Vec<(String, String)>, value: String) -> String {
    let name = format!("__tpz_vararg_{}", evals.len());
    evals.push((name.clone(), value));
    name
}

pub(super) fn render_variadic_tail(tail: &[VariadicTailPiece]) -> String {
    if tail.is_empty() {
        return "[]".to_string();
    }
    let items = tail
        .iter()
        .map(|piece| match piece {
            VariadicTailPiece::Value(value) => value.clone(),
            VariadicTailPiece::Spread(value) => format!("*{value}"),
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{items}]")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BoundStaticArgs {
    pub(super) slots: Vec<String>,
    pub(super) ordered: Vec<(usize, String)>,
    pub(super) cooperative_callback_slots: Vec<bool>,
    pub(super) spread_fault: Option<String>,
}

impl BoundStaticArgs {
    pub(super) fn slot_is_cooperative_callback(&self, index: usize) -> bool {
        self.cooperative_callback_slots
            .get(index)
            .copied()
            .unwrap_or(false)
    }
}

pub(super) fn emit_free_builtin_call(
    name: &str,
    args: &[CallArg],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<Option<String>, PyEmitError> {
    match name {
        "Some" => {
            let bound = bind_static_call_args(args, &["value"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("Some({})", slots[0])
            })))
        }
        "Ok" => {
            let bound = bind_static_call_args(args, &["value"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("Ok({})", slots[0])
            })))
        }
        "Err" => {
            let bound = bind_static_call_args(args, &["value"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("Err({})", slots[0])
            })))
        }
        "input" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |_| {
                "host.input()".to_string()
            })))
        }
        "print" => {
            let bound = bind_fixed_static_call_args(args, &["value"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("host.print({}, {})", slots[0], py_span(span))
            })))
        }
        "toInt" => {
            let bound = bind_fixed_static_call_args(args, &["text"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_to_int({}, {})", slots[0], py_span(span))
            })))
        }
        "fromCodePoint" => {
            let bound = bind_fixed_static_call_args(args, &["n"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_from_code_point({}, {})", slots[0], py_span(span))
            })))
        }
        "map" => {
            let bound = bind_fixed_static_call_args(args, &["xs", "f"], &[(1, 1)], span, ctx)?;
            let cooperative = ctx.cooperative_yields;
            let cooperative_callback = bound.slot_is_cooperative_callback(1);
            Ok(Some(render_bound_static_call(&bound, |slots| {
                render_array_map_call_with_callback(
                    &slots[0],
                    &slots[1],
                    span,
                    cooperative,
                    cooperative_callback,
                )
            })))
        }
        "filter" => {
            let bound = bind_fixed_static_call_args(args, &["xs", "f"], &[(1, 1)], span, ctx)?;
            let cooperative = ctx.cooperative_yields;
            let cooperative_callback = bound.slot_is_cooperative_callback(1);
            Ok(Some(render_bound_static_call(&bound, |slots| {
                render_array_filter_call_with_callback(
                    &slots[0],
                    &slots[1],
                    span,
                    cooperative,
                    cooperative_callback,
                )
            })))
        }
        "reduce" => {
            let bound =
                bind_fixed_static_call_args(args, &["xs", "initial", "f"], &[(2, 2)], span, ctx)?;
            let cooperative = ctx.cooperative_yields;
            let cooperative_callback = bound.slot_is_cooperative_callback(2);
            Ok(Some(render_bound_static_call(&bound, |slots| {
                render_array_reduce_call_with_callback(
                    &slots[0],
                    &slots[1],
                    &slots[2],
                    span,
                    cooperative,
                    cooperative_callback,
                )
            })))
        }
        "open" => {
            let bound = bind_fixed_static_call_args(args, &["path"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("host.open_file({}, {})", slots[0], py_span(span))
            })))
        }
        _ => Ok(None),
    }
}

pub(super) fn emit_namespace_builtin_call(
    namespace: &str,
    method: &str,
    args: &[CallArg],
    span: Span,
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
        ("Array", "of") => {
            let items = emit_variadic_namespace_args(args, ctx)?;
            Ok(Some(format!("[{}]", items.join(", "))))
        }
        ("Set", "of") => {
            let items = emit_variadic_namespace_args(args, ctx)?;
            Ok(Some(format!(
                "tpz_set_of([{}], {})",
                items.join(", "),
                py_span(span)
            )))
        }
        ("Map", "new") => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |_| {
                "tpz_map_new()".to_string()
            })))
        }
        ("Map", "ofEntries") => {
            let bound = bind_fixed_static_call_args(args, &["entries"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_map_of_entries({}, {})", slots[0], py_span(span))
            })))
        }
        ("JSON", "parse") => {
            let bound = bind_fixed_static_call_args(args, &["text"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_json_parse({}, {})", slots[0], py_span(span))
            })))
        }
        ("JSON", "stringify") => {
            let bound = bind_fixed_static_call_args(args, &["value"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_json_stringify({})", slots[0])
            })))
        }
        ("Bytes", "empty") => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |_| {
                "tpz_bytes_empty()".to_string()
            })))
        }
        ("Bytes", "encodeUtf8") => {
            let bound = bind_fixed_static_call_args(args, &["s"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_bytes_encode_utf8({}, {})", slots[0], py_span(span))
            })))
        }
        ("Bytes", "fromArray") => {
            let bound = bind_fixed_static_call_args(args, &["values"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_bytes_from_array({}, {})", slots[0], py_span(span))
            })))
        }
        ("Bytes", "fromHex") => {
            let bound = bind_fixed_static_call_args(args, &["s"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_bytes_from_hex({}, {})", slots[0], py_span(span))
            })))
        }
        ("Bytes", "fromBase64") => {
            let bound = bind_fixed_static_call_args(args, &["s"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_bytes_from_base64({}, {})", slots[0], py_span(span))
            })))
        }
        ("Bytes", "concat") => {
            let bound = bind_fixed_static_call_args(args, &["a", "b"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!(
                    "tpz_bytes_concat({}, {}, {})",
                    slots[0],
                    slots[1],
                    py_span(span)
                )
            })))
        }
        ("ByteBuffer", "allocate") => {
            let params: &[&str] = if args.len() == 1 {
                &["length"]
            } else {
                &["length", "value"]
            };
            let bound = bind_fixed_static_call_args(args, params, &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                if slots.len() == 1 {
                    format!(
                        "tpz_byte_buffer_allocate({}, 0, {})",
                        slots[0],
                        py_span(span)
                    )
                } else {
                    format!(
                        "tpz_byte_buffer_allocate({}, {}, {})",
                        slots[0],
                        slots[1],
                        py_span(span)
                    )
                }
            })))
        }
        ("ByteBuffer", "fromBytes") => {
            let bound = bind_fixed_static_call_args(args, &["value"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!(
                    "tpz_byte_buffer_from_bytes({}, {})",
                    slots[0],
                    py_span(span)
                )
            })))
        }
        ("Encoding", "utf8Encode") => {
            let bound = bind_fixed_static_call_args(args, &["text"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_bytes_encode_utf8({}, {})", slots[0], py_span(span))
            })))
        }
        ("Encoding", "utf8Decode") => {
            let bound = bind_fixed_static_call_args(args, &["bytes"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_bytes_decode_utf8({}, {})", slots[0], py_span(span))
            })))
        }
        ("Encoding", "hexEncode") => {
            let bound = bind_fixed_static_call_args(args, &["bytes"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_bytes_to_hex({}, {})", slots[0], py_span(span))
            })))
        }
        ("Encoding", "hexDecode") => {
            let bound = bind_fixed_static_call_args(args, &["text"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_bytes_from_hex({}, {})", slots[0], py_span(span))
            })))
        }
        ("Encoding", "base64Encode") => {
            let bound = bind_fixed_static_call_args(args, &["bytes"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_bytes_to_base64({}, {})", slots[0], py_span(span))
            })))
        }
        ("Encoding", "base64Decode") => {
            let bound = bind_fixed_static_call_args(args, &["text"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_bytes_from_base64({}, {})", slots[0], py_span(span))
            })))
        }
        ("Math", method) => {
            let (leaf, params): (&str, &[&str]) = match method {
                "sqrt" => ("tpz_math_sqrt", &["x"]),
                "abs" => ("tpz_math_abs", &["x"]),
                "floor" => ("tpz_math_floor", &["x"]),
                "ceil" => ("tpz_math_ceil", &["x"]),
                "round" => ("tpz_math_round", &["x"]),
                "sin" => ("tpz_math_sin", &["x"]),
                "cos" => ("tpz_math_cos", &["x"]),
                "tan" => ("tpz_math_tan", &["x"]),
                "isNaN" => ("tpz_math_is_nan", &["x"]),
                "isFinite" => ("tpz_math_is_finite", &["x"]),
                "parseFloat" => ("tpz_math_parse_float", &["s"]),
                "min" => ("tpz_math_min", &["a", "b"]),
                "max" => ("tpz_math_max", &["a", "b"]),
                _ => return Ok(None),
            };
            let bound = bind_fixed_static_call_args(args, params, &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                let mut rendered = slots.join(", ");
                if !rendered.is_empty() {
                    rendered.push_str(", ");
                }
                format!("{leaf}({rendered}{})", py_span(span))
            })))
        }
        ("FS", "readText") => {
            let bound = bind_fixed_static_call_args(args, &["path"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_fs_read_text(host, {}, {})", slots[0], py_span(span))
            })))
        }
        ("FS", "writeText") => {
            let bound = bind_fixed_static_call_args(args, &["path", "text"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!(
                    "tpz_fs_write_text(host, {}, {}, {})",
                    slots[0],
                    slots[1],
                    py_span(span)
                )
            })))
        }
        ("FS", "readBytes") => {
            let bound = bind_fixed_static_call_args(args, &["path"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_fs_read_bytes(host, {}, {})", slots[0], py_span(span))
            })))
        }
        ("FS", "writeBytes") => {
            let bound = bind_fixed_static_call_args(args, &["path", "bytes"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!(
                    "tpz_fs_write_bytes(host, {}, {}, {})",
                    slots[0],
                    slots[1],
                    py_span(span)
                )
            })))
        }
        ("FS", "list") => {
            let bound = bind_fixed_static_call_args(args, &["path"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_fs_list(host, {}, {})", slots[0], py_span(span))
            })))
        }
        ("Cli", "hasFlag") => {
            let bound = bind_fixed_static_call_args(args, &["args", "name"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!(
                    "tpz_cli_has_flag({}, {}, {})",
                    slots[0],
                    slots[1],
                    py_span(span)
                )
            })))
        }
        ("Cli", "option") => {
            let bound = bind_fixed_static_call_args(args, &["args", "name"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!(
                    "tpz_cli_option({}, {}, {})",
                    slots[0],
                    slots[1],
                    py_span(span)
                )
            })))
        }
        ("Cli", "options") => {
            let bound = bind_fixed_static_call_args(args, &["args", "name"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!(
                    "tpz_cli_options({}, {}, {})",
                    slots[0],
                    slots[1],
                    py_span(span)
                )
            })))
        }
        ("Cli", "positionals") => {
            let bound = bind_fixed_static_call_args(args, &["args"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_cli_positionals({}, {})", slots[0], py_span(span))
            })))
        }
        ("Hash", "sha256") => {
            let bound = bind_fixed_static_call_args(args, &["data"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_hash_sha256({}, {})", slots[0], py_span(span))
            })))
        }
        ("Hash", "sha512") => {
            let bound = bind_fixed_static_call_args(args, &["data"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_hash_sha512({}, {})", slots[0], py_span(span))
            })))
        }
        ("Hash", "hmacSha256") => {
            let bound = bind_fixed_static_call_args(args, &["key", "message"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!(
                    "tpz_hash_hmac_sha256({}, {}, {})",
                    slots[0],
                    slots[1],
                    py_span(span)
                )
            })))
        }
        ("Hash", "crc32") => {
            let bound = bind_fixed_static_call_args(args, &["data"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_hash_crc32({}, {})", slots[0], py_span(span))
            })))
        }
        ("Regex", "compile") => {
            let bound = bind_fixed_static_call_args(args, &["pattern"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_regex_compile({}, {})", slots[0], py_span(span))
            })))
        }
        ("CSV", "parseWithHeader") => {
            let bound = bind_fixed_static_call_args(args, &["text"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_csv_parse_with_header({}, {})", slots[0], py_span(span))
            })))
        }
        ("TOML", "parse") => {
            let bound = bind_fixed_static_call_args(args, &["text"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_toml_parse({}, {})", slots[0], py_span(span))
            })))
        }
        ("TOML", "toJson") => {
            let bound = bind_fixed_static_call_args(args, &["value"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_toml_to_json({}, {})", slots[0], py_span(span))
            })))
        }
        ("URL", "parse") => {
            let bound = bind_fixed_static_call_args(args, &["text"], &[], span, ctx)?;
            Ok(Some(render_bound_static_call(&bound, |slots| {
                format!("tpz_url_parse({}, {})", slots[0], py_span(span))
            })))
        }
        _ => Ok(None),
    }
}

pub(super) fn emit_variadic_namespace_args(
    args: &[CallArg],
    ctx: &Ctx<'_>,
) -> Result<Vec<String>, PyEmitError> {
    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        match arg {
            CallArg::Positional(expr) => out.push(emit_expr(expr, ctx)?),
            CallArg::Spread(expr) => out.push(format!(
                "*tpz_spread_values({}, {})",
                emit_expr(expr, ctx)?,
                py_span(expr.span)
            )),
            CallArg::Named { name, .. } => {
                return Err(PyEmitError::unsupported("call argument shape").at(name.span));
            }
        }
    }
    Ok(out)
}

#[derive(Clone, Copy)]
pub(super) struct ReceiverBuiltinSpec {
    pub(super) params: &'static [&'static str],
    pub(super) returns_unit: bool,
    pub(super) render: fn(&str, &[String], Span) -> String,
}

impl ReceiverBuiltinSpec {
    pub(super) fn value(
        params: &'static [&'static str],
        render: fn(&str, &[String], Span) -> String,
    ) -> Self {
        Self {
            params,
            returns_unit: false,
            render,
        }
    }

    pub(super) fn unit(
        params: &'static [&'static str],
        render: fn(&str, &[String], Span) -> String,
    ) -> Self {
        Self {
            params,
            returns_unit: true,
            render,
        }
    }

    pub(super) fn render(self, recv: &str, slots: &[String], span: Span) -> String {
        (self.render)(recv, slots, span)
    }
}

pub(super) fn string_receiver_builtin(method: &str) -> Option<ReceiverBuiltinSpec> {
    let spec = match method {
        "startsWith" => ReceiverBuiltinSpec::value(&["prefix"], |recv, slots, span| {
            format!(
                "tpz_string_starts_with({recv}, {}, {})",
                slots[0],
                py_span(span)
            )
        }),
        "endsWith" => ReceiverBuiltinSpec::value(&["suffix"], |recv, slots, span| {
            format!(
                "tpz_string_ends_with({recv}, {}, {})",
                slots[0],
                py_span(span)
            )
        }),
        "contains" => ReceiverBuiltinSpec::value(&["sub"], |recv, slots, span| {
            format!(
                "tpz_string_contains({recv}, {}, {})",
                slots[0],
                py_span(span)
            )
        }),
        "indexOf" => ReceiverBuiltinSpec::value(&["sub"], |recv, slots, span| {
            format!(
                "tpz_string_index_of({recv}, {}, {})",
                slots[0],
                py_span(span)
            )
        }),
        "lastIndexOf" => ReceiverBuiltinSpec::value(&["sub"], |recv, slots, span| {
            format!(
                "tpz_string_last_index_of({recv}, {}, {})",
                slots[0],
                py_span(span)
            )
        }),
        "trim" => ReceiverBuiltinSpec::value(&[], |recv, _, span| {
            format!("tpz_string_trim({recv}, {})", py_span(span))
        }),
        "trimStart" => ReceiverBuiltinSpec::value(&[], |recv, _, span| {
            format!("tpz_string_trim_start({recv}, {})", py_span(span))
        }),
        "trimEnd" => ReceiverBuiltinSpec::value(&[], |recv, _, span| {
            format!("tpz_string_trim_end({recv}, {})", py_span(span))
        }),
        "byteLength" => ReceiverBuiltinSpec::value(&[], |recv, _, span| {
            format!("tpz_string_byte_length({recv}, {})", py_span(span))
        }),
        "scalars" => ReceiverBuiltinSpec::value(&[], |recv, _, _| format!("list({recv})")),
        "split" => ReceiverBuiltinSpec::value(&["sep"], |recv, slots, span| {
            format!("tpz_string_split({recv}, {}, {})", slots[0], py_span(span))
        }),
        "slice" => ReceiverBuiltinSpec::value(&["start", "end"], |recv, slots, span| {
            format!(
                "tpz_string_slice({recv}, {}, {}, {})",
                slots[0],
                slots[1],
                py_span(span)
            )
        }),
        "replace" => ReceiverBuiltinSpec::value(&["old", "new"], |recv, slots, span| {
            format!(
                "tpz_string_replace({recv}, {}, {}, {})",
                slots[0],
                slots[1],
                py_span(span)
            )
        }),
        _ => return None,
    };
    Some(spec)
}

pub(super) fn array_receiver_builtin(method: &str) -> Option<ReceiverBuiltinSpec> {
    let spec = match method {
        "push" => ReceiverBuiltinSpec::unit(&["x"], |recv, slots, span| {
            format!("tpz_array_push({recv}, {}, {})", slots[0], py_span(span))
        }),
        "get" => ReceiverBuiltinSpec::value(&["i"], |recv, slots, span| {
            format!("tpz_get({recv}, {}, {})", slots[0], py_span(span))
        }),
        "slice" => ReceiverBuiltinSpec::value(&["start", "end"], |recv, slots, span| {
            format!(
                "tpz_array_slice({recv}, {}, {}, {})",
                slots[0],
                slots[1],
                py_span(span)
            )
        }),
        "join" => ReceiverBuiltinSpec::value(&["sep"], |recv, slots, span| {
            format!("tpz_array_join({recv}, {}, {})", slots[0], py_span(span))
        }),
        "indexOf" => ReceiverBuiltinSpec::value(&["x"], |recv, slots, span| {
            format!(
                "tpz_array_index_of({recv}, {}, {})",
                slots[0],
                py_span(span)
            )
        }),
        "sorted" => ReceiverBuiltinSpec::value(&[], |recv, _, span| {
            format!("tpz_array_sorted({recv}, {})", py_span(span))
        }),
        "sort" => ReceiverBuiltinSpec::unit(&[], |recv, _, span| {
            format!("tpz_array_sort({recv}, {})", py_span(span))
        }),
        "pop" => ReceiverBuiltinSpec::value(&[], |recv, _, span| {
            format!("tpz_array_pop({recv}, {})", py_span(span))
        }),
        "clear" => ReceiverBuiltinSpec::unit(&[], |recv, _, span| {
            format!("tpz_array_clear({recv}, {})", py_span(span))
        }),
        "reverse" => ReceiverBuiltinSpec::unit(&[], |recv, _, span| {
            format!("tpz_array_reverse({recv}, {})", py_span(span))
        }),
        "insert" => ReceiverBuiltinSpec::unit(&["index", "value"], |recv, slots, span| {
            format!(
                "tpz_array_insert({recv}, {}, {}, {})",
                slots[0],
                slots[1],
                py_span(span)
            )
        }),
        "removeAt" => ReceiverBuiltinSpec::value(&["index"], |recv, slots, span| {
            format!(
                "tpz_array_remove_at({recv}, {}, {})",
                slots[0],
                py_span(span)
            )
        }),
        _ => return None,
    };
    Some(spec)
}
