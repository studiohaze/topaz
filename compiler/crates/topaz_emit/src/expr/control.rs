use crate::*;

pub(crate) fn emit_if_expr(
    expr: &Expr,
    condition: &Expr,
    then_block: &Block,
    else_branch: Option<&Expr>,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let condition = emit_expr(condition, src, aliases, locals, in_loop)?;
    let then_branch = emit_block(then_block, src, aliases, locals, in_loop)?;
    let else_branch = match else_branch {
        Some(branch) => match &branch.kind {
            ExprKind::Block(block) => emit_block(block, src, aliases, locals, in_loop)?,
            _ => emit_expr(branch, src, aliases, locals, in_loop)?,
        },
        None => "{ Value::Unit }".to_string(),
    };
    Ok(format!(
        "if condition_bool(&{condition}, \"if\", {})? {then_branch} else {else_branch}",
        emit_span(expr.span)
    ))
}

pub(crate) fn emit_comprehension_expr(
    expr: &Expr,
    kind: CompKind,
    clauses: &[CompClause],
    body: &CompBody,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        ..
    } = ctx;
    let mut scope = locals.to_vec();
    let inner = emit_comp_clauses(
        clauses,
        &mut scope,
        ComprehensionEmission {
            body,
            kind,
            span: expr.span,
            src,
            aliases,
        },
    )?;
    let span = emit_span(expr.span);
    Ok(match kind {
        CompKind::Array => {
            format!("{{ let mut __cacc: Vec<Value> = Vec::new(); {inner} Value::array(__cacc) }}")
        }
        CompKind::Set => format!(
            "{{ let mut __cacc: Vec<Value> = Vec::new(); {inner} builtin_set_of(__cacc, {span})? }}"
        ),
        CompKind::Map => format!(
            "{{ let mut __cacc: Vec<(Value, Value)> = Vec::new(); {inner} builtin_map_of(__cacc, {span})? }}"
        ),
    })
}

pub(crate) fn emit_range_expr(
    expr: &Expr,
    lo: &Expr,
    hi: &Expr,
    inclusive: bool,
    step: Option<&Expr>,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let lo = emit_expr(lo, src, aliases, locals, in_loop)?;
    let hi = emit_expr(hi, src, aliases, locals, in_loop)?;
    let step = match step {
        Some(step) => format!("Some({})", emit_expr(step, src, aliases, locals, in_loop)?),
        None => "None".to_string(),
    };
    Ok(format!(
        "make_range({lo}, {hi}, {inclusive}, {step}, {})?",
        emit_span(expr.span)
    ))
}
