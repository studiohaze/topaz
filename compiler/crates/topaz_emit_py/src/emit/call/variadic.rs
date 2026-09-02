use crate::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StaticArgValue {
    pub(super) py: String,
    pub(super) cooperative_callback: bool,
}

impl StaticArgValue {
    pub(super) fn plain(py: String) -> Self {
        Self {
            py,
            cooperative_callback: false,
        }
    }
}

pub(super) fn render_callback_adapter(py_name: &str, needs_host: bool, arity: usize) -> String {
    let params = (0..arity)
        .map(|idx| format!("__tpz_cb_{idx}"))
        .collect::<Vec<_>>();
    let mut args = Vec::new();
    if needs_host {
        args.push("host".to_string());
    }
    args.extend(params.iter().cloned());
    format!(
        "(lambda {}: {}({}))",
        params.join(", "),
        py_name,
        args.join(", ")
    )
}

pub(super) fn render_cooperative_method_callback_adapter(value_py: &str, arity: usize) -> String {
    let params = (0..arity)
        .map(|idx| format!("__tpz_cb_{idx}"))
        .collect::<Vec<_>>();
    format!(
        "(lambda __tpz_target: (lambda {}: __tpz_target.__call_cooperative__({})))({value_py})",
        params.join(", "),
        params.join(", ")
    )
}

pub(super) fn module_export_callback_target(
    export: &ModuleRuntimeExport<'_>,
    cooperative_yields: bool,
) -> Option<(String, bool, bool)> {
    match export {
        ModuleRuntimeExport::Function { info } => {
            let (py_name, cooperative_callback) = if cooperative_yields {
                match info.cooperative_py_name.as_deref() {
                    Some(py_name) => (py_name.to_string(), true),
                    None => (info.py_name.clone(), false),
                }
            } else {
                (info.py_name.clone(), false)
            };
            Some((py_name, info.needs_host, cooperative_callback))
        }
        ModuleRuntimeExport::Value {
            cooperative_callback: Some((py_name, needs_host)),
            ..
        } if cooperative_yields => Some((py_name.clone(), *needs_host, true)),
        _ => None,
    }
}

pub(super) fn emit_callback_expr(
    expr: &Expr,
    arity: usize,
    ctx: &Ctx<'_>,
) -> Result<StaticArgValue, PyEmitError> {
    if let ExprKind::Ident = &expr.kind {
        let name = ctx.text(expr.span);
        if ctx.cooperative_yields && ctx.binding_is_composed(name) {
            let py = ctx
                .binding_py_name(name)
                .map(str::to_string)
                .unwrap_or_else(|| mangle(name));
            return Ok(StaticArgValue {
                py: render_cooperative_method_callback_adapter(&py, arity),
                cooperative_callback: true,
            });
        }
        if ctx.cooperative_yields
            && let Some((py_name, needs_host)) =
                ctx.binding_cooperative_callback_target(name, expr.span)
        {
            return Ok(StaticArgValue {
                py: render_callback_adapter(&py_name, needs_host, arity),
                cooperative_callback: true,
            });
        }
        if !ctx.binding_is_bound(name)
            && let Some(info) = ctx.function_info(name)
        {
            let (py_name, cooperative_callback) = if ctx.cooperative_yields {
                match info.cooperative_py_name.as_deref() {
                    Some(py_name) => (py_name, true),
                    None => (info.py_name.as_str(), false),
                }
            } else {
                (info.py_name.as_str(), false)
            };
            return Ok(StaticArgValue {
                py: render_callback_adapter(py_name, info.needs_host, arity),
                cooperative_callback,
            });
        }
    }
    if ctx.cooperative_yields
        && let ExprKind::Index { object, index } = &expr.kind
        && let Some((py_name, needs_host)) = ctx
            .array_element_projection_for_index(object, index)
            .and_then(ArrayElementProjection::cooperative_callback_target)
    {
        return Ok(StaticArgValue {
            py: render_callback_adapter(&py_name, needs_host, arity),
            cooperative_callback: true,
        });
    }
    if ctx.cooperative_yields
        && let ExprKind::Member { object, field } = &expr.kind
        && let Some((py_name, needs_host)) = ctx
            .record_member_field_projection(object, field)
            .cooperative_callback_target
    {
        return Ok(StaticArgValue {
            py: render_callback_adapter(&py_name, needs_host, arity),
            cooperative_callback: true,
        });
    }
    if let ExprKind::Member { object, field } = &expr.kind
        && let ExprKind::Ident = &object.kind
    {
        let namespace = ctx.text(object.span);
        let member = ctx.text(field.span);
        if let Some(export) = ctx.namespace_export(namespace, member)
            && let Some((py_name, needs_host, cooperative_callback)) =
                module_export_callback_target(export, ctx.cooperative_yields)
        {
            return Ok(StaticArgValue {
                py: render_callback_adapter(&py_name, needs_host, arity),
                cooperative_callback,
            });
        }
    }
    let py = emit_expr(expr, ctx)?;
    Ok(StaticArgValue {
        py,
        cooperative_callback: callback_expr_requires_cooperative_driver(expr, ctx),
    })
}

pub(super) fn callback_expr_requires_cooperative_driver(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    if !ctx.cooperative_yields {
        return false;
    }
    match &expr.kind {
        ExprKind::Paren(inner) => callback_expr_requires_cooperative_driver(inner, ctx),
        ExprKind::Lambda { body, .. } => expr_contains_cooperative_known_call(body, ctx),
        _ => false,
    }
}

pub(super) fn cooperative_callback_sibling_py_name_for_value(
    value: &Expr,
    mutable: bool,
    py_name: &str,
    ctx: &Ctx<'_>,
) -> Option<String> {
    if mutable {
        return None;
    }
    match &value.kind {
        ExprKind::Lambda { body, .. } if expr_contains_cooperative_known_call(body, ctx) => {
            Some(format!("{py_name}__co"))
        }
        ExprKind::Paren(inner) => {
            cooperative_callback_sibling_py_name_for_value(inner, mutable, py_name, ctx)
        }
        _ => None,
    }
}

pub(super) fn expr_contains_cooperative_known_call(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    match &expr.kind {
        ExprKind::Int
        | ExprKind::Float
        | ExprKind::Duration(_)
        | ExprKind::Bool(_)
        | ExprKind::Null
        | ExprKind::Unit
        | ExprKind::Ident
        | ExprKind::Placeholder => false,
        ExprKind::String(lit) => lit.parts.iter().any(|part| match part {
            StringPart::Text(_) => false,
            StringPart::Interpolation(expr) => expr_contains_cooperative_known_call(expr, ctx),
        }),
        ExprKind::Paren(inner) | ExprKind::Try(inner) => {
            expr_contains_cooperative_known_call(inner, ctx)
        }
        ExprKind::Block(block) => block_contains_cooperative_known_call(block, ctx),
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            expr_contains_cooperative_known_call(cond, ctx)
                || block_contains_cooperative_known_call(then_block, ctx)
                || else_branch
                    .as_deref()
                    .is_some_and(|expr| expr_contains_cooperative_known_call(expr, ctx))
        }
        ExprKind::Match { scrutinee, cases } => {
            expr_contains_cooperative_known_call(scrutinee, ctx)
                || cases
                    .iter()
                    .any(|case| case_contains_cooperative_known_call(case, ctx))
        }
        ExprKind::For { iter, body, .. } => {
            expr_contains_cooperative_known_call(iter, ctx)
                || block_contains_cooperative_known_call(body, ctx)
        }
        ExprKind::Loop { body, .. } => block_contains_cooperative_known_call(body, ctx),
        ExprKind::Concurrent {
            timeout,
            arms,
            else_block,
        } => {
            timeout
                .as_deref()
                .is_some_and(|expr| expr_contains_cooperative_known_call(expr, ctx))
                || arms
                    .iter()
                    .any(|arm| expr_contains_cooperative_known_call(&arm.value, ctx))
                || else_block
                    .as_deref()
                    .is_some_and(|block| block_contains_cooperative_known_call(block, ctx))
        }
        ExprKind::Call { callee, args, .. } => {
            callee_is_cooperative_known_function(callee, ctx)
                || expr_contains_cooperative_known_call(callee, ctx)
                || args
                    .iter()
                    .any(|arg| call_arg_contains_cooperative_known_call(arg, ctx))
        }
        ExprKind::Member { object, .. } | ExprKind::OptionalAccess { object, .. } => {
            expr_contains_cooperative_known_call(object, ctx)
        }
        ExprKind::Index { object, index } => {
            expr_contains_cooperative_known_call(object, ctx)
                || expr_contains_cooperative_known_call(index, ctx)
        }
        ExprKind::Unary { operand, .. } => expr_contains_cooperative_known_call(operand, ctx),
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Compose { lhs, rhs } => {
            expr_contains_cooperative_known_call(lhs, ctx)
                || expr_contains_cooperative_known_call(rhs, ctx)
        }
        ExprKind::Range { lo, hi, step, .. } => {
            expr_contains_cooperative_known_call(lo, ctx)
                || expr_contains_cooperative_known_call(hi, ctx)
                || step
                    .as_deref()
                    .is_some_and(|expr| expr_contains_cooperative_known_call(expr, ctx))
        }
        ExprKind::Pipe { lhs, rhs } => {
            expr_contains_cooperative_known_call(lhs, ctx)
                || pipe_rhs_contains_cooperative_known_call(rhs, ctx)
        }
        ExprKind::Lambda { .. } => false,
        ExprKind::RecordLiteral { fields } => fields
            .iter()
            .any(|field| expr_contains_cooperative_known_call(&field.value, ctx)),
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            expr_contains_cooperative_known_call(base, ctx)
                || spread
                    .as_deref()
                    .is_some_and(|expr| expr_contains_cooperative_known_call(expr, ctx))
                || fields
                    .iter()
                    .any(|field| expr_contains_cooperative_known_call(&field.value, ctx))
        }
        ExprKind::Array(elements) => elements
            .iter()
            .any(|element| array_element_contains_cooperative_known_call(element, ctx)),
        ExprKind::SetLiteral(items) => items
            .iter()
            .any(|expr| expr_contains_cooperative_known_call(expr, ctx)),
        ExprKind::MapLiteral(entries) => entries.iter().any(|(key, value)| {
            expr_contains_cooperative_known_call(key, ctx)
                || expr_contains_cooperative_known_call(value, ctx)
        }),
        ExprKind::Comprehension { clauses, body, .. } => {
            clauses
                .iter()
                .any(|clause| comp_clause_contains_cooperative_known_call(clause, ctx))
                || comp_body_contains_cooperative_known_call(body, ctx)
        }
    }
}

pub(super) fn callee_is_cooperative_known_function(callee: &Expr, ctx: &Ctx<'_>) -> bool {
    match &callee.kind {
        ExprKind::Ident => {
            let name = ctx.text(callee.span);
            if ctx
                .binding_cooperative_callback_target(name, callee.span)
                .is_some()
            {
                return true;
            }
            !ctx.binding_is_bound(name)
                && ctx
                    .function_info(name)
                    .and_then(|info| info.cooperative_py_name.as_ref())
                    .is_some()
        }
        ExprKind::Paren(inner) => callee_is_cooperative_known_function(inner, ctx),
        _ => false,
    }
}

pub(super) fn block_contains_cooperative_known_call(block: &Block, ctx: &Ctx<'_>) -> bool {
    block
        .stmts
        .iter()
        .any(|stmt| stmt_contains_cooperative_known_call(stmt, ctx))
        || block
            .tail
            .as_deref()
            .is_some_and(|expr| expr_contains_cooperative_known_call(expr, ctx))
}

pub(super) fn stmt_contains_cooperative_known_call(stmt: &Stmt, ctx: &Ctx<'_>) -> bool {
    match &stmt.kind {
        StmtKind::Let { value, .. } | StmtKind::Const { value, .. } => {
            expr_contains_cooperative_known_call(value, ctx)
        }
        StmtKind::Assign { target, value, .. } => {
            expr_contains_cooperative_known_call(target, ctx)
                || expr_contains_cooperative_known_call(value, ctx)
        }
        StmtKind::Return(value) | StmtKind::Break { value, .. } => value
            .as_ref()
            .is_some_and(|expr| expr_contains_cooperative_known_call(expr, ctx)),
        StmtKind::Defer(expr) => expr_contains_cooperative_known_call(expr, ctx),
        StmtKind::Expr(expr) => expr_contains_cooperative_known_call(expr, ctx),
        StmtKind::Using { value, body, .. } => {
            expr_contains_cooperative_known_call(value, ctx)
                || block_contains_cooperative_known_call(body, ctx)
        }
        StmtKind::While { cond, body } => {
            expr_contains_cooperative_known_call(cond, ctx)
                || block_contains_cooperative_known_call(body, ctx)
        }
        StmtKind::Import(_)
        | StmtKind::Export(_)
        | StmtKind::Function(_)
        | StmtKind::TypeAlias(_)
        | StmtKind::Enum(_)
        | StmtKind::Record(_)
        | StmtKind::Newtype(_)
        | StmtKind::Impl(_)
        | StmtKind::Protocol(_)
        | StmtKind::Continue { .. } => false,
    }
}

pub(super) fn case_contains_cooperative_known_call(case: &CaseClause, ctx: &Ctx<'_>) -> bool {
    case.guard
        .as_ref()
        .is_some_and(|expr| expr_contains_cooperative_known_call(expr, ctx))
        || match &case.body {
            CaseArmBody::Expr(expr) => expr_contains_cooperative_known_call(expr, ctx),
            CaseArmBody::Return { value, .. } => value
                .as_ref()
                .is_some_and(|expr| expr_contains_cooperative_known_call(expr, ctx)),
        }
}

pub(super) fn call_arg_contains_cooperative_known_call(arg: &CallArg, ctx: &Ctx<'_>) -> bool {
    match arg {
        CallArg::Positional(expr) | CallArg::Spread(expr) => {
            expr_contains_cooperative_known_call(expr, ctx)
        }
        CallArg::Named { value, .. } => expr_contains_cooperative_known_call(value, ctx),
    }
}

pub(super) fn array_element_contains_cooperative_known_call(
    element: &ArrayElement,
    ctx: &Ctx<'_>,
) -> bool {
    match element {
        ArrayElement::Expr(expr) | ArrayElement::Spread(expr) => {
            expr_contains_cooperative_known_call(expr, ctx)
        }
    }
}

pub(super) fn pipe_rhs_contains_cooperative_known_call(rhs: &PipeRhs, ctx: &Ctx<'_>) -> bool {
    match rhs {
        PipeRhs::Expr(expr) => expr_contains_cooperative_known_call(expr, ctx),
        PipeRhs::Field(_) => false,
    }
}

pub(super) fn comp_clause_contains_cooperative_known_call(
    clause: &CompClause,
    ctx: &Ctx<'_>,
) -> bool {
    match clause {
        CompClause::For { iter, .. } | CompClause::If(iter) => {
            expr_contains_cooperative_known_call(iter, ctx)
        }
    }
}

pub(super) fn comp_body_contains_cooperative_known_call(body: &CompBody, ctx: &Ctx<'_>) -> bool {
    match body {
        CompBody::Elem(expr) => expr_contains_cooperative_known_call(expr, ctx),
        CompBody::Entry { key, value } => {
            expr_contains_cooperative_known_call(key, ctx)
                || expr_contains_cooperative_known_call(value, ctx)
        }
    }
}

pub(super) fn render_hof_call(
    cooperative: bool,
    regular_name: &str,
    cooperative_name: &str,
    args: Vec<String>,
) -> String {
    let callee = if cooperative {
        cooperative_name
    } else {
        regular_name
    };
    let call = format!("{callee}({})", args.join(", "));
    if cooperative {
        format!("(yield from {call})")
    } else {
        call
    }
}

pub(super) fn cooperative_hof_driver_enabled(
    cooperative: bool,
    _cooperative_callback: bool,
) -> bool {
    // Static callback metadata still matters for direct sibling lowering. Built-in
    // HOFs choose the cooperative runtime driver from the surrounding generator
    // context, and the runtime adapter keeps plain callables behavior-preserving.
    cooperative
}

pub(super) fn render_array_map_call_with_callback(
    recv: &str,
    callback: &str,
    span: Span,
    cooperative: bool,
    cooperative_callback: bool,
) -> String {
    let cooperative = cooperative_hof_driver_enabled(cooperative, cooperative_callback);
    render_hof_call(
        cooperative,
        "tpz_array_map",
        "tpz_array_map__co",
        vec![recv.to_string(), callback.to_string(), py_span(span)],
    )
}

pub(super) fn render_array_filter_call_with_callback(
    recv: &str,
    callback: &str,
    span: Span,
    cooperative: bool,
    cooperative_callback: bool,
) -> String {
    let cooperative = cooperative_hof_driver_enabled(cooperative, cooperative_callback);
    render_hof_call(
        cooperative,
        "tpz_array_filter",
        "tpz_array_filter__co",
        vec![recv.to_string(), callback.to_string(), py_span(span)],
    )
}

pub(super) fn render_array_reduce_call_with_callback(
    recv: &str,
    initial: &str,
    callback: &str,
    span: Span,
    cooperative: bool,
    cooperative_callback: bool,
) -> String {
    let cooperative = cooperative_hof_driver_enabled(cooperative, cooperative_callback);
    render_hof_call(
        cooperative,
        "tpz_array_reduce",
        "tpz_array_reduce__co",
        vec![
            recv.to_string(),
            initial.to_string(),
            callback.to_string(),
            py_span(span),
        ],
    )
}

pub(super) fn render_array_sorted_by_call_with_callback(
    recv: &str,
    callback: &str,
    span: Span,
    cooperative: bool,
    cooperative_callback: bool,
) -> String {
    let cooperative = cooperative_hof_driver_enabled(cooperative, cooperative_callback);
    render_hof_call(
        cooperative,
        "tpz_array_sorted_by",
        "tpz_array_sorted_by__co",
        vec![recv.to_string(), callback.to_string(), py_span(span)],
    )
}

pub(super) fn render_array_sort_by_call_with_callback(
    recv: &str,
    callback: &str,
    span: Span,
    cooperative: bool,
    cooperative_callback: bool,
) -> String {
    let cooperative = cooperative_hof_driver_enabled(cooperative, cooperative_callback);
    render_hof_call(
        cooperative,
        "tpz_array_sort_by",
        "tpz_array_sort_by__co",
        vec![recv.to_string(), callback.to_string(), py_span(span)],
    )
}

pub(super) fn render_array_retain_call_with_callback(
    recv: &str,
    callback: &str,
    span: Span,
    cooperative: bool,
    cooperative_callback: bool,
) -> String {
    let cooperative = cooperative_hof_driver_enabled(cooperative, cooperative_callback);
    render_hof_call(
        cooperative,
        "tpz_array_retain",
        "tpz_array_retain__co",
        vec![recv.to_string(), callback.to_string(), py_span(span)],
    )
}

pub(super) fn render_option_map_call_with_callback(
    recv: &str,
    callback: &str,
    span: Span,
    cooperative: bool,
    cooperative_callback: bool,
) -> String {
    let cooperative = cooperative_hof_driver_enabled(cooperative, cooperative_callback);
    render_hof_call(
        cooperative,
        "tpz_option_map",
        "tpz_option_map__co",
        vec![recv.to_string(), callback.to_string(), py_span(span)],
    )
}

pub(super) fn render_option_flat_map_call_with_callback(
    recv: &str,
    callback: &str,
    span: Span,
    cooperative: bool,
    cooperative_callback: bool,
) -> String {
    let cooperative = cooperative_hof_driver_enabled(cooperative, cooperative_callback);
    render_hof_call(
        cooperative,
        "tpz_option_flat_map",
        "tpz_option_flat_map__co",
        vec![recv.to_string(), callback.to_string(), py_span(span)],
    )
}

pub(super) fn render_option_ok_or_else_call_with_callback(
    recv: &str,
    callback: &str,
    span: Span,
    cooperative: bool,
    cooperative_callback: bool,
) -> String {
    let cooperative = cooperative_hof_driver_enabled(cooperative, cooperative_callback);
    render_hof_call(
        cooperative,
        "tpz_option_ok_or_else",
        "tpz_option_ok_or_else__co",
        vec![recv.to_string(), callback.to_string(), py_span(span)],
    )
}

pub(super) fn render_result_map_call_with_callback(
    recv: &str,
    callback: &str,
    span: Span,
    cooperative: bool,
    cooperative_callback: bool,
) -> String {
    let cooperative = cooperative_hof_driver_enabled(cooperative, cooperative_callback);
    render_hof_call(
        cooperative,
        "tpz_result_map",
        "tpz_result_map__co",
        vec![recv.to_string(), callback.to_string(), py_span(span)],
    )
}

pub(super) fn render_result_flat_map_call_with_callback(
    recv: &str,
    callback: &str,
    span: Span,
    cooperative: bool,
    cooperative_callback: bool,
) -> String {
    let cooperative = cooperative_hof_driver_enabled(cooperative, cooperative_callback);
    render_hof_call(
        cooperative,
        "tpz_result_flat_map",
        "tpz_result_flat_map__co",
        vec![recv.to_string(), callback.to_string(), py_span(span)],
    )
}

pub(super) fn render_map_map_values_call_with_callback(
    recv: &str,
    callback: &str,
    span: Span,
    cooperative: bool,
    cooperative_callback: bool,
) -> String {
    let cooperative = cooperative_hof_driver_enabled(cooperative, cooperative_callback);
    render_hof_call(
        cooperative,
        "tpz_map_map_values",
        "tpz_map_map_values__co",
        vec![recv.to_string(), callback.to_string(), py_span(span)],
    )
}

pub(super) fn render_map_filter_call_with_callback(
    recv: &str,
    callback: &str,
    span: Span,
    cooperative: bool,
    cooperative_callback: bool,
) -> String {
    let cooperative = cooperative_hof_driver_enabled(cooperative, cooperative_callback);
    render_hof_call(
        cooperative,
        "tpz_map_filter",
        "tpz_map_filter__co",
        vec![recv.to_string(), callback.to_string(), py_span(span)],
    )
}

pub(super) fn render_map_update_call_with_callback(
    recv: &str,
    key: &str,
    initial: &str,
    callback: &str,
    span: Span,
    cooperative: bool,
    cooperative_callback: bool,
) -> String {
    let cooperative = cooperative_hof_driver_enabled(cooperative, cooperative_callback);
    render_hof_call(
        cooperative,
        "tpz_map_update",
        "tpz_map_update__co",
        vec![
            recv.to_string(),
            key.to_string(),
            initial.to_string(),
            callback.to_string(),
            py_span(span),
        ],
    )
}

pub(super) fn emit_record_literal(
    fields: &[FieldInit],
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let shape = record_shape(fields, ctx.map);
    let mut args = Vec::with_capacity(fields.len());
    for field in fields {
        args.push(emit_expr(&field.value, ctx)?);
    }
    Ok(format!(
        "{}({})",
        record_class_name(&shape),
        args.join(", ")
    ))
}

pub(super) fn emit_record_update(
    base: &Expr,
    spread: Option<&Expr>,
    fields: &[FieldInit],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    if let Some(record) = nominal_record_for_construct_base(base, ctx) {
        return emit_nominal_record_construct(record, spread, fields, span, ctx);
    }
    if spread.is_some() {
        return Err(PyEmitError::unsupported("nominal record spread").at(span));
    }
    let base_py = emit_expr(base, ctx)?;
    let mut updates = Vec::with_capacity(fields.len());
    for field in fields {
        let source_name = ctx.text(field.name.span);
        let value_py = emit_expr(&field.value, ctx)?;
        updates.push(format!(
            "({}, {}, lambda: {value_py})",
            py_string(&mangle(source_name)),
            py_string(source_name)
        ));
    }
    Ok(format!(
        "tpz_record_update({base_py}, [{}], {})",
        updates.join(", "),
        py_span(span)
    ))
}

pub(super) fn nominal_record_for_construct_base<'ctx, 'ast>(
    base: &Expr,
    ctx: &'ctx Ctx<'ast>,
) -> Option<&'ctx NominalRecordDef<'ast>> {
    let ExprKind::Ident = &base.kind else {
        return None;
    };
    let name = ctx.text(base.span);
    (!ctx.binding_is_bound(name))
        .then(|| ctx.records.get(name))
        .flatten()
}

pub(super) fn emit_nominal_record_construct(
    record: &NominalRecordDef<'_>,
    spread: Option<&Expr>,
    fields: &[FieldInit],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let decl_fields = emit_nominal_record_decl_fields(record, ctx)?;
    let spread_py = match spread {
        Some(spread) => format!("lambda: {}", emit_expr(spread, ctx)?),
        None => "None".to_string(),
    };
    let explicit_fields = fields
        .iter()
        .map(|field| {
            let source_name = ctx.text(field.name.span);
            let value_py = emit_expr(&field.value, ctx)?;
            Ok(format!(
                "({}, {}, lambda: {value_py})",
                py_string(&mangle(source_name)),
                py_string(source_name)
            ))
        })
        .collect::<Result<Vec<_>, PyEmitError>>()?;
    let declaration_identity = record
        .declaration_identity
        .as_ref()
        .map_or(String::new(), |identity| {
            format!(", {}", py_string(identity))
        });
    Ok(format!(
        "tpz_nominal_record({}, {}, [{}], {spread_py}, [{}], {}{declaration_identity})",
        record.py_class_name,
        py_string(&record.source_name),
        decl_fields.join(", "),
        explicit_fields.join(", "),
        py_span(span)
    ))
}

pub(super) fn emit_nominal_record_decl_fields(
    record: &NominalRecordDef<'_>,
    ctx: &Ctx<'_>,
) -> Result<Vec<String>, PyEmitError> {
    record
        .fields
        .iter()
        .map(|field| {
            let default = match &field.default {
                Some(default) => {
                    if let Some(helper_names) = &default.helper_py_names {
                        return Ok(format!(
                            "({}, {}, tpz_host_callable({}, host, {}))",
                            py_string(&mangle(&field.source_name)),
                            py_string(&field.source_name),
                            helper_names.direct,
                            helper_names.cooperative,
                        ));
                    }
                    let default_py = match &default.imported_py {
                        Some(imported_py) => imported_py.clone(),
                        None => match &default.defining_py {
                            Some(defining_py) => defining_py.clone(),
                            None if imported_nominal_record_default_is_self_contained(
                                default.expr,
                            ) =>
                            {
                                emit_expr(default.expr, ctx)?
                            }
                            None => {
                                return Err(PyEmitError::unsupported(
                                    "imported nominal record reference default",
                                )
                                .at(default.expr.span));
                            }
                        },
                    };
                    format!("lambda: {default_py}")
                }
                None => "None".to_string(),
            };
            Ok(format!(
                "({}, {}, {default})",
                py_string(&mangle(&field.source_name)),
                py_string(&field.source_name)
            ))
        })
        .collect()
}

pub(super) fn collect_nominal_record_default_helpers<'a>(
    records: &std::collections::BTreeMap<String, NominalRecordDef<'a>>,
) -> Vec<NominalRecordDefaultHelper<'a>> {
    records
        .values()
        .flat_map(|record| &record.fields)
        .filter_map(|field| {
            let default = field.default.as_ref()?;
            Some(NominalRecordDefaultHelper {
                expr: default.expr,
                names: Rc::clone(default.helper_py_names.as_ref()?),
            })
        })
        .collect()
}

pub(super) fn emit_nominal_record_default_helpers(
    helpers: &[NominalRecordDefaultHelper<'_>],
    ctx: &mut Ctx<'_>,
    out: &mut String,
) -> Result<(), PyEmitError> {
    for helper in helpers {
        emit_nominal_record_default_helper_variant(
            &helper.names.direct,
            helper.expr,
            false,
            ctx,
            out,
        )?;
        emit_nominal_record_default_helper_variant(
            &helper.names.cooperative,
            helper.expr,
            true,
            ctx,
            out,
        )?;
        out.push('\n');
    }
    Ok(())
}

pub(super) fn emit_nominal_record_default_helper_variant(
    helper_py_name: &str,
    expr: &Expr,
    cooperative: bool,
    ctx: &mut Ctx<'_>,
    out: &mut String,
) -> Result<(), PyEmitError> {
    writeln!(out, "def {helper_py_name}(host):").expect("write to string");
    out.push_str("    __tpz_defers = []\n");
    emit_defer_helpers(out, 4);
    if cooperative {
        out.push_str("    if False:\n        yield None\n");
    }
    out.push_str("    try:\n");
    ctx.with_cooperative_yields(cooperative, |ctx| {
        emit_statement_lowered_expr_to_target(expr, "__tpz_result", ctx, 8, out)
    })?;
    out.push_str("    except TpzFault:\n        raise\n");
    out.push_str("    __tpz_run_defers()\n");
    out.push_str("    return __tpz_result\n");
    Ok(())
}

pub(super) fn emit_statement_lowered_nominal_record_construct_to_target(
    record: &NominalRecordDef<'_>,
    spread: Option<&Expr>,
    fields: &[FieldInit],
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
    let decl_fields = emit_nominal_record_decl_fields(record, ctx)?;
    let spread_thunk = match spread {
        Some(spread) => {
            emit_statement_lowered_operand_thunk(spread, "nominal_spread", ctx, indent, out)?
        }
        None => "None".to_string(),
    };
    let mut explicit_fields = Vec::with_capacity(fields.len());
    for field in fields {
        let source_name = ctx.text(field.name.span).to_string();
        let thunk =
            emit_statement_lowered_operand_thunk(&field.value, "nominal_field", ctx, indent, out)?;
        explicit_fields.push(format!(
            "({}, {}, {thunk})",
            py_string(&mangle(&source_name)),
            py_string(&source_name)
        ));
    }
    let runtime = if ctx.cooperative_yields {
        "tpz_nominal_record__co"
    } else {
        "tpz_nominal_record"
    };
    let declaration_identity = record
        .declaration_identity
        .as_ref()
        .map_or(String::new(), |identity| {
            format!(", {}", py_string(identity))
        });
    let call = format!(
        "{runtime}({}, {}, [{}], {spread_thunk}, [{}], {}{declaration_identity})",
        record.py_class_name,
        py_string(&record.source_name),
        decl_fields.join(", "),
        explicit_fields.join(", "),
        py_span(span)
    );
    if ctx.cooperative_yields {
        writeln!(out, "{pad}{target_py} = yield from {call}").expect("write to string");
    } else {
        writeln!(out, "{pad}{target_py} = {call}").expect("write to string");
    }
    Ok(())
}

pub(super) fn emit_statement_lowered_operand_thunk(
    expr: &Expr,
    prefix: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<String, PyEmitError> {
    let pad = " ".repeat(indent);
    let body_pad = " ".repeat(indent + 4);
    let thunk = ctx.fresh_temp(prefix);
    writeln!(out, "{pad}def {thunk}():").expect("write to string");
    let nonlocal_py_names = expression_nonlocal_py_names(expr, ctx);
    emit_nonlocal_declarations(&nonlocal_py_names, indent + 4, out);
    if ctx.cooperative_yields {
        writeln!(out, "{body_pad}if False:").expect("write to string");
        writeln!(out, "{body_pad}    yield None").expect("write to string");
    }
    emit_statement_lowered_expr_to_target(expr, "__tpz_result", ctx, indent + 4, out)?;
    writeln!(out, "{body_pad}return __tpz_result").expect("write to string");
    Ok(thunk)
}

pub(super) fn emit_newtype_construct(
    newtype: &NewtypeDef,
    args: &[CallArg],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let positional = positional_args(args)?;
    if positional.len() != 1 {
        return Err(PyEmitError::unsupported("call argument shape").at(span));
    }
    let value_py = emit_expr(positional[0], ctx)?;
    Ok(render_newtype_construct(newtype, &value_py, span))
}

pub(super) fn render_newtype_construct(newtype: &NewtypeDef, value_py: &str, span: Span) -> String {
    let method_identity = newtype
        .method_identity
        .as_ref()
        .map_or("None".to_string(), |identity| py_string(identity));
    let declaration_identity = newtype
        .declaration_identity
        .as_ref()
        .map_or(String::new(), |identity| {
            format!(", {}", py_string(identity))
        });
    format!(
        "tpz_newtype({}, {value_py}, {}, {method_identity}{declaration_identity})",
        py_string(&newtype.source_name),
        py_span(span)
    )
}

pub(super) fn emit_enum_construct(
    enum_def: &EnumDef,
    variant_name: &str,
    variant: &EnumVariantDef,
    args: &[CallArg],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let positional = positional_args(args)?;
    let values = positional
        .iter()
        .map(|arg| emit_expr(arg, ctx))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(render_enum_construct(
        enum_def,
        variant_name,
        variant,
        values,
        span,
    ))
}

pub(super) fn render_enum_construct(
    enum_def: &EnumDef,
    variant_name: &str,
    variant: &EnumVariantDef,
    values: Vec<String>,
    span: Span,
) -> String {
    if values.len() != variant.arity {
        return format!(
            "tpz_call_order_fault([{}], {}, {})",
            values.join(", "),
            py_string(&format!(
                "enum variant `{}.{}` takes {} payload{}",
                enum_def.source_name,
                variant_name,
                variant.arity,
                if variant.arity == 1 { "" } else { "s" }
            )),
            py_span(span)
        );
    }
    let method_identity = enum_def
        .method_identity
        .as_ref()
        .map_or("None".to_string(), |identity| py_string(identity));
    let declaration_identity = enum_def
        .declaration_identity
        .as_ref()
        .map_or(String::new(), |identity| {
            format!(", {}", py_string(identity))
        });
    format!(
        "tpz_enum({}, {}, {}, [{}], {}, {method_identity}{declaration_identity})",
        py_string(&enum_def.source_name),
        py_string(variant_name),
        variant.variant_index,
        values.join(", "),
        py_span(span)
    )
}

pub(super) fn payloadless_enum_member_construct(
    object: &Expr,
    field: &Ident,
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<Option<String>, PyEmitError> {
    if !matches!(&object.kind, ExprKind::Ident) {
        return Ok(None);
    }
    let enum_name = ctx.text(object.span);
    if ctx.binding_is_bound(enum_name) {
        return Ok(None);
    }
    let Some(enum_def) = ctx.enums.get(enum_name) else {
        return Ok(None);
    };
    let variant_name = ctx.text(field.span);
    let Some(variant) = enum_def.variants.get(variant_name) else {
        return Ok(None);
    };
    if variant.arity != 0 {
        return Ok(None);
    }
    Ok(Some(render_enum_construct(
        enum_def,
        variant_name,
        variant,
        Vec::new(),
        span,
    )))
}
