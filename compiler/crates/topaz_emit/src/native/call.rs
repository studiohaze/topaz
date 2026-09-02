use super::*;

/// Lower a direct same-module call `f(a, b, …)` to a real Rust call into the
/// emitted native `fn`. Only a bare-identifier callee bound to a collected
/// `NativeFn` lowers; arguments must be positional scalars matching the param
/// types. Read-only `Array<scalar>` parameters pass boxed `Value`s and are only
/// usable through `.length`/`[i]` in the callee. A builtin / method /
/// first-class / generic call refuses.
pub(super) fn emit_call(
    callee: &Expr,
    args: &[CallArg],
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
    span: Span,
) -> Result<Lowered, EmitError> {
    if let Some(low) = emit_byte_call(callee, args, ctx, scope, span)? {
        return Ok(low);
    }
    if ctx.hybrid && !matches!(callee.kind, ExprKind::Ident) {
        return Err(decline("a non-direct hybrid call").at(callee.span));
    }
    if let Some(low) = emit_math_call(callee, args, ctx, scope, span)? {
        if ctx.hybrid {
            return Err(decline("a hybrid call outside the scalar helper set").at(callee.span));
        }
        return Ok(low);
    }

    let ExprKind::Ident = &callee.kind else {
        return Err(decline("a non-direct call").at(callee.span));
    };
    let name = text(ctx.src, callee.span);
    // A LOCAL shadowing the function name is not a native fn call (it would be a
    // boxed callable) — refuse.
    if scope.iter().any(|l| l.name == name) {
        return Err(decline("a call through a local binding").at(callee.span));
    }
    if name == "print" {
        if ctx.hybrid {
            return Err(decline("a hybrid host-effect call").at(callee.span));
        }
        let [CallArg::Positional(arg)] = args else {
            return Err(decline("a native `print` call shape").at(span));
        };
        let low = emit_expr(arg, ctx, scope)?;
        return Ok(Lowered {
            rs: format!(
                "{{ builtin_print(&*cx.host(), {}, {})?; () }}",
                low.ty.box_expr(&low.rs),
                emit_span(span)
            ),
            ty: NativeTy::Unit,
        });
    }
    let Some(sig) = ctx.fns.get(name).cloned() else {
        if ctx.hybrid {
            return Err(decline("a call outside the hybrid helper set").at(callee.span));
        }
        let Some(decl) = ctx.generic_fns.get(name).copied() else {
            return Err(decline("a call to a non-native function").at(callee.span));
        };
        return emit_generic_call(name, decl, args, ctx, scope, span);
    };
    let (bindings, arg_rs) = bind_native_call_args(
        args,
        &sig.params,
        &sig.names,
        &sig.defaults,
        ctx,
        scope,
        span,
    )?;
    // The native call ABI: `Box::pin(f(cx.clone(), <call-expr-span>, args)).await?`. The
    // call-expression span is threaded so the callee's `__native_enter_call` cap
    // fault points at the call site, byte-identical to interp+boxed. Arguments are
    // lowered (evaluated) BEFORE the call, so the order is args → cap-check →
    // body. Both `entry` and every native fn body are `async`, so `.await` is
    // valid here. Boxing the callee future gives recursive async calls the
    // required indirection and prevents deep acyclic chains from nesting concrete
    // future types past rustc's query-depth limit.
    let hybrid_guard = if ctx.hybrid { ", false" } else { "" };
    let prefix = if arg_rs.is_empty() {
        format!("cx.clone(), {}{hybrid_guard}", emit_span(span))
    } else {
        format!(
            "cx.clone(), {}{hybrid_guard}, {}",
            emit_span(span),
            arg_rs.join(", ")
        )
    };
    let call = format!("Box::pin({}({prefix})).await?", sig.rust_name);
    Ok(Lowered {
        rs: if bindings.is_empty() {
            call
        } else {
            format!("{{ {bindings}{call} }}")
        },
        ty: sig.ret,
    })
}

pub(super) fn emit_byte_call(
    callee: &Expr,
    args: &[CallArg],
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
    span: Span,
) -> Result<Option<Lowered>, EmitError> {
    let ExprKind::Member { object, field } = &callee.kind else {
        return Ok(None);
    };
    let ExprKind::Ident = object.kind else {
        return Ok(None);
    };
    let receiver_name = text(ctx.src, object.span);
    let Some(receiver) = scope.iter().rev().find(|local| local.name == receiver_name) else {
        return Ok(None);
    };
    let Some(handle) = receiver.byte_handle() else {
        return Ok(None);
    };
    let member = text(ctx.src, field.span);
    let receiver_rs = mangle(receiver_name);
    let call_span = emit_span(span);

    let i64_args = |expected: usize| -> Result<(String, Vec<String>), EmitError> {
        if args.len() != expected
            || args
                .iter()
                .any(|arg| !matches!(arg, CallArg::Positional(_)))
        {
            return Err(decline("a byte leaf argument shape").at(span));
        }
        let mut bindings = String::new();
        let mut values = Vec::with_capacity(expected);
        for (index, arg) in args.iter().enumerate() {
            let CallArg::Positional(expr) = arg else {
                unreachable!("shape checked above")
            };
            let low = emit_expr(expr, ctx, scope)?;
            if low.ty != NativeTy::I64 {
                return Err(decline("a non-integer byte leaf argument").at(expr.span));
            }
            let temp = format!("__byte_arg_{index}");
            bindings.push_str(&format!("let {temp}: i64 = {}; ", low.rs));
            values.push(temp);
        }
        Ok((bindings, values))
    };

    let lowered = match (handle, member) {
        (MonoTy::BytesHandle, "length") if args.is_empty() => Lowered {
            rs: format!("builtin_bytes_length_i64(&{receiver_rs}, {call_span})?"),
            ty: NativeTy::I64,
        },
        (MonoTy::ByteBufferHandle, "length") if args.is_empty() => Lowered {
            rs: format!("builtin_byte_buffer_length_i64(&{receiver_rs}, {call_span})?"),
            ty: NativeTy::I64,
        },
        (MonoTy::ByteBufferHandle, "get") => {
            let (bindings, values) = i64_args(1)?;
            Lowered {
                rs: format!(
                    "{{ {bindings}builtin_byte_buffer_get_raw_i64(&{receiver_rs}, {}, {call_span})? }}",
                    values[0]
                ),
                ty: NativeTy::I64,
            }
        }
        (MonoTy::ByteBufferHandle, "set") => {
            let (bindings, values) = i64_args(2)?;
            Lowered {
                rs: format!(
                    "{{ {bindings}builtin_byte_buffer_set_i64(&{receiver_rs}, {}, {}, {call_span})?; () }}",
                    values[0], values[1]
                ),
                ty: NativeTy::Unit,
            }
        }
        (MonoTy::ByteBufferHandle, "fill") => {
            let (bindings, values) = i64_args(3)?;
            Lowered {
                rs: format!(
                    "{{ {bindings}builtin_byte_buffer_fill_i64(&{receiver_rs}, {}, {}, {}, {call_span})?; () }}",
                    values[0], values[1], values[2]
                ),
                ty: NativeTy::Unit,
            }
        }
        (MonoTy::ByteBufferHandle, "copy") => {
            if args.len() != 4
                || args
                    .iter()
                    .any(|arg| !matches!(arg, CallArg::Positional(_)))
            {
                return Err(decline("a byte leaf argument shape").at(span));
            }
            let CallArg::Positional(source_expr) = &args[0] else {
                unreachable!()
            };
            let ExprKind::Ident = source_expr.kind else {
                return Err(decline("a non-direct ByteBuffer copy source").at(source_expr.span));
            };
            let source_name = text(ctx.src, source_expr.span);
            let Some(source) = scope.iter().rev().find(|local| local.name == source_name) else {
                return Err(decline("an unknown ByteBuffer copy source").at(source_expr.span));
            };
            if source.byte_handle() != Some(MonoTy::ByteBufferHandle) {
                return Err(decline("a non-ByteBuffer copy source").at(source_expr.span));
            }
            let mut bindings = format!(
                "let __byte_source: Value = {}.clone(); ",
                mangle(source_name)
            );
            let mut values = Vec::with_capacity(3);
            for (index, arg) in args[1..].iter().enumerate() {
                let CallArg::Positional(expr) = arg else {
                    unreachable!()
                };
                let low = emit_expr(expr, ctx, scope)?;
                if low.ty != NativeTy::I64 {
                    return Err(decline("a non-integer byte leaf argument").at(expr.span));
                }
                let temp = format!("__byte_arg_{}", index + 1);
                bindings.push_str(&format!("let {temp}: i64 = {}; ", low.rs));
                values.push(temp);
            }
            Lowered {
                rs: format!(
                    "{{ {bindings}builtin_byte_buffer_copy_i64(&{receiver_rs}, &__byte_source, {}, {}, {}, {call_span})?; () }}",
                    values[0], values[1], values[2]
                ),
                ty: NativeTy::Unit,
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(lowered))
}

pub(super) fn emit_boxed_byte_call(
    callee: &Expr,
    args: &[CallArg],
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
    span: Span,
) -> Result<Option<String>, EmitError> {
    let ExprKind::Member { object, field } = &callee.kind else {
        return Ok(None);
    };
    let ExprKind::Ident = object.kind else {
        return Ok(None);
    };
    let receiver_name = text(ctx.src, object.span);
    let Some(handle) = scope
        .iter()
        .rev()
        .find(|local| local.name == receiver_name)
        .and_then(NativeLocal::byte_handle)
    else {
        return Ok(None);
    };
    let member = text(ctx.src, field.span);
    let receiver_rs = mangle(receiver_name);
    let call_span = emit_span(span);
    let positional_i64 = |expected: usize| -> Result<(String, Vec<String>), EmitError> {
        if args.len() != expected
            || args
                .iter()
                .any(|arg| !matches!(arg, CallArg::Positional(_)))
        {
            return Err(decline("a byte leaf argument shape").at(span));
        }
        let mut bindings = String::new();
        let mut values = Vec::with_capacity(expected);
        for (index, arg) in args.iter().enumerate() {
            let CallArg::Positional(expr) = arg else {
                unreachable!()
            };
            let low = emit_expr(expr, ctx, scope)?;
            if low.ty != NativeTy::I64 {
                return Err(decline("a non-integer byte leaf argument").at(expr.span));
            }
            let temp = format!("__byte_arg_{index}");
            bindings.push_str(&format!("let {temp}: i64 = {}; ", low.rs));
            values.push(temp);
        }
        Ok((bindings, values))
    };
    let rendered = match (handle, member) {
        (MonoTy::BytesHandle, "get") => {
            let (bindings, values) = positional_i64(1)?;
            format!(
                "{{ {bindings}builtin_bytes_get_i64(&{receiver_rs}, {}, {call_span})? }}",
                values[0]
            )
        }
        (MonoTy::BytesHandle, "slice") => {
            let (bindings, values) = positional_i64(2)?;
            format!(
                "{{ {bindings}builtin_bytes_slice_i64(&{receiver_rs}, {}, {}, {call_span})? }}",
                values[0], values[1]
            )
        }
        (MonoTy::ByteBufferHandle, "toBytes") if args.is_empty() => {
            format!("builtin_byte_buffer_to_bytes_ref(&{receiver_rs}, {call_span})?")
        }
        _ => return Ok(None),
    };
    Ok(Some(rendered))
}

pub(super) fn emit_generic_call(
    name: &str,
    decl: &FunctionDecl,
    args: &[CallArg],
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
    span: Span,
) -> Result<Lowered, EmitError> {
    let type_params = generic_type_param_names(decl, ctx.src);
    let mut inferred: Vec<Option<NativeTy>> = vec![None; type_params.len()];
    let (bindings, arg_rs) =
        bind_generic_call_args(args, decl, &type_params, &mut inferred, ctx, scope, span)?;
    let type_args = inferred
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| decline("an uninferred native generic type argument").at(span))?;
    let sig = ctx
        .generic_specs
        .get(name, &type_args)
        .cloned()
        .ok_or_else(|| decline("a missing native generic specialization").at(span))?;
    let prefix = if arg_rs.is_empty() {
        format!("cx.clone(), {}", emit_span(span))
    } else {
        format!("cx.clone(), {}, {}", emit_span(span), arg_rs.join(", "))
    };
    let call = format!("Box::pin({}({prefix})).await?", sig.rust_name);
    Ok(Lowered {
        rs: if bindings.is_empty() {
            call
        } else {
            format!("{{ {bindings}{call} }}")
        },
        ty: sig.ret,
    })
}

pub(super) fn emit_native_arg_expr(
    arg: &Expr,
    param: &NativeParam,
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
) -> Result<String, EmitError> {
    match param {
        NativeParam::Scalar(ty) => {
            let low = emit_expr(arg, ctx, scope)?;
            if low.ty != *ty {
                return Err(
                    decline("an argument whose type differs from the parameter").at(arg.span)
                );
            }
            Ok(low.rs)
        }
        NativeParam::ScalarArray(elem) => emit_array_arg(arg, *elem, ctx, scope),
        NativeParam::ByteHandle(expected) => {
            let ExprKind::Ident = arg.kind else {
                return Err(decline("a non-direct byte handle argument").at(arg.span));
            };
            let name = text(ctx.src, arg.span);
            let Some(local) = scope.iter().rev().find(|local| local.name == name) else {
                return Err(decline("an unknown byte handle argument").at(arg.span));
            };
            if local.byte_handle() != Some(*expected) {
                return Err(decline("a byte handle argument whose type differs").at(arg.span));
            }
            Ok(format!("{}.clone()", mangle(name)))
        }
        NativeParam::ByteRecord(_) => {
            Err(decline("a direct user-function aggregate call").at(arg.span))
        }
    }
}

pub(super) fn bind_native_call_args(
    args: &[CallArg],
    params: &[NativeParam],
    names: &[&str],
    defaults: &[Option<&Expr>],
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
    span: Span,
) -> Result<(String, Vec<String>), EmitError> {
    if ctx.hybrid
        && (args.len() != params.len()
            || args
                .iter()
                .any(|arg| !matches!(arg, CallArg::Positional(_))))
    {
        return Err(decline("a non-positional hybrid call").at(span));
    }
    if args.len() > params.len() {
        return Err(decline("a call with a mismatched argument count").at(span));
    }
    let mut slots: Vec<Option<String>> = vec![None; params.len()];
    let mut next_positional = 0usize;
    let mut bindings = String::new();

    for (idx, arg) in args.iter().enumerate() {
        let (param_idx, expr) = match arg {
            CallArg::Positional(expr) => {
                if next_positional >= params.len() || slots[next_positional].is_some() {
                    return Err(
                        decline("a positional argument after a named argument").at(expr.span)
                    );
                }
                let param_idx = next_positional;
                next_positional += 1;
                (param_idx, expr)
            }
            CallArg::Named { name, value } => {
                let n = text(ctx.src, name.span);
                let Some(param_idx) = names.iter().position(|param| *param == n) else {
                    return Err(decline("a named argument with no native parameter").at(name.span));
                };
                if slots[param_idx].is_some() {
                    return Err(decline("a native argument supplied twice").at(name.span));
                }
                (param_idx, value)
            }
            CallArg::Spread(expr) => return Err(decline("a spread argument").at(expr.span)),
        };

        let rs = emit_native_arg_expr(expr, &params[param_idx], ctx, scope)?;
        let temp = format!("__arg_{idx}");
        bindings.push_str(&format!("let {temp} = {rs}; "));
        slots[param_idx] = Some(temp);
    }

    let mut ordered = Vec::with_capacity(params.len());
    for (idx, slot) in slots.into_iter().enumerate() {
        let value = match slot {
            Some(value) => value,
            None => {
                let Some(default) = defaults.get(idx).and_then(|default| *default) else {
                    return Err(decline("a call with a mismatched argument count").at(span));
                };
                if !crate::is_pure_literal_default(default) {
                    return Err(decline("a function default shape").at(default.span));
                }
                let rs = emit_native_arg_expr(default, &params[idx], ctx, &[])?;
                let temp = format!("__default_{idx}");
                bindings.push_str(&format!("let {temp} = {rs}; "));
                temp
            }
        };
        ordered.push(value);
    }
    Ok((bindings, ordered))
}

pub(super) fn emit_generic_arg_expr(
    arg: &Expr,
    param: &ast::Param,
    type_params: &[&str],
    inferred: &mut [Option<NativeTy>],
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
) -> Result<String, EmitError> {
    if let Some(idx) = generic_type_param_index(&param.ty, type_params, ctx.src) {
        let low = emit_expr(arg, ctx, scope)?;
        bind_generic_arg(inferred, idx, low.ty, arg.span)?;
        return Ok(low.rs);
    }
    if let Some(idx) = generic_array_param_index(&param.ty, type_params, ctx.src) {
        let (array_rs, elem) = infer_array_arg(arg, ctx, scope)?;
        bind_generic_arg(inferred, idx, elem, arg.span)?;
        return Ok(array_rs);
    }
    match generic_param_repr(&param.ty, type_params, &GENERIC_NATIVE_TYPES[..1], ctx.src) {
        Some(NativeParam::Scalar(ty)) => {
            let low = emit_expr(arg, ctx, scope)?;
            if low.ty != ty {
                return Err(decline("a generic argument whose type differs").at(arg.span));
            }
            Ok(low.rs)
        }
        Some(NativeParam::ScalarArray(elem)) => emit_array_arg(arg, elem, ctx, scope),
        Some(NativeParam::ByteHandle(_) | NativeParam::ByteRecord(_)) => {
            Err(decline("a byte or record generic parameter").at(param.span))
        }
        None => Err(decline("a non-native generic parameter").at(param.span)),
    }
}

pub(super) fn bind_generic_call_args(
    args: &[CallArg],
    decl: &FunctionDecl,
    type_params: &[&str],
    inferred: &mut [Option<NativeTy>],
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
    span: Span,
) -> Result<(String, Vec<String>), EmitError> {
    if args.len() != decl.params.len() {
        return Err(decline("a generic call with a mismatched argument count").at(span));
    }
    let mut slots: Vec<Option<String>> = vec![None; decl.params.len()];
    let mut next_positional = 0usize;
    let mut bindings = String::new();

    for (idx, arg) in args.iter().enumerate() {
        let (param_idx, expr) = match arg {
            CallArg::Positional(expr) => {
                if next_positional >= decl.params.len() || slots[next_positional].is_some() {
                    return Err(
                        decline("a positional generic argument after a named argument")
                            .at(expr.span),
                    );
                }
                let param_idx = next_positional;
                next_positional += 1;
                (param_idx, expr)
            }
            CallArg::Named { name, value } => {
                let n = text(ctx.src, name.span);
                let Some(param_idx) = decl
                    .params
                    .iter()
                    .position(|param| text(ctx.src, param.name.span) == n)
                else {
                    return Err(
                        decline("a named generic argument with no native parameter").at(name.span)
                    );
                };
                if slots[param_idx].is_some() {
                    return Err(decline("a native generic argument supplied twice").at(name.span));
                }
                (param_idx, value)
            }
            CallArg::Spread(expr) => return Err(decline("a spread generic argument").at(expr.span)),
        };

        let rs = emit_generic_arg_expr(
            expr,
            &decl.params[param_idx],
            type_params,
            inferred,
            ctx,
            scope,
        )?;
        let temp = format!("__arg_{idx}");
        bindings.push_str(&format!("let {temp} = {rs}; "));
        slots[param_idx] = Some(temp);
    }

    let mut ordered = Vec::with_capacity(decl.params.len());
    for slot in slots {
        let Some(value) = slot else {
            return Err(decline("a generic call with a mismatched argument count").at(span));
        };
        ordered.push(value);
    }
    Ok((bindings, ordered))
}

pub(super) fn bind_generic_arg(
    inferred: &mut [Option<NativeTy>],
    idx: usize,
    ty: NativeTy,
    span: Span,
) -> Result<(), EmitError> {
    match inferred.get_mut(idx) {
        Some(slot @ None) => {
            *slot = Some(ty);
            Ok(())
        }
        Some(Some(prev)) if *prev == ty => Ok(()),
        Some(Some(_)) => Err(decline("conflicting native generic type arguments").at(span)),
        None => Err(decline("a native generic type argument index").at(span)),
    }
}

pub(super) fn infer_array_arg(
    arg: &Expr,
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
) -> Result<(String, NativeTy), EmitError> {
    if let ExprKind::Ident = &arg.kind {
        let name = text(ctx.src, arg.span);
        let local =
            scope.iter().rev().find(|l| l.name == name).ok_or_else(|| {
                decline("an array argument from a non-array binding").at(arg.span)
            })?;
        let elem = local
            .array_elem()
            .ok_or_else(|| decline("an array argument from a non-array binding").at(arg.span))?;
        return Ok((format!("{}.clone()", mangle(name)), elem));
    }
    if matches!(arg.kind, ExprKind::Array(_)) {
        return emit_boxed_scalar_array_inferred(arg, ctx, scope);
    }
    Err(decline("a non-array argument").at(arg.span))
}
