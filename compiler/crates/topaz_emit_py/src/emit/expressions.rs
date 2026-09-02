use crate::*;

pub(super) fn emit_expr(expr: &Expr, ctx: &Ctx<'_>) -> Result<String, PyEmitError> {
    emit_expr_inner(expr, ctx).map_err(|e| e.at(expr.span))
}

pub(super) fn emit_expr_inner(expr: &Expr, ctx: &Ctx<'_>) -> Result<String, PyEmitError> {
    Ok(match &expr.kind {
        ExprKind::Int => ctx
            .text(expr.span)
            .parse::<i64>()
            .map(|n| n.to_string())
            .map_err(|_| PyEmitError::malformed_literal("integer"))?,
        ExprKind::Float => emit_float_literal(ctx.text(expr.span), expr.span)?,
        ExprKind::Bool(true) => "True".to_string(),
        ExprKind::Bool(false) => "False".to_string(),
        ExprKind::Null => "TPZ_NULL".to_string(),
        ExprKind::Unit => "TPZ_UNIT".to_string(),
        ExprKind::Ident => {
            let name = ctx.text(expr.span);
            if name == "None" {
                "None".to_string()
            } else if let Some(info) = ctx.function_info(name)
                && !ctx.binding_is_bound(name)
            {
                render_host_callable(info)
            } else if ctx.records.contains_key(name) && !ctx.binding_is_bound(name) {
                return Err(PyEmitError::unsupported("record type value").at(expr.span));
            } else if let Some(py_name) = ctx.receiver_method_module_value_py_name(name) {
                format!(
                    "__tpz_module_value({}, {}, {})",
                    py_string(py_name),
                    py_string(name),
                    py_span(expr.span)
                )
            } else if let Some(py_name) = ctx.binding_py_name(name) {
                if ctx.binding_is_forward_function_cell(name) {
                    ctx.forward_function_value_py(name, py_name, expr.span)
                } else {
                    py_name.to_string()
                }
            } else {
                mangle(name)
            }
        }
        ExprKind::Paren(inner) => format!("({})", emit_expr(inner, ctx)?),
        ExprKind::String(lit) if lit.tag.is_none() => {
            let has_interp = lit
                .parts
                .iter()
                .any(|part| matches!(part, StringPart::Interpolation(_)));
            if !has_interp {
                py_string(&decode_string_parts(&lit.parts, ctx.map)?)
            } else {
                let mut parts = Vec::new();
                for part in &lit.parts {
                    match part {
                        StringPart::Text(span) => {
                            let mut decoded = String::new();
                            decode_escapes(text_in_map(ctx.map, *span), &mut decoded, *span)
                                .map_err(|_| PyEmitError::malformed_literal("string escape"))?;
                            parts.push(py_string(&decoded));
                        }
                        StringPart::Interpolation(expr) => {
                            parts.push(format!("tpz_render({})", emit_expr(expr, ctx)?));
                        }
                    }
                }
                format!("''.join([{}])", parts.join(", "))
            }
        }
        ExprKind::String(lit) => emit_template_expr(lit, ctx)?,
        ExprKind::Unary { op, operand } => match op {
            UnaryOp::Plus if matches!(&operand.kind, ExprKind::Float) => {
                emit_float_literal(ctx.text(expr.span), expr.span)?
            }
            UnaryOp::Plus => emit_expr(operand, ctx)?,
            UnaryOp::Minus if matches!(&operand.kind, ExprKind::Float) => {
                emit_float_literal(ctx.text(expr.span), expr.span)?
            }
            UnaryOp::Minus => format!(
                "tpz_neg({}, {})",
                emit_expr(operand, ctx)?,
                py_span(expr.span)
            ),
            UnaryOp::Not => format!(
                "not tpz_condition({}, {})",
                emit_expr(operand, ctx)?,
                py_span(expr.span)
            ),
        },
        ExprKind::Binary { op, lhs, rhs } => emit_binary(*op, lhs, rhs, expr.span, ctx)?,
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => emit_if_expr(cond, then_block, else_branch.as_deref(), expr.span, ctx)?,
        ExprKind::Match { scrutinee, cases } => emit_match_expr(scrutinee, cases, expr.span, ctx)?,
        ExprKind::Try(inner) => format!(
            "tpz_try({}, {})",
            emit_expr(inner, ctx)?,
            py_span(expr.span)
        ),
        ExprKind::Call {
            callee,
            args,
            type_args,
        } => {
            if !type_args.is_empty()
                && let Some(method) = typed_json_call_method(callee, ctx)
            {
                if type_args.len() != 1 {
                    return Err(PyEmitError::unsupported("typed JSON type arguments").at(expr.span));
                }
                let schema = emit_json_schema(&type_args[0], ctx)?;
                let params = if method == "parseAs" {
                    &["text"][..]
                } else {
                    &["value"][..]
                };
                let bound = bind_fixed_static_call_args(args, params, &[], expr.span, ctx)?;
                return Ok(render_bound_static_call(&bound, |slots| {
                    render_typed_json_runtime_call(method, &slots[0], &schema, expr.span)
                }));
            }
            emit_call(callee, args, expr.span, ctx)?
        }
        ExprKind::Member { object, field } => {
            if let Some(value_py) =
                payloadless_enum_member_construct(object, field, expr.span, ctx)?
            {
                value_py
            } else {
                let member = ctx.text(field.span);
                if let ExprKind::Ident = &object.kind {
                    let namespace = ctx.text(object.span);
                    if let Some(ModuleRuntimeExport::Function { info }) =
                        ctx.namespace_export(namespace, member)
                    {
                        return Ok(render_host_callable(info));
                    }
                }
                format!(
                    "tpz_member({}, {}, {}, {})",
                    emit_expr(object, ctx)?,
                    py_string(&mangle(member)),
                    py_string(member),
                    py_span(expr.span)
                )
            }
        }
        ExprKind::OptionalAccess { object, field } => {
            let member = ctx.text(field.span);
            format!(
                "tpz_optional_member({}, {}, {}, {})",
                emit_expr(object, ctx)?,
                py_string(&mangle(member)),
                py_string(member),
                py_span(expr.span)
            )
        }
        ExprKind::Index { object, index } => {
            format!(
                "tpz_index({}, {}, {})",
                emit_expr(object, ctx)?,
                emit_expr(index, ctx)?,
                py_span(expr.span)
            )
        }
        ExprKind::Array(elements) => {
            let mut out = Vec::new();
            for element in elements {
                match element {
                    ArrayElement::Expr(expr) => out.push(emit_expr(expr, ctx)?),
                    ArrayElement::Spread(spread) => out.push(format!(
                        "*tpz_spread_values({}, {})",
                        emit_expr(spread, ctx)?,
                        py_span(spread.span)
                    )),
                }
            }
            format!("[{}]", out.join(", "))
        }
        ExprKind::SetLiteral(elements) => {
            let mut out = Vec::new();
            for element in elements {
                out.push(emit_expr(element, ctx)?);
            }
            format!("tpz_set_of([{}], {})", out.join(", "), py_span(expr.span))
        }
        ExprKind::MapLiteral(entries) => {
            let mut out = Vec::new();
            for (key, value) in entries {
                out.push(format!(
                    "({}, {})",
                    emit_expr(key, ctx)?,
                    emit_expr(value, ctx)?
                ));
            }
            format!("tpz_map_of([{}], {})", out.join(", "), py_span(expr.span))
        }
        ExprKind::Block(block) => emit_simple_block_expr(block, ctx)?,
        ExprKind::Lambda { params, body } => emit_lambda_expr(params, body, ctx)?,
        ExprKind::Range {
            lo,
            hi,
            inclusive,
            step,
        } => {
            let lo_py = emit_expr(lo, ctx)?;
            let hi_py = emit_expr(hi, ctx)?;
            let inclusive_py = if *inclusive { "True" } else { "False" };
            let step_py = match step {
                Some(step) => emit_expr(step, ctx)?,
                None => "None".to_string(),
            };
            format!(
                "tpz_range({lo_py}, {hi_py}, {inclusive_py}, {step_py}, {})",
                py_span(expr.span)
            )
        }
        ExprKind::For { .. } => return Err(PyEmitError::unsupported("for expression")),
        ExprKind::Pipe { lhs, rhs } => emit_pipe(lhs, rhs, expr.span, ctx)?,
        ExprKind::Compose { lhs, rhs } => emit_compose_expr(lhs, rhs, expr.span, ctx)?,
        ExprKind::Comprehension {
            kind,
            clauses,
            body,
        } => emit_comprehension_expr(*kind, clauses, body, expr.span, ctx)?,
        ExprKind::Duration(_) => {
            return Err(PyEmitError::unsupported("duration expression").at(expr.span));
        }
        ExprKind::Placeholder => match ctx.pipe_placeholder_replacement() {
            Some(replacement) => replacement,
            None => return Err(PyEmitError::unsupported("pipe placeholder").at(expr.span)),
        },
        ExprKind::Loop { .. } => {
            return Err(PyEmitError::unsupported("loop expression value").at(expr.span));
        }
        ExprKind::Concurrent { .. } => {
            return Err(PyEmitError::unsupported("concurrent expression value").at(expr.span));
        }
        ExprKind::RecordLiteral { fields } => emit_record_literal(fields, ctx)?,
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => emit_record_update(base, spread.as_deref(), fields, expr.span, ctx)?,
    })
}

pub(super) fn emit_binary(
    op: BinaryOp,
    lhs: &Expr,
    rhs: &Expr,
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let l = emit_expr(lhs, ctx)?;
    let span_py = py_span(span);
    if op == BinaryOp::Coalesce {
        let r = emit_expr(rhs, ctx)?;
        return Ok(format!("tpz_coalesce({l}, lambda: {r})"));
    }
    if op == BinaryOp::And {
        let r = emit_expr(rhs, ctx)?;
        return Ok(format!(
            "(tpz_condition({l}, {span_py}) and tpz_condition({r}, {span_py}))"
        ));
    }
    if op == BinaryOp::Or {
        let r = emit_expr(rhs, ctx)?;
        return Ok(format!(
            "(tpz_condition({l}, {span_py}) or tpz_condition({r}, {span_py}))"
        ));
    }
    let r = emit_expr(rhs, ctx)?;
    let leaf = match op {
        BinaryOp::Add => "tpz_add",
        BinaryOp::Sub => "tpz_sub",
        BinaryOp::Mul => "tpz_mul",
        BinaryOp::Div => "tpz_div",
        BinaryOp::Rem => "tpz_rem_trunc_i64",
        BinaryOp::Pow => "tpz_pow",
        BinaryOp::Lt => "tpz_lt",
        BinaryOp::Le => "tpz_le",
        BinaryOp::Gt => "tpz_gt",
        BinaryOp::Ge => "tpz_ge",
        BinaryOp::Eq => "tpz_eq",
        BinaryOp::Ne => "tpz_ne",
        BinaryOp::In => "tpz_in",
        BinaryOp::And | BinaryOp::Or | BinaryOp::Coalesce => {
            return Err(PyEmitError::unsupported("binary operator").at(span));
        }
    };
    Ok(format!("{leaf}({l}, {r}, {span_py})"))
}

pub(super) fn emit_float_literal(raw: &str, span: Span) -> Result<String, PyEmitError> {
    let value = raw
        .parse::<f64>()
        .map_err(|_| PyEmitError::malformed_literal("float").at(span))?;
    Ok(format!("tpz_f64_from_bits(0x{:016x})", value.to_bits()))
}

pub(super) fn should_trace_final_expr(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    match &expr.kind {
        ExprKind::Loop { .. } | ExprKind::Concurrent { .. } => true,
        ExprKind::If {
            then_block,
            else_branch: Some(branch),
            ..
        } => {
            !expr_has_bare_return(expr)
                && block_tail_can_report_final_value(then_block, ctx)
                && should_trace_final_expr(branch, ctx)
        }
        ExprKind::Match { cases, .. } => {
            !expr_has_bare_return(expr)
                && cases
                    .iter()
                    .all(|case| case_arm_can_report_final_value(&case.body, ctx))
        }
        ExprKind::If {
            else_branch: None, ..
        } => false,
        ExprKind::Block(block) => {
            !block_has_bare_return(block) && block_tail_can_report_final_value(block, ctx)
        }
        ExprKind::Paren(inner) => should_trace_final_expr(inner, ctx),
        _ => is_final_trace_value_expr(expr, ctx),
    }
}

pub(super) fn block_tail_can_report_final_value(block: &Block, ctx: &Ctx<'_>) -> bool {
    block
        .tail
        .as_deref()
        .is_some_and(|tail| should_trace_final_expr(tail, ctx))
}

pub(super) fn case_arm_can_report_final_value(body: &CaseArmBody, ctx: &Ctx<'_>) -> bool {
    match body {
        CaseArmBody::Expr(expr) => should_trace_final_expr(expr, ctx),
        CaseArmBody::Return { .. } => false,
    }
}

pub(super) fn is_final_trace_value_expr(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    match &expr.kind {
        ExprKind::Int
        | ExprKind::Float
        | ExprKind::Bool(_)
        | ExprKind::Null
        | ExprKind::Unit
        | ExprKind::Member { .. }
        | ExprKind::OptionalAccess { .. }
        | ExprKind::Index { .. }
        | ExprKind::SetLiteral(_)
        | ExprKind::MapLiteral(_)
        | ExprKind::RecordLiteral { .. } => true,
        ExprKind::Ident => !template_value(expr, ctx),
        ExprKind::String(lit) => lit.tag.is_none(),
        ExprKind::Paren(inner) => is_final_trace_value_expr(inner, ctx),
        ExprKind::Unary { operand, .. } => is_final_trace_value_expr(operand, ctx),
        ExprKind::Binary { lhs, rhs, .. } => {
            is_final_trace_value_expr(lhs, ctx) && is_final_trace_value_expr(rhs, ctx)
        }
        ExprKind::Try(inner) => is_final_trace_value_expr(inner, ctx),
        ExprKind::Call { .. } if call_return_shape(expr, ctx) == Some(ReceiverShape::Template) => {
            false
        }
        ExprKind::Call { callee, args, .. } => match &callee.kind {
            ExprKind::Ident => {
                let name = ctx.text(callee.span);
                (matches!(name, "Ok" | "Err") && args.len() == 1)
                    || (name == "map" && args.len() == 2 && !ctx.binding_is_bound(name))
                    || ctx.function_py_name(name).is_some()
            }
            ExprKind::Member { field, .. } => {
                let method = ctx.text(field.span);
                !matches!(
                    method,
                    "add" | "clear" | "close" | "insert" | "push" | "remove"
                )
            }
            _ => false,
        },
        ExprKind::Array(elements) => elements.iter().all(|element| match element {
            ArrayElement::Expr(expr) => is_final_trace_value_expr(expr, ctx),
            ArrayElement::Spread(_) => false,
        }),
        ExprKind::Comprehension { clauses, body, .. } => {
            clauses.iter().all(|clause| match clause {
                CompClause::For { iter, .. } => is_final_trace_value_expr(iter, ctx),
                CompClause::If(cond) => is_final_trace_value_expr(cond, ctx),
            }) && match body.as_ref() {
                CompBody::Elem(expr) => is_final_trace_value_expr(expr, ctx),
                CompBody::Entry { key, value } => {
                    is_final_trace_value_expr(key, ctx) && is_final_trace_value_expr(value, ctx)
                }
            }
        }
        _ => false,
    }
}

pub(super) fn emit_if_expr(
    cond: &Expr,
    then_block: &Block,
    else_branch: Option<&Expr>,
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let cond_py = emit_expr(cond, ctx)?;
    let then_py = emit_simple_block_expr(then_block, ctx)?;
    let else_py = match else_branch {
        Some(branch) => emit_expr(branch, ctx)?,
        None => "TPZ_UNIT".to_string(),
    };
    Ok(format!(
        "({then_py} if tpz_condition({cond_py}, {}) else {else_py})",
        py_span(span)
    ))
}

pub(super) fn emit_match_expr(
    scrutinee: &Expr,
    cases: &[CaseClause],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let scrutinee_py = emit_expr(scrutinee, ctx)?;
    let fallback = format!("tpz_impossible_match(__tpz_match, {})", py_span(span));
    let body = emit_option_match_chain(scrutinee, cases, 0, &fallback, ctx)?;
    Ok(format!("(lambda __tpz_match: {body})({scrutinee_py})"))
}

pub(super) fn emit_option_match_chain(
    scrutinee: &Expr,
    cases: &[CaseClause],
    index: usize,
    fallback: &str,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let Some(case) = cases.get(index) else {
        return Ok(fallback.to_string());
    };
    let next = emit_option_match_chain(scrutinee, cases, index + 1, fallback, ctx)?;
    let pattern_metadata = pattern_binding_metadata(scrutinee, case, ctx);
    let (condition, bindings) = emit_pattern_condition("__tpz_match", &case.pattern, ctx)?;
    let mut arm_ctx = ctx.clone();
    arm_ctx.push_scope();
    let binding_py_names = bindings
        .iter()
        .map(|binding| {
            register_pattern_binding_with_metadata(binding, pattern_metadata.as_ref(), &mut arm_ctx)
        })
        .collect::<Vec<_>>();
    let condition = emit_case_condition_with_guard_for_expr(
        condition,
        &bindings,
        &binding_py_names,
        case.guard.as_ref(),
        &arm_ctx,
    )?;
    let body = match &case.body {
        CaseArmBody::Expr(expr) => emit_expr(expr, &arm_ctx)?,
        CaseArmBody::Return { value, .. } => {
            let value_py = match value {
                Some(value) => emit_expr(value, &arm_ctx)?,
                None => "TPZ_UNIT".to_string(),
            };
            format!("tpz_return({value_py})")
        }
    };
    if bindings.is_empty() {
        Ok(format!("({body} if {condition} else {next})"))
    } else {
        let params = binding_py_names
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        let args = bindings
            .iter()
            .map(|binding| binding.value_py.clone())
            .collect::<Vec<_>>()
            .join(", ");
        Ok(format!(
            "((lambda {params}: {body})({args}) if {condition} else {next})"
        ))
    }
}

pub(super) fn emit_case_condition_with_guard_for_expr(
    condition: String,
    bindings: &[PatternBinding],
    binding_py_names: &[String],
    guard: Option<&Expr>,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let Some(guard) = guard else {
        return Ok(condition);
    };
    let guard_py = emit_expr(guard, ctx)?;
    let guarded = if bindings.is_empty() {
        format!("tpz_condition({guard_py}, {})", py_span(guard.span))
    } else {
        let params = binding_py_names
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        let args = bindings
            .iter()
            .map(|binding| binding.value_py.clone())
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "(lambda {params}: tpz_condition({guard_py}, {}))({args})",
            py_span(guard.span)
        )
    };
    Ok(format!("({condition} and {guarded})"))
}

pub(super) fn emit_simple_block_expr(block: &Block, ctx: &Ctx<'_>) -> Result<String, PyEmitError> {
    if !block.stmts.is_empty() {
        return Err(PyEmitError::unsupported("statementful block expression").at(block.span));
    }
    match block.tail.as_deref() {
        Some(tail) => emit_expr(tail, ctx),
        None => Ok("TPZ_UNIT".to_string()),
    }
}

pub(super) fn block_has_bare_return(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_bare_return)
        || block.tail.as_deref().is_some_and(expr_has_bare_return)
}

pub(super) fn block_has_try_expr(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_try_expr)
        || block.tail.as_deref().is_some_and(expr_has_try_expr)
}

pub(super) fn stmt_has_bare_return(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Return(_) => true,
        StmtKind::Function(_) => false,
        StmtKind::Let { value, .. } | StmtKind::Const { value, .. } | StmtKind::Expr(value) => {
            expr_has_bare_return(value)
        }
        StmtKind::Assign { target, value, .. } => {
            expr_has_bare_return(target) || expr_has_bare_return(value)
        }
        StmtKind::While { cond, body } => expr_has_bare_return(cond) || block_has_bare_return(body),
        StmtKind::Using { value, body, .. } => {
            expr_has_bare_return(value) || block_has_bare_return(body)
        }
        StmtKind::Export(inner) => stmt_has_bare_return(inner),
        StmtKind::Break { value, .. } => value.as_ref().is_some_and(expr_has_bare_return),
        StmtKind::Import(_)
        | StmtKind::TypeAlias(_)
        | StmtKind::Enum(_)
        | StmtKind::Record(_)
        | StmtKind::Newtype(_)
        | StmtKind::Impl(_)
        | StmtKind::Protocol(_)
        | StmtKind::Continue { .. }
        | StmtKind::Defer(_) => false,
    }
}

pub(super) fn stmt_has_try_expr(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Return(value) | StmtKind::Break { value, .. } => {
            value.as_ref().is_some_and(expr_has_try_expr)
        }
        StmtKind::Function(_) => false,
        StmtKind::Defer(value) => expr_has_try_expr(value),
        StmtKind::Let { value, .. } | StmtKind::Const { value, .. } | StmtKind::Expr(value) => {
            expr_has_try_expr(value)
        }
        StmtKind::Assign { target, value, .. } => {
            expr_has_try_expr(target) || expr_has_try_expr(value)
        }
        StmtKind::While { cond, body } => expr_has_try_expr(cond) || block_has_try_expr(body),
        StmtKind::Using { value, body, .. } => expr_has_try_expr(value) || block_has_try_expr(body),
        StmtKind::Export(inner) => stmt_has_try_expr(inner),
        StmtKind::Import(_)
        | StmtKind::TypeAlias(_)
        | StmtKind::Enum(_)
        | StmtKind::Record(_)
        | StmtKind::Newtype(_)
        | StmtKind::Impl(_)
        | StmtKind::Protocol(_)
        | StmtKind::Continue { .. } => false,
    }
}

pub(super) fn expr_has_bare_return(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Lambda { .. } => false,
        ExprKind::Concurrent {
            timeout,
            arms,
            else_block,
        } => {
            timeout.as_deref().is_some_and(expr_has_bare_return)
                || arms.iter().any(|arm| expr_has_bare_return(&arm.value))
                || else_block.as_deref().is_some_and(block_has_bare_return)
        }
        ExprKind::Try(_) => true,
        ExprKind::Paren(inner) | ExprKind::Unary { operand: inner, .. } => {
            expr_has_bare_return(inner)
        }
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Compose { lhs, rhs } => {
            expr_has_bare_return(lhs) || expr_has_bare_return(rhs)
        }
        ExprKind::Range { lo, hi, step, .. } => {
            expr_has_bare_return(lo)
                || expr_has_bare_return(hi)
                || step.as_deref().is_some_and(expr_has_bare_return)
        }
        ExprKind::Array(elements) => elements.iter().any(|element| match element {
            ArrayElement::Expr(expr) | ArrayElement::Spread(expr) => expr_has_bare_return(expr),
        }),
        ExprKind::SetLiteral(elements) => elements.iter().any(expr_has_bare_return),
        ExprKind::MapLiteral(entries) => entries
            .iter()
            .any(|(key, value)| expr_has_bare_return(key) || expr_has_bare_return(value)),
        ExprKind::Comprehension { clauses, body, .. } => {
            clauses.iter().any(|clause| match clause {
                CompClause::For { iter, .. } => expr_has_bare_return(iter),
                CompClause::If(cond) => expr_has_bare_return(cond),
            }) || match body.as_ref() {
                CompBody::Elem(expr) => expr_has_bare_return(expr),
                CompBody::Entry { key, value } => {
                    expr_has_bare_return(key) || expr_has_bare_return(value)
                }
            }
        }
        ExprKind::RecordLiteral { fields } => fields
            .iter()
            .any(|field| expr_has_bare_return(&field.value)),
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            expr_has_bare_return(base)
                || spread
                    .as_ref()
                    .is_some_and(|expr| expr_has_bare_return(expr))
                || fields
                    .iter()
                    .any(|field| expr_has_bare_return(&field.value))
        }
        ExprKind::String(lit) => lit.parts.iter().any(
            |part| matches!(part, StringPart::Interpolation(expr) if expr_has_bare_return(expr)),
        ),
        ExprKind::Call { callee, args, .. } => {
            expr_has_bare_return(callee)
                || args.iter().any(|arg| match arg {
                    CallArg::Positional(expr) | CallArg::Spread(expr) => expr_has_bare_return(expr),
                    CallArg::Named { value, .. } => expr_has_bare_return(value),
                })
        }
        ExprKind::Block(block) => block_has_bare_return(block),
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            expr_has_bare_return(cond)
                || block_has_bare_return(then_block)
                || else_branch.as_deref().is_some_and(expr_has_bare_return)
        }
        ExprKind::For { iter, body, .. } => {
            expr_has_bare_return(iter) || block_has_bare_return(body)
        }
        ExprKind::Loop { body, .. } => block_has_bare_return(body),
        ExprKind::Match { scrutinee, cases } => {
            expr_has_bare_return(scrutinee)
                || cases.iter().any(|case| {
                    case.guard.as_ref().is_some_and(expr_has_bare_return)
                        || match &case.body {
                            CaseArmBody::Expr(expr) => expr_has_bare_return(expr),
                            CaseArmBody::Return { .. } => true,
                        }
                })
        }
        ExprKind::Member { object, .. } | ExprKind::OptionalAccess { object, .. } => {
            expr_has_bare_return(object)
        }
        ExprKind::Index { object, index } => {
            expr_has_bare_return(object) || expr_has_bare_return(index)
        }
        ExprKind::Pipe { lhs, rhs } => {
            expr_has_bare_return(lhs)
                || matches!(rhs.as_ref(), PipeRhs::Expr(expr) if expr_has_bare_return(expr))
        }
        ExprKind::Float
        | ExprKind::Duration(_)
        | ExprKind::Bool(_)
        | ExprKind::Int
        | ExprKind::Null
        | ExprKind::Unit
        | ExprKind::Ident
        | ExprKind::Placeholder => false,
    }
}

pub(super) fn expr_has_try_expr(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Lambda { .. } => false,
        ExprKind::Concurrent {
            timeout,
            arms,
            else_block,
        } => {
            timeout.as_deref().is_some_and(expr_has_try_expr)
                || arms.iter().any(|arm| expr_has_try_expr(&arm.value))
                || else_block.as_deref().is_some_and(block_has_try_expr)
        }
        ExprKind::Try(_) => true,
        ExprKind::Paren(inner) | ExprKind::Unary { operand: inner, .. } => expr_has_try_expr(inner),
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Compose { lhs, rhs } => {
            expr_has_try_expr(lhs) || expr_has_try_expr(rhs)
        }
        ExprKind::Range { lo, hi, step, .. } => {
            expr_has_try_expr(lo)
                || expr_has_try_expr(hi)
                || step.as_deref().is_some_and(expr_has_try_expr)
        }
        ExprKind::Array(elements) => elements.iter().any(|element| match element {
            ArrayElement::Expr(expr) | ArrayElement::Spread(expr) => expr_has_try_expr(expr),
        }),
        ExprKind::SetLiteral(elements) => elements.iter().any(expr_has_try_expr),
        ExprKind::MapLiteral(entries) => entries
            .iter()
            .any(|(key, value)| expr_has_try_expr(key) || expr_has_try_expr(value)),
        ExprKind::Comprehension { clauses, body, .. } => {
            clauses.iter().any(|clause| match clause {
                CompClause::For { iter, .. } => expr_has_try_expr(iter),
                CompClause::If(cond) => expr_has_try_expr(cond),
            }) || match body.as_ref() {
                CompBody::Elem(expr) => expr_has_try_expr(expr),
                CompBody::Entry { key, value } => {
                    expr_has_try_expr(key) || expr_has_try_expr(value)
                }
            }
        }
        ExprKind::RecordLiteral { fields } => {
            fields.iter().any(|field| expr_has_try_expr(&field.value))
        }
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            expr_has_try_expr(base)
                || spread.as_ref().is_some_and(|expr| expr_has_try_expr(expr))
                || fields.iter().any(|field| expr_has_try_expr(&field.value))
        }
        ExprKind::String(lit) => lit
            .parts
            .iter()
            .any(|part| matches!(part, StringPart::Interpolation(expr) if expr_has_try_expr(expr))),
        ExprKind::Call { callee, args, .. } => {
            expr_has_try_expr(callee)
                || args.iter().any(|arg| match arg {
                    CallArg::Positional(expr) | CallArg::Spread(expr) => expr_has_try_expr(expr),
                    CallArg::Named { value, .. } => expr_has_try_expr(value),
                })
        }
        ExprKind::Block(block) => block_has_try_expr(block),
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            expr_has_try_expr(cond)
                || block_has_try_expr(then_block)
                || else_branch.as_deref().is_some_and(expr_has_try_expr)
        }
        ExprKind::For { iter, body, .. } => expr_has_try_expr(iter) || block_has_try_expr(body),
        ExprKind::Loop { body, .. } => block_has_try_expr(body),
        ExprKind::Match { scrutinee, cases } => {
            expr_has_try_expr(scrutinee)
                || cases.iter().any(|case| {
                    case.guard.as_ref().is_some_and(expr_has_try_expr)
                        || match &case.body {
                            CaseArmBody::Expr(expr) => expr_has_try_expr(expr),
                            CaseArmBody::Return { value, .. } => {
                                value.as_ref().is_some_and(expr_has_try_expr)
                            }
                        }
                })
        }
        ExprKind::Member { object, .. } | ExprKind::OptionalAccess { object, .. } => {
            expr_has_try_expr(object)
        }
        ExprKind::Index { object, index } => expr_has_try_expr(object) || expr_has_try_expr(index),
        ExprKind::Pipe { lhs, rhs } => {
            expr_has_try_expr(lhs)
                || matches!(rhs.as_ref(), PipeRhs::Expr(expr) if expr_has_try_expr(expr))
        }
        ExprKind::Float
        | ExprKind::Duration(_)
        | ExprKind::Bool(_)
        | ExprKind::Int
        | ExprKind::Null
        | ExprKind::Unit
        | ExprKind::Ident
        | ExprKind::Placeholder => false,
    }
}

pub(super) fn emit_comprehension_expr(
    kind: CompKind,
    clauses: &[CompClause],
    body: &CompBody,
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let mut comp_ctx = ctx.clone();
    let mut clause_py = String::new();
    let mut capture_py_names = Vec::new();
    let mut pushed_scopes = 0usize;
    for (idx, clause) in clauses.iter().enumerate() {
        match clause {
            CompClause::For { pattern, iter } => {
                let iter_py = emit_expr(iter, &comp_ctx)?;
                let item_py = comp_item_name(span, idx);
                write!(
                    clause_py,
                    " for {item_py} in tpz_for_items({iter_py}, {})",
                    py_span(span)
                )
                .expect("write to string");
                let (condition, bindings) = emit_pattern_condition(&item_py, pattern, &comp_ctx)?;
                if condition != "True" {
                    write!(
                        clause_py,
                        " if tpz_for_pattern({condition}, {})",
                        py_span(span)
                    )
                    .expect("write to string");
                }
                // Match the checker: each `for` clause owns a child scope. Later
                // clauses may shadow an earlier loop variable without reusing its
                // Python target or capture parameter.
                comp_ctx.push_scope();
                pushed_scopes += 1;
                for binding in &bindings {
                    let py_name = comp_ctx.new_binding_py_name(&binding.name);
                    write!(clause_py, " for {py_name} in [{}]", binding.value_py)
                        .expect("write to string");
                }
                for binding in bindings {
                    let py_name = comp_ctx.new_binding_py_name(&binding.name);
                    comp_ctx.register_binding(&binding.name, false);
                    comp_ctx.set_binding_py_name(&binding.name, py_name.clone());
                    capture_py_names.push(py_name);
                }
            }
            CompClause::If(cond) => {
                let cond_py = emit_expr(cond, &comp_ctx)?;
                write!(clause_py, " if tpz_condition({cond_py}, {})", py_span(span))
                    .expect("write to string");
            }
        }
    }
    let body_py = match (kind, body) {
        (CompKind::Map, CompBody::Entry { key, value }) => {
            format!(
                "({}, {})",
                emit_expr(key, &comp_ctx)?,
                emit_expr(value, &comp_ctx)?
            )
        }
        (_, CompBody::Elem(expr)) => emit_expr(expr, &comp_ctx)?,
        _ => unreachable!("comprehension kind/body shape paired by the parser"),
    };
    // Python comprehension variables are one cell per clause, so a lambda created
    // directly or inside a compound body would otherwise observe the clause's final
    // value. Rebind every current comprehension name through a per-production call;
    // nested lambdas then close over that call's fresh parameters, matching Topaz's
    // fresh per-iteration binding semantics.
    let body_py = if capture_py_names.is_empty() {
        body_py
    } else {
        let captures = capture_py_names.join(", ");
        format!("(lambda {captures}: {body_py})({captures})")
    };
    for _ in 0..pushed_scopes {
        comp_ctx.pop_scope();
    }
    let array_py = format!("[{body_py}{clause_py}]");
    Ok(match kind {
        CompKind::Array => array_py,
        CompKind::Set => format!("tpz_set_of({array_py}, {})", py_span(span)),
        CompKind::Map => format!("tpz_map_of({array_py}, {})", py_span(span)),
    })
}

pub(super) fn comp_item_name(span: Span, idx: usize) -> String {
    format!("__tpz_comp_{}_{}_{}_{}", span.file.0, span.lo, span.hi, idx)
}

pub(super) fn emit_lambda_expr(
    params: &[LambdaParam],
    body: &Expr,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    emit_contextually_typed_lambda_expr(params, body, None, ctx)
}

pub(super) fn emit_contextually_typed_lambda_expr(
    params: &[LambdaParam],
    body: &Expr,
    contextual_ty: Option<&Type>,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let mut lambda_ctx = ctx.clone();
    lambda_ctx.push_scope();
    let mut params_py = Vec::with_capacity(params.len());
    for (index, param) in params.iter().enumerate() {
        let source_name = lambda_ctx.text(param.name.span).to_string();
        let py_name = mangle(&source_name);
        register_lambda_parameter_binding(
            &source_name,
            param,
            index,
            contextual_ty,
            &mut lambda_ctx,
        );
        lambda_ctx.set_binding_py_name(&source_name, py_name.clone());
        params_py.push(py_name);
    }
    let body_py = emit_expr(body, &lambda_ctx)?;
    Ok(format!("(lambda {}: {body_py})", params_py.join(", ")))
}

pub(super) fn register_lambda_parameter_binding(
    source_name: &str,
    param: &LambdaParam,
    index: usize,
    contextual_ty: Option<&Type>,
    ctx: &mut Ctx<'_>,
) {
    if let Some(ty) = param.ty.as_ref() {
        ctx.register_typed_binding(source_name, false, ty);
        return;
    }
    if let Some(TypeKind::Function { params, .. }) = contextual_ty.map(|ty| &ty.kind)
        && let Some(param) = params.get(index)
    {
        ctx.register_typed_binding(source_name, false, &param.ty);
        return;
    }
    let checked_ty = contextual_ty
        .and_then(|ty| checked_alias_for_ast_type(ty, ctx))
        .and_then(|alias| match &alias.body {
            CheckType::Func { params, .. } => params.get(index).cloned(),
            _ => None,
        });
    if let Some(checked_ty) = checked_ty.as_ref() {
        ctx.register_checked_binding(source_name, false, checked_ty);
    } else {
        ctx.register_binding(source_name, false);
    }
}

pub(super) fn emit_contextually_typed_value_expr(
    value: &Expr,
    contextual_ty: Option<&Type>,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    match &value.kind {
        ExprKind::Lambda { params, body } => {
            emit_contextually_typed_lambda_expr(params, body, contextual_ty, ctx)
        }
        ExprKind::Paren(inner) => Ok(format!(
            "({})",
            emit_contextually_typed_value_expr(inner, contextual_ty, ctx)?
        )),
        _ => emit_expr(value, ctx),
    }
}

pub(super) fn emit_contextually_typed_value_expr_to_target_if_needed(
    value: &Expr,
    contextual_ty: Option<&Type>,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<bool, PyEmitError> {
    if !expr_needs_statement_lowering(value, ctx) {
        return Ok(false);
    }
    match &value.kind {
        ExprKind::Lambda { params, body } => {
            emit_statement_lowered_lambda_to_target(
                params,
                body,
                contextual_ty,
                target_py,
                ctx,
                indent,
                out,
            )?;
            Ok(true)
        }
        ExprKind::Paren(inner) => emit_contextually_typed_value_expr_to_target_if_needed(
            inner,
            contextual_ty,
            target_py,
            ctx,
            indent,
            out,
        ),
        _ => emit_expr_to_target_if_needed(value, target_py, ctx, indent, out),
    }
}
