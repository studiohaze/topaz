use super::*;

pub(crate) fn optional_mutator_fault(
    root: Option<&str>,
    locals: &[(String, Bind)],
    m_span: &str,
) -> Result<Option<String>, EmitError> {
    if let Some(name) = root
        && lookup_bind(locals, name).is_none()
    {
        return Err(EmitError::unsupported(
            "optional mutator on a non-local-rooted receiver",
        ));
    }
    Ok(
        match root.and_then(|name| lookup_bind(locals, name).map(|b| (name, b))) {
            Some((
                root_name,
                Bind::Imm | Bind::ImmCell | Bind::TopFnCell | Bind::TopValueCell | Bind::Namespace,
            )) => {
                let msg = format!(
                    "`{root_name}` is not `let mut`; in-place collection mutation requires a mutable binding (§9)"
                );
                Some(format!(
                    "return Err(fault(codes::GUARD_IMMUTABLE, {msg:?}, {m_span}))"
                ))
            }
            Some((_, Bind::Mut | Bind::Cell | Bind::TopMutValueCell)) | None => None,
        },
    )
}

pub(crate) fn emit_optional_one_callback_dispatch(
    method: &str,
    args: &[CallArg],
    leading: Option<&str>,
    root: Option<&str>,
    ctx: RenderedCallContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let RenderedCallContext {
        expression: ExprEmitContext { locals, .. },
        member_span: m_span,
        call_span: c_span,
    } = ctx;
    if method == "okOrElse" {
        let rendered =
            render_ok_or_else_args(args, OkOrElseCallMode::Optional { leading }, ctx.expression)?;
        let shadow_call = rendered.shadow_call("__field", c_span);
        let none_arm = rendered.builtin_arm(m_span, c_span);
        return Ok(format!(
            "match member_value(&__recv, {method:?}, {m_span})? {{ \
             Some(__field) => {shadow_call}, \
             None => {{ {none_arm} }}, \
             }}"
        ));
    }

    let (f_rs, named) = emit_single_callback_arg(args, leading, ctx.expression)?;
    let shadow_call = emit_single_callback_shadow_call(&f_rs, named, c_span);
    let none_arm = if matches!(method, "sortBy" | "retain") {
        let recv_gate = format!(
            "match &__recv {{ Value::Array(_) => {{}}, _ => return Err(no_member_fault(&__recv, {method:?}, {m_span})), }};"
        );
        match optional_mutator_fault(root, locals, m_span)? {
            Some(immutable_fault) => format!("{recv_gate} {immutable_fault}"),
            None => {
                let body = emit_array_callback_mutator(method, "__recv", &f_rs, m_span, c_span);
                format!("{recv_gate} {body}")
            }
        }
    } else {
        emit_receiver_one_callback_body(method, "__recv", &f_rs, m_span, c_span)
    };
    Ok(format!(
        "match member_value(&__recv, {method:?}, {m_span})? {{ \
         Some(__field) => {shadow_call}, \
         None => {{ {none_arm} }}, \
         }}"
    ))
}

pub(crate) fn emit_optional_reduce_dispatch(
    args: &[CallArg],
    leading: Option<&str>,
    ctx: RenderedCallContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let RenderedCallContext {
        expression:
            ExprEmitContext {
                src,
                aliases,
                locals,
                in_loop,
            },
        member_span: m_span,
        call_span: c_span,
    } = ctx;
    if needs_pipe_static_binding(args, leading) {
        let bound = bind_pipe_static_args(args, &["initial", "f"], leading, ctx.expression)?;
        let init_rs = bound.slot(0);
        let f_rs = bound.slot(1);
        let shadow_call = render_bound_field_call(&bound, c_span);
        let temps = &bound.temps;
        let none_arm = format!("{temps}{}", emit_reduce("__recv", init_rs, f_rs, c_span));
        return Ok(format!(
            "match member_value(&__recv, \"reduce\", {m_span})? {{ \
             Some(__field) => {{ {}{shadow_call} }}, \
             None => {{ {none_arm} }}, \
             }}",
            temps
        ));
    }

    let (init_rs, f_rs) = match (leading, args) {
        (Some(lead), [CallArg::Positional(f)]) => (
            lead.to_string(),
            emit_expr(f, src, aliases, locals, in_loop)?,
        ),
        (None, [CallArg::Positional(init), CallArg::Positional(f)]) => (
            emit_expr(init, src, aliases, locals, in_loop)?,
            emit_expr(f, src, aliases, locals, in_loop)?,
        ),
        _ => return Err(EmitError::unsupported("call argument shape")),
    };
    let shadow_call =
        format!("call_value(__field, vec![{init_rs}, {f_rs}], cx.clone(), {c_span}).await?");
    let none_arm = emit_reduce("__recv", &init_rs, &f_rs, c_span);
    Ok(format!(
        "match member_value(&__recv, \"reduce\", {m_span})? {{ \
         Some(__field) => {shadow_call}, \
         None => {{ {none_arm} }}, \
         }}"
    ))
}

pub(crate) fn emit_optional_update_dispatch(
    args: &[CallArg],
    leading: Option<&str>,
    root: Option<&str>,
    ctx: RenderedCallContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let RenderedCallContext {
        expression:
            ExprEmitContext {
                src,
                aliases,
                locals,
                in_loop,
            },
        member_span: m_span,
        call_span: c_span,
    } = ctx;
    if needs_pipe_static_binding(args, leading) {
        let bound = bind_pipe_static_args(args, &["k", "initial", "f"], leading, ctx.expression)?;
        let k_rs = bound.slot(0);
        let init_rs = bound.slot(1);
        let f_rs = bound.slot(2);
        let shadow_call = render_bound_field_call(&bound, c_span);
        let temps = &bound.temps;
        let recv_gate = format!(
            "match &__recv {{ Value::Map(_) => {{}}, _ => return Err(no_member_fault(&__recv, \"update\", {m_span})), }};"
        );
        let none_arm = match optional_mutator_fault(root, locals, m_span)? {
            Some(immutable_fault) => format!("{recv_gate} {immutable_fault}"),
            None => format!(
                "{recv_gate} {}{}",
                temps,
                emit_map_update("__recv", k_rs, init_rs, f_rs, m_span, c_span)
            ),
        };
        return Ok(format!(
            "match member_value(&__recv, \"update\", {m_span})? {{ \
             Some(__field) => {{ {}{shadow_call} }}, \
             None => {{ {none_arm} }}, \
             }}",
            temps
        ));
    }

    let emit_update_arg = |arg: &CallArg| -> Result<String, EmitError> {
        let CallArg::Positional(expr) = arg else {
            return Err(EmitError::unsupported("call argument shape"));
        };
        if matches!(&expr.kind, ExprKind::Placeholder) {
            let Some(lead) = leading else {
                return Err(EmitError::unsupported("placeholder outside pipe stage"));
            };
            return Ok(lead.to_string());
        }
        if contains_placeholder(expr) {
            return Err(EmitError::unsupported("pipe placeholder"));
        }
        emit_expr(expr, src, aliases, locals, in_loop)
    };

    let has_placeholder = pipe_args_contain_placeholder(args);

    let (k_rs, init_rs, f_rs) = if leading.is_some() && has_placeholder {
        let [k, init, f] = args else {
            return Err(EmitError::unsupported("call argument shape"));
        };
        (
            emit_update_arg(k)?,
            emit_update_arg(init)?,
            emit_update_arg(f)?,
        )
    } else if let Some(lead) = leading {
        let [CallArg::Positional(init), CallArg::Positional(f)] = args else {
            return Err(EmitError::unsupported("call argument shape"));
        };
        (
            lead.to_string(),
            emit_expr(init, src, aliases, locals, in_loop)?,
            emit_expr(f, src, aliases, locals, in_loop)?,
        )
    } else {
        let [
            CallArg::Positional(k),
            CallArg::Positional(init),
            CallArg::Positional(f),
        ] = args
        else {
            return Err(EmitError::unsupported("call argument shape"));
        };
        (
            emit_expr(k, src, aliases, locals, in_loop)?,
            emit_expr(init, src, aliases, locals, in_loop)?,
            emit_expr(f, src, aliases, locals, in_loop)?,
        )
    };
    let shadow_call = format!(
        "call_value(__field, vec![{k_rs}, {init_rs}, {f_rs}], cx.clone(), {c_span}).await?"
    );
    let recv_gate = format!(
        "match &__recv {{ Value::Map(_) => {{}}, _ => return Err(no_member_fault(&__recv, \"update\", {m_span})), }};"
    );
    let none_arm = match optional_mutator_fault(root, locals, m_span)? {
        Some(immutable_fault) => format!("{recv_gate} {immutable_fault}"),
        None => format!(
            "{recv_gate} {}",
            emit_map_update("__recv", &k_rs, &init_rs, &f_rs, m_span, c_span)
        ),
    };
    Ok(format!(
        "match member_value(&__recv, \"update\", {m_span})? {{ \
         Some(__field) => {shadow_call}, \
         None => {{ {none_arm} }}, \
         }}"
    ))
}

pub(crate) fn wrap_optional_call_dispatch(object: &str, dispatch: &str) -> String {
    format!(
        "{{ let __obj = {object}; match __obj {{ \
         Value::None => Value::None, \
         Value::Null => Value::Null, \
         Value::Some(__inner) => {{ let __recv = (*__inner).clone(); wrap_optional({dispatch}) }}, \
         __other => {{ let __recv = __other; {dispatch} }}, }} }}"
    )
}

pub(crate) fn emit_optional_call_expr(
    expr: &Expr,
    object: &Expr,
    field: &Ident,
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let method = text(src, field.span);
    let is_resource = is_resource_receiver_method(method);
    // §9 a MUTATING optional call resolves the `mut`-root from the path root; a
    // const/import-rooted mutator is refused (a safe over-refusal). `clear` is a
    // `call_method`-leaf mutator (§6); `update` is a callback mutator refused below.
    // §6 (v5.4) the SIMPLE array mutators (`pop`/`reverse`/`removeAt`/`sort`) are
    // `call_method` leaves too, so they ride this path; the CALLBACK array mutators
    // `sortBy`/`retain` are refused below (like `update`/`mapValues`).
    let is_mutator = is_collection_mutator_name(method);
    let root = if is_mutator {
        mutation_root(object, src)
    } else {
        None
    };
    if is_mutator
        && let Some(name) = root
        && lookup_bind(locals, name).is_none()
    {
        return Err(EmitError::unsupported(
            "optional mutator on a non-local-rooted receiver",
        ));
    }
    let obj_rs = emit_expr(object, src, aliases, locals, in_loop)?;
    if is_resource {
        let span = emit_span(expr.span);
        let rendered = render_call_args(args, ctx, expr.span, "call argument shape")?;
        let dispatch = emit_resource_method_dispatch(method, &rendered, &[], "", &span, &span);
        return Ok(wrap_optional_call_dispatch(&obj_rs, &dispatch));
    }
    if method == "okOrElse" && args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        let dispatch = emit_nonvariadic_receiver_hof_spread_dispatch(
            "__recv", method, args, ctx, expr.span, expr.span,
        )?;
        return Ok(wrap_optional_call_dispatch(&obj_rs, &dispatch));
    }
    if method == "okOrElse" {
        let span = emit_span(expr.span);
        let dispatch = emit_optional_one_callback_dispatch(
            method,
            args,
            None,
            None,
            RenderedCallContext {
                expression: ctx,
                member_span: &span,
                call_span: &span,
            },
        )?;
        return Ok(wrap_optional_call_dispatch(&obj_rs, &dispatch));
    }
    if matches!(
        method,
        "map"
            | "filter"
            | "reduce"
            | "flatMap"
            | "sortedBy"
            | "mapValues"
            | "update"
            | "sortBy"
            | "retain"
    ) {
        if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
            if matches!(method, "update" | "sortBy" | "retain") {
                let dispatch = emit_nonvariadic_receiver_mutator_spread_dispatch(
                    "__recv",
                    method,
                    root,
                    args,
                    ExprEmitContext {
                        src,
                        aliases,
                        locals,
                        in_loop,
                    },
                    expr.span,
                    expr.span,
                )?;
                return Ok(wrap_optional_call_dispatch(&obj_rs, &dispatch));
            }
            let dispatch = emit_nonvariadic_receiver_hof_spread_dispatch(
                "__recv", method, args, ctx, expr.span, expr.span,
            )?;
            return Ok(wrap_optional_call_dispatch(&obj_rs, &dispatch));
        }
        let span = emit_span(expr.span);
        let call_ctx = RenderedCallContext {
            expression: ctx,
            member_span: &span,
            call_span: &span,
        };
        let dispatch = match method {
            "reduce" => emit_optional_reduce_dispatch(args, None, call_ctx)?,
            "update" => emit_optional_update_dispatch(args, None, root, call_ctx)?,
            _ => emit_optional_one_callback_dispatch(method, args, None, root, call_ctx)?,
        };
        return Ok(wrap_optional_call_dispatch(&obj_rs, &dispatch));
    }
    if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        let span = emit_span(expr.span);
        let (some_arm, none_inner) =
            emit_nonvariadic_receiver_spread_branches("__f", args, ctx, expr.span)?;
        let dispatch = format!(
            "match member_value(&__recv, {method:?}, {span})? {{ \
                         Some(__f) => {some_arm}, \
                         None => {{ check_member_method(&__recv, {method:?}, {span})?; {none_inner} }}, }}"
        );
        return Ok(wrap_optional_call_dispatch(&obj_rs, &dispatch));
    }
    let span = emit_span(expr.span);
    let rendered = render_call_args(args, ctx, expr.span, "call argument shape")?;
    let field_call = rendered.value_call("__f", &[]);
    let method_call = rendered.method_call(method, &[], &span, &span);
    let none_inner = {
        match is_mutator.then(|| root.and_then(|n| lookup_bind(locals, n).map(|b| (n, b)))) {
            Some(Some((
                root_name,
                Bind::Imm | Bind::ImmCell | Bind::TopFnCell | Bind::TopValueCell | Bind::Namespace,
            ))) => {
                let msg = format!(
                    "`{root_name}` is not `let mut`; in-place collection mutation requires a mutable binding (§9)"
                );
                format!(
                    "check_member_method(&__recv, {method:?}, {span})?; return Err(fault(codes::GUARD_IMMUTABLE, {msg:?}, {span}))"
                )
            }
            _ => format!("check_member_method(&__recv, {method:?}, {span})?; {method_call}"),
        }
    };
    let dispatch = format!(
        "match member_value(&__recv, {method:?}, {span})? {{ \
                     Some(__f) => {field_call}, \
                     None => {{ {none_inner} }}, }}"
    );
    Ok(wrap_optional_call_dispatch(&obj_rs, &dispatch))
}
