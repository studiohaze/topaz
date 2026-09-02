//! Python call emission after checker-owned target and argument planning.
//! Free, namespace, receiver, optional, and statement-lowered routes converge
//! here on the same runtime calling conventions.

use crate::*;

pub(super) fn emit_statement_lowered_call_to_target(
    callee: &Expr,
    args: &[CallArg],
    span: Span,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let info = match &callee.kind {
        ExprKind::Ident => {
            let name = ctx.text(callee.span);
            if let Some(info) = ctx.function_info(name)
                && !ctx.binding_is_bound(name)
            {
                info.clone()
            } else if ctx.binding_is_bound(name) {
                ctx.binding_callable_info_at(name, callee.span)
                    .ok_or_else(|| PyEmitError::unsupported("call target").at(callee.span))?
            } else {
                if let Some(newtype) = ctx.newtypes.get(name).cloned() {
                    return emit_statement_lowered_newtype_construct_to_target(
                        &newtype, args, span, target_py, ctx, indent, out,
                    );
                }
                if emit_statement_lowered_free_builtin_call_to_target(
                    name, args, span, target_py, ctx, indent, out,
                )? {
                    return Ok(());
                }
                return Err(PyEmitError::unsupported("call target").at(callee.span));
            }
        }
        ExprKind::Member { object, field } => {
            let method = ctx.text(field.span);
            if let ExprKind::Ident = &object.kind {
                let namespace = ctx.text(object.span);
                if !ctx.binding_is_bound(namespace)
                    && let Some(enum_def) = ctx.enums.get(namespace).cloned()
                    && let Some(variant) = enum_def.variants.get(method).cloned()
                {
                    return emit_statement_lowered_enum_construct_to_target(
                        &enum_def,
                        method,
                        &variant,
                        args,
                        span,
                        StatementTarget::new(target_py, ctx, indent, out),
                    );
                }
                if let Some(export) = ctx.namespace_export(namespace, method) {
                    match export {
                        ModuleRuntimeExport::Function { info } => info.clone(),
                        ModuleRuntimeExport::Value { metadata, .. } => {
                            if let Some(params) = metadata.callable_params.clone() {
                                return emit_statement_lowered_static_callable_value_call_to_target(
                                    callee,
                                    args,
                                    &params,
                                    span,
                                    StatementTarget::new(target_py, ctx, indent, out),
                                );
                            }
                            if emit_statement_lowered_namespace_builtin_call_to_target(
                                namespace,
                                method,
                                args,
                                span,
                                StatementTarget::new(target_py, ctx, indent, out),
                            )? {
                                return Ok(());
                            }
                            if method == "value" {
                                return emit_statement_lowered_newtype_unwrap_to_target(
                                    object, args, span, target_py, ctx, indent, out,
                                );
                            }
                            if emit_statement_lowered_receiver_builtin_call_to_target(
                                object,
                                method,
                                args,
                                span,
                                StatementTarget::new(target_py, ctx, indent, out),
                            )? {
                                return Ok(());
                            }
                            return Err(PyEmitError::unsupported("call target").at(callee.span));
                        }
                        _ => {
                            if emit_statement_lowered_namespace_builtin_call_to_target(
                                namespace,
                                method,
                                args,
                                span,
                                StatementTarget::new(target_py, ctx, indent, out),
                            )? {
                                return Ok(());
                            }
                            if method == "value" {
                                return emit_statement_lowered_newtype_unwrap_to_target(
                                    object, args, span, target_py, ctx, indent, out,
                                );
                            }
                            if emit_statement_lowered_receiver_builtin_call_to_target(
                                object,
                                method,
                                args,
                                span,
                                StatementTarget::new(target_py, ctx, indent, out),
                            )? {
                                return Ok(());
                            }
                            return Err(PyEmitError::unsupported("call target").at(callee.span));
                        }
                    }
                } else {
                    if emit_statement_lowered_namespace_builtin_call_to_target(
                        namespace,
                        method,
                        args,
                        span,
                        StatementTarget::new(target_py, ctx, indent, out),
                    )? {
                        return Ok(());
                    }
                    if method == "value" {
                        return emit_statement_lowered_newtype_unwrap_to_target(
                            object, args, span, target_py, ctx, indent, out,
                        );
                    }
                    if emit_statement_lowered_receiver_builtin_call_to_target(
                        object,
                        method,
                        args,
                        span,
                        StatementTarget::new(target_py, ctx, indent, out),
                    )? {
                        return Ok(());
                    }
                    if let Some(params) = ctx
                        .record_member_field_projection(object, field)
                        .callable_params
                    {
                        return emit_statement_lowered_static_callable_value_call_to_target(
                            callee,
                            args,
                            &params,
                            span,
                            StatementTarget::new(target_py, ctx, indent, out),
                        );
                    }
                    return Err(PyEmitError::unsupported("call target").at(callee.span));
                }
            } else {
                if method == "value" {
                    return emit_statement_lowered_newtype_unwrap_to_target(
                        object, args, span, target_py, ctx, indent, out,
                    );
                }
                if emit_statement_lowered_receiver_builtin_call_to_target(
                    object,
                    method,
                    args,
                    span,
                    StatementTarget::new(target_py, ctx, indent, out),
                )? {
                    return Ok(());
                }
                if let Some(params) = ctx
                    .record_member_field_projection(object, field)
                    .callable_params
                {
                    return emit_statement_lowered_static_callable_value_call_to_target(
                        callee,
                        args,
                        &params,
                        span,
                        StatementTarget::new(target_py, ctx, indent, out),
                    );
                }
                return Err(PyEmitError::unsupported("call target").at(callee.span));
            }
        }
        ExprKind::OptionalAccess { object, field } => {
            let method = ctx.text(field.span);
            if emit_statement_lowered_optional_receiver_builtin_call_to_target(
                object,
                method,
                args,
                span,
                StatementTarget::new(target_py, ctx, indent, out),
            )? {
                return Ok(());
            }
            return Err(PyEmitError::unsupported("call target").at(callee.span));
        }
        ExprKind::Paren(inner) => {
            return emit_statement_lowered_call_to_target(
                inner, args, span, target_py, ctx, indent, out,
            );
        }
        ExprKind::Index { object, index } => {
            if let Some(params) = ctx.array_element_callable_params_for_index(object, index) {
                return emit_statement_lowered_static_callable_value_call_to_target(
                    callee,
                    args,
                    &params,
                    span,
                    StatementTarget::new(target_py, ctx, indent, out),
                );
            }
            return emit_statement_lowered_positional_callable_value_call_to_target(
                callee, args, span, target_py, ctx, indent, out,
            );
        }
        ExprKind::Compose { .. } => {
            return emit_statement_lowered_positional_callable_value_call_to_target(
                callee, args, span, target_py, ctx, indent, out,
            );
        }
        _ => return Err(PyEmitError::unsupported("call target").at(callee.span)),
    };
    if info.params.last().is_some_and(|param| param.variadic) {
        emit_statement_lowered_variadic_known_function_call_to_target(
            &info, args, span, target_py, ctx, indent, out,
        )
    } else {
        emit_statement_lowered_known_function_call_to_target(
            &info, args, span, target_py, ctx, indent, out,
        )
    }
}

pub(super) fn emit_statement_lowered_immediate_lambda_call_to_target(
    callee: &Expr,
    args: &[CallArg],
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let callee_py = bind_statement_lowered_expr_value(callee, "lambda_callee", ctx, indent, out)?;
    let positional = positional_args(args)?;
    let mut emitted_args = Vec::with_capacity(positional.len());
    for arg in positional {
        emitted_args.push(emit_statement_lowered_expr_value(arg, ctx, indent, out)?);
    }
    writeln!(
        out,
        "{}{target_py} = {callee_py}({})",
        " ".repeat(indent),
        emitted_args.join(", ")
    )
    .expect("write to string");
    Ok(())
}

pub(super) fn emit_statement_lowered_positional_callable_value_call_to_target(
    callee: &Expr,
    args: &[CallArg],
    span: Span,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let callee_py = bind_statement_lowered_expr_value(callee, "call_callee", ctx, indent, out)?;
    let positional = positional_args(args)?;
    let mut emitted_args = Vec::with_capacity(positional.len());
    for arg in positional {
        emitted_args.push(emit_statement_lowered_expr_value(arg, ctx, indent, out)?);
    }
    writeln!(
        out,
        "{pad}{target_py} = tpz_call({callee_py}, {}, {{}}, {})",
        py_tuple(emitted_args),
        py_span(span)
    )
    .expect("write to string");
    Ok(())
}

pub(super) fn emit_statement_lowered_newtype_construct_to_target(
    newtype: &NewtypeDef,
    args: &[CallArg],
    span: Span,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let positional = positional_args(args)?;
    if positional.len() != 1 {
        return Err(PyEmitError::unsupported("call argument shape").at(span));
    }
    let value_py = emit_statement_lowered_expr_value(positional[0], ctx, indent, out)?;
    let value_py = render_newtype_construct(newtype, &value_py, span);
    writeln!(out, "{}{target_py} = {value_py}", " ".repeat(indent)).expect("write to string");
    Ok(())
}

pub(super) fn emit_statement_lowered_enum_construct_to_target(
    enum_def: &EnumDef,
    variant_name: &str,
    variant: &EnumVariantDef,
    args: &[CallArg],
    span: Span,
    target: StatementTarget<'_, '_, '_, '_>,
) -> Result<(), PyEmitError> {
    let StatementTarget {
        target_py,
        ctx,
        indent,
        out,
    } = target;
    let positional = positional_args(args)?;
    let values = positional
        .iter()
        .map(|arg| emit_statement_lowered_expr_value(arg, ctx, indent, out))
        .collect::<Result<Vec<_>, _>>()?;
    let value_py = render_enum_construct(enum_def, variant_name, variant, values, span);
    writeln!(out, "{}{target_py} = {value_py}", " ".repeat(indent)).expect("write to string");
    Ok(())
}

pub(super) fn emit_statement_lowered_newtype_unwrap_to_target(
    object: &Expr,
    args: &[CallArg],
    span: Span,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
    let recv = emit_statement_lowered_expr_value(object, ctx, indent, out)?;
    writeln!(
        out,
        "{}{target_py} = tpz_newtype_unwrap({recv}, {})",
        " ".repeat(indent),
        py_span(span)
    )
    .expect("write to string");
    Ok(())
}

pub(super) fn emit_statement_lowered_free_builtin_call_to_target(
    name: &str,
    args: &[CallArg],
    span: Span,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<bool, PyEmitError> {
    match name {
        "Some" => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["value"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("Some({})", slots[0]),
        )?,
        "Ok" => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["value"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("Ok({})", slots[0]),
        )?,
        "Err" => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["value"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("Err({})", slots[0]),
        )?,
        "input" => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |_| "host.input()".to_string(),
        )?,
        "print" => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["value"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("host.print({}, {})", slots[0], py_span(span)),
        )?,
        "toInt" => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["text"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_to_int({}, {})", slots[0], py_span(span)),
        )?,
        "fromCodePoint" => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["n"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_from_code_point({}, {})", slots[0], py_span(span)),
        )?,
        "map" => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_static_call_to_target_with_bound(
                args,
                StaticCallSpec::new(&["xs", "f"], &[(1, 1)], span),
                StatementTarget::new(target_py, ctx, indent, out),
                move |bound| {
                    render_array_map_call_with_callback(
                        &bound.slots[0],
                        &bound.slots[1],
                        span,
                        cooperative,
                        bound.slot_is_cooperative_callback(1),
                    )
                },
            )?
        }
        "filter" => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_static_call_to_target_with_bound(
                args,
                StaticCallSpec::new(&["xs", "f"], &[(1, 1)], span),
                StatementTarget::new(target_py, ctx, indent, out),
                move |bound| {
                    render_array_filter_call_with_callback(
                        &bound.slots[0],
                        &bound.slots[1],
                        span,
                        cooperative,
                        bound.slot_is_cooperative_callback(1),
                    )
                },
            )?
        }
        "reduce" => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_static_call_to_target_with_bound(
                args,
                StaticCallSpec::new(&["xs", "initial", "f"], &[(2, 2)], span),
                StatementTarget::new(target_py, ctx, indent, out),
                move |bound| {
                    render_array_reduce_call_with_callback(
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
        "open" => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["path"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("host.open_file({}, {})", slots[0], py_span(span)),
        )?,
        _ => return Ok(false),
    }
    Ok(true)
}

pub(super) fn emit_statement_lowered_namespace_builtin_call_to_target(
    namespace: &str,
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
        ("Map", "new") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |_| "tpz_map_new()".to_string(),
        )?,
        ("Map", "ofEntries") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["entries"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_map_of_entries({}, {})", slots[0], py_span(span)),
        )?,
        ("JSON", "parse") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["text"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_json_parse({}, {})", slots[0], py_span(span)),
        )?,
        ("JSON", "stringify") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["value"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_json_stringify({})", slots[0]),
        )?,
        ("Bytes", "empty") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |_| "tpz_bytes_empty()".to_string(),
        )?,
        ("Bytes", "encodeUtf8") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["s"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_bytes_encode_utf8({}, {})", slots[0], py_span(span)),
        )?,
        ("Bytes", "fromArray") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["values"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_bytes_from_array({}, {})", slots[0], py_span(span)),
        )?,
        ("Bytes", "fromHex") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["s"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_bytes_from_hex({}, {})", slots[0], py_span(span)),
        )?,
        ("Bytes", "fromBase64") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["s"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_bytes_from_base64({}, {})", slots[0], py_span(span)),
        )?,
        ("Bytes", "concat") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["a", "b"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| {
                format!(
                    "tpz_bytes_concat({}, {}, {})",
                    slots[0],
                    slots[1],
                    py_span(span)
                )
            },
        )?,
        ("Encoding", "utf8Encode") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["text"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_bytes_encode_utf8({}, {})", slots[0], py_span(span)),
        )?,
        ("Encoding", "utf8Decode") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["bytes"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_bytes_decode_utf8({}, {})", slots[0], py_span(span)),
        )?,
        ("Encoding", "hexEncode") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["bytes"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_bytes_to_hex({}, {})", slots[0], py_span(span)),
        )?,
        ("Encoding", "hexDecode") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["text"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_bytes_from_hex({}, {})", slots[0], py_span(span)),
        )?,
        ("Encoding", "base64Encode") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["bytes"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_bytes_to_base64({}, {})", slots[0], py_span(span)),
        )?,
        ("Encoding", "base64Decode") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["text"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_bytes_from_base64({}, {})", slots[0], py_span(span)),
        )?,
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
                _ => return Ok(false),
            };
            emit_statement_lowered_static_call_to_target(
                args,
                StaticCallSpec::new(params, &[], span),
                StatementTarget::new(target_py, ctx, indent, out),
                |slots| {
                    let mut rendered = slots.join(", ");
                    if !rendered.is_empty() {
                        rendered.push_str(", ");
                    }
                    format!("{leaf}({rendered}{})", py_span(span))
                },
            )?
        }
        ("FS", "readText") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["path"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_fs_read_text(host, {}, {})", slots[0], py_span(span)),
        )?,
        ("FS", "writeText") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["path", "text"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| {
                format!(
                    "tpz_fs_write_text(host, {}, {}, {})",
                    slots[0],
                    slots[1],
                    py_span(span)
                )
            },
        )?,
        ("FS", "readBytes") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["path"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_fs_read_bytes(host, {}, {})", slots[0], py_span(span)),
        )?,
        ("FS", "writeBytes") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["path", "bytes"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| {
                format!(
                    "tpz_fs_write_bytes(host, {}, {}, {})",
                    slots[0],
                    slots[1],
                    py_span(span)
                )
            },
        )?,
        ("FS", "list") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["path"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_fs_list(host, {}, {})", slots[0], py_span(span)),
        )?,
        ("Cli", "hasFlag") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["args", "name"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| {
                format!(
                    "tpz_cli_has_flag({}, {}, {})",
                    slots[0],
                    slots[1],
                    py_span(span)
                )
            },
        )?,
        ("Cli", "option") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["args", "name"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| {
                format!(
                    "tpz_cli_option({}, {}, {})",
                    slots[0],
                    slots[1],
                    py_span(span)
                )
            },
        )?,
        ("Hash", "sha256" | "sha512") => {
            let leaf = if method == "sha256" {
                "tpz_hash_sha256"
            } else {
                "tpz_hash_sha512"
            };
            emit_statement_lowered_static_call_to_target(
                args,
                StaticCallSpec::new(&["data"], &[], span),
                StatementTarget::new(target_py, ctx, indent, out),
                |slots| format!("{leaf}({}, {})", slots[0], py_span(span)),
            )?
        }
        ("Hash", "hmacSha256") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["key", "message"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| {
                format!(
                    "tpz_hash_hmac_sha256({}, {}, {})",
                    slots[0],
                    slots[1],
                    py_span(span)
                )
            },
        )?,
        ("Hash", "crc32") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["data"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_hash_crc32({}, {})", slots[0], py_span(span)),
        )?,
        ("Regex", "compile") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["pattern"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_regex_compile({}, {})", slots[0], py_span(span)),
        )?,
        ("CSV", "parseWithHeader") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["text"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_csv_parse_with_header({}, {})", slots[0], py_span(span)),
        )?,
        ("TOML", "parse") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["text"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_toml_parse({}, {})", slots[0], py_span(span)),
        )?,
        ("TOML", "toJson") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["value"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_toml_to_json({}, {})", slots[0], py_span(span)),
        )?,
        ("URL", "parse") => emit_statement_lowered_static_call_to_target(
            args,
            StaticCallSpec::new(&["text"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |slots| format!("tpz_url_parse({}, {})", slots[0], py_span(span)),
        )?,
        _ => return Ok(false),
    }
    Ok(true)
}

pub(super) fn emit_statement_lowered_receiver_builtin_call_to_target(
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
    if template_value(object, ctx) {
        return Err(PyEmitError::unsupported("member call").at(span));
    }
    if string_value(object, ctx)
        && let Some(spec) = string_receiver_builtin(method)
    {
        emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(spec.params, &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, slots| spec.render(recv, slots, span),
        )?;
        return Ok(true);
    }
    if receiver_is_array_value(object, ctx)
        && let Some(spec) = array_receiver_builtin(method)
    {
        emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(spec.params, &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, slots| spec.render(recv, slots, span),
        )?;
        return Ok(true);
    }
    match method {
        "split" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&["sep"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, slots| format!("tpz_split({recv}, {}, {})", slots[0], py_span(span)),
        )?,
        "codePointAt" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&["i"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, slots| {
                format!(
                    "tpz_string_code_point_at({recv}, {}, {})",
                    slots[0],
                    py_span(span)
                )
            },
        )?,
        "byteLength" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("tpz_string_byte_length({recv}, {})", py_span(span)),
        )?,
        "trim" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("tpz_string_trim({recv}, {})", py_span(span)),
        )?,
        "scalars" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("list({recv})"),
        )?,
        "join" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&["sep"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, slots| format!("tpz_array_join({recv}, {}, {})", slots[0], py_span(span)),
        )?,
        "get" if receiver_is_array_value(object, ctx) => {
            emit_statement_lowered_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&["i"], &[], span),
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| format!("tpz_get({recv}, {}, {})", slots[0], py_span(span)),
            )?
        }
        "get" if receiver_is_map_value(object, ctx) => {
            emit_statement_lowered_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&["k"], &[], span),
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| format!("tpz_get({recv}, {}, {})", slots[0], py_span(span)),
            )?
        }
        "get" if receiver_is_json_value(object, ctx) => {
            emit_statement_lowered_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&["key"], &[], span),
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| format!("tpz_get({recv}, {}, {})", slots[0], py_span(span)),
            )?
        }
        "get" if receiver_is_bytes_value(object, ctx) => {
            emit_statement_lowered_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&["index"], &[], span),
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| format!("tpz_get({recv}, {}, {})", slots[0], py_span(span)),
            )?
        }
        "get" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&["key"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, slots| format!("tpz_get({recv}, {}, {})", slots[0], py_span(span)),
        )?,
        "getOr" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&["k", "default"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, slots| {
                format!(
                    "tpz_map_get_or({recv}, {}, {}, {})",
                    slots[0],
                    slots[1],
                    py_span(span)
                )
            },
        )?,
        "containsKey" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&["k"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, slots| {
                format!(
                    "tpz_map_contains_key({recv}, {}, {})",
                    slots[0],
                    py_span(span)
                )
            },
        )?,
        "contains" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&["x"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, slots| format!("tpz_set_contains({recv}, {}, {})", slots[0], py_span(span)),
        )?,
        "union" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&["other"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, slots| format!("tpz_set_union({recv}, {}, {})", slots[0], py_span(span)),
        )?,
        "intersection" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&["other"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, slots| {
                format!(
                    "tpz_set_intersection({recv}, {}, {})",
                    slots[0],
                    py_span(span)
                )
            },
        )?,
        "difference" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&["other"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, slots| {
                format!(
                    "tpz_set_difference({recv}, {}, {})",
                    slots[0],
                    py_span(span)
                )
            },
        )?,
        "decodeUtf8" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("tpz_bytes_decode_utf8({recv}, {})", py_span(span)),
        )?,
        "toHex" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("tpz_bytes_to_hex({recv}, {})", py_span(span)),
        )?,
        "toBase64" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("tpz_bytes_to_base64({recv}, {})", py_span(span)),
        )?,
        "isEmpty" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("tpz_is_empty({recv}, {})", py_span(span)),
        )?,
        "slice" if receiver_is_bytes_value(object, ctx) => {
            emit_statement_lowered_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&["start", "end"], &[], span),
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
        "toArray" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("tpz_to_array({recv}, {})", py_span(span)),
        )?,
        "kind" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("tpz_json_kind({recv}, {})", py_span(span)),
        )?,
        "isNull" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("tpz_json_is_null({recv}, {})", py_span(span)),
        )?,
        "asString" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("tpz_json_as_string({recv}, {})", py_span(span)),
        )?,
        "asBool" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("tpz_json_as_bool({recv}, {})", py_span(span)),
        )?,
        "asInt" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("tpz_json_as_int({recv}, {})", py_span(span)),
        )?,
        "numberText" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("tpz_json_number_text({recv}, {})", py_span(span)),
        )?,
        "asArray" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("tpz_json_as_array({recv}, {})", py_span(span)),
        )?,
        "keys" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("tpz_json_keys({recv}, {})", py_span(span)),
        )?,
        "values" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("tpz_json_values({recv}, {})", py_span(span)),
        )?,
        "isMatch" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&["text"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, slots| {
                format!(
                    "tpz_regex_is_match({recv}, {}, {})",
                    slots[0],
                    py_span(span)
                )
            },
        )?,
        "replaceAll" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&["text", "replacement"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, slots| {
                format!(
                    "tpz_regex_replace_all({recv}, {}, {}, {})",
                    slots[0],
                    slots[1],
                    py_span(span)
                )
            },
        )?,
        "path" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("tpz_url_path({recv}, {})", py_span(span)),
        )?,
        "toString" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("tpz_url_to_string({recv}, {})", py_span(span)),
        )?,
        "at" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&["index"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, slots| {
                format!(
                    "tpz_json_at({recv}, {}, {}, {})",
                    slots[0],
                    py_span(span),
                    py_span(span)
                )
            },
        )?,
        "length" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("tpz_length({recv}, {})", py_span(span)),
        )?,
        "map" => {
            if receiver_is_array_value(object, ctx) {
                let cooperative = ctx.cooperative_yields;
                emit_statement_lowered_receiver_static_call_to_target_with_bound(
                    object,
                    args,
                    StaticCallSpec::new(&["f"], &[(0, 1)], span),
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
            } else if receiver_is_option_value(object, ctx) {
                let cooperative = ctx.cooperative_yields;
                emit_statement_lowered_receiver_static_call_to_target_with_bound(
                    object,
                    args,
                    StaticCallSpec::new(&["f"], &[(0, 1)], span),
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
            } else if receiver_is_result_value(object, ctx) {
                let cooperative = ctx.cooperative_yields;
                emit_statement_lowered_receiver_static_call_to_target_with_bound(
                    object,
                    args,
                    StaticCallSpec::new(&["f"], &[(0, 1)], span),
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
            } else {
                return Ok(false);
            }
        }
        "filter" => {
            if receiver_is_map_value(object, ctx) {
                let cooperative = ctx.cooperative_yields;
                emit_statement_lowered_receiver_static_call_to_target_with_bound(
                    object,
                    args,
                    StaticCallSpec::new(&["f"], &[(0, 2)], span),
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
            } else if receiver_is_array_value(object, ctx) {
                let cooperative = ctx.cooperative_yields;
                emit_statement_lowered_receiver_static_call_to_target_with_bound(
                    object,
                    args,
                    StaticCallSpec::new(&["f"], &[(0, 1)], span),
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
            } else {
                return Ok(false);
            }
        }
        "reduce" if receiver_is_array_value(object, ctx) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["initial", "f"], &[(1, 2)], span),
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
        "sorted" if receiver_is_array_value(object, ctx) => {
            emit_statement_lowered_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&[], &[], span),
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_array_sorted({recv}, {})", py_span(span)),
            )?
        }
        "sort" if receiver_is_array_value(object, ctx) => {
            emit_statement_lowered_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&[], &[], span),
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, _| format!("tpz_array_sort({recv}, {})", py_span(span)),
            )?
        }
        "sortedBy" if receiver_is_array_value(object, ctx) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["f"], &[(0, 1)], span),
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
        "sortBy" if receiver_is_array_value(object, ctx) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["f"], &[(0, 1)], span),
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
        "retain" if receiver_is_array_value(object, ctx) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["f"], &[(0, 1)], span),
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
        "flatMap" => {
            if receiver_is_option_value(object, ctx) {
                let cooperative = ctx.cooperative_yields;
                emit_statement_lowered_receiver_static_call_to_target_with_bound(
                    object,
                    args,
                    StaticCallSpec::new(&["f"], &[(0, 1)], span),
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
            } else if receiver_is_result_value(object, ctx) {
                let cooperative = ctx.cooperative_yields;
                emit_statement_lowered_receiver_static_call_to_target_with_bound(
                    object,
                    args,
                    StaticCallSpec::new(&["f"], &[(0, 1)], span),
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
            } else {
                return Ok(false);
            }
        }
        "okOrElse" if receiver_is_option_value(object, ctx) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["f"], &[(0, 0)], span),
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
        "okOr" if receiver_is_option_value(object, ctx) => {
            emit_statement_lowered_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&["error"], &[], span),
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| format!("tpz_option_ok_or({recv}, {}, {})", slots[0], py_span(span)),
            )?
        }
        "mapValues" if receiver_is_map_value(object, ctx) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["f"], &[(0, 1)], span),
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
        "update" if receiver_is_map_value(object, ctx) => {
            let cooperative = ctx.cooperative_yields;
            emit_statement_lowered_receiver_static_call_to_target_with_bound(
                object,
                args,
                StaticCallSpec::new(&["k", "initial", "f"], &[(2, 1)], span),
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
        "insert" if receiver_is_map_value(object, ctx) => {
            emit_statement_lowered_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&["k", "v"], &[], span),
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| {
                    format!(
                        "tpz_map_insert({recv}, {}, {}, {})",
                        slots[0],
                        slots[1],
                        py_span(span)
                    )
                },
            )?
        }
        "remove" if receiver_is_map_value(object, ctx) => {
            emit_statement_lowered_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&["k"], &[], span),
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| format!("tpz_remove({recv}, {}, {})", slots[0], py_span(span)),
            )?
        }
        "remove" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&["value"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, slots| format!("tpz_remove({recv}, {}, {})", slots[0], py_span(span)),
        )?,
        "clear" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&[], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, _| format!("tpz_clear({recv}, {})", py_span(span)),
        )?,
        "add" => emit_statement_lowered_receiver_static_call_to_target(
            object,
            args,
            StaticCallSpec::new(&["value"], &[], span),
            StatementTarget::new(target_py, ctx, indent, out),
            |recv, slots| format!("tpz_set_add({recv}, {}, {})", slots[0], py_span(span)),
        )?,
        "push" if receiver_is_array_value(object, ctx) => {
            emit_statement_lowered_receiver_static_call_to_target(
                object,
                args,
                StaticCallSpec::new(&["x"], &[], span),
                StatementTarget::new(target_py, ctx, indent, out),
                |recv, slots| format!("tpz_array_push({recv}, {}, {})", slots[0], py_span(span)),
            )?
        }
        _ => return Ok(false),
    }
    Ok(true)
}
