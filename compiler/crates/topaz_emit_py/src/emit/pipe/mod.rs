//! Pipeline emission for member stages, callbacks, static calls, and spreads.
//! Placeholder binding is resolved within the stage; the surrounding expression
//! emitter receives one Python expression with the piped value evaluated once.

use crate::*;

pub(super) fn emit_pipe(
    lhs: &Expr,
    rhs: &PipeRhs,
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let lhs_py = emit_expr(lhs, ctx)?;
    let piped = "__tpz_piped";
    let body = match rhs {
        PipeRhs::Field(field) => {
            let member = ctx.text(field.span);
            format!(
                "tpz_member({piped}, {}, {}, {})",
                py_string(&mangle(member)),
                py_string(member),
                py_span(span)
            )
        }
        PipeRhs::Expr(stage) => emit_pipe_stage(lhs, stage, piped, ctx)?,
    };
    Ok(format!("(lambda {piped}: {body})({lhs_py})"))
}

pub(super) fn emit_statement_lowered_pipe_to_target(
    lhs: &Expr,
    rhs: &PipeRhs,
    span: Span,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let lhs_py = emit_statement_lowered_expr_value(lhs, ctx, indent, out)?;
    let piped = ctx.fresh_temp("pipe_value");
    writeln!(out, "{pad}{piped} = {lhs_py}").expect("write to string");
    match rhs {
        PipeRhs::Field(field) => {
            let member = ctx.text(field.span);
            writeln!(
                out,
                "{pad}{target_py} = tpz_member({piped}, {}, {}, {})",
                py_string(&mangle(member)),
                py_string(member),
                py_span(span)
            )
            .expect("write to string");
            Ok(())
        }
        PipeRhs::Expr(stage) => emit_statement_lowered_pipe_stage_to_target(
            lhs, stage, &piped, target_py, ctx, indent, out,
        ),
    }
}

pub(super) fn emit_statement_lowered_pipe_stage_to_target(
    lhs: &Expr,
    stage: &Expr,
    piped: &str,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
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
                let bound = bind_statement_lowered_pipe_static_call_args(
                    args,
                    PipeStaticCall::new(lhs, params, &[], piped, stage.span),
                    ctx,
                    indent,
                    out,
                )?;
                let rendered = render_bound_static_call(&bound, |slots| {
                    render_typed_json_runtime_call(method, &slots[0], &schema, stage.span)
                });
                writeln!(out, "{}{target_py} = {}", " ".repeat(indent), rendered)
                    .expect("write to string");
                return Ok(());
            }
            if let ExprKind::Member { object, field } = &callee.kind {
                let method = ctx.text(field.span);
                if let ExprKind::Ident = &object.kind {
                    let namespace = ctx.text(object.span);
                    if !ctx.binding_is_bound(namespace)
                        && emit_statement_lowered_pipe_namespace_builtin_call_to_target(
                            lhs,
                            namespace,
                            method,
                            args,
                            piped,
                            stage.span,
                            StatementTarget::new(target_py, ctx, indent, out),
                        )?
                    {
                        return Ok(());
                    }
                }
                if emit_statement_lowered_pipe_receiver_callback_builtin_call_to_target(
                    lhs,
                    object,
                    method,
                    args,
                    piped,
                    stage.span,
                    StatementTarget::new(target_py, ctx, indent, out),
                )? {
                    return Ok(());
                }
            }
            if let ExprKind::OptionalAccess { object, field } = &callee.kind {
                let method = ctx.text(field.span);
                if emit_statement_lowered_pipe_optional_receiver_callback_builtin_call_to_target(
                    lhs,
                    object,
                    method,
                    args,
                    piped,
                    stage.span,
                    StatementTarget::new(target_py, ctx, indent, out),
                )? {
                    return Ok(());
                }
            }
            if let Some(params) = pipe_static_callable_value_params(callee, ctx) {
                return emit_statement_lowered_pipe_static_callable_value_call_to_target(
                    callee,
                    args,
                    &params,
                    piped,
                    stage.span,
                    StatementTarget::new(target_py, ctx, indent, out),
                );
            }
            if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
                return emit_statement_lowered_pipe_spread_call_to_target(
                    callee,
                    args,
                    piped,
                    stage.span,
                    StatementTarget::new(target_py, ctx, indent, out),
                );
            }
            if args.iter().any(|arg| matches!(arg, CallArg::Named { .. })) {
                return emit_statement_lowered_pipe_named_call_to_target(
                    callee,
                    args,
                    piped,
                    stage.span,
                    StatementTarget::new(target_py, ctx, indent, out),
                );
            }
            emit_statement_lowered_pipe_positional_call_to_target(
                callee,
                args,
                piped,
                stage.span,
                StatementTarget::new(target_py, ctx, indent, out),
            )
        }
        ExprKind::Paren(inner) => emit_statement_lowered_pipe_stage_to_target(
            lhs, inner, piped, target_py, ctx, indent, out,
        ),
        _ => emit_statement_lowered_pipe_callable_call_to_target(
            stage,
            vec![piped.to_string()],
            stage.span,
            target_py,
            ctx,
            indent,
            out,
        ),
    }
}

pub(super) fn emit_expr_with_pipe_placeholder(
    expr: &Expr,
    piped: &str,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    ctx.push_pipe_placeholder(piped);
    let result = emit_expr(expr, ctx);
    ctx.pop_pipe_placeholder();
    result
}

pub(super) fn emit_statement_lowered_expr_value_with_pipe_placeholder(
    expr: &Expr,
    piped: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<String, PyEmitError> {
    ctx.push_pipe_placeholder(piped);
    let result = emit_statement_lowered_expr_value(expr, ctx, indent, out);
    ctx.pop_pipe_placeholder();
    result
}

pub(super) fn emit_statement_lowered_pipe_namespace_builtin_call_to_target(
    lhs: &Expr,
    namespace: &str,
    method: &str,
    args: &[CallArg],
    piped: &str,
    span: Span,
    target: StatementTarget<'_, '_, '_, '_>,
) -> Result<bool, PyEmitError> {
    let StatementTarget {
        target_py,
        ctx,
        indent,
        out,
    } = target;
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
    let pad = " ".repeat(indent);
    let bound = match (namespace, method) {
        ("Map", "ofEntries") if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) => {
            BoundStaticArgs {
                slots: Vec::new(),
                ordered: Vec::new(),
                cooperative_callback_slots: Vec::new(),
                spread_fault: Some(bind_statement_lowered_pipe_static_spread_fault_expr(
                    args,
                    PipeStaticCall::new(lhs, &[], &[], piped, span),
                    ctx,
                    indent,
                    out,
                )?),
            }
        }
        ("Map", "ofEntries") => bind_statement_lowered_pipe_static_call_args(
            args,
            PipeStaticCall::new(lhs, &["entries"], &[], piped, span),
            ctx,
            indent,
            out,
        )?,
        _ => return Ok(false),
    };
    let call = render_bound_static_call(&bound, |slots| {
        format!("tpz_map_of_entries({}, {})", slots[0], py_span(span))
    });
    writeln!(out, "{pad}{target_py} = {call}").expect("write to string");
    Ok(true)
}

pub(super) fn emit_statement_lowered_pipe_receiver_callback_builtin_call_to_target(
    lhs: &Expr,
    object: &Expr,
    method: &str,
    args: &[CallArg],
    piped: &str,
    span: Span,
    target: StatementTarget<'_, '_, '_, '_>,
) -> Result<bool, PyEmitError> {
    let StatementTarget {
        target_py,
        ctx,
        indent,
        out,
    } = target;
    if receiver_is_array_value(object, ctx)
        && let Some(spec) = array_receiver_builtin(method)
    {
        return emit_statement_lowered_pipe_receiver_static_call_to_target(
            object,
            args,
            PipeStaticCall::new(lhs, spec.params, &[], piped, span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, slots| spec.render(recv, slots, span),
        );
    }
    match method {
        "map" => {
            if receiver_is_array_value(object, ctx) {
                let cooperative = ctx.cooperative_yields;
                return emit_statement_lowered_pipe_receiver_static_call_to_target_with_bound(
                    object,
                    args,
                    PipeStaticCall::new(lhs, &["f"], &[(0, 1)], piped, span),
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
                );
            }
            if receiver_is_option_value(object, ctx) {
                let cooperative = ctx.cooperative_yields;
                return emit_statement_lowered_pipe_receiver_static_call_to_target_with_bound(
                    object,
                    args,
                    PipeStaticCall::new(lhs, &["f"], &[(0, 1)], piped, span),
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
                );
            }
            if receiver_is_result_value(object, ctx) {
                let cooperative = ctx.cooperative_yields;
                return emit_statement_lowered_pipe_receiver_static_call_to_target_with_bound(
                    object,
                    args,
                    PipeStaticCall::new(lhs, &["f"], &[(0, 1)], piped, span),
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
                );
            }
            Ok(false)
        }
        "filter" => {
            if receiver_is_map_value(object, ctx) {
                let cooperative = ctx.cooperative_yields;
                return emit_statement_lowered_pipe_receiver_static_call_to_target_with_bound(
                    object,
                    args,
                    PipeStaticCall::new(lhs, &["f"], &[(0, 2)], piped, span),
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
                );
            }
            if receiver_is_array_value(object, ctx) {
                let cooperative = ctx.cooperative_yields;
                return emit_statement_lowered_pipe_receiver_static_call_to_target_with_bound(
                    object,
                    args,
                    PipeStaticCall::new(lhs, &["f"], &[(0, 1)], piped, span),
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
                );
            }
            Ok(false)
        }
        "reduce" if receiver_is_array_value(object, ctx) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_pipe_receiver_static_call_to_target_with_bound(
                object,
                args,
                PipeStaticCall::new(lhs, &["initial", "f"], &[(1, 2)], piped, span),
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
            )
        }
        "sorted" if receiver_is_array_value(object, ctx) => {
            emit_statement_lowered_pipe_receiver_static_call_to_target(
                object,
                args,
                PipeStaticCall::new(lhs, &[], &[], piped, span),
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_array_sorted({recv}, {})", py_span(span)),
            )
        }
        "sort" if receiver_is_array_value(object, ctx) => {
            emit_statement_lowered_pipe_receiver_static_call_to_target(
                object,
                args,
                PipeStaticCall::new(lhs, &[], &[], piped, span),
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_array_sort({recv}, {})", py_span(span)),
            )
        }
        "sortedBy" if receiver_is_array_value(object, ctx) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_pipe_receiver_static_call_to_target_with_bound(
                object,
                args,
                PipeStaticCall::new(lhs, &["f"], &[(0, 1)], piped, span),
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
            )
        }
        "flatMap" => {
            if receiver_is_option_value(object, ctx) {
                let cooperative = ctx.cooperative_yields;
                return emit_statement_lowered_pipe_receiver_static_call_to_target_with_bound(
                    object,
                    args,
                    PipeStaticCall::new(lhs, &["f"], &[(0, 1)], piped, span),
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
                );
            }
            if receiver_is_result_value(object, ctx) {
                let cooperative = ctx.cooperative_yields;
                return emit_statement_lowered_pipe_receiver_static_call_to_target_with_bound(
                    object,
                    args,
                    PipeStaticCall::new(lhs, &["f"], &[(0, 1)], piped, span),
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
                );
            }
            Ok(false)
        }
        "okOrElse" if receiver_is_option_value(object, ctx) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_pipe_receiver_static_call_to_target_with_bound(
                object,
                args,
                PipeStaticCall::new(lhs, &["f"], &[(0, 0)], piped, span),
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
            )
        }
        "okOr" if receiver_is_option_value(object, ctx) => {
            emit_statement_lowered_pipe_receiver_static_call_to_target(
                object,
                args,
                PipeStaticCall::new(lhs, &["error"], &[], piped, span),
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| format!("tpz_option_ok_or({recv}, {}, {})", slots[0], py_span(span)),
            )
        }
        "mapValues" if receiver_is_map_value(object, ctx) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_pipe_receiver_static_call_to_target_with_bound(
                object,
                args,
                PipeStaticCall::new(lhs, &["f"], &[(0, 1)], piped, span),
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
            )
        }
        "update" if receiver_is_map_value(object, ctx) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_pipe_receiver_static_call_to_target_with_bound(
                object,
                args,
                PipeStaticCall::new(lhs, &["k", "initial", "f"], &[(2, 1)], piped, span),
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
            )
        }
        _ => Ok(false),
    }
}

pub(super) fn emit_statement_lowered_pipe_optional_receiver_callback_builtin_call_to_target(
    lhs: &Expr,
    object: &Expr,
    method: &str,
    args: &[CallArg],
    piped: &str,
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
    if shape == ReceiverShape::Array
        && let Some(spec) = array_receiver_builtin(method)
    {
        emit_statement_lowered_pipe_optional_receiver_static_call_to_target(
            object,
            args,
            PipeStaticCall::new(lhs, spec.params, &[], piped, span),
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
        ("map", ReceiverShape::Array) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_pipe_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                PipeStaticCall::new(lhs, &["f"], &[(0, 1)], piped, span),
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
            emit_statement_lowered_pipe_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                PipeStaticCall::new(lhs, &["f"], &[(0, 1)], piped, span),
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
            emit_statement_lowered_pipe_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                PipeStaticCall::new(lhs, &["f"], &[(0, 1)], piped, span),
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
            emit_statement_lowered_pipe_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                PipeStaticCall::new(lhs, &["f"], &[(0, 1)], piped, span),
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
            emit_statement_lowered_pipe_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                PipeStaticCall::new(lhs, &["f"], &[(0, 2)], piped, span),
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
            emit_statement_lowered_pipe_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                PipeStaticCall::new(lhs, &["initial", "f"], &[(1, 2)], piped, span),
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
            emit_statement_lowered_pipe_optional_receiver_static_call_to_target(
                object,
                args,
                PipeStaticCall::new(lhs, &[], &[], piped, span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_array_sorted({recv}, {})", py_span(span)),
            )?
        }
        ("sort", ReceiverShape::Array) => {
            emit_statement_lowered_pipe_optional_receiver_static_call_to_target(
                object,
                args,
                PipeStaticCall::new(lhs, &[], &[], piped, span),
                StatementLoweredOptionalWrap::Unit,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_array_sort({recv}, {})", py_span(span)),
            )?
        }
        ("sortedBy", ReceiverShape::Array) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_pipe_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                PipeStaticCall::new(lhs, &["f"], &[(0, 1)], piped, span),
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
            emit_statement_lowered_pipe_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                PipeStaticCall::new(lhs, &["f"], &[(0, 1)], piped, span),
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
            emit_statement_lowered_pipe_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                PipeStaticCall::new(lhs, &["f"], &[(0, 1)], piped, span),
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
            emit_statement_lowered_pipe_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                PipeStaticCall::new(lhs, &["f"], &[(0, 1)], piped, span),
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
            emit_statement_lowered_pipe_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                PipeStaticCall::new(lhs, &["f"], &[(0, 1)], piped, span),
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
            emit_statement_lowered_pipe_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                PipeStaticCall::new(lhs, &["f"], &[(0, 0)], piped, span),
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
            emit_statement_lowered_pipe_optional_receiver_static_call_to_target(
                object,
                args,
                PipeStaticCall::new(lhs, &["error"], &[], piped, span),
                StatementLoweredOptionalWrap::Value,
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| format!("tpz_option_ok_or({recv}, {}, {})", slots[0], py_span(span)),
            )?
        }
        ("mapValues", ReceiverShape::Map) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_pipe_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                PipeStaticCall::new(lhs, &["f"], &[(0, 1)], piped, span),
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
            emit_statement_lowered_pipe_optional_receiver_static_call_to_target_with_bound(
                object,
                args,
                PipeStaticCall::new(lhs, &["k", "initial", "f"], &[(2, 1)], piped, span),
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
