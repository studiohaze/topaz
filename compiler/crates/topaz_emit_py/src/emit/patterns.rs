use crate::*;

pub(super) struct IterationPatternBinding {
    pub(super) source_name: String,
    pub(super) py_name: String,
}

pub(super) fn emit_iteration_pattern_bindings(
    item_py: &str,
    pattern: &Pattern,
    span: Span,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<Vec<IterationPatternBinding>, PyEmitError> {
    let (condition, bindings) = emit_pattern_condition(item_py, pattern, ctx)?;
    writeln!(
        out,
        "{}tpz_for_pattern({condition}, {})",
        " ".repeat(indent),
        py_span(span)
    )
    .expect("write to string");
    let mut registered = Vec::with_capacity(bindings.len());
    for binding in &bindings {
        let py_name = ctx.new_binding_py_name(&binding.name);
        write_pattern_binding_assignment(out, indent, &py_name, binding);
        ctx.register_binding(&binding.name, false);
        ctx.set_binding_py_name(&binding.name, py_name.clone());
        registered.push(IterationPatternBinding {
            source_name: binding.name.clone(),
            py_name,
        });
    }
    if ctx.cooperative_yields {
        writeln!(out, "{}yield None", " ".repeat(indent)).expect("write to string");
    }
    Ok(registered)
}

pub(super) fn emit_for_stmt(
    pattern: &Pattern,
    iter: &Expr,
    body: &Block,
    span: Span,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let iter_py = bind_statement_lowered_expr_value(iter, "for_iter", ctx, indent, out)?;
    let item_py = ctx.fresh_temp("for_item");
    writeln!(
        out,
        "{pad}for {item_py} in tpz_for_items({iter_py}, {}):",
        py_span(span)
    )
    .expect("write to string");

    ctx.push_scope();
    let result = (|| -> Result<(), PyEmitError> {
        emit_iteration_pattern_bindings(&item_py, pattern, span, ctx, indent + 4, out)?;
        writeln!(out, "{pad}    try:").expect("write to string");
        ctx.push_loop_frame(LoopFrameKind::Plain);
        let body_result =
            ctx.with_metadata_control_flow(|ctx| emit_block_as_stmt(body, ctx, indent + 8, out));
        ctx.pop_loop_frame();
        body_result?;
        emit_plain_loop_control_handlers(ctx, indent + 4, out);
        Ok(())
    })();
    ctx.pop_scope();
    result
}

pub(super) fn emit_for_expr_to_target(
    pattern: &Pattern,
    iter: &Expr,
    body: &Block,
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
    let inner_pad = " ".repeat(indent + 4);
    let body_pad = " ".repeat(indent + 8);
    let iter_py = bind_statement_lowered_expr_value(iter, "for_iter", ctx, indent, out)?;
    let item_py = ctx.fresh_temp("for_item");
    writeln!(out, "{pad}{target_py} = []").expect("write to string");
    writeln!(
        out,
        "{pad}for {item_py} in tpz_for_items({iter_py}, {}):",
        py_span(span)
    )
    .expect("write to string");

    ctx.push_scope();
    let result = (|| -> Result<(), PyEmitError> {
        let binding_py_names =
            emit_iteration_pattern_bindings(&item_py, pattern, span, ctx, indent + 4, out)?
                .into_iter()
                .map(|binding| binding.py_name)
                .collect::<Vec<_>>();

        let body_fn = ctx.fresh_temp("for_body");
        writeln!(
            out,
            "{inner_pad}def {body_fn}({}):",
            binding_py_names.join(", ")
        )
        .expect("write to string");
        if ctx.cooperative_yields {
            writeln!(out, "{body_pad}if False:").expect("write to string");
            writeln!(out, "{body_pad}    yield None").expect("write to string");
        }
        let nonlocal_py_names = collecting_for_body_nonlocal_py_names(pattern, body, ctx);
        emit_nonlocal_declarations(&nonlocal_py_names, indent + 8, out);
        ctx.with_metadata_control_flow(|ctx| {
            emit_statement_lowered_block_expr_to_target(body, "__tpz_result", ctx, indent + 8, out)
        })?;
        writeln!(out, "{body_pad}return __tpz_result").expect("write to string");
        let call = format!("{body_fn}({})", binding_py_names.join(", "));
        let call = if ctx.cooperative_yields {
            format!("(yield from {call})")
        } else {
            call
        };
        writeln!(out, "{inner_pad}{target_py}.append({call})").expect("write to string");
        Ok(())
    })();
    ctx.pop_scope();
    result
}

pub(super) fn emit_statement_lowered_lambda_to_target(
    params: &[LambdaParam],
    body: &Expr,
    contextual_ty: Option<&Type>,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let body_pad = " ".repeat(indent + 4);
    let try_pad = " ".repeat(indent + 8);
    let lambda_fn = ctx.fresh_temp("lambda_body");
    ctx.push_scope();
    let result = (|| -> Result<(), PyEmitError> {
        let mut param_py_names = Vec::with_capacity(params.len());
        for (index, param) in params.iter().enumerate() {
            let source_name = ctx.text(param.name.span).to_string();
            let py_name = ctx.new_binding_py_name(&source_name);
            register_lambda_parameter_binding(&source_name, param, index, contextual_ty, ctx);
            ctx.set_binding_py_name(&source_name, py_name.clone());
            param_py_names.push(py_name);
        }
        writeln!(out, "{pad}def {lambda_fn}({}):", param_py_names.join(", "))
            .expect("write to string");
        let nonlocal_py_names = lambda_body_nonlocal_py_names(params, body, ctx);
        emit_nonlocal_declarations(&nonlocal_py_names, indent + 4, out);
        writeln!(out, "{body_pad}__tpz_defers = []").expect("write to string");
        emit_defer_helpers(out, indent + 4);
        if ctx.cooperative_yields {
            writeln!(out, "{body_pad}if False:").expect("write to string");
            writeln!(out, "{body_pad}    yield None").expect("write to string");
        }
        writeln!(out, "{body_pad}try:").expect("write to string");
        emit_statement_lowered_expr_to_target(body, "__tpz_result", ctx, indent + 8, out)?;
        writeln!(out, "{body_pad}except TpzReturn as __tpz_return:").expect("write to string");
        writeln!(out, "{try_pad}__tpz_result = __tpz_return.value").expect("write to string");
        writeln!(out, "{body_pad}except TpzFault:").expect("write to string");
        writeln!(out, "{try_pad}raise").expect("write to string");
        writeln!(out, "{body_pad}__tpz_run_defers()").expect("write to string");
        writeln!(out, "{body_pad}return __tpz_result").expect("write to string");
        writeln!(out, "{pad}{target_py} = {lambda_fn}").expect("write to string");
        Ok(())
    })();
    ctx.pop_scope();
    result
}

pub(super) fn emit_statement_lowered_comprehension_to_target(
    kind: CompKind,
    clauses: &[CompClause],
    body: &CompBody,
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
    writeln!(out, "{pad}{target_py} = []").expect("write to string");
    let mut captures = Vec::new();
    emit_statement_lowered_comprehension_clauses(
        kind,
        clauses,
        body,
        span,
        StatementTarget::new(target_py, ctx, indent, out),
        &mut captures,
    )?;
    match kind {
        CompKind::Array => return Ok(()),
        CompKind::Set => {
            writeln!(
                out,
                "{pad}{target_py} = tpz_set_of({target_py}, {})",
                py_span(span)
            )
            .expect("write to string");
        }
        CompKind::Map => {
            writeln!(
                out,
                "{pad}{target_py} = tpz_map_of({target_py}, {})",
                py_span(span)
            )
            .expect("write to string");
        }
    }
    Ok(())
}

pub(super) fn emit_statement_lowered_comprehension_clauses(
    kind: CompKind,
    clauses: &[CompClause],
    body: &CompBody,
    span: Span,
    target: StatementTarget<'_, '_, '_, '_>,
    captures: &mut Vec<(String, String)>,
) -> Result<(), PyEmitError> {
    let StatementTarget {
        target_py,
        ctx,
        indent,
        out,
    } = target;
    let Some((clause, rest)) = clauses.split_first() else {
        return emit_statement_lowered_comprehension_body(
            kind,
            body,
            StatementTarget::new(target_py, ctx, indent, out),
            captures,
        );
    };
    let pad = " ".repeat(indent);
    match clause {
        CompClause::For { pattern, iter } => {
            let iter_py = bind_statement_lowered_expr_value(iter, "comp_iter", ctx, indent, out)?;
            let item_py = ctx.fresh_temp("comp_item");
            writeln!(
                out,
                "{pad}for {item_py} in tpz_for_items({iter_py}, {}):",
                py_span(span)
            )
            .expect("write to string");
            ctx.push_scope();
            let capture_base = captures.len();
            let result = ctx.with_metadata_control_flow(|ctx| {
                let bindings =
                    emit_iteration_pattern_bindings(&item_py, pattern, span, ctx, indent + 4, out)?;
                captures.extend(
                    bindings
                        .into_iter()
                        .map(|binding| (binding.source_name, binding.py_name)),
                );
                emit_statement_lowered_comprehension_clauses(
                    kind,
                    rest,
                    body,
                    span,
                    StatementTarget::new(target_py, ctx, indent + 4, out),
                    captures,
                )
            });
            captures.truncate(capture_base);
            ctx.pop_scope();
            result
        }
        CompClause::If(condition) => {
            let condition_py = emit_statement_lowered_expr_value(condition, ctx, indent, out)?;
            writeln!(
                out,
                "{pad}if tpz_condition({condition_py}, {}):",
                py_span(condition.span)
            )
            .expect("write to string");
            ctx.with_metadata_control_flow(|ctx| {
                emit_statement_lowered_comprehension_clauses(
                    kind,
                    rest,
                    body,
                    span,
                    StatementTarget::new(target_py, ctx, indent + 4, out),
                    captures,
                )
            })
        }
    }
}

pub(super) fn emit_statement_lowered_comprehension_body(
    kind: CompKind,
    body: &CompBody,
    target: StatementTarget<'_, '_, '_, '_>,
    captures: &[(String, String)],
) -> Result<(), PyEmitError> {
    let StatementTarget {
        target_py,
        ctx,
        indent,
        out,
    } = target;
    let pad = " ".repeat(indent);
    let body_pad = " ".repeat(indent + 4);
    let body_fn = ctx.fresh_temp("comp_body");
    let params = captures
        .iter()
        .map(|(_, py_name)| py_name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(out, "{pad}def {body_fn}({params}):").expect("write to string");
    if ctx.cooperative_yields {
        writeln!(out, "{body_pad}if False:").expect("write to string");
        writeln!(out, "{body_pad}    yield None").expect("write to string");
    }
    let nonlocal_py_names = comprehension_body_nonlocal_py_names(body, captures, ctx);
    emit_nonlocal_declarations(&nonlocal_py_names, indent + 4, out);
    let result = match (kind, body) {
        (CompKind::Map, CompBody::Entry { key, value }) => {
            let key_py = emit_statement_lowered_expr_value(key, ctx, indent + 4, out)?;
            let value_py = emit_statement_lowered_map_value(value, ctx, indent + 4, out)?;
            format!("({key_py}, {value_py})")
        }
        (_, CompBody::Elem(value)) => {
            emit_statement_lowered_expr_value(value, ctx, indent + 4, out)?
        }
        _ => unreachable!("comprehension kind/body shape paired by the parser"),
    };
    writeln!(out, "{body_pad}return {result}").expect("write to string");
    let call = format!("{body_fn}({params})");
    let call = if ctx.cooperative_yields {
        format!("(yield from {call})")
    } else {
        call
    };
    writeln!(out, "{pad}{target_py}.append({call})").expect("write to string");
    Ok(())
}

pub(super) fn emit_match_stmt(
    scrutinee: &Expr,
    cases: &[CaseClause],
    span: Span,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    if expr_needs_statement_lowering(scrutinee, ctx)
        || cases
            .iter()
            .any(|case| case_guard_needs_statement_lowering(case, ctx))
    {
        return emit_statement_lowered_guarded_match_stmt(scrutinee, cases, span, ctx, indent, out);
    }
    let pad = " ".repeat(indent);
    let scrutinee_py = emit_expr(scrutinee, ctx)?;
    let tmp = ctx.fresh_temp("match");
    writeln!(out, "{pad}{tmp} = {scrutinee_py}").expect("write to string");
    for (idx, case) in cases.iter().enumerate() {
        let head = if idx == 0 { "if" } else { "elif" };
        let pattern_metadata = pattern_binding_metadata(scrutinee, case, ctx);
        let (condition, bindings) = emit_match_stmt_condition(&tmp, &case.pattern, ctx)?;
        let condition = emit_direct_case_condition_with_guard(
            condition,
            &bindings,
            pattern_metadata.as_ref(),
            case.guard.as_ref(),
            ctx,
        )?;
        writeln!(out, "{pad}{head} {condition}:").expect("write to string");
        ctx.push_scope();
        let result = {
            emit_pattern_bindings_with_metadata(
                &bindings,
                pattern_metadata.as_ref(),
                ctx,
                indent + 4,
                out,
            );
            ctx.with_metadata_control_flow(|ctx| {
                emit_case_arm_body_as_stmt(&case.body, ctx, indent + 4, out)
            })
        };
        ctx.pop_scope();
        result?;
    }
    writeln!(out, "{pad}else:").expect("write to string");
    writeln!(
        out,
        "{}tpz_impossible_match({tmp}, {})",
        " ".repeat(indent + 4),
        py_span(span)
    )
    .expect("write to string");
    Ok(())
}

pub(super) fn emit_statement_lowered_guarded_match_stmt(
    scrutinee: &Expr,
    cases: &[CaseClause],
    span: Span,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let tmp = bind_statement_lowered_expr_value(scrutinee, "match", ctx, indent, out)?;
    let matched = ctx.fresh_temp("match_done");
    writeln!(out, "{pad}{matched} = False").expect("write to string");
    for case in cases {
        writeln!(out, "{pad}if not {matched}:").expect("write to string");
        let pattern_metadata = pattern_binding_metadata(scrutinee, case, ctx);
        emit_statement_lowered_guarded_match_case(
            &tmp,
            case,
            GuardedMatchCase {
                target_py: None,
                matched_py: &matched,
                pattern_metadata: pattern_metadata.as_ref(),
            },
            ctx,
            indent + 4,
            out,
        )?;
    }
    writeln!(out, "{pad}if not {matched}:").expect("write to string");
    writeln!(
        out,
        "{}tpz_impossible_match({tmp}, {})",
        " ".repeat(indent + 4),
        py_span(span)
    )
    .expect("write to string");
    Ok(())
}

#[derive(Clone, Debug)]
pub(super) struct PatternBinding {
    pub(super) name: String,
    pub(super) value_py: String,
    pub(super) bind_if: Option<String>,
}

impl PatternBinding {
    pub(super) fn always(name: impl Into<String>, value_py: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value_py: value_py.into(),
            bind_if: None,
        }
    }

    pub(super) fn when(
        name: impl Into<String>,
        value_py: impl Into<String>,
        bind_if: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            value_py: value_py.into(),
            bind_if: Some(bind_if.into()),
        }
    }
}

pub(super) fn write_pattern_binding_assignment(
    out: &mut String,
    indent: usize,
    target_py: &str,
    binding: &PatternBinding,
) {
    let pad = " ".repeat(indent);
    let assignment = format!(
        "{target_py} = {}  # {}",
        binding.value_py,
        py_comment_name(&binding.name)
    );
    if let Some(bind_if) = &binding.bind_if {
        writeln!(out, "{pad}if {bind_if}:").expect("write to string");
        writeln!(out, "{}{assignment}", " ".repeat(indent + 4)).expect("write to string");
    } else {
        writeln!(out, "{pad}{assignment}").expect("write to string");
    }
}

pub(super) fn emit_match_stmt_condition(
    tmp: &str,
    pattern: &Pattern,
    ctx: &Ctx<'_>,
) -> Result<(String, Vec<PatternBinding>), PyEmitError> {
    emit_pattern_condition(tmp, pattern, ctx)
}

pub(super) fn emit_pattern_condition(
    value_py: &str,
    pattern: &Pattern,
    ctx: &Ctx<'_>,
) -> Result<(String, Vec<PatternBinding>), PyEmitError> {
    match &pattern.kind {
        PatternKind::Wildcard => Ok(("True".to_string(), Vec::new())),
        PatternKind::Binding(binding) => {
            let binding_name = ctx.text(binding.span);
            let enum_owners = enum_owner_names_declaring_variant(ctx, binding_name);
            if enum_owners.is_empty() {
                Ok((
                    "True".to_string(),
                    vec![PatternBinding::always(binding_name, value_py)],
                ))
            } else {
                let owners_py =
                    py_tuple(enum_owners.iter().map(|owner| py_string(owner)).collect());
                Ok((
                    format!(
                        "tpz_enum_bare_variant_matches({value_py}, {owners_py}, {})",
                        py_string(binding_name)
                    ),
                    vec![PatternBinding::when(
                        binding_name,
                        value_py,
                        format!("tpz_enum_bare_variant_binds({value_py}, {owners_py})"),
                    )],
                ))
            }
        }
        PatternKind::Typed { name, ty } => Ok((
            format!(
                "tpz_type_matches({value_py}, {})",
                emit_type_spec_for_typed_pattern(ty, ctx)?
            ),
            vec![PatternBinding::always(ctx.text(name.span), value_py)],
        )),
        PatternKind::Literal(lit) => Ok((
            format!(
                "tpz_eq({value_py}, {}, {})",
                emit_pattern_literal_expr(lit, ctx)?,
                py_span(pattern.span)
            ),
            Vec::new(),
        )),
        PatternKind::Range { lo, hi, inclusive } => Ok((
            emit_range_pattern_condition(value_py, lo, hi, *inclusive, pattern.span, ctx)?,
            Vec::new(),
        )),
        PatternKind::Or(alts) => emit_or_pattern_condition(value_py, alts, pattern.span, ctx),
        PatternKind::Constructor { name, args }
            if ctx.text(name.span) == "Some" && args.len() == 1 =>
        {
            let (subcondition, bindings) =
                emit_pattern_condition(&format!("{value_py}.value"), &args[0], ctx)?;
            Ok((
                format!("(isinstance({value_py}, Some) and {subcondition})"),
                bindings,
            ))
        }
        PatternKind::Constructor { name, args }
            if ctx.text(name.span) == "None" && args.is_empty() =>
        {
            Ok((format!("{value_py} is None"), Vec::new()))
        }
        PatternKind::Constructor { name, args }
            if ctx.text(name.span) == "Ok" && args.len() == 1 =>
        {
            let (subcondition, bindings) =
                emit_pattern_condition(&format!("{value_py}.value"), &args[0], ctx)?;
            Ok((
                format!("(isinstance({value_py}, Ok) and {subcondition})"),
                bindings,
            ))
        }
        PatternKind::Constructor { name, args }
            if ctx.text(name.span) == "Err" && args.len() == 1 =>
        {
            let (subcondition, bindings) =
                emit_pattern_condition(&format!("{value_py}.value"), &args[0], ctx)?;
            Ok((
                format!("(isinstance({value_py}, Err) and {subcondition})"),
                bindings,
            ))
        }
        PatternKind::Constructor { name, args }
            if ctx.newtypes.contains_key(ctx.text(name.span)) =>
        {
            if args.len() != 1 {
                return Err(PyEmitError::unsupported("match pattern").at(pattern.span));
            }
            let newtype = ctx
                .newtypes
                .get(ctx.text(name.span))
                .expect("checked newtype");
            let (subcondition, bindings) =
                emit_pattern_condition(&format!("{value_py}.value"), &args[0], ctx)?;
            Ok((
                format!(
                    "(tpz_is_newtype({value_py}, {}) and {subcondition})",
                    py_string(nominal_declaration_identity(
                        &newtype.source_name,
                        newtype.declaration_identity.as_deref(),
                    ))
                ),
                bindings,
            ))
        }
        PatternKind::Constructor { name, args } => {
            let variant_name = ctx.text(name.span);
            let enum_owners = enum_owner_names_declaring_variant(ctx, variant_name);
            if enum_owners.is_empty() {
                return Err(PyEmitError::unsupported("match pattern").at(pattern.span));
            }
            emit_enum_pattern_condition(value_py, variant_name, args, pattern.span, ctx)
        }
        PatternKind::NominalRecord { name, fields } => {
            emit_nominal_pattern_condition(value_py, name, fields, ctx)
        }
        PatternKind::Record(fields) => emit_record_pattern_condition(value_py, fields, ctx),
        PatternKind::List(elements) => emit_list_pattern_condition(value_py, elements, ctx),
    }
}

pub(super) fn enum_owner_names_declaring_variant(ctx: &Ctx<'_>, variant_name: &str) -> Vec<String> {
    let mut owners = BTreeSet::new();
    for enum_def in ctx.enums.values() {
        if enum_def.variants.contains_key(variant_name) {
            owners.insert(
                nominal_declaration_identity(
                    &enum_def.source_name,
                    enum_def.declaration_identity.as_deref(),
                )
                .to_string(),
            );
        }
    }
    for exports in ctx.namespaces.values() {
        for export in exports.values() {
            if let ModuleRuntimeExport::Enum { enum_def, .. } = export
                && enum_def.variants.contains_key(variant_name)
            {
                owners.insert(
                    nominal_declaration_identity(
                        &enum_def.source_name,
                        enum_def.declaration_identity.as_deref(),
                    )
                    .to_string(),
                );
            }
        }
    }
    owners.into_iter().collect()
}

pub(super) fn emit_enum_pattern_condition(
    value_py: &str,
    variant_name: &str,
    args: &[Pattern],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<(String, Vec<PatternBinding>), PyEmitError> {
    let enum_owners = enum_owner_names_declaring_variant(ctx, variant_name);
    if enum_owners.is_empty() {
        return Err(PyEmitError::unsupported("match pattern").at(span));
    }
    let mut conditions = vec![format!(
        "tpz_enum_pattern({value_py}, {}, {}, {}, {})",
        py_tuple(enum_owners.iter().map(|owner| py_string(owner)).collect()),
        py_string(variant_name),
        args.len(),
        py_span(span)
    )];
    let mut bindings = Vec::new();
    for (idx, arg) in args.iter().enumerate() {
        let access = format!("{value_py}.payloads[{idx}]");
        let (subcondition, subbindings) = emit_pattern_condition(&access, arg, ctx)?;
        conditions.push(subcondition);
        bindings.extend(subbindings);
    }
    Ok((format!("({})", conditions.join(" and ")), bindings))
}

pub(super) fn emit_pattern_literal_expr(expr: &Expr, ctx: &Ctx<'_>) -> Result<String, PyEmitError> {
    match &expr.kind {
        ExprKind::Int | ExprKind::Float | ExprKind::Bool(_) | ExprKind::Null | ExprKind::Unit => {
            emit_expr(expr, ctx)
        }
        ExprKind::String(lit) if lit.tag.is_none() => {
            if lit
                .parts
                .iter()
                .any(|part| matches!(part, StringPart::Interpolation(_)))
            {
                return Err(PyEmitError::unsupported("match pattern").at(expr.span));
            }
            emit_expr(expr, ctx)
        }
        ExprKind::Unary { op, operand }
            if matches!(op, UnaryOp::Plus | UnaryOp::Minus)
                && matches!(operand.kind, ExprKind::Int | ExprKind::Float) =>
        {
            emit_expr(expr, ctx)
        }
        ExprKind::Paren(inner) => emit_pattern_literal_expr(inner, ctx),
        _ => Err(PyEmitError::unsupported("match pattern").at(expr.span)),
    }
}

pub(super) fn emit_range_pattern_condition(
    value_py: &str,
    lo: &Expr,
    hi: &Expr,
    inclusive: bool,
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let lo_n = range_pattern_endpoint(lo, ctx)
        .ok_or_else(|| PyEmitError::unsupported("range pattern endpoint shape").at(span))?;
    let hi_n = range_pattern_endpoint(hi, ctx)
        .ok_or_else(|| PyEmitError::unsupported("range pattern endpoint shape").at(span))?;
    let cmp = if inclusive { "<=" } else { "<" };
    Ok(format!(
        "(type({value_py}) is int and {value_py} >= {lo_n} and {value_py} {cmp} {hi_n})"
    ))
}

pub(super) fn range_pattern_endpoint(expr: &Expr, ctx: &Ctx<'_>) -> Option<i64> {
    match &expr.kind {
        ExprKind::Int => ctx.text(expr.span).parse().ok(),
        ExprKind::Unary {
            op: UnaryOp::Minus,
            operand,
        } if matches!(operand.kind, ExprKind::Int) => {
            ctx.text(operand.span).parse::<i64>().ok().map(|n| -n)
        }
        _ => None,
    }
}
