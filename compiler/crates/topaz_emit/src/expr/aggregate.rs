use crate::*;

pub(crate) fn emit_aggregate_literal_expr(
    expr: &Expr,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    Ok(match &expr.kind {
        ExprKind::Array(elements) => {
            if elements
                .iter()
                .all(|element| matches!(element, ArrayElement::Expr(_)))
            {
                let values = elements
                    .iter()
                    .map(|element| {
                        let ArrayElement::Expr(value) = element else {
                            unreachable!("all elements were checked")
                        };
                        emit_expr(value, src, aliases, locals, in_loop)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                format!("Value::array(vec![{}])", values.join(", "))
            } else {
                let span = emit_span(expr.span);
                let mut body = String::from("let mut __acc = Vec::new(); ");
                for element in elements {
                    match element {
                        ArrayElement::Expr(value) => body.push_str(&format!(
                            "__acc.push({}); ",
                            emit_expr(value, src, aliases, locals, in_loop)?
                        )),
                        ArrayElement::Spread(value) => body.push_str(&format!(
                            "array_spread_extend(&mut __acc, {}, {span})?; ",
                            emit_expr(value, src, aliases, locals, in_loop)?
                        )),
                    }
                }
                format!("{{ {body}Value::array(__acc) }}")
            }
        }
        ExprKind::SetLiteral(elements) => {
            let values = elements
                .iter()
                .map(|value| emit_expr(value, src, aliases, locals, in_loop))
                .collect::<Result<Vec<_>, _>>()?;
            format!(
                "builtin_set_of(vec![{}], {})?",
                values.join(", "),
                emit_span(expr.span)
            )
        }
        ExprKind::MapLiteral(entries) => {
            let mut pairs = Vec::with_capacity(entries.len());
            for (key, value) in entries {
                let key = emit_expr(key, src, aliases, locals, in_loop)?;
                let value = emit_expr(value, src, aliases, locals, in_loop)?;
                pairs.push(format!("({key}, {value})"));
            }
            format!(
                "builtin_map_of(vec![{}], {})?",
                pairs.join(", "),
                emit_span(expr.span)
            )
        }
        ExprKind::RecordLiteral { fields } => {
            let mut pairs = Vec::with_capacity(fields.len());
            for field in fields {
                let name = text(src, field.name.span);
                let value = emit_expr(&field.value, src, aliases, locals, in_loop)?;
                pairs.push(format!("({name:?}.to_string(), {value})"));
            }
            format!("Value::record([{}])", pairs.join(", "))
        }
        _ => unreachable!("aggregate literal route checked by expression dispatch"),
    })
}

pub(crate) fn emit_record_update_expr(
    expr: &Expr,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprKind::RecordUpdate {
        base,
        spread,
        fields,
    } = &expr.kind
    else {
        unreachable!("record-update helper received another expression kind");
    };
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let lowered = {
        // §3 (v5.4) NOMINAL record CONSTRUCTION `User { name: …, age: … }`
        // (optionally with a leading spread `User { ...u, … }`): when `base` is a
        // declared record NAME not shadowed by a local, build the value in
        // DETERMINISTIC order (SPREAD base first, then explicit fields L→R, then
        // missing defaults in decl order) and assemble in DECLARATION order via
        // the shared `Value::nominal_record` leaf — byte-identical to the interp.
        if let ExprKind::Ident = &base.kind {
            let head = text(src, base.span);
            if !locals.iter().any(|(n, _)| n == head)
                && let Some(record_def) = aliases.records.get(head)
            {
                let decl_fields = &record_def.fields;
                let span = emit_span(expr.span);
                let required_fields = decl_fields
                    .iter()
                    .map(|(name, _)| format!("{name:?}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                // VALIDATE the explicit field set at EMIT TIME — an UNKNOWN or
                // DUPLICATE field emits a runtime GUARD fault as the WHOLE value
                // (before evaluating ANY field OR the spread base, so a side
                // effect never runs), byte-identical to the interpreter under
                // `--unchecked` (which validates the same set before any eval).
                let mut seen_names: Vec<&str> = Vec::new();
                for field in fields {
                    let fname = text(src, field.name.span);
                    if !decl_fields.iter().any(|(n, _)| *n == fname) {
                        let msg = format!("record `{head}` has no field `{fname}`");
                        return Ok(format!(
                            "{{ let __v: Value = return Err(fault(codes::GUARD_NO_FIELD, {msg:?}, {span})); __v }}"
                        ));
                    }
                    if seen_names.contains(&fname) {
                        let msg = format!("field `{fname}` is given twice in `{head}`");
                        return Ok(format!(
                            "{{ let __v: Value = return Err(fault(codes::GUARD_ARITY, {msg:?}, {span})); __v }}"
                        ));
                    }
                    seen_names.push(fname);
                }
                let mut stmts: Vec<String> = Vec::new();
                // The SPREAD base (if any) evaluates FIRST and is validated via
                // the shared `nominal_spread_base` leaf (same fault as interp).
                // Its fields seed `__seed`, read per-field with `nominal_field`.
                let has_spread = spread.is_some();
                if let Some(spread) = spread {
                    let sval = emit_expr(spread, src, aliases, locals, in_loop)?;
                    stmts.push(format!("let __spread: Value = {sval};"));
                    stmts.push(match &record_def.declaration_identity {
                        Some(identity) => format!(
                            "let __seed: Vec<(Rc<str>, Value)> = nominal_spread_base_required(__spread, {:?}, Some({identity:?}), &[{required_fields}], {span})?;",
                            record_def.id,
                        ),
                        None => format!(
                            "let __seed: Vec<(Rc<str>, Value)> = nominal_spread_base_required(__spread, {:?}, None, &[{required_fields}], {span})?;",
                            record_def.id,
                        ),
                    });
                }
                // Pre-evaluate explicit field values L→R into `let`s (preserving
                // source eval order), then defaults for still-missing fields in
                // decl order, then assemble the decl-ordered tuple.
                let mut explicit: Vec<&str> = Vec::new();
                for (i, field) in fields.iter().enumerate() {
                    let fname = text(src, field.name.span);
                    let value = emit_expr(&field.value, src, aliases, locals, in_loop)?;
                    stmts.push(format!("let __f{i}: Value = {value};"));
                    explicit.push(fname);
                }
                // For each declared field, find its value source: an explicit
                // slot overrides; else the spread's field (`__seed`); else its
                // default expr — defaults only emitted for fields no explicit AND
                // no spread supplies (a spread supplies EVERY field).
                let mut field_exprs: Vec<(String, String)> = Vec::new();
                let mut next_default = 0usize;
                for (fname, default) in decl_fields {
                    if let Some(idx) = explicit.iter().position(|e| e == fname) {
                        field_exprs.push(((*fname).to_string(), format!("__f{idx}.clone()")));
                    } else if has_spread {
                        // Filled by the checked spread seed. Keep the read fallible:
                        // export adapters accept external `Value`s, so no generated
                        // source path relies on a host-provided field invariant.
                        field_exprs.push((
                            (*fname).to_string(),
                            format!(
                                "nominal_record_field_required(&__seed, {:?}, {fname:?}, {span})?",
                                record_def.id,
                            ),
                        ));
                    } else {
                        // A missing field MUST have a default in a checked
                        // program; emit it as a `let __d{n}` so eval order is
                        // deterministic (decl order, after the explicit fields).
                        match default {
                            Some((default_src, dexpr)) => {
                                let def_module = aliases
                                    .type_ctx
                                    .module(record_def.origin_identity)
                                    .ok_or_else(|| {
                                        EmitError::unsupported(
                                            "imported nominal record reference default",
                                        )
                                        .at(dexpr.span)
                                    })?;
                                let thunk = def_module
                                    .record_defaults
                                    .thunks
                                    .get(record_def.id)
                                    .and_then(|thunks| {
                                        thunks.iter().find(|thunk| thunk.field == *fname)
                                    });
                                let dval = if let Some(thunk) = thunk {
                                    let thunk_value =
                                        if record_def.origin_identity == aliases.identity {
                                            format!(
                                                "top_cell_get(&{}, {:?}, {})?",
                                                thunk.cell,
                                                thunk.label,
                                                emit_span(dexpr.span),
                                            )
                                        } else {
                                            format!(
                                                "member_value_required(&{}, {:?}, {})?",
                                                canonical_module(record_def.origin_identity),
                                                thunk.hidden_field,
                                                emit_span(dexpr.span),
                                            )
                                        };
                                    format!(
                                        "call_value({thunk_value}, vec![], cx.clone(), {}).await?",
                                        emit_span(dexpr.span),
                                    )
                                } else if !imported_nominal_record_default_is_self_contained(dexpr)
                                {
                                    let const_values = &def_module.record_defaults.const_values;
                                    let runtime_refs = def_module
                                        .record_defaults
                                        .runtime_refs
                                        .iter()
                                        .filter(|(_, identity, _)| identity != aliases.identity)
                                        .cloned()
                                        .collect::<Vec<_>>();
                                    let self_runtime_refs =
                                        if record_def.origin_identity == aliases.identity {
                                            def_module
                                                .record_defaults
                                                .self_runtime_refs
                                                .get(record_def.id)
                                                .cloned()
                                                .unwrap_or_default()
                                        } else {
                                            Vec::new()
                                        };
                                    let mut hidden_runtime_refs = if record_def.origin_identity
                                        != aliases.identity
                                    {
                                        def_module
                                            .record_defaults
                                            .self_runtime_refs
                                            .get(record_def.id)
                                            .map(|refs| {
                                                refs.iter()
                                                    .filter(|(local, _)| {
                                                        !def_module
                                                            .runtime_values
                                                            .export_names
                                                            .contains(local.as_str())
                                                    })
                                                    .map(|(local, _)| {
                                                        (
                                                            local.clone(),
                                                            record_def.origin_identity.to_string(),
                                                            hidden_self_runtime_default_field(
                                                                record_def.origin_identity,
                                                                local,
                                                            ),
                                                        )
                                                    })
                                                    .collect::<Vec<_>>()
                                            })
                                            .unwrap_or_default()
                                    } else {
                                        Vec::new()
                                    };
                                    hidden_runtime_refs.extend(
                                        def_module
                                            .record_defaults
                                            .hidden_runtime_refs
                                            .get(record_def.id)
                                            .cloned()
                                            .unwrap_or_default(),
                                    );
                                    emit_nominal_record_reference_default(
                                        dexpr,
                                        default_src,
                                        const_values,
                                        &runtime_refs,
                                        &self_runtime_refs,
                                        &hidden_runtime_refs,
                                    )?
                                } else {
                                    emit_expr(dexpr, default_src, aliases, locals, in_loop)?
                                };
                                stmts.push(format!("let __d{next_default}: Value = {dval};"));
                                field_exprs.push((
                                    (*fname).to_string(),
                                    format!("__d{next_default}.clone()"),
                                ));
                                next_default += 1;
                            }
                            None => {
                                // Under `--unchecked`, fault identically to interp.
                                let msg = format!("record `{head}` is missing field `{fname}`");
                                return Ok(format!(
                                    "{{ let __v: Value = return Err(fault(codes::GUARD_ARITY, {msg:?}, {span})); __v }}"
                                ));
                            }
                        }
                    }
                }
                let pairs: Vec<String> = field_exprs
                    .iter()
                    .map(|(n, v)| format!("(Rc::from({n:?}), {v})"))
                    .collect();
                let constructor = match &record_def.declaration_identity {
                    Some(identity) => {
                        let method_identity = record_def
                            .method_identity
                            .as_ref()
                            .map_or("None::<&str>".to_string(), |method| {
                                format!("Some({method:?})")
                            });
                        format!(
                            "Value::nominal_record_with_identities({:?}, {identity:?}, {method_identity}, vec![{}])",
                            record_def.id,
                            pairs.join(", ")
                        )
                    }
                    None => match &record_def.method_identity {
                        Some(method_identity) => format!(
                            "Value::nominal_record_with_method_identity({:?}, Some({method_identity:?}), vec![{}])",
                            record_def.id,
                            pairs.join(", ")
                        ),
                        None => format!(
                            "Value::nominal_record({:?}, vec![{}])",
                            record_def.id,
                            pairs.join(", ")
                        ),
                    },
                };
                return Ok(format!("{{ {} {constructor} }}", stmts.join(" ")));
            }
        }
        if spread.is_some() {
            let name = text(src, base.span);
            let span = emit_span(expr.span);
            let msg = format!("record spread `...` needs a declared record; `{name}` is not one");
            return Ok(format!(
                "{{ let __v: Value = return Err(fault(codes::GUARD_TYPE, {msg:?}, {span})); __v }}"
            ));
        }
        let base_rs = emit_expr(base, src, aliases, locals, in_loop)?;
        let mut pairs = Vec::with_capacity(fields.len());
        for field in fields {
            let name = text(src, field.name.span);
            let value = emit_expr(&field.value, src, aliases, locals, in_loop)?;
            pairs.push(format!("({name:?}.to_string(), {value})"));
        }
        let span = emit_span(expr.span);
        format!(
            "{{ let __base = record_update_base({base_rs}, {span})?; record_update_merge(__base, vec![{}], {span})? }}",
            pairs.join(", ")
        )
    };
    Ok(lowered)
}
