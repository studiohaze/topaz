use crate::*;

pub(super) fn emit_statement_lowered_receiver_static_call_to_target(
    object: &Expr,
    args: &[CallArg],
    call: StaticCallSpec<'_>,
    target: StatementTarget<'_, '_, '_, '_>,
    render_body: impl FnOnce(&str, &[String]) -> String,
) -> Result<(), PyEmitError> {
    emit_statement_lowered_receiver_static_call_to_target_with_bound(
        object,
        args,
        call,
        target,
        |recv, bound| render_body(recv, &bound.slots),
    )
}

pub(super) fn emit_statement_lowered_receiver_static_call_to_target_with_bound(
    object: &Expr,
    args: &[CallArg],
    call: StaticCallSpec<'_>,
    target: StatementTarget<'_, '_, '_, '_>,
    render_body: impl FnOnce(&str, &BoundStaticArgs) -> String,
) -> Result<(), PyEmitError> {
    let StaticCallSpec {
        params,
        callback_arities,
        span,
    } = call;
    let StatementTarget {
        target_py,
        ctx,
        indent,
        out,
    } = target;
    if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        let pad = " ".repeat(indent);
        let receiver_value = emit_statement_lowered_expr_value(object, ctx, indent, out)?;
        let receiver_tmp = ctx.fresh_temp("call_recv");
        writeln!(out, "{pad}{receiver_tmp} = {receiver_value}").expect("write to string");
        return emit_statement_lowered_nonvariadic_static_spread_fault_to_target(
            args, span, target_py, ctx, indent, out,
        );
    }
    let pad = " ".repeat(indent);
    let receiver_py = emit_statement_lowered_expr_value(object, ctx, indent, out)?;
    let bound = bind_statement_lowered_static_call_args(
        args,
        params,
        callback_arities,
        span,
        ctx,
        indent,
        out,
    )?;
    writeln!(
        out,
        "{pad}{target_py} = {}",
        render_body(&receiver_py, &bound)
    )
    .expect("write to string");
    Ok(())
}

#[derive(Clone, Copy)]
pub(super) enum StatementLoweredOptionalWrap {
    Value,
    Unit,
}

pub(super) fn parens_are_balanced_outside_strings(expr: &str) -> bool {
    let mut depth = 0usize;
    let mut in_string = None;
    let mut escaped = false;
    for ch in expr.chars() {
        if let Some(quote) = in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                in_string = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => in_string = Some(ch),
            '(' => depth += 1,
            ')' => {
                let Some(next_depth) = depth.checked_sub(1) else {
                    return false;
                };
                depth = next_depth;
            }
            _ => {}
        }
    }
    depth == 0 && in_string.is_none()
}

pub(super) fn contains_yield_from_outside_strings(expr: &str) -> bool {
    let mut in_string = None;
    let mut escaped = false;
    for (idx, ch) in expr.char_indices() {
        if let Some(quote) = in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                in_string = None;
            }
            continue;
        }
        if expr[idx..].starts_with("yield from") {
            return true;
        }
        if matches!(ch, '\'' | '"') {
            in_string = Some(ch);
        }
    }
    false
}

pub(super) fn statement_lowered_yield_from_call_expr(
    expr: &str,
) -> Result<Option<&str>, PyEmitError> {
    let trimmed = expr.trim();
    let Some(rest) = trimmed.strip_prefix("(yield from ") else {
        if contains_yield_from_outside_strings(trimmed) {
            return Err(PyEmitError::unsupported(
                "statement-lowered expression shape",
            ));
        }
        return Ok(None);
    };
    let Some(inner) = rest.strip_suffix(')') else {
        return Err(PyEmitError::unsupported(
            "statement-lowered expression shape",
        ));
    };
    if !parens_are_balanced_outside_strings(inner) {
        return Err(PyEmitError::unsupported(
            "statement-lowered expression shape",
        ));
    }
    Ok(Some(inner))
}

pub(super) fn render_statement_lowered_optional_wrap(
    wrap: StatementLoweredOptionalWrap,
    value: &str,
) -> String {
    match wrap {
        StatementLoweredOptionalWrap::Value => format!("tpz_wrap_optional({value})"),
        StatementLoweredOptionalWrap::Unit => format!("tpz_wrap_optional_unit({value})"),
    }
}

pub(super) fn emit_statement_lowered_optional_wrapped_assignment(
    target_py: &str,
    wrap: StatementLoweredOptionalWrap,
    call: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    if let Some(call) = statement_lowered_yield_from_call_expr(call)? {
        let value_tmp = ctx.fresh_temp("optional_value");
        writeln!(out, "{pad}{value_tmp} = yield from {call}").expect("write to string");
        let wrapped = render_statement_lowered_optional_wrap(wrap, &value_tmp);
        writeln!(out, "{pad}{target_py} = {wrapped}").expect("write to string");
    } else {
        let wrapped = render_statement_lowered_optional_wrap(wrap, call);
        writeln!(out, "{pad}{target_py} = {wrapped}").expect("write to string");
    }
    Ok(())
}

pub(super) fn emit_statement_lowered_optional_receiver_builtin_call_to_target(
    object: &Expr,
    method: &str,
    args: &[CallArg],
    span: Span,
    target: StatementTarget<'_, '_, '_, '_>,
) -> Result<bool, PyEmitError> {
    let StatementTarget {
        target_py,
        ctx,
        indent,
        out,
    } = target;
    let Some(shape) = optional_receiver_inner_shape(object, ctx) else {
        return Ok(false);
    };
    if shape == ReceiverShape::String
        && let Some(spec) = string_receiver_builtin(method)
    {
        emit_statement_lowered_optional_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(spec.params, &[], span),
            StatementLoweredOptionalWrap::Value,
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, slots| spec.render(recv, slots, span),
        )?;
        return Ok(true);
    }
    if shape == ReceiverShape::Array
        && let Some(spec) = array_receiver_builtin(method)
    {
        emit_statement_lowered_optional_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(spec.params, &[], span),
            if spec.returns_unit {
                StatementLoweredOptionalWrap::Unit
            } else {
                StatementLoweredOptionalWrap::Value
            },
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, slots| spec.render(recv, slots, span),
        )?;
        return Ok(true);
    }
    match (method, shape) {
        ("split", ReceiverShape::String) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&["sep"], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| format!("tpz_split({recv}, {}, {})", slots[0], py_span(span)),
            )?
        }
        ("codePointAt", ReceiverShape::String) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&["i"], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| {
                    format!(
                        "tpz_string_code_point_at({recv}, {}, {})",
                        slots[0],
                        py_span(span)
                    )
                },
            )?
        }
        ("byteLength", ReceiverShape::String) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&[], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_string_byte_length({recv}, {})", py_span(span)),
            )?
        }
        ("scalars", ReceiverShape::String) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&[], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("list({recv})"),
            )?
        }
        ("get", ReceiverShape::Array) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&["i"], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| format!("tpz_get({recv}, {}, {})", slots[0], py_span(span)),
            )?
        }
        ("get", ReceiverShape::Map) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&["k"], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| format!("tpz_get({recv}, {}, {})", slots[0], py_span(span)),
            )?
        }
        ("get", ReceiverShape::Bytes) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&["index"], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| format!("tpz_get({recv}, {}, {})", slots[0], py_span(span)),
            )?
        }
        ("get", ReceiverShape::Json) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&["key"], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| format!("tpz_get({recv}, {}, {})", slots[0], py_span(span)),
            )?
        }
        ("getOr", ReceiverShape::Map) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&["k", "default"], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| {
                    format!(
                        "tpz_map_get_or({recv}, {}, {}, {})",
                        slots[0],
                        slots[1],
                        py_span(span)
                    )
                },
            )?
        }
        ("containsKey", ReceiverShape::Map) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&["k"], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| {
                    format!(
                        "tpz_map_contains_key({recv}, {}, {})",
                        slots[0],
                        py_span(span)
                    )
                },
            )?
        }
        ("decodeUtf8", ReceiverShape::Bytes) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&[], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_bytes_decode_utf8({recv}, {})", py_span(span)),
            )?
        }
        ("toHex", ReceiverShape::Bytes) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&[], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_bytes_to_hex({recv}, {})", py_span(span)),
            )?
        }
        ("toBase64", ReceiverShape::Bytes) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&[], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_bytes_to_base64({recv}, {})", py_span(span)),
            )?
        }
        ("isEmpty", ReceiverShape::Bytes | ReceiverShape::Map) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&[], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_is_empty({recv}, {})", py_span(span)),
            )?
        }
        ("slice", ReceiverShape::Bytes) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&["start", "end"], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| {
                    format!(
                        "tpz_bytes_slice({recv}, {}, {}, {})",
                        slots[0],
                        slots[1],
                        py_span(span)
                    )
                },
            )?
        }
        ("toArray", ReceiverShape::Bytes) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&[], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_to_array({recv}, {})", py_span(span)),
            )?
        }
        ("kind", ReceiverShape::Json) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&[], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_json_kind({recv}, {})", py_span(span)),
            )?
        }
        ("isNull", ReceiverShape::Json) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&[], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_json_is_null({recv}, {})", py_span(span)),
            )?
        }
        ("asString", ReceiverShape::Json) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&[], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_json_as_string({recv}, {})", py_span(span)),
            )?
        }
        ("asBool", ReceiverShape::Json) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&[], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_json_as_bool({recv}, {})", py_span(span)),
            )?
        }
        ("asInt", ReceiverShape::Json) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&[], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_json_as_int({recv}, {})", py_span(span)),
            )?
        }
        ("numberText", ReceiverShape::Json) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&[], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_json_number_text({recv}, {})", py_span(span)),
            )?
        }
        ("at", ReceiverShape::Json) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&["index"], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| {
                    format!(
                        "tpz_json_at({recv}, {}, {}, {})",
                        slots[0],
                        py_span(span),
                        py_span(span)
                    )
                },
            )?
        }
        ("length", ReceiverShape::Bytes | ReceiverShape::Json) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&[], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_length({recv}, {})", py_span(span)),
            )?
        }
        ("map", ReceiverShape::Array) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["f"], &[(0, 1)], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                move |recv, bound| {
                    render_array_map_call_with_callback(
                        recv,
                        &bound.slots[0],
                        span,
                        cooperative,
                        bound.slot_is_cooperative_callback(0),
                    )
                },
            )?
        }
        ("map", ReceiverShape::Option) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["f"], &[(0, 1)], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                move |recv, bound| {
                    render_option_map_call_with_callback(
                        recv,
                        &bound.slots[0],
                        span,
                        cooperative,
                        bound.slot_is_cooperative_callback(0),
                    )
                },
            )?
        }
        ("map", ReceiverShape::Result) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["f"], &[(0, 1)], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                move |recv, bound| {
                    render_result_map_call_with_callback(
                        recv,
                        &bound.slots[0],
                        span,
                        cooperative,
                        bound.slot_is_cooperative_callback(0),
                    )
                },
            )?
        }
        ("filter", ReceiverShape::Array) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["f"], &[(0, 1)], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                move |recv, bound| {
                    render_array_filter_call_with_callback(
                        recv,
                        &bound.slots[0],
                        span,
                        cooperative,
                        bound.slot_is_cooperative_callback(0),
                    )
                },
            )?
        }
        ("filter", ReceiverShape::Map) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["f"], &[(0, 2)], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                move |recv, bound| {
                    render_map_filter_call_with_callback(
                        recv,
                        &bound.slots[0],
                        span,
                        cooperative,
                        bound.slot_is_cooperative_callback(0),
                    )
                },
            )?
        }
        ("reduce", ReceiverShape::Array) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["initial", "f"], &[(1, 2)], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                move |recv, bound| {
                    render_array_reduce_call_with_callback(
                        recv,
                        &bound.slots[0],
                        &bound.slots[1],
                        span,
                        cooperative,
                        bound.slot_is_cooperative_callback(1),
                    )
                },
            )?
        }
        ("sorted", ReceiverShape::Array) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&[], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_array_sorted({recv}, {})", py_span(span)),
            )?
        }
        ("sort", ReceiverShape::Array) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&[], &[], span),
                StatementLoweredOptionalWrap::Unit,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_array_sort({recv}, {})", py_span(span)),
            )?
        }
        ("sortedBy", ReceiverShape::Array) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["f"], &[(0, 1)], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                move |recv, bound| {
                    render_array_sorted_by_call_with_callback(
                        recv,
                        &bound.slots[0],
                        span,
                        cooperative,
                        bound.slot_is_cooperative_callback(0),
                    )
                },
            )?
        }
        ("sortBy", ReceiverShape::Array) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["f"], &[(0, 1)], span),
                StatementLoweredOptionalWrap::Unit,
                StatementTarget::new(target_py, ctx, indent, out),
                move |recv, bound| {
                    render_array_sort_by_call_with_callback(
                        recv,
                        &bound.slots[0],
                        span,
                        cooperative,
                        bound.slot_is_cooperative_callback(0),
                    )
                },
            )?
        }
        ("retain", ReceiverShape::Array) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["f"], &[(0, 1)], span),
                StatementLoweredOptionalWrap::Unit,
                StatementTarget::new(target_py, ctx, indent, out),
                move |recv, bound| {
                    render_array_retain_call_with_callback(
                        recv,
                        &bound.slots[0],
                        span,
                        cooperative,
                        bound.slot_is_cooperative_callback(0),
                    )
                },
            )?
        }
        ("flatMap", ReceiverShape::Option) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["f"], &[(0, 1)], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                move |recv, bound| {
                    render_option_flat_map_call_with_callback(
                        recv,
                        &bound.slots[0],
                        span,
                        cooperative,
                        bound.slot_is_cooperative_callback(0),
                    )
                },
            )?
        }
        ("flatMap", ReceiverShape::Result) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["f"], &[(0, 1)], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                move |recv, bound| {
                    render_result_flat_map_call_with_callback(
                        recv,
                        &bound.slots[0],
                        span,
                        cooperative,
                        bound.slot_is_cooperative_callback(0),
                    )
                },
            )?
        }
        ("okOrElse", ReceiverShape::Option) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["f"], &[(0, 0)], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                move |recv, bound| {
                    render_option_ok_or_else_call_with_callback(
                        recv,
                        &bound.slots[0],
                        span,
                        cooperative,
                        bound.slot_is_cooperative_callback(0),
                    )
                },
            )?
        }
        ("okOr", ReceiverShape::Option) => {
            emit_statement_lowered_optional_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&["error"], &[], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| format!("tpz_option_ok_or({recv}, {}, {})", slots[0], py_span(span)),
            )?
        }
        ("mapValues", ReceiverShape::Map) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["f"], &[(0, 1)], span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                move |recv, bound| {
                    render_map_map_values_call_with_callback(
                        recv,
                        &bound.slots[0],
                        span,
                        cooperative,
                        bound.slot_is_cooperative_callback(0),
                    )
                },
            )?
        }
        ("update", ReceiverShape::Map) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["k", "initial", "f"], &[(2, 1)], span),
                StatementLoweredOptionalWrap::Unit,
                StatementTarget::new(target_py, ctx, indent, out),
                move |recv, bound| {
                    render_map_update_call_with_callback(
                        recv,
                        &bound.slots[0],
                        &bound.slots[1],
                        &bound.slots[2],
                        span,
                        cooperative,
                        bound.slot_is_cooperative_callback(2),
                    )
                },
            )?
        }
        _ => return Ok(false),
    }
    Ok(true)
}

pub(super) fn emit_statement_lowered_optional_receiver_static_call_to_target(
    object: &Expr,
    args: &[CallArg],
    call: StaticCallSpec<'_>,
    wrap: StatementLoweredOptionalWrap,
    target: StatementTarget<'_, '_, '_, '_>,
    render_body: impl Fn(&str, &[String]) -> String,
) -> Result<(), PyEmitError> {
    emit_statement_lowered_optional_receiver_static_call_to_target_with_bound(
        object,
        args,
        call,
        wrap,
        target,
        |recv, bound| render_body(recv, &bound.slots),
    )
}

pub(super) fn emit_statement_lowered_optional_receiver_static_call_to_target_with_bound(
    object: &Expr,
    args: &[CallArg],
    call: StaticCallSpec<'_>,
    wrap: StatementLoweredOptionalWrap,
    target: StatementTarget<'_, '_, '_, '_>,
    render_body: impl Fn(&str, &BoundStaticArgs) -> String,
) -> Result<(), PyEmitError> {
    let StaticCallSpec {
        params,
        callback_arities,
        span,
    } = call;
    let StatementTarget {
        target_py,
        ctx,
        indent,
        out,
    } = target;
    let pad = " ".repeat(indent);
    let receiver_value = emit_statement_lowered_expr_value(object, ctx, indent, out)?;
    let receiver_tmp = ctx.fresh_temp("optional_receiver");
    writeln!(out, "{pad}{receiver_tmp} = {receiver_value}").expect("write to string");
    writeln!(out, "{pad}if {receiver_tmp} is None:").expect("write to string");
    writeln!(out, "{pad}    {target_py} = None").expect("write to string");
    writeln!(out, "{pad}else:").expect("write to string");
    writeln!(out, "{pad}    if isinstance({receiver_tmp}, Some):").expect("write to string");
    if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        ctx.with_metadata_control_flow(|ctx| {
            emit_statement_lowered_nonvariadic_static_spread_fault_to_target(
                args,
                span,
                target_py,
                ctx,
                indent + 8,
                out,
            )
        })?;
        writeln!(out, "{pad}    else:").expect("write to string");
        ctx.with_metadata_control_flow(|ctx| {
            emit_statement_lowered_nonvariadic_static_spread_fault_to_target(
                args,
                span,
                target_py,
                ctx,
                indent + 8,
                out,
            )
        })?;
        return Ok(());
    }
    ctx.with_metadata_control_flow(|ctx| {
        let some_slots = bind_statement_lowered_static_call_args(
            args,
            params,
            callback_arities,
            span,
            ctx,
            indent + 8,
            out,
        )?;
        let some_body = render_body(&format!("{receiver_tmp}.value"), &some_slots);
        emit_statement_lowered_optional_wrapped_assignment(
            target_py,
            wrap,
            &some_body,
            ctx,
            indent + 8,
            out,
        )
    })?;
    writeln!(out, "{pad}    else:").expect("write to string");
    ctx.with_metadata_control_flow(|ctx| {
        let direct_slots = bind_statement_lowered_static_call_args(
            args,
            params,
            callback_arities,
            span,
            ctx,
            indent + 8,
            out,
        )?;
        let direct_body = render_body(&receiver_tmp, &direct_slots);
        writeln!(out, "{pad}        {target_py} = {direct_body}").expect("write to string");
        Ok(())
    })
}

pub(super) fn emit_statement_lowered_static_call_to_target(
    args: &[CallArg],
    call: StaticCallSpec<'_>,
    target: StatementTarget<'_, '_, '_, '_>,
    render_body: impl FnOnce(&[String]) -> String,
) -> Result<(), PyEmitError> {
    emit_statement_lowered_static_call_to_target_with_bound(args, call, target, |bound| {
        render_body(&bound.slots)
    })
}

pub(super) fn emit_statement_lowered_static_call_to_target_with_bound(
    args: &[CallArg],
    call: StaticCallSpec<'_>,
    target: StatementTarget<'_, '_, '_, '_>,
    render_body: impl FnOnce(&BoundStaticArgs) -> String,
) -> Result<(), PyEmitError> {
    let StaticCallSpec {
        params,
        callback_arities,
        span,
    } = call;
    let StatementTarget {
        target_py,
        ctx,
        indent,
        out,
    } = target;
    if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        return emit_statement_lowered_nonvariadic_static_spread_fault_to_target(
            args, span, target_py, ctx, indent, out,
        );
    }
    let pad = " ".repeat(indent);
    let bound = bind_statement_lowered_static_call_args(
        args,
        params,
        callback_arities,
        span,
        ctx,
        indent,
        out,
    )?;
    writeln!(out, "{pad}{target_py} = {}", render_body(&bound)).expect("write to string");
    Ok(())
}

pub(super) fn emit_statement_lowered_nonvariadic_static_spread_fault_to_target(
    args: &[CallArg],
    span: Span,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    if positional_after_named_py(args) {
        return emit_statement_lowered_call_order_fault_to_target(
            args, span, target_py, ctx, indent, out,
        );
    }
    let (prefix, tail, named) =
        bind_statement_lowered_spread_fault_parts(args, span, ctx, indent, out)?;
    writeln!(
        out,
        "{}{target_py} = tpz_nonvariadic_static_spread_call([{}], [{}], [{}], {})",
        " ".repeat(indent),
        prefix.join(", "),
        tail.join(", "),
        named.join(", "),
        py_span(span)
    )
    .expect("write to string");
    Ok(())
}

pub(super) fn bind_statement_lowered_static_call_args(
    args: &[CallArg],
    params: &[&str],
    callback_arities: &[(usize, usize)],
    span: Span,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<BoundStaticArgs, PyEmitError> {
    let mut slots = vec![None; params.len()];
    let mut cooperative_callback_slots = vec![false; params.len()];
    let mut positional_index = 0usize;
    let mut saw_named = false;
    for arg in args {
        match arg {
            CallArg::Positional(_) if saw_named => {
                return Err(PyEmitError::unsupported("call argument shape").at(call_arg_span(arg)));
            }
            CallArg::Positional(expr) => {
                if positional_index >= params.len() || slots[positional_index].is_some() {
                    return Err(PyEmitError::unsupported("call argument shape").at(expr.span));
                }
                let emitted = emit_statement_lowered_static_arg_expr(
                    expr,
                    positional_index,
                    callback_arities,
                    ctx,
                    indent,
                    out,
                )?;
                cooperative_callback_slots[positional_index] = emitted.cooperative_callback;
                slots[positional_index] = Some(emitted.py);
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
                let emitted = emit_statement_lowered_static_arg_expr(
                    value,
                    param_index,
                    callback_arities,
                    ctx,
                    indent,
                    out,
                )?;
                cooperative_callback_slots[param_index] = emitted.cooperative_callback;
                slots[param_index] = Some(emitted.py);
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
        ordered: Vec::new(),
        cooperative_callback_slots,
        spread_fault: None,
    })
}

pub(super) fn emit_statement_lowered_static_arg_expr(
    expr: &Expr,
    param_index: usize,
    callback_arities: &[(usize, usize)],
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<StaticArgValue, PyEmitError> {
    if let Some((_, arity)) = callback_arities
        .iter()
        .find(|(callback_index, _)| *callback_index == param_index)
    {
        emit_statement_lowered_callback_expr(expr, *arity, ctx, indent, out)
    } else {
        emit_statement_lowered_expr_value(expr, ctx, indent, out).map(StaticArgValue::plain)
    }
}

pub(super) fn emit_statement_lowered_callback_expr(
    expr: &Expr,
    arity: usize,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<StaticArgValue, PyEmitError> {
    if expr_needs_statement_lowering(expr, ctx) {
        emit_statement_lowered_expr_value(expr, ctx, indent, out).map(StaticArgValue::plain)
    } else {
        emit_callback_expr(expr, arity, ctx)
    }
}

pub(super) fn emit_statement_lowered_known_function_call_to_target(
    info: &FunctionInfo,
    args: &[CallArg],
    span: Span,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        return emit_statement_lowered_nonvariadic_known_function_spread_fault_to_target(
            info, args, span, target_py, ctx, indent, out,
        );
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
            CallArg::Positional(expr) if saw_named => {
                return Err(PyEmitError::unsupported("call argument shape").at(expr.span));
            }
            CallArg::Positional(expr) => {
                if positional_index >= info.params.len() || filled[positional_index] {
                    return Err(PyEmitError::unsupported("call argument shape").at(expr.span));
                }
                filled[positional_index] = true;
                positional_index += 1;
                call_args.push(emit_statement_lowered_expr_value(expr, ctx, indent, out)?);
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
                let value_py = emit_statement_lowered_expr_value(value, ctx, indent, out)?;
                call_args.push(format!("{}={value_py}", info.params[param_index].py_name));
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
    write_known_function_call_to_target(out, indent, target_py, info, &call_args, ctx);
    Ok(())
}

pub(super) fn emit_statement_lowered_static_callable_value_call_to_target(
    callee: &Expr,
    args: &[CallArg],
    params: &[FunctionParamInfo],
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
        return emit_statement_lowered_variadic_static_callable_value_call_to_target(
            callee,
            args,
            params,
            span,
            StatementTarget::new(target_py, ctx, indent, out),
        );
    }
    if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        return emit_statement_lowered_nonvariadic_static_spread_fault_to_target(
            args, span, target_py, ctx, indent, out,
        );
    }
    let pad = " ".repeat(indent);
    let callee_py = bind_statement_lowered_expr_value(callee, "call_callee", ctx, indent, out)?;
    let mut positional = Vec::new();
    let mut kwargs = Vec::new();
    let mut filled = vec![false; params.len()];
    let mut positional_index = 0usize;
    let mut saw_named = false;
    for arg in args {
        match arg {
            CallArg::Positional(_) if saw_named => {
                return emit_statement_lowered_call_order_fault_to_target(
                    args, span, target_py, ctx, indent, out,
                );
            }
            CallArg::Positional(expr) => {
                if positional_index >= params.len() || filled[positional_index] {
                    return Err(PyEmitError::unsupported("call argument shape").at(expr.span));
                }
                filled[positional_index] = true;
                positional_index += 1;
                positional.push(emit_statement_lowered_expr_value(expr, ctx, indent, out)?);
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
                    emit_statement_lowered_expr_value(value, ctx, indent, out)?
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

pub(super) fn emit_statement_lowered_variadic_static_callable_value_call_to_target(
    callee: &Expr,
    args: &[CallArg],
    params: &[FunctionParamInfo],
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
        .expect("variadic static callable call has at least one parameter");
    let callee_py = bind_statement_lowered_expr_value(callee, "call_callee", ctx, indent, out)?;
    let mut fixed_slots = vec![None; fixed_count];
    let mut tail = Vec::new();
    let mut positional_index = 0usize;
    let mut saw_named = false;
    let mut saw_spread = false;
    let mut skipped_required_by_spread = false;
    let mut evaluated_args = Vec::new();

    for arg in args {
        match arg {
            CallArg::Positional(expr) if saw_named => {
                let value = bind_statement_lowered_call_arg_expr(expr, ctx, indent, out)?;
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
                let value = bind_statement_lowered_call_arg_expr(expr, ctx, indent, out)?;
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
                let value = bind_statement_lowered_call_arg_expr(expr, ctx, indent, out)?;
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
                let value = bind_statement_lowered_call_arg_expr(value, ctx, indent, out)?;
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

pub(super) fn emit_statement_lowered_nonvariadic_known_function_spread_fault_to_target(
    info: &FunctionInfo,
    args: &[CallArg],
    span: Span,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    if positional_after_named_py(args) {
        return emit_statement_lowered_call_order_fault_to_target(
            args, span, target_py, ctx, indent, out,
        );
    }
    let (prefix, tail, named) =
        bind_statement_lowered_spread_fault_parts(args, span, ctx, indent, out)?;
    writeln!(
        out,
        "{}{target_py} = tpz_nonvariadic_spread_call([{}], [{}], [{}], {}, {})",
        " ".repeat(indent),
        prefix.join(", "),
        tail.join(", "),
        named.join(", "),
        info.params.len(),
        py_span(span)
    )
    .expect("write to string");
    Ok(())
}

pub(super) fn emit_statement_lowered_variadic_known_function_call_to_target(
    info: &FunctionInfo,
    args: &[CallArg],
    span: Span,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let fixed_count = info.params.len().saturating_sub(1);
    let variadic_param = info
        .params
        .last()
        .expect("variadic call has at least one parameter");
    let mut fixed_slots = vec![None; fixed_count];
    let mut tail = Vec::new();
    let mut positional_index = 0usize;
    let mut saw_named = false;
    let mut saw_spread = false;
    let mut skipped_required_by_spread = false;
    let mut evaluated_args = Vec::new();

    for arg in args {
        match arg {
            CallArg::Positional(expr) if saw_named => {
                let value = bind_statement_lowered_call_arg_expr(expr, ctx, indent, out)?;
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
                let value = bind_statement_lowered_call_arg_expr(expr, ctx, indent, out)?;
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
                let value = bind_statement_lowered_call_arg_expr(expr, ctx, indent, out)?;
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
                let value = bind_statement_lowered_call_arg_expr(value, ctx, indent, out)?;
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

pub(super) fn bind_statement_lowered_call_arg_expr(
    expr: &Expr,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<String, PyEmitError> {
    let value_py = emit_statement_lowered_expr_value(expr, ctx, indent, out)?;
    let tmp = ctx.fresh_temp("call_arg");
    writeln!(out, "{}{tmp} = {value_py}", " ".repeat(indent)).expect("write to string");
    Ok(tmp)
}

pub(super) fn bind_statement_lowered_pipe_call_arg_expr(
    expr: &Expr,
    piped: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<String, PyEmitError> {
    let value_py = emit_statement_lowered_pipe_arg_expr(expr, piped, ctx, indent, out)?;
    let tmp = ctx.fresh_temp("call_arg");
    writeln!(out, "{}{tmp} = {value_py}", " ".repeat(indent)).expect("write to string");
    Ok(tmp)
}

pub(super) fn emit_statement_lowered_call_order_fault_to_target(
    args: &[CallArg],
    span: Span,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let mut saw_named = false;
    let mut evaluated = Vec::new();
    for arg in args {
        match arg {
            CallArg::Named { value, .. } => {
                saw_named = true;
                evaluated.push(bind_statement_lowered_call_arg_expr(
                    value, ctx, indent, out,
                )?);
            }
            CallArg::Positional(expr) if saw_named => {
                evaluated.push(bind_statement_lowered_call_arg_expr(
                    expr, ctx, indent, out,
                )?);
                writeln!(
                    out,
                    "{pad}{target_py} = tpz_call_order_fault([{}], {}, {})",
                    evaluated.join(", "),
                    py_string("positional arguments may not follow named arguments (§5)"),
                    py_span(span)
                )
                .expect("write to string");
                return Ok(());
            }
            CallArg::Positional(expr) => {
                evaluated.push(bind_statement_lowered_call_arg_expr(
                    expr, ctx, indent, out,
                )?);
            }
            CallArg::Spread(expr) if saw_named => {
                evaluated.push(bind_statement_lowered_call_arg_expr(
                    expr, ctx, indent, out,
                )?);
                writeln!(
                    out,
                    "{pad}{target_py} = tpz_call_order_fault([{}], {}, {})",
                    evaluated.join(", "),
                    py_string("named arguments must follow spread arguments (§5)"),
                    py_span(span)
                )
                .expect("write to string");
                return Ok(());
            }
            CallArg::Spread(expr) => {
                let value = bind_statement_lowered_call_arg_expr(expr, ctx, indent, out)?;
                let spread_value = ctx.fresh_temp("call_spread");
                writeln!(
                    out,
                    "{pad}{spread_value} = tpz_spread_values({value}, {})",
                    py_span(expr.span)
                )
                .expect("write to string");
                evaluated.push(format!("*{spread_value}"));
            }
        }
    }
    Err(PyEmitError::unsupported("call argument shape").at(span))
}

pub(super) fn emit_statement_lowered_pipe_call_order_fault_to_target(
    args: &[CallArg],
    piped: &str,
    span: Span,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let mut saw_named = false;
    let mut evaluated = Vec::new();
    for arg in args {
        match arg {
            CallArg::Named { value, .. } => {
                saw_named = true;
                evaluated.push(bind_statement_lowered_pipe_call_arg_expr(
                    value, piped, ctx, indent, out,
                )?);
            }
            CallArg::Positional(expr) if saw_named => {
                evaluated.push(bind_statement_lowered_pipe_call_arg_expr(
                    expr, piped, ctx, indent, out,
                )?);
                writeln!(
                    out,
                    "{pad}{target_py} = tpz_call_order_fault([{}], {}, {})",
                    evaluated.join(", "),
                    py_string("positional arguments may not follow named arguments (§5)"),
                    py_span(span)
                )
                .expect("write to string");
                return Ok(());
            }
            CallArg::Positional(expr) => {
                evaluated.push(bind_statement_lowered_pipe_call_arg_expr(
                    expr, piped, ctx, indent, out,
                )?);
            }
            CallArg::Spread(expr) if saw_named => {
                let value =
                    bind_statement_lowered_pipe_call_arg_expr(expr, piped, ctx, indent, out)?;
                writeln!(
                    out,
                    "{pad}{target_py} = tpz_call_order_fault([{}], {}, {})",
                    evaluated
                        .into_iter()
                        .chain([value])
                        .collect::<Vec<_>>()
                        .join(", "),
                    py_string("named arguments must follow spread arguments (§5)"),
                    py_span(span)
                )
                .expect("write to string");
                return Ok(());
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
                evaluated.push(format!("*{spread_value}"));
            }
        }
    }
    Err(PyEmitError::unsupported("call argument shape").at(span))
}
