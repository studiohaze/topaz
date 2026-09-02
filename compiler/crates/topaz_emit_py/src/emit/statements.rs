use crate::*;

pub(super) fn emit_return_value(
    value: &Expr,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let tmp = ctx.fresh_temp("return_value");
    if emit_expr_to_target_if_needed(value, &tmp, ctx, indent, out)? {
        writeln!(out, "{pad}raise TpzReturn({tmp})").expect("write to string");
        return Ok(());
    }
    let value_py = emit_expr(value, ctx)?;
    writeln!(out, "{pad}raise TpzReturn({value_py})").expect("write to string");
    Ok(())
}

pub(super) fn emit_stmt(
    stmt: &Stmt,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    match &stmt.kind {
        StmtKind::Let {
            mutable,
            pattern,
            ty,
            value,
        } => {
            if matches!(
                &pattern.kind,
                PatternKind::Binding(_) | PatternKind::Typed { .. }
            ) {
                let name = ctx.binding_name(pattern)?;
                let annotation = ty.as_ref().or_else(|| pattern_type(pattern));
                emit_value_binding(
                    ValueBindingInput {
                        source_name: name,
                        mutable: *mutable,
                        value,
                        annotation,
                        runtime_guard: (!*mutable).then(|| pattern_type(pattern)).flatten(),
                        span: stmt.span,
                    },
                    ValueBindingStorage::Local,
                    ctx,
                    indent,
                    out,
                )?;
            } else {
                emit_destructuring_let(pattern, *mutable, value, stmt.span, ctx, indent, out)?;
            }
        }
        StmtKind::Assign { target, op, value } => {
            emit_assign(target, *op, value, stmt.span, ctx, indent, out)?
        }
        StmtKind::While { cond, body } => emit_while_stmt(cond, body, ctx, indent, out)?,
        StmtKind::Return(value) => match value {
            Some(value) => emit_return_value(value, ctx, indent, out)?,
            None => {
                writeln!(out, "{pad}raise TpzReturn(TPZ_UNIT)").expect("write to string");
            }
        },
        StmtKind::Defer(value) => {
            if expr_needs_statement_lowering(value, ctx) {
                let action = ctx.fresh_temp("defer_action");
                writeln!(out, "{pad}def {action}():").expect("write to string");
                emit_nonlocal_declarations(
                    &expression_nonlocal_py_names(value, ctx),
                    indent + 4,
                    out,
                );
                let result = ctx.fresh_temp("defer_result");
                ctx.push_scope();
                let emit_result = ctx.with_cooperative_yields(false, |ctx| {
                    emit_statement_lowered_expr_to_target(value, &result, ctx, indent + 4, out)
                });
                ctx.pop_scope();
                emit_result?;
                writeln!(out, "{pad}    return {result}").expect("write to string");
                writeln!(out, "{pad}__tpz_defers.append({action})").expect("write to string");
            } else {
                let value_py = emit_expr(value, ctx)?;
                writeln!(out, "{pad}__tpz_defers.append(lambda: {value_py})")
                    .expect("write to string");
            }
        }
        StmtKind::Break { label, value } => {
            emit_break_stmt(*label, value.as_ref(), stmt.span, ctx, indent, out)?
        }
        StmtKind::Continue { label } => emit_continue_stmt(*label, stmt.span, ctx, indent, out)?,
        StmtKind::Expr(expr) => emit_expr_stmt(expr, ctx, indent, out)?,
        StmtKind::Function(decl) => emit_nested_function(decl, ctx, indent, out)?,
        StmtKind::Import(_) => {
            return Err(PyEmitError::unsupported("import statement").at(stmt.span));
        }
        StmtKind::Export(_) => {
            return Err(PyEmitError::unsupported("export statement").at(stmt.span));
        }
        StmtKind::TypeAlias(_) => {}
        StmtKind::Const { name, ty, value } => {
            let source_name = ctx.text(name.span).to_string();
            emit_value_binding(
                ValueBindingInput {
                    source_name: &source_name,
                    mutable: false,
                    value,
                    annotation: ty.as_ref(),
                    runtime_guard: None,
                    span: stmt.span,
                },
                ValueBindingStorage::Local,
                ctx,
                indent,
                out,
            )?;
        }
        StmtKind::Enum(_) => {}
        StmtKind::Record(_) => {
            return Err(PyEmitError::unsupported("record declaration statement").at(stmt.span));
        }
        StmtKind::Newtype(_) => {}
        StmtKind::Impl(_) => {
            return Err(PyEmitError::unsupported("impl declaration").at(stmt.span));
        }
        StmtKind::Protocol(_) => {
            return Err(PyEmitError::unsupported("protocol declaration").at(stmt.span));
        }
        StmtKind::Using { name, value, body } => {
            emit_using_stmt(*name, value, body, stmt.span, ctx, indent, out)?
        }
    }
    note_collection_storage_mutations_in_stmt(stmt, ctx);
    Ok(())
}

pub(super) fn emit_break_stmt(
    label: Option<Ident>,
    value: Option<&Expr>,
    span: Span,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    if ctx.innermost_loop_frame().is_none() {
        return Err(PyEmitError::unsupported("break outside loop").at(span));
    }
    let label_py = match label {
        Some(label) => py_string(ctx.text(label.span)),
        None => "None".to_string(),
    };
    let value_py = match value {
        Some(value) => {
            if expr_needs_statement_lowering(value, ctx) {
                emit_statement_lowered_expr_value(value, ctx, indent, out)?
            } else {
                emit_expr(value, ctx)?
            }
        }
        None => "TPZ_UNIT".to_string(),
    };
    writeln!(out, "{pad}raise TpzLoopBreak({label_py}, {value_py})").expect("write to string");
    Ok(())
}

pub(super) fn emit_continue_stmt(
    label: Option<Ident>,
    span: Span,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    if ctx.innermost_loop_frame().is_none() {
        return Err(PyEmitError::unsupported("continue outside loop").at(span));
    }
    let label_py = match label {
        Some(label) => py_string(ctx.text(label.span)),
        None => "None".to_string(),
    };
    writeln!(out, "{pad}raise TpzLoopContinue({label_py})").expect("write to string");
    Ok(())
}

pub(super) fn emit_while_stmt(
    cond: &Expr,
    body: &Block,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    if expr_needs_statement_lowering(cond, ctx) {
        writeln!(out, "{pad}while True:").expect("write to string");
        let cond_py = ctx.fresh_temp("while_condition");
        emit_statement_lowered_expr_to_target(cond, &cond_py, ctx, indent + 4, out)?;
        writeln!(
            out,
            "{pad}    if not tpz_condition({cond_py}, {}):",
            py_span(cond.span)
        )
        .expect("write to string");
        writeln!(out, "{pad}        break").expect("write to string");
    } else {
        let cond_py = emit_expr(cond, ctx)?;
        writeln!(
            out,
            "{pad}while tpz_condition({cond_py}, {}):",
            py_span(cond.span)
        )
        .expect("write to string");
    }
    if ctx.cooperative_yields {
        writeln!(out, "{pad}    yield None").expect("write to string");
    }
    writeln!(out, "{pad}    try:").expect("write to string");
    ctx.push_loop_frame(LoopFrameKind::Plain);
    let result =
        ctx.with_metadata_control_flow(|ctx| emit_block_as_stmt(body, ctx, indent + 8, out));
    ctx.pop_loop_frame();
    result?;
    emit_plain_loop_control_handlers(ctx, indent + 4, out);
    Ok(())
}

pub(super) fn emit_plain_loop_control_handlers(ctx: &mut Ctx<'_>, indent: usize, out: &mut String) {
    let pad = " ".repeat(indent);
    let continue_var = ctx.fresh_temp("loop_continue");
    let break_var = ctx.fresh_temp("loop_break");
    writeln!(out, "{pad}except TpzLoopContinue as {continue_var}:").expect("write to string");
    writeln!(out, "{pad}    if {continue_var}.label is None:").expect("write to string");
    writeln!(out, "{pad}        continue").expect("write to string");
    writeln!(out, "{pad}    raise").expect("write to string");
    writeln!(out, "{pad}except TpzLoopBreak as {break_var}:").expect("write to string");
    writeln!(out, "{pad}    if {break_var}.label is None:").expect("write to string");
    writeln!(out, "{pad}        break").expect("write to string");
    writeln!(out, "{pad}    raise").expect("write to string");
}

pub(super) fn emit_destructuring_let(
    pattern: &Pattern,
    mutable: bool,
    value: &Expr,
    span: Span,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let bindings =
        emit_destructuring_let_pattern_guard(pattern, mutable, value, span, ctx, indent, out)?;
    finalize_destructuring_bindings(
        bindings,
        DestructuringBindingStorage::Local,
        ctx,
        indent,
        out,
    );
    Ok(())
}

pub(super) fn emit_destructuring_let_pattern_guard(
    pattern: &Pattern,
    mutable: bool,
    value: &Expr,
    span: Span,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<Vec<PatternBinding>, PyEmitError> {
    if mutable {
        return Err(PyEmitError::unsupported("mutable destructuring let").at(pattern.span));
    }
    let pad = " ".repeat(indent);
    let value_py = bind_statement_lowered_expr_value(value, "let_value", ctx, indent, out)?;
    let (condition, bindings) = emit_pattern_condition(&value_py, pattern, ctx)?;
    writeln!(out, "{pad}tpz_let_pattern({condition}, {})", py_span(span)).expect("write to string");
    let mut seen = BTreeSet::new();
    if bindings
        .iter()
        .any(|binding| !seen.insert(binding.name.as_str()))
    {
        return Err(PyEmitError::unsupported("binding pattern").at(pattern.span));
    }
    Ok(bindings)
}

pub(super) enum DestructuringBindingStorage<'a> {
    Local,
    Global(&'a dyn Fn(&str) -> String),
}

pub(super) fn finalize_destructuring_bindings(
    bindings: Vec<PatternBinding>,
    storage: DestructuringBindingStorage<'_>,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Vec<(String, String)> {
    let global = matches!(storage, DestructuringBindingStorage::Global(_));
    let mut registered = Vec::with_capacity(bindings.len());
    for binding in bindings {
        let py_name = match &storage {
            DestructuringBindingStorage::Local => ctx.new_binding_py_name(&binding.name),
            DestructuringBindingStorage::Global(py_name_for) => py_name_for(&binding.name),
        };
        let target_py = if global {
            format!("globals()[{}]", py_string(&py_name))
        } else {
            ctx.new_binding_target_py_name(&py_name)
        };
        write_pattern_binding_assignment(out, indent, &target_py, &binding);
        ctx.register_binding(&binding.name, false);
        ctx.set_binding_py_name(&binding.name, py_name.clone());
        registered.push((binding.name, py_name));
    }
    registered
}

pub(super) fn emit_global_destructuring_let(
    pattern: &Pattern,
    mutable: bool,
    value: &Expr,
    span: Span,
    emission: StatementEmission<'_, '_, '_>,
    py_name_for: impl Fn(&str) -> String,
) -> Result<Vec<(String, String)>, PyEmitError> {
    let StatementEmission { ctx, indent, out } = emission;
    let bindings =
        emit_destructuring_let_pattern_guard(pattern, mutable, value, span, ctx, indent, out)?;
    let registered = finalize_destructuring_bindings(
        bindings,
        DestructuringBindingStorage::Global(&py_name_for),
        ctx,
        indent,
        out,
    );
    note_collection_storage_mutations_in_expr(value, ctx);
    Ok(registered)
}

pub(super) struct RecordAssignmentField {
    pub(super) py_name: String,
    pub(super) source_name: String,
}

pub(super) enum RecordAssignmentRoot<'a> {
    Binding(String),
    Cell { base: &'a Expr, index: &'a Expr },
}

pub(super) struct RecordAssignmentPath<'a> {
    pub(super) root: RecordAssignmentRoot<'a>,
    pub(super) fields: Vec<RecordAssignmentField>,
}

pub(super) fn record_assignment_path<'a>(
    target: &'a Expr,
    ctx: &Ctx<'_>,
) -> Option<RecordAssignmentPath<'a>> {
    let mut fields = Vec::new();
    let mut cursor = target;
    loop {
        match &cursor.kind {
            ExprKind::Member { object, field } => {
                let source_name = ctx.text(field.span).to_string();
                fields.push(RecordAssignmentField {
                    py_name: mangle(&source_name),
                    source_name,
                });
                cursor = object;
            }
            ExprKind::Ident if !fields.is_empty() => {
                fields.reverse();
                return Some(RecordAssignmentPath {
                    root: RecordAssignmentRoot::Binding(ctx.text(cursor.span).to_string()),
                    fields,
                });
            }
            ExprKind::Index { object, index } if !fields.is_empty() => {
                fields.reverse();
                return Some(RecordAssignmentPath {
                    root: RecordAssignmentRoot::Cell {
                        base: object,
                        index,
                    },
                    fields,
                });
            }
            _ => return None,
        }
    }
}

pub(super) fn emit_record_path_read(
    root_py: &str,
    fields: &[RecordAssignmentField],
    span: Span,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> String {
    let pad = " ".repeat(indent);
    let mut current = root_py.to_string();
    for field in fields {
        let next = ctx.fresh_temp("record_path_value");
        writeln!(
            out,
            "{pad}{next} = tpz_member({current}, {}, {}, {})",
            py_string(&field.py_name),
            py_string(&field.source_name),
            py_span(span)
        )
        .expect("write to string");
        current = next;
    }
    current
}

pub(super) fn emit_record_path_rebuild(
    root_py: &str,
    fields: &[RecordAssignmentField],
    value_py: &str,
    span: Span,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> String {
    let pad = " ".repeat(indent);
    let root = ctx.fresh_temp("record_path_root");
    writeln!(out, "{pad}{root} = {root_py}").expect("write to string");
    let mut parents = vec![root.clone()];
    let mut current = root;
    for field in fields.iter().take(fields.len() - 1) {
        let next = ctx.fresh_temp("record_path_parent");
        writeln!(
            out,
            "{pad}{next} = tpz_member({current}, {}, {}, {})",
            py_string(&field.py_name),
            py_string(&field.source_name),
            py_span(span)
        )
        .expect("write to string");
        current = next.clone();
        parents.push(next);
    }

    let mut updated = value_py.to_string();
    for index in (0..fields.len()).rev() {
        let field = &fields[index];
        let next = ctx.fresh_temp("record_path_update");
        writeln!(
            out,
            "{pad}{next} = tpz_record_update({}, [({}, {}, lambda: {updated})], {})",
            parents[index],
            py_string(&field.py_name),
            py_string(&field.source_name),
            py_span(span)
        )
        .expect("write to string");
        updated = next;
    }
    updated
}

pub(super) enum PathAssignmentAdmission {
    WritableExpression,
    WritableBinding(String),
    ImmutableFaultEmitted,
}

pub(super) fn emit_path_assignment_admission(
    target: &Expr,
    span: Span,
    ctx: &Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> PathAssignmentAdmission {
    let Some(root_name) = mutation_root_name(target, ctx) else {
        return PathAssignmentAdmission::WritableExpression;
    };
    if ctx.binding_is_mutable(root_name) {
        return PathAssignmentAdmission::WritableBinding(root_name.to_string());
    }
    writeln!(
        out,
        "{}tpz_immutable_assignment({}, {})",
        " ".repeat(indent),
        py_string(root_name),
        py_span(span)
    )
    .expect("write to string");
    PathAssignmentAdmission::ImmutableFaultEmitted
}

pub(super) fn emit_record_cell_path_assign(
    base: &Expr,
    index: &Expr,
    fields: &[RecordAssignmentField],
    op: AssignOp,
    value: &Expr,
    span: Span,
    emission: StatementEmission<'_, '_, '_>,
) -> Result<(), PyEmitError> {
    let StatementEmission { ctx, indent, out } = emission;
    let pad = " ".repeat(indent);
    let mutation_root = match emit_path_assignment_admission(base, span, ctx, indent, out) {
        PathAssignmentAdmission::WritableExpression => None,
        PathAssignmentAdmission::WritableBinding(root_name) => Some(root_name),
        PathAssignmentAdmission::ImmutableFaultEmitted => return Ok(()),
    };

    let base_py = bind_statement_lowered_expr_value(base, "cell_path_base", ctx, indent, out)?;
    let index_py = bind_statement_lowered_expr_value(index, "cell_path_index", ctx, indent, out)?;
    let resolve_slot = |ctx: &mut Ctx<'_>, out: &mut String, indent: usize| {
        let slot = ctx.fresh_temp("cell_path_slot");
        writeln!(
            out,
            "{}{slot} = tpz_index_slot({base_py}, {index_py}, {})",
            " ".repeat(indent),
            py_span(span)
        )
        .expect("write to string");
        slot
    };
    let read_cell = |slot: &str, ctx: &mut Ctx<'_>, out: &mut String, indent: usize| {
        let cell = ctx.fresh_temp("cell_path_record");
        writeln!(
            out,
            "{}{cell} = tpz_index_slot_get({slot})",
            " ".repeat(indent)
        )
        .expect("write to string");
        cell
    };

    match op {
        AssignOp::Assign => {
            resolve_slot(ctx, out, indent);
            let rhs = emit_assignment_rhs_value(
                value,
                AssignmentRhsTiming::BeforeTargetReread("cell_path_rhs"),
                ctx,
                indent,
                out,
            )?;
            let slot = resolve_slot(ctx, out, indent);
            let cell = read_cell(&slot, ctx, out, indent);
            let updated = emit_record_path_rebuild(&cell, fields, &rhs, span, ctx, indent, out);
            writeln!(out, "{pad}tpz_index_slot_set({slot}, {updated})").expect("write to string");
        }
        AssignOp::Coalesce => {
            let slot = resolve_slot(ctx, out, indent);
            let cell = read_cell(&slot, ctx, out, indent);
            let current = emit_record_path_read(&cell, fields, span, ctx, indent, out);
            writeln!(out, "{pad}if {current} is None or {current} is TPZ_NULL:")
                .expect("write to string");
            let rhs = emit_assignment_rhs_value(
                value,
                AssignmentRhsTiming::BeforeTargetReread("cell_path_rhs"),
                ctx,
                indent + 4,
                out,
            )?;
            let slot = resolve_slot(ctx, out, indent + 4);
            let cell = read_cell(&slot, ctx, out, indent + 4);
            let updated = emit_record_path_rebuild(&cell, fields, &rhs, span, ctx, indent + 4, out);
            writeln!(out, "{pad}    tpz_index_slot_set({slot}, {updated})")
                .expect("write to string");
        }
        AssignOp::Add | AssignOp::Sub | AssignOp::Mul | AssignOp::Div | AssignOp::Rem => {
            let slot = resolve_slot(ctx, out, indent);
            let cell = read_cell(&slot, ctx, out, indent);
            let current = emit_record_path_read(&cell, fields, span, ctx, indent, out);
            let next = emit_compound_assignment_value(
                op,
                &current,
                value,
                span,
                "cell_path_next",
                StatementEmission::new(ctx, indent, out),
            )?;
            let slot = resolve_slot(ctx, out, indent);
            let cell = read_cell(&slot, ctx, out, indent);
            let updated = emit_record_path_rebuild(&cell, fields, &next, span, ctx, indent, out);
            writeln!(out, "{pad}tpz_index_slot_set({slot}, {updated})").expect("write to string");
        }
    }
    finalize_assignment_metadata(
        AssignmentMetadataTarget::RecordCell {
            root: mutation_root.as_deref(),
            base,
            index,
            fields,
        },
        op,
        value,
        ctx,
    );
    Ok(())
}

pub(super) fn emit_assign(
    target: &Expr,
    op: AssignOp,
    value: &Expr,
    span: Span,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    match &target.kind {
        ExprKind::Ident => emit_identifier_assignment(target, op, value, span, ctx, indent, out),
        ExprKind::Index { object, index } => emit_index_assignment(
            target,
            object,
            index,
            op,
            value,
            span,
            StatementEmission::new(ctx, indent, out),
        ),
        ExprKind::Member { .. } => {
            emit_record_assignment(target, op, value, span, ctx, indent, out)
        }
        _ => Err(PyEmitError::unsupported("assignment target").at(target.span)),
    }
}

pub(super) fn emit_identifier_assignment(
    target: &Expr,
    op: AssignOp,
    value: &Expr,
    span: Span,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let name = ctx.text(target.span);
    if !ctx.binding_is_mutable(name) {
        return Err(PyEmitError::unsupported("assign to immutable").at(target.span));
    }
    let pad = " ".repeat(indent);
    let target_py = ctx.assignment_target_py_name(name);
    match op {
        AssignOp::Assign => {
            let rhs =
                emit_assignment_rhs_value(value, AssignmentRhsTiming::OnWrite, ctx, indent, out)?;
            writeln!(out, "{pad}{target_py} = {rhs}").expect("write to string");
        }
        AssignOp::Coalesce => {
            writeln!(
                out,
                "{pad}if {target_py} is None or {target_py} is TPZ_NULL:"
            )
            .expect("write to string");
            let rhs = emit_assignment_rhs_value(
                value,
                AssignmentRhsTiming::OnWrite,
                ctx,
                indent + 4,
                out,
            )?;
            writeln!(out, "{pad}    {target_py} = {rhs}").expect("write to string");
        }
        AssignOp::Add | AssignOp::Sub | AssignOp::Mul | AssignOp::Div | AssignOp::Rem => {
            let current = ctx.fresh_temp("assign_current");
            writeln!(out, "{pad}{current} = {target_py}").expect("write to string");
            let next = emit_compound_assignment_value(
                op,
                &current,
                value,
                span,
                "assign_next",
                StatementEmission::new(ctx, indent, out),
            )?;
            writeln!(out, "{pad}{target_py} = {next}").expect("write to string");
        }
    }
    finalize_assignment_metadata(AssignmentMetadataTarget::Binding { name }, op, value, ctx);
    Ok(())
}

pub(super) fn emit_index_assignment(
    target: &Expr,
    object: &Expr,
    index: &Expr,
    op: AssignOp,
    value: &Expr,
    span: Span,
    emission: StatementEmission<'_, '_, '_>,
) -> Result<(), PyEmitError> {
    let StatementEmission { ctx, indent, out } = emission;
    let root = match emit_path_assignment_admission(target, span, ctx, indent, out) {
        PathAssignmentAdmission::WritableExpression => None,
        PathAssignmentAdmission::WritableBinding(root_name) => Some(root_name),
        PathAssignmentAdmission::ImmutableFaultEmitted => return Ok(()),
    };
    let pad = " ".repeat(indent);
    let slot = ctx.fresh_temp("index_slot");
    let object_py = bind_statement_lowered_expr_value(object, "index_object", ctx, indent, out)?;
    let index_py = bind_statement_lowered_expr_value(index, "index_value", ctx, indent, out)?;
    writeln!(
        out,
        "{pad}{slot} = tpz_index_slot({object_py}, {index_py}, {})",
        py_span(span)
    )
    .expect("write to string");
    match op {
        AssignOp::Assign => {
            let rhs =
                emit_assignment_rhs_value(value, AssignmentRhsTiming::OnWrite, ctx, indent, out)?;
            writeln!(out, "{pad}tpz_index_slot_set({slot}, {rhs})").expect("write to string");
        }
        AssignOp::Coalesce => {
            writeln!(out, "{pad}if tpz_index_slot_is_empty({slot}):").expect("write to string");
            let rhs = emit_assignment_rhs_value(
                value,
                AssignmentRhsTiming::OnWrite,
                ctx,
                indent + 4,
                out,
            )?;
            writeln!(out, "{pad}    tpz_index_slot_set({slot}, {rhs})").expect("write to string");
        }
        AssignOp::Add | AssignOp::Sub | AssignOp::Mul | AssignOp::Div | AssignOp::Rem => {
            let current = ctx.fresh_temp("index_current");
            writeln!(out, "{pad}{current} = tpz_index_slot_get({slot})").expect("write to string");
            let next = emit_compound_assignment_value(
                op,
                &current,
                value,
                span,
                "index_next",
                StatementEmission::new(ctx, indent, out),
            )?;
            writeln!(out, "{pad}tpz_index_slot_set({slot}, {next})").expect("write to string");
        }
    }
    finalize_assignment_metadata(
        AssignmentMetadataTarget::Index {
            root: root.as_deref(),
            target,
        },
        op,
        value,
        ctx,
    );
    Ok(())
}

pub(super) fn emit_record_assignment(
    target: &Expr,
    op: AssignOp,
    value: &Expr,
    span: Span,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let Some(path) = record_assignment_path(target, ctx) else {
        return Err(PyEmitError::unsupported("assignment target").at(target.span));
    };
    let RecordAssignmentPath { root, fields } = path;
    let root_name = match root {
        RecordAssignmentRoot::Binding(root_name) => root_name,
        RecordAssignmentRoot::Cell { base, index } => {
            return emit_record_cell_path_assign(
                base,
                index,
                &fields,
                op,
                value,
                span,
                StatementEmission::new(ctx, indent, out),
            );
        }
    };
    match emit_path_assignment_admission(target, span, ctx, indent, out) {
        PathAssignmentAdmission::WritableBinding(_) => {}
        PathAssignmentAdmission::ImmutableFaultEmitted => return Ok(()),
        PathAssignmentAdmission::WritableExpression => {
            return Err(PyEmitError::unsupported("assignment target").at(target.span));
        }
    }
    let pad = " ".repeat(indent);
    let root_py = ctx.assignment_target_py_name(&root_name);
    match op {
        AssignOp::Assign => {
            let rhs = emit_assignment_rhs_value(
                value,
                AssignmentRhsTiming::BeforeTargetReread("record_rhs"),
                ctx,
                indent,
                out,
            )?;
            let updated = emit_record_path_rebuild(&root_py, &fields, &rhs, span, ctx, indent, out);
            writeln!(out, "{pad}{root_py} = {updated}").expect("write to string");
        }
        AssignOp::Coalesce => {
            let current = emit_record_path_read(&root_py, &fields, span, ctx, indent, out);
            writeln!(out, "{pad}if {current} is None or {current} is TPZ_NULL:")
                .expect("write to string");
            let rhs = emit_assignment_rhs_value(
                value,
                AssignmentRhsTiming::BeforeTargetReread("record_rhs"),
                ctx,
                indent + 4,
                out,
            )?;
            let updated =
                emit_record_path_rebuild(&root_py, &fields, &rhs, span, ctx, indent + 4, out);
            writeln!(out, "{pad}    {root_py} = {updated}").expect("write to string");
        }
        AssignOp::Add | AssignOp::Sub | AssignOp::Mul | AssignOp::Div | AssignOp::Rem => {
            let current = emit_record_path_read(&root_py, &fields, span, ctx, indent, out);
            let next = emit_compound_assignment_value(
                op,
                &current,
                value,
                span,
                "record_next",
                StatementEmission::new(ctx, indent, out),
            )?;
            let updated =
                emit_record_path_rebuild(&root_py, &fields, &next, span, ctx, indent, out);
            writeln!(out, "{pad}{root_py} = {updated}").expect("write to string");
        }
    }
    finalize_assignment_metadata(
        AssignmentMetadataTarget::Record {
            root: &root_name,
            fields: &fields,
        },
        op,
        value,
        ctx,
    );
    Ok(())
}

pub(super) fn clear_binding_value_metadata(ctx: &mut Ctx<'_>, source_name: &str) {
    if let Some((_, info)) = ctx.binding_lookup_mut(source_name) {
        clear_binding_info_value_metadata(info);
    }
}

pub(super) fn clear_binding_info_value_metadata(info: &mut BindingInfo) {
    info.cooperative_callback_py_name = None;
    info.cooperative_callback_needs_host = false;
    info.callable_params = None;
    info.mutated_collection_params.clear();
    info.callable_params_flow_allowed = false;
    info.composed = false;
    info.array_elements.clear_observations();
    info.record_descendants = RecordDescendantCatalog::default();
    info.map_value.observed_by_key.clear();
    info.map_value.known_present_keys.clear();
    info.map_value.observed_keys_complete = false;
    info.namespace_member_value_metadata = false;
}

pub(super) enum AssignmentMetadataTarget<'a> {
    Binding {
        name: &'a str,
    },
    Index {
        root: Option<&'a str>,
        target: &'a Expr,
    },
    Record {
        root: &'a str,
        fields: &'a [RecordAssignmentField],
    },
    RecordCell {
        root: Option<&'a str>,
        base: &'a Expr,
        index: &'a Expr,
        fields: &'a [RecordAssignmentField],
    },
}

pub(super) fn finalize_assignment_metadata(
    target: AssignmentMetadataTarget<'_>,
    op: AssignOp,
    value: &Expr,
    ctx: &mut Ctx<'_>,
) {
    let root = match &target {
        AssignmentMetadataTarget::Binding { name } => Some(*name),
        AssignmentMetadataTarget::Index { root, .. }
        | AssignmentMetadataTarget::RecordCell { root, .. } => *root,
        AssignmentMetadataTarget::Record { root, .. } => Some(*root),
    };
    let Some(root) = root else {
        return;
    };

    if op != AssignOp::Assign || !ctx.current_scope_contains(root) || ctx.in_metadata_control_flow()
    {
        clear_binding_value_metadata(ctx, root);
        return;
    }

    match target {
        AssignmentMetadataTarget::Binding { .. } => {
            refresh_binding_value_metadata_from_value(ctx, root, value);
        }
        AssignmentMetadataTarget::Index { target, .. } => {
            if let Some((static_root, static_index)) =
                static_array_index_assignment_target(target, ctx)
                && static_root == root
            {
                update_static_array_index_value_metadata_from_value(ctx, root, static_index, value);
            } else {
                clear_binding_value_metadata(ctx, root);
            }
        }
        AssignmentMetadataTarget::Record { fields, .. } => {
            let field_path = fields
                .iter()
                .map(|field| field.source_name.as_str())
                .collect::<Vec<_>>()
                .join(".");
            update_record_path_value_metadata_from_value(ctx, root, &field_path, value);
        }
        AssignmentMetadataTarget::RecordCell {
            base,
            index,
            fields,
            ..
        } => {
            if let Some((static_root, static_index)) = static_array_index_target(base, index, ctx)
                && static_root == root
            {
                let field_path = fields
                    .iter()
                    .map(|field| field.source_name.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                update_static_array_element_record_path_metadata_from_value(
                    ctx,
                    root,
                    static_index,
                    &field_path,
                    value,
                );
            } else {
                clear_binding_value_metadata(ctx, root);
            }
        }
    }
}

pub(super) fn refresh_binding_value_metadata_from_value(
    ctx: &mut Ctx<'_>,
    source_name: &str,
    value: &Expr,
) {
    let collection_storage_identity = ctx
        .collection_storage_identity_for_binding_value(value, ctx.binding_is_mutable(source_name));
    ctx.refresh_array_element_observations_from_value(source_name, value);
    ctx.refresh_record_value_metadata_from_value(source_name, value);
    ctx.refresh_callable_value_metadata_from_value(source_name, value);
    ctx.refresh_map_value_observations_from_value(source_name, value);
    if let Some((_, info)) = ctx.binding_lookup_mut(source_name) {
        info.collection_storage_identity = collection_storage_identity;
    }
}

pub(super) fn update_static_array_index_value_metadata_from_value(
    ctx: &mut Ctx<'_>,
    source_name: &str,
    index: usize,
    value: &Expr,
) {
    ctx.clear_callable_value_metadata(source_name);
    ctx.clear_record_value_metadata(source_name);
    ctx.clear_map_value_observations(source_name);
    ctx.update_array_element_observation_from_value(source_name, index, value);
}

pub(super) fn update_record_path_value_metadata_from_value(
    ctx: &mut Ctx<'_>,
    source_name: &str,
    field_path: &str,
    value: &Expr,
) {
    ctx.clear_callable_value_metadata(source_name);
    ctx.clear_array_element_observations(source_name);
    ctx.clear_map_value_observations(source_name);
    ctx.update_record_path_metadata_from_value(source_name, field_path, value);
}

pub(super) fn update_static_array_element_record_path_metadata_from_value(
    ctx: &mut Ctx<'_>,
    source_name: &str,
    index: usize,
    field_path: &str,
    value: &Expr,
) {
    ctx.clear_callable_value_metadata(source_name);
    ctx.clear_record_value_metadata(source_name);
    ctx.clear_map_value_observations(source_name);
    ctx.update_array_element_record_path_metadata_from_value(source_name, index, field_path, value);
}

pub(super) fn emit_assignment_general_binary_call(
    op: AssignOp,
    lhs_py: &str,
    rhs_py: &str,
    span: Span,
) -> Result<String, PyEmitError> {
    let leaf = match op {
        AssignOp::Add => "tpz_add",
        AssignOp::Sub => "tpz_sub",
        AssignOp::Mul => "tpz_mul",
        AssignOp::Div => "tpz_div",
        AssignOp::Rem => "tpz_rem_trunc_i64",
        AssignOp::Assign | AssignOp::Coalesce => {
            return Err(PyEmitError::unsupported("compound assignment").at(span));
        }
    };
    Ok(format!("{leaf}({lhs_py}, {rhs_py}, {})", py_span(span)))
}

pub(super) enum AssignmentRhsTiming<'a> {
    OnWrite,
    BeforeTargetReread(&'a str),
}

pub(super) fn emit_assignment_rhs_value(
    value: &Expr,
    timing: AssignmentRhsTiming<'_>,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<String, PyEmitError> {
    match timing {
        AssignmentRhsTiming::OnWrite => emit_statement_lowered_expr_value(value, ctx, indent, out),
        AssignmentRhsTiming::BeforeTargetReread(tmp_hint) => {
            bind_statement_lowered_expr_value(value, tmp_hint, ctx, indent, out)
        }
    }
}

pub(super) fn emit_compound_assignment_value(
    op: AssignOp,
    current_py: &str,
    value: &Expr,
    span: Span,
    next_hint: &str,
    emission: StatementEmission<'_, '_, '_>,
) -> Result<String, PyEmitError> {
    let StatementEmission { ctx, indent, out } = emission;
    let next = ctx.fresh_temp(next_hint);
    let rhs = emit_assignment_rhs_value(value, AssignmentRhsTiming::OnWrite, ctx, indent, out)?;
    let next_py = emit_assignment_general_binary_call(op, current_py, &rhs, span)?;
    writeln!(out, "{}{next} = {next_py}", " ".repeat(indent)).expect("write to string");
    Ok(next)
}

pub(super) fn mutation_root_name<'a>(expr: &Expr, ctx: &Ctx<'a>) -> Option<&'a str> {
    match &expr.kind {
        ExprKind::Ident => Some(ctx.text(expr.span)),
        ExprKind::Paren(inner) => mutation_root_name(inner, ctx),
        ExprKind::Member { object, .. } | ExprKind::Index { object, .. } => {
            mutation_root_name(object, ctx)
        }
        _ => None,
    }
}

pub(super) fn static_array_index_assignment_target<'a>(
    target: &Expr,
    ctx: &Ctx<'a>,
) -> Option<(&'a str, usize)> {
    match &target.kind {
        ExprKind::Index { object, index } => static_array_index_target(object, index, ctx),
        ExprKind::Paren(inner) => static_array_index_assignment_target(inner, ctx),
        _ => None,
    }
}

pub(super) fn static_array_index_target<'a>(
    object: &Expr,
    index: &Expr,
    ctx: &Ctx<'a>,
) -> Option<(&'a str, usize)> {
    match &object.kind {
        ExprKind::Ident => Some((ctx.text(object.span), ctx.static_usize_index(index)?)),
        ExprKind::Paren(inner) => static_array_index_target(inner, index, ctx),
        _ => None,
    }
}

pub(super) fn array_mutating_method(method: &str) -> bool {
    matches!(
        method,
        "clear" | "insert" | "pop" | "push" | "removeAt" | "retain" | "reverse" | "sort" | "sortBy"
    )
}

pub(super) fn map_mutating_method(method: &str) -> bool {
    matches!(method, "clear" | "insert" | "remove" | "update")
}

pub(super) fn static_call_argument_values<'a>(
    args: &'a [CallArg],
    params: &[&str],
    ctx: &Ctx<'_>,
) -> Option<Vec<&'a Expr>> {
    let mut values = vec![None; params.len()];
    let mut positional_index = 0;
    let mut saw_named = false;
    for arg in args {
        let (index, value) = match arg {
            CallArg::Positional(value) if !saw_named => {
                let index = positional_index;
                positional_index += 1;
                (index, value)
            }
            CallArg::Named { name, value } => {
                saw_named = true;
                (
                    params
                        .iter()
                        .position(|param| *param == ctx.text(name.span))?,
                    value,
                )
            }
            CallArg::Positional(_) | CallArg::Spread(_) => return None,
        };
        let slot = values.get_mut(index)?;
        if slot.replace(value).is_some() {
            return None;
        }
    }
    values.into_iter().collect()
}

pub(super) enum CollectionMutationMetadataEffect<'a> {
    Array {
        source_name: String,
        mutation: ArrayObservationMutation,
    },
    RefreshMapEntry {
        source_name: String,
        key: String,
        value: &'a Expr,
    },
    RemoveMapEntry {
        source_name: String,
        static_key: Option<String>,
    },
    ClearMap {
        source_name: String,
    },
    UpdateMapEntry {
        source_name: String,
        key: String,
        initial: &'a Expr,
        callback: &'a Expr,
    },
    Invalidate {
        source_name: String,
    },
}

pub(super) fn collection_mutation_metadata_effect<'a>(
    callee: &Expr,
    args: &'a [CallArg],
    ctx: &Ctx<'_>,
) -> Option<CollectionMutationMetadataEffect<'a>> {
    let source_name = collection_mutation_root_for_call(callee, ctx)?;
    let ExprKind::Member { object, field } = &callee.kind else {
        return Some(CollectionMutationMetadataEffect::Invalidate { source_name });
    };
    let method = ctx.text(field.span);
    if receiver_is_array_value(object, ctx) {
        let mutation = match method {
            "clear" => Some(ArrayObservationMutation::Clear),
            "push" => static_call_argument_values(args, &["x"], ctx).map(|values| {
                ArrayObservationMutation::Push(static_array_element_metadata_for_value(
                    values[0], ctx,
                ))
            }),
            "insert" => {
                static_call_argument_values(args, &["index", "value"], ctx).and_then(|values| {
                    Some(ArrayObservationMutation::Insert {
                        index: ctx.static_usize_index(values[0])?,
                        metadata: static_array_element_metadata_for_value(values[1], ctx),
                    })
                })
            }
            "pop" => Some(ArrayObservationMutation::Pop),
            "removeAt" => static_call_argument_values(args, &["index"], ctx)
                .and_then(|values| ctx.static_usize_index(values[0]))
                .map(ArrayObservationMutation::RemoveAt),
            "reverse" => Some(ArrayObservationMutation::Reverse),
            "sort" | "sortBy" => Some(ArrayObservationMutation::Reorder),
            "retain" => Some(ArrayObservationMutation::Retain),
            _ => None,
        };
        return Some(match mutation {
            Some(mutation) => CollectionMutationMetadataEffect::Array {
                source_name,
                mutation,
            },
            None => CollectionMutationMetadataEffect::Invalidate { source_name },
        });
    }
    if !receiver_is_map_value(object, ctx) {
        return Some(CollectionMutationMetadataEffect::Invalidate { source_name });
    }
    match method {
        "insert" => {
            let values = static_call_argument_values(args, &["k", "v"], ctx);
            let static_update = values.and_then(|values| {
                static_string_literal_value(values[0], ctx).map(|key| (key, values[1]))
            });
            match static_update {
                Some((key, value)) => Some(CollectionMutationMetadataEffect::RefreshMapEntry {
                    source_name,
                    key,
                    value,
                }),
                None => Some(CollectionMutationMetadataEffect::Invalidate { source_name }),
            }
        }
        "remove" => {
            let static_key = static_call_argument_values(args, &["k"], ctx)
                .and_then(|values| static_string_literal_value(values[0], ctx));
            Some(CollectionMutationMetadataEffect::RemoveMapEntry {
                source_name,
                static_key,
            })
        }
        "clear" => Some(CollectionMutationMetadataEffect::ClearMap { source_name }),
        "update" => {
            let values = static_call_argument_values(args, &["k", "initial", "f"], ctx);
            let static_update = values.and_then(|values| {
                static_string_literal_value(values[0], ctx).map(|key| (key, values[1], values[2]))
            });
            match static_update {
                Some((key, initial, callback)) => {
                    Some(CollectionMutationMetadataEffect::UpdateMapEntry {
                        source_name,
                        key,
                        initial,
                        callback,
                    })
                }
                None => Some(CollectionMutationMetadataEffect::Invalidate { source_name }),
            }
        }
        _ => Some(CollectionMutationMetadataEffect::Invalidate { source_name }),
    }
}

pub(super) fn apply_collection_mutation_metadata(
    callee: &Expr,
    args: &[CallArg],
    ctx: &mut Ctx<'_>,
) {
    match collection_mutation_metadata_effect(callee, args, ctx) {
        Some(CollectionMutationMetadataEffect::Array {
            source_name,
            mutation,
        }) => ctx.apply_collection_alias_array_observation_mutation(&source_name, mutation),
        Some(CollectionMutationMetadataEffect::RefreshMapEntry {
            source_name,
            key,
            value,
        }) => ctx.update_collection_alias_map_value_metadata(&source_name, key, value),
        Some(CollectionMutationMetadataEffect::RemoveMapEntry {
            source_name,
            static_key,
        }) => {
            if let Some(key) = static_key {
                ctx.remove_collection_alias_map_value_metadata(&source_name, &key);
            } else {
                ctx.clear_collection_alias_map_known_presence(&source_name);
            }
        }
        Some(CollectionMutationMetadataEffect::ClearMap { source_name }) => {
            ctx.clear_collection_alias_map_value_metadata(&source_name);
        }
        Some(CollectionMutationMetadataEffect::UpdateMapEntry {
            source_name,
            key,
            initial,
            callback,
        }) => ctx.update_collection_alias_map_value_metadata_from_update(
            &source_name,
            key,
            initial,
            callback,
        ),
        Some(CollectionMutationMetadataEffect::Invalidate { source_name }) => {
            ctx.clear_collection_alias_value_metadata(&source_name);
        }
        None => {}
    }
}

pub(super) fn collection_mutation_root_for_call(callee: &Expr, ctx: &Ctx<'_>) -> Option<String> {
    match &callee.kind {
        ExprKind::Member { object, field } => {
            let method = ctx.text(field.span);
            collection_mutation_root_for_receiver(object, method, ctx)
        }
        ExprKind::Paren(inner) => collection_mutation_root_for_call(inner, ctx),
        _ => None,
    }
}

pub(super) fn collection_mutation_receiver_source_name(
    callee: &Expr,
    map: &SourceMap,
) -> Option<String> {
    match &callee.kind {
        ExprKind::Member { object, field } => {
            let method = text_in_map(map, field.span);
            (array_mutating_method(method) || map_mutating_method(method))
                .then(|| direct_source_identifier_name(object, map))?
        }
        ExprKind::Paren(inner) => collection_mutation_receiver_source_name(inner, map),
        _ => None,
    }
}

pub(super) fn collection_mutation_root_for_receiver(
    receiver: &Expr,
    method: &str,
    ctx: &Ctx<'_>,
) -> Option<String> {
    match &receiver.kind {
        ExprKind::Ident => {
            let name = ctx.text(receiver.span);
            let mutates_bound_collection = (ctx.binding_is_array(name)
                && array_mutating_method(method))
                || (ctx.binding_is_map(name) && map_mutating_method(method));
            if ctx.binding_is_mutable(name) && mutates_bound_collection {
                Some(name.to_string())
            } else {
                None
            }
        }
        ExprKind::Paren(inner) => collection_mutation_root_for_receiver(inner, method, ctx),
        _ => None,
    }
}

pub(super) fn note_collection_storage_mutations_in_stmt(stmt: &Stmt, ctx: &mut Ctx<'_>) {
    match &stmt.kind {
        StmtKind::Export(inner) => note_collection_storage_mutations_in_stmt(inner, ctx),
        StmtKind::Let { value, .. }
        | StmtKind::Const { value, .. }
        | StmtKind::Return(Some(value))
        | StmtKind::Break {
            value: Some(value), ..
        }
        | StmtKind::Expr(value) => {
            note_collection_storage_mutations_in_expr(value, ctx);
        }
        StmtKind::Assign { target, value, .. } => {
            note_collection_storage_mutations_in_expr(target, ctx);
            note_collection_storage_mutations_in_expr(value, ctx);
        }
        StmtKind::Using { value, body, .. } => {
            note_collection_storage_mutations_in_expr(value, ctx);
            note_collection_storage_mutations_in_block(body, ctx);
        }
        StmtKind::While { cond, body } => {
            note_collection_storage_mutations_in_expr(cond, ctx);
            note_collection_storage_mutations_in_block(body, ctx);
        }
        StmtKind::Import(_)
        | StmtKind::Function(_)
        | StmtKind::TypeAlias(_)
        | StmtKind::Enum(_)
        | StmtKind::Record(_)
        | StmtKind::Newtype(_)
        | StmtKind::Impl(_)
        | StmtKind::Protocol(_)
        | StmtKind::Return(None)
        | StmtKind::Defer(_)
        | StmtKind::Break { value: None, .. }
        | StmtKind::Continue { .. } => {}
    }
}

pub(super) fn note_collection_storage_mutations_in_block(block: &Block, ctx: &mut Ctx<'_>) {
    for stmt in &block.stmts {
        note_collection_storage_mutations_in_stmt(stmt, ctx);
    }
    if let Some(tail) = block.tail.as_deref() {
        note_collection_storage_mutations_in_expr(tail, ctx);
    }
}

pub(super) fn note_collection_storage_mutations_in_expr(expr: &Expr, ctx: &mut Ctx<'_>) {
    if expr_needs_statement_lowering(expr, ctx) {
        return;
    }
    match &expr.kind {
        ExprKind::Paren(inner) | ExprKind::Try(inner) => {
            note_collection_storage_mutations_in_expr(inner, ctx);
        }
        ExprKind::Block(block) => note_collection_storage_mutations_in_block(block, ctx),
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            note_collection_storage_mutations_in_expr(cond, ctx);
            note_collection_storage_mutations_in_block(then_block, ctx);
            if let Some(else_branch) = else_branch.as_deref() {
                note_collection_storage_mutations_in_expr(else_branch, ctx);
            }
        }
        ExprKind::Match { scrutinee, cases } => {
            note_collection_storage_mutations_in_expr(scrutinee, ctx);
            for case in cases {
                if let Some(guard) = &case.guard {
                    note_collection_storage_mutations_in_expr(guard, ctx);
                }
                match &case.body {
                    CaseArmBody::Expr(expr) => note_collection_storage_mutations_in_expr(expr, ctx),
                    CaseArmBody::Return { value, .. } => {
                        if let Some(value) = value {
                            note_collection_storage_mutations_in_expr(value, ctx);
                        }
                    }
                }
            }
        }
        ExprKind::For { iter, body, .. } => {
            note_collection_storage_mutations_in_expr(iter, ctx);
            note_collection_storage_mutations_in_block(body, ctx);
        }
        ExprKind::Loop { body, .. } => note_collection_storage_mutations_in_block(body, ctx),
        ExprKind::Concurrent {
            timeout,
            arms,
            else_block,
        } => {
            if let Some(timeout) = timeout.as_deref() {
                note_collection_storage_mutations_in_expr(timeout, ctx);
            }
            for arm in arms {
                note_collection_storage_mutations_in_expr(&arm.value, ctx);
            }
            if let Some(else_block) = else_block.as_deref() {
                note_collection_storage_mutations_in_block(else_block, ctx);
            }
        }
        ExprKind::Call { callee, args, .. } => {
            note_collection_storage_mutations_in_expr(callee, ctx);
            for arg in args {
                match arg {
                    CallArg::Positional(expr) | CallArg::Spread(expr) => {
                        note_collection_storage_mutations_in_expr(expr, ctx);
                    }
                    CallArg::Named { value, .. } => {
                        note_collection_storage_mutations_in_expr(value, ctx);
                    }
                }
            }
            apply_collection_mutation_metadata(callee, args, ctx);
        }
        ExprKind::Member { object, .. } | ExprKind::OptionalAccess { object, .. } => {
            note_collection_storage_mutations_in_expr(object, ctx);
        }
        ExprKind::Index { object, index } => {
            note_collection_storage_mutations_in_expr(object, ctx);
            note_collection_storage_mutations_in_expr(index, ctx);
        }
        ExprKind::Unary { operand, .. } => note_collection_storage_mutations_in_expr(operand, ctx),
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Compose { lhs, rhs } => {
            note_collection_storage_mutations_in_expr(lhs, ctx);
            note_collection_storage_mutations_in_expr(rhs, ctx);
        }
        ExprKind::Pipe { lhs, rhs } => {
            note_collection_storage_mutations_in_expr(lhs, ctx);
            if let PipeRhs::Expr(stage) = rhs.as_ref() {
                note_collection_storage_mutations_in_expr(stage, ctx);
            }
        }
        ExprKind::Range { lo, hi, step, .. } => {
            note_collection_storage_mutations_in_expr(lo, ctx);
            note_collection_storage_mutations_in_expr(hi, ctx);
            if let Some(step) = step.as_deref() {
                note_collection_storage_mutations_in_expr(step, ctx);
            }
        }
        ExprKind::Lambda { .. } => {}
        ExprKind::RecordLiteral { fields } => {
            for field in fields {
                note_collection_storage_mutations_in_expr(&field.value, ctx);
            }
        }
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            note_collection_storage_mutations_in_expr(base, ctx);
            if let Some(spread) = spread.as_deref() {
                note_collection_storage_mutations_in_expr(spread, ctx);
            }
            for field in fields {
                note_collection_storage_mutations_in_expr(&field.value, ctx);
            }
        }
        ExprKind::Array(elements) => {
            for element in elements {
                match element {
                    ArrayElement::Expr(expr) | ArrayElement::Spread(expr) => {
                        note_collection_storage_mutations_in_expr(expr, ctx);
                    }
                }
            }
        }
        ExprKind::SetLiteral(elements) => {
            for element in elements {
                note_collection_storage_mutations_in_expr(element, ctx);
            }
        }
        ExprKind::MapLiteral(entries) => {
            for (key, value) in entries {
                note_collection_storage_mutations_in_expr(key, ctx);
                note_collection_storage_mutations_in_expr(value, ctx);
            }
        }
        ExprKind::Comprehension { clauses, body, .. } => {
            for clause in clauses {
                match clause {
                    CompClause::For { iter, .. } => {
                        note_collection_storage_mutations_in_expr(iter, ctx);
                    }
                    CompClause::If(cond) => note_collection_storage_mutations_in_expr(cond, ctx),
                }
            }
            match body.as_ref() {
                CompBody::Elem(value) => note_collection_storage_mutations_in_expr(value, ctx),
                CompBody::Entry { key, value } => {
                    note_collection_storage_mutations_in_expr(key, ctx);
                    note_collection_storage_mutations_in_expr(value, ctx);
                }
            }
        }
        ExprKind::String(lit) => {
            for part in &lit.parts {
                if let StringPart::Interpolation(value) = part {
                    note_collection_storage_mutations_in_expr(value, ctx);
                }
            }
        }
        ExprKind::Int
        | ExprKind::Float
        | ExprKind::Duration(_)
        | ExprKind::Bool(_)
        | ExprKind::Null
        | ExprKind::Unit
        | ExprKind::Ident
        | ExprKind::Placeholder => {}
    }
}

pub(super) fn emit_expr_stmt(
    expr: &Expr,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    match &expr.kind {
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => emit_if_stmt(cond, then_block, else_branch.as_deref(), ctx, indent, out),
        ExprKind::For {
            pattern,
            iter,
            body,
        } => emit_for_stmt(pattern, iter, body, expr.span, ctx, indent, out),
        ExprKind::Match { scrutinee, cases } => {
            emit_match_stmt(scrutinee, cases, expr.span, ctx, indent, out)
        }
        ExprKind::Loop { .. } | ExprKind::Concurrent { .. } => {
            let tmp = ctx.fresh_temp("stmt_value");
            emit_expr_to_target_if_needed(expr, &tmp, ctx, indent, out).map(|_| ())
        }
        _ if expr_needs_statement_lowering(expr, ctx) => {
            let tmp = ctx.fresh_temp("stmt_value");
            emit_expr_to_target_if_needed(expr, &tmp, ctx, indent, out).map(|_| ())
        }
        _ => {
            let pad = " ".repeat(indent);
            let expr_py = emit_expr(expr, ctx)?;
            writeln!(out, "{pad}{expr_py}").expect("write to string");
            Ok(())
        }
    }
}

pub(super) fn emit_if_stmt(
    cond: &Expr,
    then_block: &Block,
    else_branch: Option<&Expr>,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let cond_py = emit_statement_lowered_expr_value(cond, ctx, indent, out)?;
    writeln!(
        out,
        "{pad}if tpz_condition({cond_py}, {}):",
        py_span(cond.span)
    )
    .expect("write to string");
    ctx.with_metadata_control_flow(|ctx| emit_block_as_stmt(then_block, ctx, indent + 4, out))?;
    if let Some(branch) = else_branch {
        writeln!(out, "{pad}else:").expect("write to string");
        ctx.with_metadata_control_flow(|ctx| match &branch.kind {
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => emit_if_stmt(
                cond,
                then_block,
                else_branch.as_deref(),
                ctx,
                indent + 4,
                out,
            ),
            ExprKind::Block(block) => emit_block_as_stmt(block, ctx, indent + 4, out),
            _ => emit_expr_stmt(branch, ctx, indent + 4, out),
        })?;
    }
    Ok(())
}

pub(super) fn emit_using_stmt(
    name: Ident,
    value: &Expr,
    body: &Block,
    span: Span,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let body_pad = " ".repeat(indent + 4);
    let source_name = ctx.text(name.span).to_string();
    let value_py = bind_statement_lowered_expr_value(value, "using_value", ctx, indent, out)?;
    let resource_py = ctx.new_binding_py_name(&source_name);
    writeln!(
        out,
        "{pad}{resource_py} = tpz_using_file({value_py}, {})  # {}",
        py_span(span),
        py_comment_name(&source_name)
    )
    .expect("write to string");

    ctx.push_scope();
    ctx.register_value_binding(&source_name, false, value, None, None);
    ctx.set_binding_py_name(&source_name, resource_py.clone());
    let result = (|| -> Result<(), PyEmitError> {
        writeln!(out, "{pad}try:").expect("write to string");
        emit_block_as_stmt(body, ctx, indent + 4, out)?;
        writeln!(
            out,
            "{pad}except (TpzReturn, TpzLoopBreak, TpzLoopContinue):"
        )
        .expect("write to string");
        writeln!(
            out,
            "{body_pad}tpz_file_close({resource_py}, {})",
            py_span(span)
        )
        .expect("write to string");
        writeln!(out, "{body_pad}raise").expect("write to string");
        writeln!(out, "{pad}tpz_file_close({resource_py}, {})", py_span(span))
            .expect("write to string");
        Ok(())
    })();
    ctx.pop_scope();
    result
}
