use crate::*;

pub(crate) fn emit_pipe_arg_expr(
    expr: &Expr,
    leading_rs: &str,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    locals: &[(String, Bind)],
    in_loop: bool,
) -> Result<String, EmitError> {
    if matches!(&expr.kind, ExprKind::Placeholder) {
        return Ok(leading_rs.to_string());
    }
    if contains_placeholder(expr) {
        return Err(EmitError::unsupported("pipe placeholder"));
    }
    emit_expr(expr, src, aliases, locals, in_loop)
}

impl RenderedPipeSpreadArgs {
    pub(crate) fn field_call(&self, callee_rs: &str, call_span: &str) -> String {
        let mut tail = String::from("{ let mut __sp: Vec<Value> = Vec::new(); ");
        append_rendered_spread_tail(&self.tail, "__sp", &mut tail);
        tail.push_str("Value::array(__sp) }");
        format!(
            "call_value_spread({callee_rs}, vec![{}], {tail}, cx.clone(), {call_span}, {}).await?",
            self.prefix.join(", "),
            self.first_spread_span
        )
    }

    pub(crate) fn arity_fault(&self, call_span: &str) -> String {
        let mut tail = String::from("let mut __tpz_recv_spread: Vec<Value> = Vec::new(); ");
        append_rendered_spread_tail(&self.tail, "__tpz_recv_spread", &mut tail);
        format!(
            "{{ let _: Vec<Value> = vec![{}]; {tail}let _ = __tpz_recv_spread; let __v: Value = return Err(fault(codes::GUARD_ARITY, {:?}, {call_span})); __v }}",
            self.prefix.join(", "),
            "spread arguments require a variadic parameter (§5)"
        )
    }
}

pub(crate) fn render_pipe_spread_args(
    leading_rs: &str,
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<RenderedPipeSpreadArgs, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    if positional_after_named(args) || args.iter().any(|arg| matches!(arg, CallArg::Named { .. })) {
        return Err(EmitError::unsupported("receiver call spread argument"));
    }
    let first_spread = args
        .iter()
        .position(|arg| matches!(arg, CallArg::Spread(_)))
        .expect("caller checked spread is present");
    let has_placeholder = args.iter().any(|arg| {
        matches!(arg, CallArg::Positional(expr) if matches!(&expr.kind, ExprKind::Placeholder))
    });
    let mut prefix = Vec::new();
    if !has_placeholder {
        prefix.push(leading_rs.to_string());
    }
    for arg in &args[..first_spread] {
        let CallArg::Positional(expr) = arg else {
            return Err(EmitError::unsupported("receiver call spread argument"));
        };
        prefix.push(emit_pipe_arg_expr(
            expr, leading_rs, src, aliases, locals, in_loop,
        )?);
    }

    let CallArg::Spread(first) = &args[first_spread] else {
        unreachable!("first_spread indexes a spread")
    };
    let mut tail = Vec::new();
    for arg in &args[first_spread..] {
        match arg {
            CallArg::Positional(expr) => tail.push(RenderedSpreadTailArg::Positional(
                emit_pipe_arg_expr(expr, leading_rs, src, aliases, locals, in_loop)?,
            )),
            CallArg::Spread(expr) => {
                if contains_placeholder(expr) {
                    return Err(EmitError::unsupported("pipe placeholder"));
                }
                tail.push(RenderedSpreadTailArg::Spread {
                    value: emit_expr(expr, src, aliases, locals, in_loop)?,
                    span: emit_span(expr.span),
                });
            }
            CallArg::Named { .. } => {
                return Err(EmitError::unsupported("receiver call spread argument"));
            }
        }
    }
    Ok(RenderedPipeSpreadArgs {
        prefix,
        tail,
        first_spread_span: emit_span(first.span),
    })
}

pub(crate) fn emit_nonvariadic_receiver_pipe_mutator_spread_dispatch(
    recv_rs: &str,
    method: &str,
    leading_rs: &str,
    root: Option<&str>,
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
    span: Span,
) -> Result<String, EmitError> {
    let ExprEmitContext { locals, .. } = ctx;
    let m_span = emit_span(span);
    let spread_args = render_pipe_spread_args(leading_rs, args, ctx)?;
    let field_call = spread_args.field_call("__field", &m_span);
    let spread_fault = spread_args.arity_fault(&m_span);
    let recv_gate = match method {
        "update" => format!(
            "match &__recv {{ Value::Map(_) => {{}}, _ => return Err(no_member_fault(&__recv, {method:?}, {m_span})), }};"
        ),
        "sortBy" | "retain" => format!(
            "match &__recv {{ Value::Array(_) => {{}}, _ => return Err(no_member_fault(&__recv, {method:?}, {m_span})), }};"
        ),
        _ => return Err(EmitError::unsupported("receiver mutator spread argument")),
    };
    let none_arm = match optional_mutator_fault(root, locals, &m_span)? {
        Some(immutable_fault) => format!("{recv_gate} {immutable_fault}"),
        None => format!("{recv_gate} {spread_fault}"),
    };
    Ok(format!(
        "{{ let __recv = {recv_rs}; \
         match member_value(&__recv, {method:?}, {m_span})? {{ \
         Some(__field) => {field_call}, \
         None => {{ {none_arm} }}, \
         }} }}"
    ))
}

pub(crate) fn emit_nonvariadic_receiver_pipe_hof_spread_dispatch(
    recv_rs: &str,
    method: &str,
    leading_rs: &str,
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
    member_span: Span,
    call_span: Span,
) -> Result<String, EmitError> {
    let Some(receiver_guard) = receiver_hof_spread_guard(method) else {
        return Err(EmitError::unsupported("receiver HOF spread argument"));
    };
    let m_span = emit_span(member_span);
    let c_span = emit_span(call_span);
    let spread_args = render_pipe_spread_args(leading_rs, args, ctx)?;
    let field_call = spread_args.field_call("__field", &c_span);
    let none_arm = spread_args.arity_fault(&c_span);
    Ok(format!(
        "{{ let __recv = {recv_rs}; \
         match member_value(&__recv, {method:?}, {m_span})? {{ \
         Some(__field) => {field_call}, \
         None => {{ if !({receiver_guard}) {{ check_member_method(&__recv, {method:?}, {m_span})?; }} {none_arm} }}, \
         }} }}"
    ))
}

/// Lower a `Value` callee invocation through the shared runtime call helpers,
/// optionally prepending already-evaluated positional snippets (receiver-method
/// dispatch uses this for `self`). This mirrors the generic function-call path:
/// positional prefix, variadic spread tail, then named arguments.
pub(crate) fn emit_call_value_with_args(
    callee_rs: &str,
    leading_positional: &[&str],
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
    call_span: Span,
) -> Result<String, EmitError> {
    Ok(
        render_call_args(args, ctx, call_span, "call argument shape")?
            .value_call(callee_rs, leading_positional),
    )
}

impl PipeStaticArgs {
    pub(crate) fn slot(&self, index: usize) -> &str {
        &self.values[self.slots[index]]
    }
}

pub(crate) fn pipe_args_contain_placeholder(args: &[CallArg]) -> bool {
    args.iter().any(|arg| match arg {
        CallArg::Positional(expr) | CallArg::Named { value: expr, .. } => {
            contains_placeholder(expr)
        }
        CallArg::Spread(_) => false,
    })
}

pub(crate) fn needs_pipe_static_binding(args: &[CallArg], leading: Option<&str>) -> bool {
    leading.is_some()
        && (args.iter().any(|arg| matches!(arg, CallArg::Named { .. }))
            || pipe_args_contain_placeholder(args))
}

pub(crate) fn bind_pipe_static_args(
    args: &[CallArg],
    params: &[&str],
    leading: Option<&str>,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<PipeStaticArgs, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let has_placeholder = leading.is_some() && pipe_args_contain_placeholder(args);
    let mut slots = vec![None; params.len()];
    let mut values = Vec::new();
    let mut temps = String::new();
    let mut positional = Vec::new();
    let mut named = Vec::new();
    let mut positional_index = 0usize;
    let mut saw_named = false;

    if let Some(lead) = leading
        && !has_placeholder
    {
        if slots.is_empty() {
            return Err(EmitError::unsupported("call argument shape"));
        }
        let value_index = values.len();
        values.push(lead.to_string());
        slots[0] = Some(value_index);
        positional.push(value_index);
        positional_index = 1;
    }

    let mut placeholder_bound = false;
    let mut emit_arg = |expr: &Expr, idx: usize| -> Result<String, EmitError> {
        if matches!(&expr.kind, ExprKind::Placeholder) {
            let Some(lead) = leading else {
                return Err(EmitError::unsupported("placeholder outside pipe stage"));
            };
            return Ok(lead.to_string());
        }
        if contains_placeholder(expr) {
            let Some(lead) = leading else {
                return Err(EmitError::unsupported("placeholder outside pipe stage"));
            };
            let ph = mangle("_");
            if !placeholder_bound {
                temps.push_str(&format!("let {ph} = {lead}.clone(); "));
                placeholder_bound = true;
            }
            let mut scope = locals.to_vec();
            scope.push(("_".to_string(), Bind::Imm));
            let value = emit_expr(expr, src, aliases, &scope, in_loop)?;
            let temp = format!("__a{idx}");
            temps.push_str(&format!("let {temp} = {value}; "));
            return Ok(temp);
        }
        let value = emit_expr(expr, src, aliases, locals, in_loop)?;
        let temp = format!("__a{idx}");
        temps.push_str(&format!("let {temp} = {value}; "));
        Ok(temp)
    };

    for (idx, arg) in args.iter().enumerate() {
        match arg {
            CallArg::Positional(expr) if saw_named => {
                return Err(EmitError::unsupported("call argument shape"));
            }
            CallArg::Positional(expr) => {
                if positional_index >= params.len() || slots[positional_index].is_some() {
                    return Err(EmitError::unsupported("call argument shape"));
                }
                let value = emit_arg(expr, idx)?;
                let value_index = values.len();
                values.push(value);
                slots[positional_index] = Some(value_index);
                positional.push(value_index);
                positional_index += 1;
            }
            CallArg::Named { name, value } => {
                saw_named = true;
                let source_name = text(src, name.span);
                let Some(param_index) = params.iter().position(|param| *param == source_name)
                else {
                    return Err(EmitError::unsupported("call argument shape"));
                };
                if slots[param_index].is_some() {
                    return Err(EmitError::unsupported("call argument shape"));
                }
                let value = emit_arg(value, idx)?;
                let value_index = values.len();
                values.push(value);
                slots[param_index] = Some(value_index);
                named.push((source_name.to_string(), value_index));
            }
            CallArg::Spread(_) => return Err(EmitError::unsupported("call argument shape")),
        }
    }

    let slots = slots
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| EmitError::unsupported("call argument shape"))?;
    Ok(PipeStaticArgs {
        values,
        slots,
        temps,
        positional,
        named,
    })
}

pub(crate) fn render_bound_field_call(bound: &PipeStaticArgs, c_span: &str) -> String {
    let positional = bound
        .positional
        .iter()
        .map(|index| bound.values[*index].as_str())
        .collect::<Vec<_>>()
        .join(", ");
    if bound.named.is_empty() {
        return format!("call_value(__field, vec![{positional}], cx.clone(), {c_span}).await?");
    }
    let named = bound
        .named
        .iter()
        .map(|(name, index)| {
            let value = &bound.values[*index];
            format!("({name:?}.to_string(), {value})")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "call_value_named(__field, vec![{positional}], vec![{named}], cx.clone(), {c_span}).await?"
    )
}

pub(crate) fn render_receiver_pipe_args(
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
    call_span: Span,
) -> Result<(RenderedCallArgs, bool, String), EmitError> {
    if !call_args_contain_placeholder(args) {
        return Ok((
            render_call_args(args, ctx, call_span, "pipe stage argument shape")?,
            false,
            String::new(),
        ));
    }
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let mut scope = locals.to_vec();
    scope.push(("_".to_string(), Bind::Imm));
    let rendered = render_call_args(
        args,
        ExprEmitContext {
            src,
            aliases,
            locals: &scope,
            in_loop,
        },
        call_span,
        "pipe stage argument shape",
    )?;
    Ok((
        rendered,
        true,
        format!("let {} = __piped.clone(); ", mangle("_")),
    ))
}

pub(crate) fn emit_pipe_expr(
    expr: &Expr,
    lhs: &Expr,
    rhs: &PipeRhs,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let lowered = {
        let lhs_rs = emit_expr(lhs, src, aliases, locals, in_loop)?;
        match rhs {
            PipeRhs::Field(field) => {
                let field_name = text(src, field.span);
                if let Some(bound) = try_emit_receiver_member_value(
                    lhs, &lhs_rs, field_name, expr.span, src, locals,
                )? {
                    bound
                } else {
                    format!(
                        "member_value_required(&({lhs_rs}), {field_name:?}, {})?",
                        emit_span(expr.span)
                    )
                }
            }
            PipeRhs::Expr(stage) => match &stage.kind {
                ExprKind::Call { .. } => emit_pipe_call_stage(
                    stage,
                    lhs_rs,
                    ExprEmitContext {
                        src,
                        aliases,
                        locals,
                        in_loop,
                    },
                )?,
                _ => {
                    let stage_rs = emit_expr(stage, src, aliases, locals, in_loop)?;
                    format!(
                        "{{ let __piped = {lhs_rs}; call_value({stage_rs}, vec![__piped], cx.clone(), {}).await? }}",
                        emit_span(expr.span)
                    )
                }
            },
        }
    };
    Ok(lowered)
}

pub(crate) fn try_emit_typed_json_pipe_stage(
    stage: &Expr,
    lhs_rs: &str,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<Option<String>, EmitError> {
    let ExprKind::Call {
        callee,
        args,
        type_args,
    } = &stage.kind
    else {
        return Ok(None);
    };
    let ExprKind::Member { object, field } = &callee.kind else {
        return Ok(None);
    };
    let ExprKind::Ident = &object.kind else {
        return Ok(None);
    };
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    if text(src, object.span) != "JSON"
        || !matches!(text(src, field.span), "parseAs" | "decode")
        || locals.iter().any(|(name, _)| name == "JSON")
    {
        return Ok(None);
    }

    let member = text(src, field.span);
    let ty = type_args.first().ok_or_else(|| {
        let message = if member == "parseAs" {
            "`JSON.parseAs` requires an explicit type argument"
        } else {
            "`JSON.decode` requires an explicit type argument"
        };
        EmitError::unsupported(message).at(stage.span)
    })?;
    let schema = build_schema_emit(ty, aliases, src).map_err(|_| {
        let message = if member == "parseAs" {
            "`JSON.parseAs` type argument is not JSON-decodable"
        } else {
            "`JSON.decode` type argument is not JSON-decodable"
        };
        EmitError::unsupported(message).at(stage.span)
    })?;
    let parameter = if member == "parseAs" { "text" } else { "value" };
    let argument = match args.as_slice() {
        [] => lhs_rs.to_string(),
        [CallArg::Positional(expr)] => emit_expr(expr, src, aliases, locals, in_loop)?,
        [CallArg::Named { name, value }] if text(src, name.span) == parameter => {
            emit_expr(value, src, aliases, locals, in_loop)?
        }
        _ if call_args_contain_placeholder(args) => {
            let mut scope = locals.to_vec();
            scope.push(("_".to_string(), Bind::Imm));
            let [CallArg::Positional(expr)] = args.as_slice() else {
                return Err(
                    EmitError::unsupported("typed JSON pipe placeholder shape").at(stage.span)
                );
            };
            format!(
                "{{ let {} = {lhs_rs}; {} }}",
                mangle("_"),
                emit_expr(expr, src, aliases, &scope, in_loop)?
            )
        }
        _ => {
            let message = if member == "parseAs" {
                "`JSON.parseAs` takes one `text` argument"
            } else {
                "`JSON.decode` takes one `value` argument"
            };
            return Err(EmitError::unsupported(message).at(stage.span));
        }
    };
    let leaf = if member == "parseAs" {
        "builtin_json_parse_as"
    } else {
        "builtin_json_decode"
    };
    Ok(Some(format!(
        "{leaf}({argument}, &({}), {})?",
        render_schema_rust(&schema),
        emit_span(stage.span)
    )))
}

pub(crate) fn try_emit_member_pipe_stage(
    stage: &Expr,
    lhs_rs: &str,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<Option<String>, EmitError> {
    let ExprKind::Call { callee, args, .. } = &stage.kind else {
        return Ok(None);
    };
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    // §11 a BOUND-METHOD stage `x |> recv.m(args)` for every synchronous
    // receiver method or a RESOURCE method
    // (`read`/`write`/`close`) inserts the piped value as the FIRST
    // argument through the SAME bound-method dispatch the direct call
    // uses (`member_value`-first → `call_value` for a record-field
    // closure SHADOW, else the shared leaf: `call_method` for a
    // read-only builtin, `call_resource_method` through the host for a
    // resource). The piped lead is bound first, then the receiver, then
    // the args — matching the interpreter's `schedule_call(recv.m, args,
    // span, Some(lead))`. (`0 |> xs.get()` → `xs.get(0)`.) MUTATING
    // methods (`push`/… — needs the mut-root) and `okOrElse`
    // (lazy bridge) now lower too, mirroring the direct call +
    // optional-pipe dispatch.
    if let ExprKind::Member { object, field } = &callee.kind
        && receiver_builtin_name_shape(text(src, field.span)).is_some_and(|shape| {
            matches!(
                shape.route,
                ReceiverBuiltinRoute::Method | ReceiverBuiltinRoute::Resource
            ) || text(src, field.span) == "okOrElse"
        })
    {
        let method = text(src, field.span);
        let is_resource = is_resource_receiver_method(method);
        let is_mutator = is_call_method_collection_mutator_name(method);
        // §9 a MUTATING stage's mut-root keys on the receiver path root;
        // a root that is NOT a local binding at all is refused (a safe
        // over-refusal). An immutable LOCAL root (`let`/`const`/namespace/
        // fn cell) instead faults GUARD_IMMUTABLE in the `none_inner` below
        // — exactly the direct call + optional pipe.
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
                "pipe mutator on a non-local-rooted receiver",
            ));
        }
        let recv_rs = emit_expr(object, src, aliases, locals, in_loop)?;
        let m_span = emit_span(callee.span);
        let c_span = emit_span(stage.span);
        if is_resource {
            let (rendered, has_placeholder, before_args) =
                render_receiver_pipe_args(args, ctx, stage.span)?;
            let leading_positional: &[&str] = if has_placeholder { &[] } else { &["__piped"] };
            let dispatch = emit_resource_method_dispatch(
                method,
                &rendered,
                leading_positional,
                &before_args,
                &m_span,
                &c_span,
            );
            return Ok(Some(format!(
                "{{ let __piped = {lhs_rs}; let __recv = {recv_rs}; {dispatch} }}"
            )));
        }
        // The `None` (no record-field shadow) leaf, per method class. Callback
        // HOFs ride the dedicated branch below; only the lazy `okOrElse` bridge
        // joins this catalog-routed path.
        if method == "okOrElse" {
            let rendered = render_ok_or_else_args(
                args,
                OkOrElseCallMode::Optional {
                    leading: Some("__piped"),
                },
                ctx,
            )?;
            let field_call = rendered.shadow_call("__f", &c_span);
            let builtin_call = rendered.builtin_arm(&m_span, &c_span);
            return Ok(Some(format!(
                "{{ let __piped = {lhs_rs}; let __recv = {recv_rs}; \
                 match member_value(&__recv, {method:?}, {m_span})? {{ \
                 Some(__f) => {field_call}, \
                 None => {{ {builtin_call} }}, }} }}"
            )));
        }
        let (rendered, has_placeholder, before_args) =
            render_receiver_pipe_args(args, ctx, stage.span)?;
        let leading_positional: &[&str] = if has_placeholder { &[] } else { &["__piped"] };
        let field_call = rendered.value_call("__f", leading_positional);
        let method_call = rendered.method_call(method, leading_positional, &m_span, &c_span);
        let none_inner = if is_mutator {
            // A `mut`/cell (or non-`Ident`, root `None`) root mutates; an
            // immutable LOCAL root faults GUARD_IMMUTABLE after the type gate.
            match root.and_then(|name| lookup_bind(locals, name).map(|b| (name, b))) {
                Some((_, Bind::Mut | Bind::Cell | Bind::TopMutValueCell)) | None => format!(
                    "check_member_method(&__recv, {method:?}, {m_span})?; {before_args}{method_call}"
                ),
                Some((
                    root_name,
                    Bind::Imm
                    | Bind::ImmCell
                    | Bind::TopFnCell
                    | Bind::TopValueCell
                    | Bind::Namespace,
                )) => {
                    let msg = format!(
                        "`{root_name}` is not `let mut`; in-place collection mutation requires a mutable binding (§9)"
                    );
                    format!(
                        "check_member_method(&__recv, {method:?}, {m_span})?; return Err(fault(codes::GUARD_IMMUTABLE, {msg:?}, {m_span}))"
                    )
                }
            }
        } else {
            // get / scalars / okOr → the shared `call_method` leaf.
            format!(
                "check_member_method(&__recv, {method:?}, {m_span})?; {before_args}{method_call}"
            )
        };
        return Ok(Some(format!(
            "{{ let __piped = {lhs_rs}; let __recv = {recv_rs}; \
         match member_value(&__recv, {method:?}, {m_span})? {{ \
         Some(__f) => {{ {before_args}{field_call} }}, \
         None => {{ {none_inner} }}, \
         }} }}"
        )));
    }
    // §11/§12 an OPTIONAL-call stage `x |> r?.f(args)` threads
    // the piped lead through the SAME optional-call dispatch
    // the direct `r?.f(args)` uses: short-circuit None/null,
    // else access `inner.f` and call with the lead as the
    // FIRST arg (`member_value`-first → `call_value` /
    // `call_method`), re-wrapping a `Some` receiver's result.
    // (Mirrors the interpreter's `schedule_call(r?.f, args,
    // span, Some(lead))` → `KOptionalCall`.) A MUTATING method is
    // supported (the mut-root resolves from the path root, on the
    // non-short-circuit branch); a resource method dispatches through
    // the host on the non-short-circuit branch (like a direct stage).
    if let ExprKind::Member { object, field } = &callee.kind {
        let method = text(src, field.span);
        if matches!(
            method,
            "map"
                | "filter"
                | "reduce"
                | "flatMap"
                | "sortedBy"
                | "okOrElse"
                | "mapValues"
                | "update"
                | "sortBy"
                | "retain"
        ) {
            let is_mutator = matches!(method, "update" | "sortBy" | "retain");
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
                    "pipe mutator on a non-local-rooted receiver",
                ));
            }
            let recv_rs = emit_expr(object, src, aliases, locals, in_loop)?;
            let m_span = emit_span(callee.span);
            let c_span = emit_span(stage.span);
            let call_ctx = RenderedCallContext {
                expression: ctx,
                member_span: &m_span,
                call_span: &c_span,
            };
            let dispatch = match method {
                "reduce" => emit_optional_reduce_dispatch(args, Some("__piped"), call_ctx)?,
                "update" => emit_optional_update_dispatch(args, Some("__piped"), root, call_ctx)?,
                _ => emit_optional_one_callback_dispatch(
                    method,
                    args,
                    Some("__piped"),
                    root,
                    call_ctx,
                )?,
            };
            return Ok(Some(format!(
                "{{ let __piped = {lhs_rs}; let __recv = {recv_rs}; {dispatch} }}"
            )));
        }
    }
    Ok(None)
}

pub(crate) fn emit_optional_pipe_stage(
    stage: &Expr,
    lhs_rs: &str,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprKind::Call { callee, args, .. } = &stage.kind else {
        unreachable!("optional pipe helper received a non-call stage");
    };
    let ExprKind::OptionalAccess { object, field } = &callee.kind else {
        unreachable!("optional pipe helper received another callee kind");
    };
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let method = text(src, field.span);
    let is_resource = is_resource_receiver_method(method);
    // §9 a MUTATING optional pipe stage resolves the mut-root from the
    // path root (`mutation_root`); a const/import-rooted one is refused.
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
            "pipe optional mutator on a non-local-rooted receiver",
        ));
    }
    let obj_rs = emit_expr(object, src, aliases, locals, in_loop)?;
    if is_resource {
        let span = emit_span(stage.span);
        let (rendered, has_placeholder, before_args) =
            render_receiver_pipe_args(args, ctx, stage.span)?;
        let leading_positional: &[&str] = if has_placeholder { &[] } else { &["__piped"] };
        let dispatch = emit_resource_method_dispatch(
            method,
            &rendered,
            leading_positional,
            &before_args,
            &span,
            &span,
        );
        let optional = wrap_optional_call_dispatch(&obj_rs, &dispatch);
        return Ok(format!("{{ let __piped = {lhs_rs}; {optional} }}"));
    }
    if matches!(
        method,
        "map"
            | "filter"
            | "reduce"
            | "flatMap"
            | "sortedBy"
            | "okOrElse"
            | "mapValues"
            | "update"
            | "sortBy"
            | "retain"
    ) {
        let span = emit_span(stage.span);
        if args.iter().any(|arg| matches!(arg, CallArg::Spread(_)))
            && matches!(method, "update" | "sortBy" | "retain")
        {
            let dispatch = emit_nonvariadic_receiver_pipe_mutator_spread_dispatch(
                "__recv", method, "__piped", root, args, ctx, stage.span,
            )?;
            return Ok(format!(
                "{{ let __piped = {lhs_rs}; let __obj = {obj_rs}; match __obj {{ \
         Value::None => Value::None, \
         Value::Null => Value::Null, \
         Value::Some(__inner) => {{ let __recv = (*__inner).clone(); wrap_optional({dispatch}) }}, \
         __other => {{ let __recv = __other; {dispatch} }}, }} }}"
            ));
        }
        if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
            let dispatch = emit_nonvariadic_receiver_pipe_hof_spread_dispatch(
                "__recv", method, "__piped", args, ctx, stage.span, stage.span,
            )?;
            return Ok(format!(
                "{{ let __piped = {lhs_rs}; let __obj = {obj_rs}; match __obj {{ \
         Value::None => Value::None, \
         Value::Null => Value::Null, \
         Value::Some(__inner) => {{ let __recv = (*__inner).clone(); wrap_optional({dispatch}) }}, \
         __other => {{ let __recv = __other; {dispatch} }}, }} }}"
            ));
        }
        let call_ctx = RenderedCallContext {
            expression: ctx,
            member_span: &span,
            call_span: &span,
        };
        let dispatch = match method {
            "reduce" => emit_optional_reduce_dispatch(args, Some("__piped"), call_ctx)?,
            "update" => emit_optional_update_dispatch(args, Some("__piped"), root, call_ctx)?,
            _ => {
                emit_optional_one_callback_dispatch(method, args, Some("__piped"), root, call_ctx)?
            }
        };
        return Ok(format!(
            "{{ let __piped = {lhs_rs}; let __obj = {obj_rs}; match __obj {{ \
     Value::None => Value::None, \
     Value::Null => Value::Null, \
     Value::Some(__inner) => {{ let __recv = (*__inner).clone(); wrap_optional({dispatch}) }}, \
     __other => {{ let __recv = __other; {dispatch} }}, }} }}"
        ));
    }
    let span = emit_span(stage.span);
    // Receiver HOFs (map/filter/reduce) are not supported on the OPTIONAL
    // path yet: their lazy/iterable dispatch cannot ride the `call_method`
    // leaf (which would build a binary that faults `Option has no member
    // map`). Refuse honestly rather than mis-build; the full optional
    // `oo?.map(f)` lowering is a follow-up.
    if matches!(
        method,
        "map" | "filter" | "reduce" | "flatMap" | "sortedBy" | "mapValues" | "update"
    // §6 (v5.4) the CALLBACK array mutators — refused on the
    // pipe path like the other HOFs (the inline write-back can't
    // ride the `call_method` leaf).
    | "sortBy" | "retain"
    ) {
        return Err(EmitError::unsupported("optional receiver HOF"));
    }
    let (rendered, has_placeholder, before_args) =
        render_receiver_pipe_args(args, ctx, stage.span)?;
    let leading_positional: &[&str] = if has_placeholder { &[] } else { &["__piped"] };
    let field_call = rendered.value_call("__f", leading_positional);
    let method_call = rendered.method_call(method, leading_positional, &span, &span);
    let none_inner = {
        match is_mutator.then(|| root.and_then(|n| lookup_bind(locals, n).map(|b| (n, b)))) {
            Some(Some((
                root_name,
                Bind::Imm | Bind::ImmCell | Bind::TopFnCell | Bind::Namespace,
            ))) => {
                let msg = format!(
                    "`{root_name}` is not `let mut`; in-place collection mutation requires a mutable binding (§9)"
                );
                format!(
                    "check_member_method(&__recv, {method:?}, {span})?; return Err(fault(codes::GUARD_IMMUTABLE, {msg:?}, {span}))"
                )
            }
            _ => format!(
                "check_member_method(&__recv, {method:?}, {span})?; {before_args}{method_call}"
            ),
        }
    };
    let dispatch = format!(
        "match member_value(&__recv, {method:?}, {span})? {{ \
 Some(__f) => {{ {before_args}{field_call} }}, \
 None => {{ {none_inner} }}, }}"
    );
    Ok(format!(
        "{{ let __piped = {lhs_rs}; let __obj = {obj_rs}; match __obj {{ \
 Value::None => Value::None, \
 Value::Null => Value::Null, \
 Value::Some(__inner) => {{ let __recv = (*__inner).clone(); wrap_optional({dispatch}) }}, \
 __other => {{ let __recv = __other; {dispatch} }}, }} }}"
    ))
}

pub(crate) fn emit_pipe_call_stage(
    stage: &Expr,
    lhs_rs: String,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprKind::Call {
        callee,
        args,
        type_args: _,
    } = &stage.kind
    else {
        unreachable!("pipe-call helper received another expression kind");
    };
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let lowered = {
        if let Some(emitted) = try_emit_typed_json_pipe_stage(stage, &lhs_rs, ctx)? {
            return Ok(emitted);
        }
        if let Some(emitted) = try_emit_member_pipe_stage(stage, &lhs_rs, ctx)? {
            return Ok(emitted);
        }
        if matches!(callee.kind, ExprKind::OptionalAccess { .. }) {
            return emit_optional_pipe_stage(stage, &lhs_rs, ctx);
        }
        // The first-argument insertion lowers the callee to a VALUE through
        // `call_value`. Receiver-bound Method, Callback, and Resource values
        // retain the route recorded by the shared receiver catalog, so the
        // generic path is valid for every first-class member shape.
        // §11: a placeholder `_` in the CALLEE position is a
        // misuse the interpreter faults — refuse it.
        if contains_placeholder(callee) {
            return Err(EmitError::unsupported("pipe placeholder callee"));
        }
        if call_args_contain_placeholder(args) {
            // §11 a PLACEHOLDER stage `x |> f(_, y)`: the piped
            // value is bound to `_` in a child scope and the call
            // runs with NO first-argument insertion (the `_`
            // marks where it goes). Mirrors the interpreter's
            // `KPipe` placeholder branch (`bind("_", lhs)` then
            // `schedule_call(.., None)`). A closure in the stage
            // captures `_` like any enclosing local (so an
            // escaping lambda keeps it).
            let mut scope = locals.to_vec();
            scope.push(("_".to_string(), Bind::Imm));
            let callee_rs = emit_expr(callee, src, aliases, &scope, in_loop)?;
            let ph = mangle("_");
            let call = emit_call_value_with_args(
                &callee_rs,
                &[],
                args,
                ExprEmitContext {
                    src,
                    aliases,
                    locals: &scope,
                    in_loop,
                },
                stage.span,
            )?;
            format!("{{ let {ph} = {lhs_rs}; {call} }}")
        } else {
            // §11 first-argument insertion `x |> f(args)` →
            // `f(x, args)`: the piped value is the first
            // positional, then the explicit positional args.
            let callee_rs = emit_expr(callee, src, aliases, locals, in_loop)?;
            let call = emit_call_value_with_args(&callee_rs, &["__piped"], args, ctx, stage.span)?;
            format!("{{ let __piped = {lhs_rs}; {call} }}")
        }
    };
    Ok(lowered)
}
