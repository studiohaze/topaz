use super::*;

pub(crate) fn emit_protocol_static_call(
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
    let protocol = text(src, object.span);
    let method = text(src, field.span);
    let mut arg_rs = Vec::with_capacity(args.len());
    for a in args {
        let CallArg::Positional(e) = a else {
            unreachable!("all-positional checked");
        };
        arg_rs.push(emit_expr(e, src, aliases, locals, in_loop)?);
    }
    let args_rs = arg_rs.join(", ");
    let c_span = emit_span(expr.span);
    let runtime_identity = aliases.runtime_identity();
    let module = if runtime_identity.is_empty() {
        "__entry__"
    } else {
        runtime_identity
    };
    Ok(format!(
        "{{ let __args: Vec<Value> = vec![{args_rs}]; \
             match __args.first().and_then(|__v| __v.nominal_id()).and_then(|__id| __protocol_method_lookup({module:?}, {protocol:?}, __id, {method:?})) {{ \
             Some(__m) => call_value(__m, __args, cx.clone(), {c_span}).await?, \
             None => builtin_protocol_dispatch({protocol:?}, {method:?}, __args, {c_span})?, \
             }} }}"
    ))
}

pub(crate) fn emit_user_receiver_method_call(
    expr: &Expr,
    callee: &Expr,
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
    let recv_rs = emit_expr(object, src, aliases, locals, in_loop)?;
    let m_span = emit_span(callee.span);
    let c_span = emit_span(expr.span);
    let rendered_args = render_call_args(args, ctx, expr.span, "call argument shape")?;
    let method_call = rendered_args.value_call("__m", &["__recv"]);
    let field_call = rendered_args.value_call("__field", &[]);
    let fallback_call = rendered_args.method_call(method, &[], &m_span, &c_span);
    let fallback = format!("check_member_method(&__recv, {method:?}, {m_span})?; {fallback_call}");
    Ok(format!(
        "{{ let __recv = {recv_rs}; \
             match __recv.method_dispatch_id().and_then(|__id| __method_lookup(__id, {method:?})) {{ \
             Some(__m) => {method_call}, \
             None => match member_value(&__recv, {method:?}, {m_span})? {{ \
             Some(__field) => {field_call}, \
             None => {{ {fallback} }}, \
             }}, \
             }} }}"
    ))
}

pub(crate) fn emit_typed_json_call(
    expr: &Expr,
    field: &Ident,
    args: &[CallArg],
    type_args: &[Type],
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let member = text(src, field.span);
    let ty = type_args.first().ok_or_else(|| {
        let msg = if member == "parseAs" {
            "`JSON.parseAs` requires an explicit type argument"
        } else {
            "`JSON.decode` requires an explicit type argument"
        };
        EmitError::unsupported(msg).at(expr.span)
    })?;
    let schema = build_schema_emit(ty, aliases, src).map_err(|_| {
        let msg = if member == "parseAs" {
            "`JSON.parseAs` type argument is not JSON-decodable"
        } else {
            "`JSON.decode` type argument is not JSON-decodable"
        };
        EmitError::unsupported(msg).at(expr.span)
    })?;
    let param = if member == "parseAs" { "text" } else { "value" };
    let e = match args {
        [CallArg::Positional(e)] => e,
        [CallArg::Named { name, value: e }] if text(src, name.span) == param => e,
        _ => {
            let msg = if member == "parseAs" {
                "`JSON.parseAs` takes one `text` argument"
            } else {
                "`JSON.decode` takes one `value` argument"
            };
            return Err(EmitError::unsupported(msg).at(expr.span));
        }
    };
    let v = emit_expr(e, src, aliases, locals, in_loop)?;
    let leaf = if member == "parseAs" {
        "builtin_json_parse_as"
    } else {
        "builtin_json_decode"
    };
    Ok(format!(
        "{leaf}({v}, &({}), {})?",
        render_schema_rust(&schema),
        emit_span(expr.span)
    ))
}

pub(crate) fn emit_module_namespace_call(
    expr: &Expr,
    callee: &Expr,
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
    let object_rs = emit_expr(object, src, aliases, locals, in_loop)?;
    let field_name = text(src, field.span);
    let callee_rs = format!(
        "member_value_required(&({object_rs}), {field_name:?}, {})?",
        emit_span(callee.span)
    );
    emit_call_value_with_args(&callee_rs, &[], args, ctx, expr.span)
}

pub(crate) fn try_emit_enum_constructor_call(
    expr: &Expr,
    object: &Expr,
    field: &Ident,
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<Option<String>, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let ExprKind::Ident = object.kind else {
        return Ok(None);
    };
    let head = text(src, object.span);
    if locals.iter().any(|(name, _)| name == head) {
        return Ok(None);
    }
    let variant = text(src, field.span);
    let Some((definition, &(arity, variant_index))) = aliases
        .enums
        .get(head)
        .and_then(|definition| {
            definition
                .variants
                .get(variant)
                .map(|variant| (definition, variant))
        })
        .filter(|(_, (arity, _))| *arity >= 1)
    else {
        return Ok(None);
    };

    let positional: Vec<&Expr> = args
        .iter()
        .map(|argument| match argument {
            CallArg::Positional(value) => Ok(value),
            _ => Err(EmitError::unsupported("enum payload construction shape")),
        })
        .collect::<Result<_, _>>()?;
    let span = emit_span(expr.span);
    if positional.len() != arity {
        let message = format!(
            "enum variant `{head}.{variant}` takes {arity} payload{}",
            if arity == 1 { "" } else { "s" }
        );
        return Ok(Some(format!(
            "{{ let __v: Value = return Err(fault(codes::GUARD_ARITY, {message:?}, {span})); __v }}"
        )));
    }

    let payloads = positional
        .into_iter()
        .map(|value| emit_expr(value, src, aliases, locals, in_loop))
        .collect::<Result<Vec<_>, _>>()?;
    let method_identity = definition
        .method_identity
        .as_ref()
        .map_or("None".to_string(), |identity| {
            format!("Some(Rc::from({identity:?}))")
        });
    let declaration_identity = definition
        .declaration_identity
        .as_ref()
        .map_or("None".to_string(), |identity| {
            format!("Some(Rc::from({identity:?}))")
        });
    Ok(Some(format!(
        "Value::Enum {{ enum_id: Rc::from({:?}), declaration_identity: {declaration_identity}, method_identity: {method_identity}, variant: Rc::from({variant:?}), variant_index: {variant_index}, payloads: Rc::from(vec![{}]) }}",
        definition.id,
        payloads.join(", ")
    )))
}

pub(crate) fn emit_static_namespace_call(
    expr: &Expr,
    object: &Expr,
    field: &Ident,
    args: &[CallArg],
    builtin: Builtin,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let namespace = text(src, object.span);
    let member_name = text(src, field.span);
    if args.iter().any(|arg| matches!(arg, CallArg::Spread(_)))
        && nonvariadic_namespace_spread_fault_surface(namespace, member_name)
    {
        return emit_nonvariadic_namespace_spread_fault(
            args, src, aliases, locals, in_loop, expr.span,
        );
    }
    if text(src, object.span) == "Test" {
        if positional_after_named(args) || args.iter().any(|a| matches!(a, CallArg::Spread(_))) {
            return Err(EmitError::unsupported("Test namespace call shape"));
        }
        let mut positional_rs = Vec::new();
        let mut named_rs = Vec::new();
        for a in args {
            match a {
                CallArg::Positional(e) => {
                    positional_rs.push(emit_expr(e, src, aliases, locals, in_loop)?);
                }
                CallArg::Named { name: n, value } => {
                    let nm = text(src, n.span);
                    named_rs.push(format!(
                        "({nm:?}.to_string(), {})",
                        emit_expr(value, src, aliases, locals, in_loop)?
                    ));
                }
                CallArg::Spread(_) => unreachable!("spread refused above"),
            }
        }
        return Ok(format!(
            "call_value_named(Value::Builtin {{ kind: Builtin::{builtin:?}, recv: None }}, vec![{}], vec![{}], cx.clone(), {}).await?",
            positional_rs.join(", "),
            named_rs.join(", "),
            emit_span(expr.span)
        ));
    }
    if let Some(spec) = fixed_namespace_spec(namespace, member_name) {
        let span = emit_span(expr.span);
        return emit_fixed_namespace_call(
            args,
            spec.params,
            spec.defaults,
            spec.locate_spread_at_argument,
            ctx,
            expr.span,
            |rendered| match spec.runtime {
                FixedNamespaceRuntime::Shared => {
                    render_fallible_namespace_call(spec.leaf, rendered, &span)
                }
                FixedNamespaceRuntime::Host => format!(
                    "{}(&*cx.host(), {}, {span})?",
                    spec.leaf,
                    rendered.join(", ")
                ),
            },
        );
    }
    let ctor = match (text(src, object.span), text(src, field.span)) {
        ("Array", "of") => Some("array"),
        ("Set", "of") => Some("set"),
        ("Map", "new") => Some("map"),
        ("Map", "ofEntries") => Some("map_entries"),
        ("JSON", "stringify") => Some("json"),
        ("JSON", "parse") => Some("json_parse"),
        _ => None,
    };
    if let Some(kind) = ctor {
        if kind == "map" {
            if !args.is_empty() {
                return Err(EmitError::unsupported("Map.new takes no arguments"));
            }
            return Ok("builtin_map_new()".to_string());
        }
        if kind == "map_entries" {
            let span = emit_span(expr.span);
            return emit_fixed_namespace_call(
                args,
                &["entries"],
                &[],
                true,
                ctx,
                expr.span,
                |rendered| format!("builtin_map_of_entries({}, {span})?", rendered[0]),
            );
        }
        if kind == "json" {
            if positional_after_named(args) {
                return emit_call_arg_order_fault(args, src, aliases, locals, in_loop, expr.span);
            }
            // §22 `JSON.stringify(value)` / `JSON.stringify(value: x)` — one
            // fixed argument, positional OR named (the checker accepts both,
            // so emit must too), through the shared leaf both engines call.
            // The named form must name the parameter `value` — the same
            // name the checker/interpreter bind; accepting any name would
            // silently serialize a mis-named arg under `--unchecked` while
            // the interpreter faults.
            let e = match args {
                [CallArg::Positional(e)] => e,
                [CallArg::Named { name, value: e }] if text(src, name.span) == "value" => e,
                _ => return Err(EmitError::unsupported("constructor argument shape")),
            };
            let v = emit_expr(e, src, aliases, locals, in_loop)?;
            return Ok(format!("builtin_json_stringify({v})"));
        }
        if kind == "json_parse" {
            if positional_after_named(args) {
                return emit_call_arg_order_fault(args, src, aliases, locals, in_loop, expr.span);
            }
            // §22 `JSON.parse(text)` / `JSON.parse(text: x)` — one fixed
            // string argument, positional OR named `text` (the same name the
            // checker/interpreter bind). Through the shared `builtin_json_parse`
            // leaf; `?` propagates the (unchecked-only) non-string fault.
            let e = match args {
                [CallArg::Positional(e)] => e,
                [CallArg::Named { name, value: e }] if text(src, name.span) == "text" => e,
                _ => return Err(EmitError::unsupported("constructor argument shape")),
            };
            let v = emit_expr(e, src, aliases, locals, in_loop)?;
            return Ok(format!(
                "builtin_json_parse({v}, {})?",
                emit_span(expr.span)
            ));
        }
        // §5/§22.2 `Array.of`/`Set.of` are variadic, so a SPREAD argument
        // (`Array.of(...xs)`, `Set.of(a, ...xs, b)`, multiple spreads) is
        // valid — flatten positionals + spreads IN ORDER into one element
        // vec. A spread rides through the shared `call_spread_extend` leaf
        // (the §5 "a spread argument must be an Array" fault — the same leaf
        // the interpreter's call-arg evaluation uses, at the spread span). A
        // purely positional constructor keeps the direct `vec![…]` form.
        let has_spread = args.iter().any(|a| matches!(a, CallArg::Spread(_)));
        if !has_spread {
            let mut elems = Vec::with_capacity(args.len());
            for a in args {
                let CallArg::Positional(e) = a else {
                    return Err(EmitError::unsupported("constructor argument shape"));
                };
                elems.push(emit_expr(e, src, aliases, locals, in_loop)?);
            }
            return Ok(if kind == "array" {
                format!("Value::array(vec![{}])", elems.join(", "))
            } else {
                format!(
                    "builtin_set_of(vec![{}], {})?",
                    elems.join(", "),
                    emit_span(expr.span)
                )
            });
        }
        let mut build = String::from("{ let mut __elems: Vec<Value> = Vec::new(); ");
        for a in args {
            match a {
                CallArg::Positional(e) => {
                    build.push_str(&format!(
                        "__elems.push({}); ",
                        emit_expr(e, src, aliases, locals, in_loop)?
                    ));
                }
                CallArg::Spread(e) => {
                    build.push_str(&format!(
                        "call_spread_extend(&mut __elems, {}, {})?; ",
                        emit_expr(e, src, aliases, locals, in_loop)?,
                        emit_span(e.span)
                    ));
                }
                CallArg::Named { .. } => {
                    return Err(EmitError::unsupported("constructor argument shape"));
                }
            }
        }
        return Ok(if kind == "array" {
            format!("{build}Value::array(__elems) }}")
        } else {
            format!(
                "{build}builtin_set_of(__elems, {})? }}",
                emit_span(expr.span)
            )
        });
    }
    emit_builtin_value_call(&format!("{builtin:?}"), args, ctx, expr.span)
}
