use crate::*;

pub(super) fn bind_statement_lowered_spread_fault_parts(
    args: &[CallArg],
    span: Span,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<SpreadFaultParts, PyEmitError> {
    let first_spread = args
        .iter()
        .position(|arg| matches!(arg, CallArg::Spread(_)))
        .ok_or_else(|| PyEmitError::unsupported("call argument shape").at(span))?;
    let mut prefix = Vec::new();
    for arg in &args[..first_spread] {
        let CallArg::Positional(expr) = arg else {
            return Err(PyEmitError::unsupported("call argument shape").at(call_arg_span(arg)));
        };
        prefix.push(bind_statement_lowered_call_arg_expr(
            expr, ctx, indent, out,
        )?);
    }

    let region_end = args[first_spread..]
        .iter()
        .position(|arg| matches!(arg, CallArg::Named { .. }))
        .map(|idx| idx + first_spread)
        .unwrap_or(args.len());
    let mut tail = Vec::new();
    let pad = " ".repeat(indent);
    for arg in &args[first_spread..region_end] {
        match arg {
            CallArg::Positional(expr) => {
                tail.push(bind_statement_lowered_call_arg_expr(
                    expr, ctx, indent, out,
                )?);
            }
            CallArg::Spread(expr) => {
                let value = bind_statement_lowered_call_arg_expr(expr, ctx, indent, out)?;
                let spread_value = ctx.fresh_temp("call_spread");
                writeln!(
                    out,
                    "{pad}{spread_value} = tpz_spread_values({value}, {})",
                    py_span(expr.span)
                )
                .expect("write to string");
                tail.push(format!("*{spread_value}"));
            }
            CallArg::Named { .. } => unreachable!("region_end stops at the first named arg"),
        }
    }

    let mut named = Vec::new();
    for arg in &args[region_end..] {
        let CallArg::Named { name, value } = arg else {
            return Err(PyEmitError::unsupported("call argument shape").at(call_arg_span(arg)));
        };
        let value = bind_statement_lowered_call_arg_expr(value, ctx, indent, out)?;
        named.push(format!("({}, {value})", py_string(ctx.text(name.span))));
    }

    Ok((prefix, tail, named))
}

pub(super) fn emit_loop_expr_to_target(
    label: Option<Ident>,
    body: &Block,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let label_py = match label {
        Some(label) => py_string(ctx.text(label.span)),
        None => "None".to_string(),
    };
    let continue_var = ctx.fresh_temp("loop_continue");
    let break_var = ctx.fresh_temp("loop_break");
    writeln!(out, "{pad}while True:").expect("write to string");
    if ctx.cooperative_yields {
        writeln!(out, "{pad}    yield None").expect("write to string");
    }
    writeln!(out, "{pad}    try:").expect("write to string");
    ctx.push_loop_frame(LoopFrameKind::Value);
    let body_result =
        ctx.with_metadata_control_flow(|ctx| emit_block_as_stmt(body, ctx, indent + 8, out));
    ctx.pop_loop_frame();
    body_result?;
    writeln!(out, "{pad}    except TpzLoopContinue as {continue_var}:").expect("write to string");
    writeln!(
        out,
        "{pad}        if {continue_var}.label is None or {continue_var}.label == {label_py}:"
    )
    .expect("write to string");
    writeln!(out, "{pad}            continue").expect("write to string");
    writeln!(out, "{pad}        raise").expect("write to string");
    writeln!(out, "{pad}    except TpzLoopBreak as {break_var}:").expect("write to string");
    writeln!(
        out,
        "{pad}        if {break_var}.label is None or {break_var}.label == {label_py}:"
    )
    .expect("write to string");
    writeln!(out, "{pad}            {target_py} = {break_var}.value").expect("write to string");
    writeln!(out, "{pad}            break").expect("write to string");
    writeln!(out, "{pad}        raise").expect("write to string");
    Ok(())
}

pub(super) fn emit_concurrent_expr_to_target(
    timeout: Option<&Expr>,
    arms: &[ConcurrentArm],
    else_block: Option<&Block>,
    span: Span,
    target: StatementTarget<'_, '_, '_, '_>,
) -> Result<(), PyEmitError> {
    let StatementTarget {
        target_py,
        ctx,
        indent,
        out,
    } = target;
    let mut zero_timeout_single_instant_else = false;
    let mut zero_timeout_multi_instant_else = false;
    if let Some(timeout) = timeout {
        let Some(_) = else_block else {
            return Err(PyEmitError::unsupported("concurrent timeout").at(span));
        };
        let timeout_ms = concurrent_timeout_ms(timeout, ctx)?;
        let all_arms_are_instant = arms
            .iter()
            .all(|arm| expr_is_instant_concurrent_arm(&arm.value));
        zero_timeout_single_instant_else =
            timeout_ms == 0 && arms.len() == 1 && all_arms_are_instant;
        zero_timeout_multi_instant_else = timeout_ms == 0 && arms.len() > 1 && all_arms_are_instant;
    } else if else_block.is_some() {
        return Err(PyEmitError::unsupported("concurrent timeout").at(span));
    }
    for arm in arms {
        if expr_has_bare_return(&arm.value) {
            return Err(PyEmitError::unsupported("`return`/? in a concurrent arm").at(arm.span));
        }
    }
    if zero_timeout_single_instant_else {
        let else_block = else_block.expect("zero-timeout single-instant path has else block");
        if !block_has_try_expr(else_block) {
            return emit_zero_timeout_single_instant_concurrent_expr_to_target(
                &arms[0], else_block, target_py, ctx, indent, out,
            );
        }
    }
    if zero_timeout_multi_instant_else {
        let else_block = else_block.expect("zero-timeout multi-instant path has else block");
        let mutated_outer_roots = concurrent_else_mutated_outer_roots(else_block, ctx);
        return with_mutated_outer_metadata_boundary(mutated_outer_roots, ctx, |ctx| {
            emit_statement_lowered_block_expr_to_target(else_block, target_py, ctx, indent, out)
        });
    }
    if let (Some(timeout), Some(else_block)) = (timeout, else_block) {
        return emit_timeout_concurrent_expr_to_target(
            timeout, arms, else_block, target_py, ctx, indent, out,
        );
    }
    emit_no_timeout_concurrent_expr_to_target(arms, target_py, ctx, indent, out)
}

pub(super) fn emit_zero_timeout_single_instant_concurrent_expr_to_target(
    arm: &ConcurrentArm,
    else_block: &Block,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let inner_pad = " ".repeat(indent + 4);
    let name = ctx.text(arm.name.span);
    let arm_target = ctx.fresh_temp(&format!("concurrent_{}", mangle(name)));
    let shape = concurrent_record_shape(std::slice::from_ref(arm), ctx.map);
    writeln!(out, "{pad}try:").expect("write to string");
    if !emit_expr_to_target_if_needed(&arm.value, &arm_target, ctx, indent + 4, out)? {
        let value_py = emit_expr(&arm.value, ctx)?;
        writeln!(
            out,
            "{inner_pad}{arm_target} = {value_py}  # concurrent {}",
            py_comment_name(name)
        )
        .expect("write to string");
    }
    writeln!(
        out,
        "{inner_pad}{target_py} = {}({arm_target})",
        record_class_name(&shape)
    )
    .expect("write to string");
    writeln!(out, "{pad}except TpzFault:").expect("write to string");
    let mutated_outer_roots = concurrent_else_mutated_outer_roots(else_block, ctx);
    with_mutated_outer_metadata_boundary(mutated_outer_roots, ctx, |ctx| {
        emit_statement_lowered_block_expr_to_target(else_block, target_py, ctx, indent + 4, out)
    })?;
    Ok(())
}

pub(super) fn emit_no_timeout_concurrent_expr_to_target(
    arms: &[ConcurrentArm],
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let shape = concurrent_record_shape(arms, ctx.map);
    let mut thunks = Vec::with_capacity(arms.len());
    let mutated_outer_roots = concurrent_arm_mutated_outer_roots(arms, ctx);
    with_mutated_outer_metadata_boundary(mutated_outer_roots, ctx, |ctx| {
        for arm in arms {
            let name = ctx.text(arm.name.span).to_string();
            let thunk = ctx.fresh_temp(&format!("concurrent_{}_arm", mangle(&name)));
            writeln!(
                out,
                "{pad}def {thunk}():  # concurrent {}",
                py_comment_name(&name)
            )
            .expect("write to string");
            emit_concurrent_arm_generator_body(&arm.value, ctx, indent + 4, out)?;
            thunks.push(format!("({}, {thunk})", py_string(&name)));
        }
        Ok(())
    })?;
    let values = ctx.fresh_temp("concurrent_values");
    writeln!(
        out,
        "{pad}{values} = tpz_concurrent_join([{}])",
        thunks.join(", ")
    )
    .expect("write to string");
    writeln!(
        out,
        "{pad}{target_py} = {}(*{values})",
        record_class_name(&shape)
    )
    .expect("write to string");
    Ok(())
}

pub(super) fn emit_timeout_concurrent_expr_to_target(
    timeout: &Expr,
    arms: &[ConcurrentArm],
    else_block: &Block,
    target_py: &str,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let timeout_ms = concurrent_timeout_ms(timeout, ctx)?;
    let shape = concurrent_record_shape(arms, ctx.map);
    let mut thunks = Vec::with_capacity(arms.len());
    let mutated_outer_roots = concurrent_arm_mutated_outer_roots(arms, ctx);
    with_mutated_outer_metadata_boundary(mutated_outer_roots, ctx, |ctx| {
        for arm in arms {
            let name = ctx.text(arm.name.span).to_string();
            let thunk = ctx.fresh_temp(&format!("concurrent_{}_arm", mangle(&name)));
            writeln!(
                out,
                "{pad}def {thunk}():  # concurrent {}",
                py_comment_name(&name)
            )
            .expect("write to string");
            emit_concurrent_arm_generator_body(&arm.value, ctx, indent + 4, out)?;
            thunks.push(format!("({}, {thunk})", py_string(&name)));
        }
        Ok(())
    })?;

    let else_thunk = ctx.fresh_temp("concurrent_else");
    writeln!(out, "{pad}def {else_thunk}():").expect("write to string");
    emit_nonlocal_declarations(
        &concurrent_else_nonlocal_py_names(else_block, ctx),
        indent + 4,
        out,
    );
    writeln!(out, "{pad}    if False:").expect("write to string");
    writeln!(out, "{pad}        yield None").expect("write to string");
    let else_target = ctx.fresh_temp("concurrent_else_value");
    let mutated_outer_roots = concurrent_else_mutated_outer_roots(else_block, ctx);
    with_mutated_outer_metadata_boundary(mutated_outer_roots, ctx, |ctx| {
        emit_statement_lowered_block_expr_to_target(else_block, &else_target, ctx, indent + 4, out)
    })?;
    writeln!(out, "{pad}    return {else_target}").expect("write to string");

    let joined = ctx.fresh_temp("concurrent_joined");
    let value = ctx.fresh_temp("concurrent_timeout_value");
    writeln!(
        out,
        "{pad}{joined}, {value} = tpz_concurrent_join_timeout([{}], {timeout_ms}, {else_thunk})",
        thunks.join(", ")
    )
    .expect("write to string");
    writeln!(out, "{pad}if {joined}:").expect("write to string");
    writeln!(
        out,
        "{pad}    {target_py} = {}(*{value})",
        record_class_name(&shape)
    )
    .expect("write to string");
    writeln!(out, "{pad}else:").expect("write to string");
    writeln!(out, "{pad}    {target_py} = {value}").expect("write to string");
    Ok(())
}

pub(super) fn concurrent_else_nonlocal_py_names(block: &Block, ctx: &Ctx<'_>) -> Vec<String> {
    let mut scope = NestedForwardScope::default();
    let mut names = BTreeSet::new();
    scope.push_frame();
    collect_nonlocal_assignments_in_block(block, ctx, &mut scope, &mut names);
    scope.pop_frame();
    names.into_iter().collect()
}

#[derive(Clone)]
pub(super) struct MutationScope {
    pub(super) frames: Vec<MutationFrame>,
    pub(super) follow_known_calls: bool,
}

impl Default for MutationScope {
    fn default() -> Self {
        Self {
            frames: Vec::new(),
            follow_known_calls: true,
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct MutationFrame {
    pub(super) bindings: BTreeSet<String>,
    pub(super) functions: BTreeSet<String>,
    pub(super) aliases: BTreeMap<String, BTreeSet<String>>,
}

impl MutationScope {
    pub(super) fn direct_function_body() -> Self {
        Self {
            frames: Vec::new(),
            follow_known_calls: false,
        }
    }

    pub(super) fn push_frame(&mut self) {
        self.frames.push(MutationFrame::default());
    }

    pub(super) fn push_binding_frame(&mut self, bindings: BTreeSet<String>) {
        self.frames.push(MutationFrame {
            bindings,
            ..MutationFrame::default()
        });
    }

    pub(super) fn pop_frame(&mut self) {
        self.frames.pop();
    }

    pub(super) fn contains(&self, name: &str) -> bool {
        self.frames
            .iter()
            .rev()
            .any(|frame| frame.bindings.contains(name) || frame.functions.contains(name))
    }

    pub(super) fn insert_binding(&mut self, name: String) {
        if let Some(frame) = self.frames.last_mut() {
            frame.bindings.insert(name);
        }
    }

    pub(super) fn insert_function(&mut self, name: String) {
        if let Some(frame) = self.frames.last_mut() {
            frame.functions.insert(name);
        }
    }

    pub(super) fn insert_pattern(&mut self, pattern: &Pattern, map: &SourceMap) {
        let mut names = BTreeSet::new();
        collect_pattern_binding_names(pattern, map, &mut names);
        if let Some(frame) = self.frames.last_mut() {
            frame.bindings.extend(names);
        }
    }

    pub(super) fn insert_alias(&mut self, name: String, outer_root: String) {
        self.insert_aliases(name, std::iter::once(outer_root).collect());
    }

    pub(super) fn insert_aliases(&mut self, name: String, outer_roots: BTreeSet<String>) {
        if let Some(frame) = self.frames.last_mut() {
            frame.bindings.insert(name.clone());
            if outer_roots.is_empty() {
                frame.aliases.remove(&name);
            } else {
                frame.aliases.insert(name, outer_roots);
            }
        }
    }

    pub(super) fn rebind_alias(&mut self, name: &str, outer_roots: BTreeSet<String>) {
        for frame in self.frames.iter_mut().rev() {
            if frame.bindings.contains(name) {
                if outer_roots.is_empty() {
                    frame.aliases.remove(name);
                } else {
                    frame.aliases.insert(name.to_string(), outer_roots);
                }
                return;
            }
            if frame.functions.contains(name) {
                return;
            }
        }
    }

    pub(super) fn aliases(&self, name: &str) -> Option<&BTreeSet<String>> {
        self.alias_in_frames(name, self.frames.iter().rev())
    }

    pub(super) fn aliases_outside_current_frame(&self, name: &str) -> Option<&BTreeSet<String>> {
        self.alias_in_frames(name, self.frames.iter().rev().skip(1))
    }

    pub(super) fn alias_in_frames<'a>(
        &'a self,
        name: &str,
        frames: impl Iterator<Item = &'a MutationFrame>,
    ) -> Option<&'a BTreeSet<String>> {
        for frame in frames {
            if let Some(root) = frame.aliases.get(name) {
                return Some(root);
            }
            if frame.bindings.contains(name) || frame.functions.contains(name) {
                return None;
            }
        }
        None
    }

    pub(super) fn join_aliases_from(&mut self, branches: &[Self]) {
        for (frame_index, frame) in self.frames.iter_mut().enumerate() {
            let names = frame.bindings.clone();
            for name in names {
                let roots = branches
                    .iter()
                    .filter_map(|branch| branch.frames.get(frame_index))
                    .filter_map(|branch_frame| branch_frame.aliases.get(&name))
                    .flat_map(|roots| roots.iter().cloned())
                    .collect::<BTreeSet<_>>();
                if roots.is_empty() {
                    frame.aliases.remove(&name);
                } else {
                    frame.aliases.insert(name, roots);
                }
            }
        }
    }

    pub(super) fn alias_roots_equal(&self, other: &Self) -> bool {
        self.frames.len() == other.frames.len()
            && self
                .frames
                .iter()
                .zip(&other.frames)
                .all(|(left, right)| left.aliases == right.aliases)
    }

    pub(super) fn contains_outside_current_frame(&self, name: &str) -> bool {
        self.frames
            .iter()
            .rev()
            .skip(1)
            .any(|frame| frame.bindings.contains(name) || frame.functions.contains(name))
    }
}

pub(super) fn collect_zero_or_more_alias_flow(
    scope: &mut MutationScope,
    mut collect_iteration: impl FnMut(&mut MutationScope),
) {
    let entry = scope.clone();
    let mut joined = entry.clone();
    loop {
        let mut iteration = joined.clone();
        collect_iteration(&mut iteration);
        let mut next = entry.clone();
        next.join_aliases_from(&[entry.clone(), joined.clone(), iteration]);
        if next.alias_roots_equal(&joined) {
            scope.join_aliases_from(&[next]);
            return;
        }
        joined = next;
    }
}

pub(super) fn collect_mutated_outer_roots_in_call_args(
    args: &[CallArg],
    ctx: &Ctx<'_>,
    scope: &mut MutationScope,
    out: &mut BTreeSet<String>,
) {
    for arg in args {
        let value = match arg {
            CallArg::Positional(value) | CallArg::Spread(value) => value,
            CallArg::Named { value, .. } => value,
        };
        collect_mutated_outer_roots_in_expr(value, ctx, scope, out);
    }
}

pub(super) fn mutated_collection_parameter_indices(
    decl: &FunctionDecl,
    ctx: &Ctx<'_>,
    follow_known_calls: bool,
) -> BTreeSet<usize> {
    let mut scope = if follow_known_calls {
        MutationScope::default()
    } else {
        MutationScope::direct_function_body()
    };
    let mut param_names = Vec::with_capacity(decl.params.len());
    scope.push_frame();
    for param in &decl.params {
        let name = ctx.text(param.name.span).to_string();
        let is_collection =
            param.variadic || is_mutable_collection_shape(receiver_shape_from_type(&param.ty, ctx));
        if is_collection {
            scope.insert_alias(name.clone(), name.clone());
        } else {
            scope.insert_binding(name.clone());
        }
        param_names.push(name);
    }

    let mut mutated_names = BTreeSet::new();
    collect_mutated_outer_roots_in_block(&decl.body, ctx, &mut scope, &mut mutated_names);
    param_names
        .iter()
        .enumerate()
        .filter_map(|(index, name)| mutated_names.contains(name).then_some(index))
        .collect()
}

pub(super) fn directly_mutated_collection_parameter_indices(
    decl: &FunctionDecl,
    ctx: &Ctx<'_>,
) -> BTreeSet<usize> {
    mutated_collection_parameter_indices(decl, ctx, false)
}

pub(super) fn mutated_lambda_collection_parameter_indices(
    params: &[LambdaParam],
    body: &Expr,
    contextual_ty: Option<&Type>,
    ctx: &Ctx<'_>,
) -> BTreeSet<usize> {
    let mut scope = MutationScope::default();
    let mut param_names = Vec::with_capacity(params.len());
    let contextual_collection_params = contextual_ty
        .map(|ty| function_collection_parameter_indices_from_type(ty, ctx))
        .unwrap_or_default();
    scope.push_frame();
    for (index, param) in params.iter().enumerate() {
        let name = ctx.text(param.name.span).to_string();
        let is_collection = is_mutable_collection_shape(
            param
                .ty
                .as_ref()
                .and_then(|ty| receiver_shape_from_type(ty, ctx)),
        ) || (param.ty.is_none()
            && contextual_collection_params.contains(&index));
        if is_collection {
            scope.insert_alias(name.clone(), name.clone());
        } else {
            scope.insert_binding(name.clone());
        }
        param_names.push(name);
    }

    let mut mutated_names = BTreeSet::new();
    collect_mutated_outer_roots_in_expr(body, ctx, &mut scope, &mut mutated_names);
    scope.pop_frame();
    param_names
        .iter()
        .enumerate()
        .filter_map(|(index, name)| mutated_names.contains(name).then_some(index))
        .collect()
}

pub(super) fn function_collection_parameter_indices_from_type(
    ty: &Type,
    ctx: &Ctx<'_>,
) -> BTreeSet<usize> {
    if let Some(alias) = checked_alias_for_ast_type(ty, ctx)
        && let CheckType::Func { params, .. } = &alias.body
    {
        return params
            .iter()
            .enumerate()
            .filter_map(|(index, ty)| {
                is_mutable_collection_shape(receiver_shape_from_checked_type(ty)).then_some(index)
            })
            .collect();
    }
    let TypeKind::Function { params, .. } = &ty.kind else {
        return BTreeSet::new();
    };
    params
        .iter()
        .enumerate()
        .filter_map(|(index, param)| {
            is_mutable_collection_shape(receiver_shape_from_type(&param.ty, ctx)).then_some(index)
        })
        .collect()
}

pub(super) fn concurrent_arm_mutated_outer_roots(
    arms: &[ConcurrentArm],
    ctx: &Ctx<'_>,
) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for arm in arms {
        let mut scope = MutationScope::default();
        scope.push_frame();
        collect_mutated_outer_roots_in_expr(&arm.value, ctx, &mut scope, &mut names);
        scope.pop_frame();
    }
    names
}

pub(super) fn concurrent_else_mutated_outer_roots(
    block: &Block,
    ctx: &Ctx<'_>,
) -> BTreeSet<String> {
    let mut scope = MutationScope::default();
    let mut names = BTreeSet::new();
    scope.push_frame();
    collect_mutated_outer_roots_in_block(block, ctx, &mut scope, &mut names);
    scope.pop_frame();
    names
}

pub(super) fn immediate_lambda_mutated_outer_roots(
    params: &[LambdaParam],
    body: &Expr,
    args: &[CallArg],
    ctx: &Ctx<'_>,
) -> BTreeSet<String> {
    let mut scope = MutationScope::default();
    let mut names = BTreeSet::new();
    scope.push_frame();
    push_immediate_lambda_parameter_frame(params, args, ctx, &mut scope);
    collect_mutated_outer_roots_in_expr(body, ctx, &mut scope, &mut names);
    scope.pop_frame();
    names
}

pub(super) fn push_immediate_lambda_parameter_frame(
    params: &[LambdaParam],
    args: &[CallArg],
    ctx: &Ctx<'_>,
    scope: &mut MutationScope,
) {
    let mut bindings = BTreeSet::new();
    for param in params {
        bindings.insert(ctx.text(param.name.span).to_string());
    }
    scope.push_binding_frame(bindings);

    for (param, arg) in params.iter().zip(args) {
        let CallArg::Positional(arg) = arg else {
            continue;
        };
        let Some(argument_name) = assignment_root_source_name(arg, ctx.map) else {
            continue;
        };
        let outer_roots =
            mutable_collection_outer_roots_outside_current_frame(argument_name, ctx, scope);
        if !outer_roots.is_empty() {
            scope.insert_aliases(ctx.text(param.name.span).to_string(), outer_roots);
        }
    }
}

pub(super) fn mutable_collection_outer_roots_for_argument(
    argument: &Expr,
    ctx: &Ctx<'_>,
    scope: &MutationScope,
) -> BTreeSet<String> {
    let Some(name) = assignment_root_source_name(argument, ctx.map) else {
        return BTreeSet::new();
    };
    if let Some(roots) = scope.aliases(&name) {
        return roots.clone();
    }
    if !scope.contains(&name)
        && ctx.binding_is_mutable(&name)
        && ctx.binding_is_mutable_collection(&name)
    {
        return std::iter::once(name).collect();
    }
    BTreeSet::new()
}

pub(super) fn mutable_collection_outer_roots_outside_current_frame(
    name: String,
    ctx: &Ctx<'_>,
    scope: &MutationScope,
) -> BTreeSet<String> {
    if let Some(roots) = scope.aliases_outside_current_frame(&name) {
        return roots.clone();
    }
    if !scope.contains_outside_current_frame(&name)
        && ctx.binding_is_mutable(&name)
        && ctx.binding_is_mutable_collection(&name)
    {
        return std::iter::once(name).collect();
    }
    BTreeSet::new()
}

pub(super) fn call_argument_for_parameter<'a>(
    args: &'a [CallArg],
    info: &FunctionInfo,
    parameter_index: usize,
    ctx: &Ctx<'_>,
) -> Option<&'a Expr> {
    let parameter = info.params.get(parameter_index)?;
    if let Some(value) = args.iter().find_map(|arg| match arg {
        CallArg::Named { name, value } if ctx.text(name.span) == parameter.source_name.as_str() => {
            Some(value)
        }
        _ => None,
    }) {
        return Some(value);
    }
    if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        return None;
    }
    args.iter()
        .filter_map(|arg| match arg {
            CallArg::Positional(value) => Some(value),
            _ => None,
        })
        .nth(parameter_index)
}

pub(super) fn known_function_call_mutated_roots_in_scope(
    callee: &Expr,
    args: &[CallArg],
    ctx: &Ctx<'_>,
    scope: &MutationScope,
) -> BTreeSet<String> {
    if known_function_callee_shadow_root(callee, ctx.map).is_some_and(|name| scope.contains(&name))
        && !scope_shadow_has_callable_effect(callee, ctx)
    {
        return BTreeSet::new();
    }
    let Some(info) = ctx.function_effect_info_for_callee(callee) else {
        return BTreeSet::new();
    };
    info.mutated_collection_params
        .iter()
        .filter_map(|index| call_argument_for_parameter(args, &info, *index, ctx))
        .flat_map(|argument| mutable_collection_outer_roots_for_argument(argument, ctx, scope))
        .collect()
}

pub(super) fn scope_shadow_has_callable_effect(callee: &Expr, ctx: &Ctx<'_>) -> bool {
    match &callee.kind {
        ExprKind::Ident => ctx.binding_callable_info(ctx.text(callee.span)).is_some(),
        ExprKind::Paren(inner) => scope_shadow_has_callable_effect(inner, ctx),
        _ => false,
    }
}

pub(super) fn known_function_call_mutated_outer_roots(
    callee: &Expr,
    args: &[CallArg],
    ctx: &Ctx<'_>,
) -> BTreeSet<String> {
    let mut scope = MutationScope::default();
    scope.push_frame();
    known_function_call_mutated_roots_in_scope(callee, args, ctx, &scope)
}

pub(super) fn known_function_callee_shadow_root(callee: &Expr, map: &SourceMap) -> Option<String> {
    match &callee.kind {
        ExprKind::Ident => Some(text_in_map(map, callee.span).to_string()),
        ExprKind::Member { object, .. } => direct_source_identifier_name(object, map),
        ExprKind::Paren(inner) => known_function_callee_shadow_root(inner, map),
        _ => None,
    }
}

pub(super) fn with_mutated_outer_metadata_boundary<T>(
    names: BTreeSet<String>,
    ctx: &mut Ctx<'_>,
    f: impl FnOnce(&mut Ctx<'_>) -> T,
) -> T {
    let result = ctx.with_flow_static_metadata_blocked_names(names.clone(), f);
    for name in &names {
        ctx.clear_collection_alias_value_metadata(name);
    }
    result
}

pub(super) fn collect_mutated_outer_roots_in_block(
    block: &Block,
    ctx: &Ctx<'_>,
    scope: &mut MutationScope,
    out: &mut BTreeSet<String>,
) {
    scope.push_frame();
    let mut deferred_actions = Vec::new();
    for stmt in &block.stmts {
        if let StmtKind::Function(decl) = &stmt.kind {
            scope.insert_function(text_in_map(ctx.map, decl.name.span).to_string());
        }
    }
    for stmt in &block.stmts {
        if let StmtKind::Defer(action) = &stmt.kind {
            deferred_actions.push(action);
        } else {
            collect_mutated_outer_roots_in_stmt(stmt, ctx, scope, out);
        }
    }
    if let Some(tail) = block.tail.as_deref() {
        collect_mutated_outer_roots_in_expr(tail, ctx, scope, out);
    }
    for action in deferred_actions.into_iter().rev() {
        collect_mutated_outer_roots_in_expr(action, ctx, scope, out);
    }
    scope.pop_frame();
}

pub(super) fn collect_mutated_outer_roots_in_stmt(
    stmt: &Stmt,
    ctx: &Ctx<'_>,
    scope: &mut MutationScope,
    out: &mut BTreeSet<String>,
) {
    match &stmt.kind {
        StmtKind::Export(inner) => collect_mutated_outer_roots_in_stmt(inner, ctx, scope, out),
        StmtKind::Function(decl) => {
            scope.insert_function(text_in_map(ctx.map, decl.name.span).to_string());
        }
        StmtKind::Let {
            mutable,
            pattern,
            value,
            ..
        } => {
            collect_mutated_outer_roots_in_expr(value, ctx, scope, out);
            let mutable_alias_name = (*mutable)
                .then(|| direct_binding_pattern_name(pattern, ctx))
                .flatten();
            let outer_roots = mutable_collection_outer_roots_for_argument(value, ctx, scope);
            if let Some(name) = mutable_alias_name
                && !outer_roots.is_empty()
            {
                scope.insert_aliases(name, outer_roots);
            } else {
                scope.insert_pattern(pattern, ctx.map);
            }
        }
        StmtKind::Const { name, value, .. } => {
            collect_mutated_outer_roots_in_expr(value, ctx, scope, out);
            scope.insert_binding(text_in_map(ctx.map, name.span).to_string());
        }
        StmtKind::Assign { target, op, value } => {
            let assignment_root = assignment_root_source_name(target, ctx.map);
            let direct_target = direct_source_identifier_name(target, ctx.map);
            if let Some(root) = assignment_root.as_deref() {
                if let Some(outer_roots) = scope.aliases(root) {
                    if direct_target.is_none() || !matches!(op, AssignOp::Assign) {
                        out.extend(outer_roots.iter().cloned());
                    }
                } else if !scope.contains(root) && ctx.binding_is_mutable(root) {
                    out.insert(root.to_string());
                }
            }
            collect_mutated_outer_roots_in_expr(target, ctx, scope, out);
            collect_mutated_outer_roots_in_expr(value, ctx, scope, out);
            if matches!(op, AssignOp::Assign)
                && let Some(target_name) = direct_target
            {
                let outer_roots = mutable_collection_outer_roots_for_argument(value, ctx, scope);
                scope.rebind_alias(&target_name, outer_roots);
            }
        }
        StmtKind::Defer(value) => {
            collect_mutated_outer_roots_in_expr(value, ctx, scope, out);
        }
        StmtKind::Return(Some(value))
        | StmtKind::Break {
            value: Some(value), ..
        }
        | StmtKind::Expr(value) => {
            collect_mutated_outer_roots_in_expr(value, ctx, scope, out);
        }
        StmtKind::Using { name, value, body } => {
            collect_mutated_outer_roots_in_expr(value, ctx, scope, out);
            let mut frame = BTreeSet::new();
            frame.insert(text_in_map(ctx.map, name.span).to_string());
            scope.push_binding_frame(frame);
            collect_mutated_outer_roots_in_block(body, ctx, scope, out);
            scope.pop_frame();
        }
        StmtKind::While { cond, body } => {
            collect_mutated_outer_roots_in_expr(cond, ctx, scope, out);
            collect_zero_or_more_alias_flow(scope, |iteration_scope| {
                collect_mutated_outer_roots_in_block(body, ctx, iteration_scope, out);
                collect_mutated_outer_roots_in_expr(cond, ctx, iteration_scope, out);
            });
        }
        StmtKind::Return(None)
        | StmtKind::Break { value: None, .. }
        | StmtKind::Continue { .. }
        | StmtKind::Import(_)
        | StmtKind::TypeAlias(_)
        | StmtKind::Enum(_)
        | StmtKind::Record(_)
        | StmtKind::Newtype(_)
        | StmtKind::Impl(_)
        | StmtKind::Protocol(_) => {}
    }
}

pub(super) fn collect_mutated_outer_roots_in_expr(
    expr: &Expr,
    ctx: &Ctx<'_>,
    scope: &mut MutationScope,
    out: &mut BTreeSet<String>,
) {
    match &expr.kind {
        ExprKind::Paren(inner) | ExprKind::Try(inner) => {
            collect_mutated_outer_roots_in_expr(inner, ctx, scope, out);
        }
        ExprKind::Block(block) => collect_mutated_outer_roots_in_block(block, ctx, scope, out),
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            collect_mutated_outer_roots_in_expr(cond, ctx, scope, out);
            let baseline = scope.clone();
            let mut branches = Vec::with_capacity(2);
            let mut then_scope = baseline.clone();
            collect_mutated_outer_roots_in_block(then_block, ctx, &mut then_scope, out);
            branches.push(then_scope);
            if let Some(else_branch) = else_branch.as_deref() {
                let mut else_scope = baseline.clone();
                collect_mutated_outer_roots_in_expr(else_branch, ctx, &mut else_scope, out);
                branches.push(else_scope);
            } else {
                branches.push(baseline);
            }
            scope.join_aliases_from(&branches);
        }
        ExprKind::Match { scrutinee, cases } => {
            collect_mutated_outer_roots_in_expr(scrutinee, ctx, scope, out);
            let baseline = scope.clone();
            let mut branches = Vec::with_capacity(cases.len() + 1);
            for case in cases {
                let mut branch_scope = baseline.clone();
                let mut bindings = BTreeSet::new();
                collect_pattern_binding_names(&case.pattern, ctx.map, &mut bindings);
                branch_scope.push_binding_frame(bindings);
                if let Some(guard) = &case.guard {
                    collect_mutated_outer_roots_in_expr(guard, ctx, &mut branch_scope, out);
                }
                match &case.body {
                    CaseArmBody::Expr(expr) => {
                        collect_mutated_outer_roots_in_expr(expr, ctx, &mut branch_scope, out)
                    }
                    CaseArmBody::Return { value, .. } => {
                        if let Some(value) = value {
                            collect_mutated_outer_roots_in_expr(value, ctx, &mut branch_scope, out);
                        }
                    }
                }
                branch_scope.pop_frame();
                branches.push(branch_scope);
            }
            if !cases.last().is_some_and(match_case_is_unguarded_catch_all) {
                branches.push(baseline);
            }
            scope.join_aliases_from(&branches);
        }
        ExprKind::For {
            pattern,
            iter,
            body,
        } => {
            collect_mutated_outer_roots_in_expr(iter, ctx, scope, out);
            let mut frame = BTreeSet::new();
            collect_pattern_binding_names(pattern, ctx.map, &mut frame);
            collect_zero_or_more_alias_flow(scope, |iteration_scope| {
                iteration_scope.push_binding_frame(frame.clone());
                collect_mutated_outer_roots_in_block(body, ctx, iteration_scope, out);
                iteration_scope.pop_frame();
            });
        }
        ExprKind::Loop { body, .. } => {
            collect_zero_or_more_alias_flow(scope, |iteration_scope| {
                collect_mutated_outer_roots_in_block(body, ctx, iteration_scope, out);
            });
        }
        ExprKind::Concurrent {
            timeout,
            arms,
            else_block,
        } => {
            if let Some(timeout) = timeout.as_deref() {
                collect_mutated_outer_roots_in_expr(timeout, ctx, scope, out);
            }
            let baseline = scope.clone();
            let mut branches = Vec::with_capacity(arms.len() + 1);
            for arm in arms {
                let mut arm_scope = baseline.clone();
                collect_mutated_outer_roots_in_expr(&arm.value, ctx, &mut arm_scope, out);
                branches.push(arm_scope);
            }
            if let Some(else_block) = else_block.as_deref() {
                let mut else_scope = baseline.clone();
                collect_mutated_outer_roots_in_block(else_block, ctx, &mut else_scope, out);
                branches.push(else_scope);
            } else {
                branches.push(baseline);
            }
            scope.join_aliases_from(&branches);
        }
        ExprKind::Call { callee, args, .. } => {
            let optional_receiver = matches!(&callee.kind, ExprKind::OptionalAccess { .. });
            if let Some(root) = collection_mutation_root_for_call(callee, ctx)
                && !scope.contains(&root)
            {
                out.insert(root);
            }
            if let Some(receiver) = collection_mutation_receiver_source_name(callee, ctx.map)
                && let Some(roots) = scope.aliases(&receiver)
            {
                out.extend(roots.iter().cloned());
            }
            if scope.follow_known_calls {
                out.extend(known_function_call_mutated_roots_in_scope(
                    callee, args, ctx, scope,
                ));
            }
            if let Some((params, body)) = immediate_lambda_callee(callee) {
                push_immediate_lambda_parameter_frame(params, args, ctx, scope);
                collect_mutated_outer_roots_in_expr(body, ctx, scope, out);
                scope.pop_frame();
            } else {
                collect_mutated_outer_roots_in_expr(callee, ctx, scope, out);
            }
            if optional_receiver {
                let baseline = scope.clone();
                let mut called = baseline.clone();
                collect_mutated_outer_roots_in_call_args(args, ctx, &mut called, out);
                scope.join_aliases_from(&[baseline, called]);
            } else {
                collect_mutated_outer_roots_in_call_args(args, ctx, scope, out);
            }
        }
        ExprKind::Member { object, .. } | ExprKind::OptionalAccess { object, .. } => {
            collect_mutated_outer_roots_in_expr(object, ctx, scope, out);
        }
        ExprKind::Index { object, index } => {
            collect_mutated_outer_roots_in_expr(object, ctx, scope, out);
            collect_mutated_outer_roots_in_expr(index, ctx, scope, out);
        }
        ExprKind::Range { lo, hi, step, .. } => {
            collect_mutated_outer_roots_in_expr(lo, ctx, scope, out);
            collect_mutated_outer_roots_in_expr(hi, ctx, scope, out);
            if let Some(step) = step.as_deref() {
                collect_mutated_outer_roots_in_expr(step, ctx, scope, out);
            }
        }
        ExprKind::Unary { operand, .. } => {
            collect_mutated_outer_roots_in_expr(operand, ctx, scope, out);
        }
        ExprKind::Binary { op, lhs, rhs } => {
            collect_mutated_outer_roots_in_expr(lhs, ctx, scope, out);
            if matches!(op, BinaryOp::And | BinaryOp::Or | BinaryOp::Coalesce) {
                let baseline = scope.clone();
                let mut rhs_scope = baseline.clone();
                collect_mutated_outer_roots_in_expr(rhs, ctx, &mut rhs_scope, out);
                scope.join_aliases_from(&[baseline, rhs_scope]);
            } else {
                collect_mutated_outer_roots_in_expr(rhs, ctx, scope, out);
            }
        }
        ExprKind::Compose { lhs, rhs } => {
            collect_mutated_outer_roots_in_expr(lhs, ctx, scope, out);
            collect_mutated_outer_roots_in_expr(rhs, ctx, scope, out);
        }
        ExprKind::Pipe { lhs, rhs } => {
            collect_mutated_outer_roots_in_expr(lhs, ctx, scope, out);
            if let PipeRhs::Expr(stage) = rhs.as_ref() {
                collect_mutated_outer_roots_in_expr(stage, ctx, scope, out);
            }
        }
        ExprKind::RecordLiteral { fields } => {
            for field in fields {
                collect_mutated_outer_roots_in_expr(&field.value, ctx, scope, out);
            }
        }
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            collect_mutated_outer_roots_in_expr(base, ctx, scope, out);
            if let Some(spread) = spread.as_deref() {
                collect_mutated_outer_roots_in_expr(spread, ctx, scope, out);
            }
            for field in fields {
                collect_mutated_outer_roots_in_expr(&field.value, ctx, scope, out);
            }
        }
        ExprKind::Array(elements) => {
            for element in elements {
                match element {
                    ArrayElement::Expr(value) | ArrayElement::Spread(value) => {
                        collect_mutated_outer_roots_in_expr(value, ctx, scope, out);
                    }
                }
            }
        }
        ExprKind::SetLiteral(elements) => {
            for value in elements {
                collect_mutated_outer_roots_in_expr(value, ctx, scope, out);
            }
        }
        ExprKind::MapLiteral(entries) => {
            for (key, value) in entries {
                collect_mutated_outer_roots_in_expr(key, ctx, scope, out);
                collect_mutated_outer_roots_in_expr(value, ctx, scope, out);
            }
        }
        ExprKind::Comprehension {
            clauses,
            body,
            kind: _,
        } => {
            collect_zero_or_more_alias_flow(scope, |iteration_scope| {
                iteration_scope.push_frame();
                for clause in clauses {
                    match clause {
                        CompClause::For { pattern, iter } => {
                            collect_mutated_outer_roots_in_expr(iter, ctx, iteration_scope, out);
                            iteration_scope.insert_pattern(pattern, ctx.map);
                        }
                        CompClause::If(cond) => {
                            collect_mutated_outer_roots_in_expr(cond, ctx, iteration_scope, out);
                        }
                    }
                }
                match body.as_ref() {
                    CompBody::Elem(value) => {
                        collect_mutated_outer_roots_in_expr(value, ctx, iteration_scope, out);
                    }
                    CompBody::Entry { key, value } => {
                        collect_mutated_outer_roots_in_expr(key, ctx, iteration_scope, out);
                        collect_mutated_outer_roots_in_expr(value, ctx, iteration_scope, out);
                    }
                }
                iteration_scope.pop_frame();
            });
        }
        ExprKind::String(lit) => {
            for part in &lit.parts {
                if let StringPart::Interpolation(value) = part {
                    collect_mutated_outer_roots_in_expr(value, ctx, scope, out);
                }
            }
        }
        ExprKind::Lambda { .. }
        | ExprKind::Int
        | ExprKind::Float
        | ExprKind::Duration(_)
        | ExprKind::Bool(_)
        | ExprKind::Null
        | ExprKind::Unit
        | ExprKind::Ident
        | ExprKind::Placeholder => {}
    }
}

pub(super) fn assignment_root_source_name(target: &Expr, map: &SourceMap) -> Option<String> {
    match &target.kind {
        ExprKind::Ident => Some(text_in_map(map, target.span).to_string()),
        ExprKind::Paren(inner) => assignment_root_source_name(inner, map),
        ExprKind::Member { object, .. }
        | ExprKind::OptionalAccess { object, .. }
        | ExprKind::Index { object, .. } => assignment_root_source_name(object, map),
        _ => None,
    }
}

pub(super) fn direct_source_identifier_name(expr: &Expr, map: &SourceMap) -> Option<String> {
    match &expr.kind {
        ExprKind::Ident => Some(text_in_map(map, expr.span).to_string()),
        ExprKind::Paren(inner) => direct_source_identifier_name(inner, map),
        _ => None,
    }
}

pub(super) fn emit_concurrent_arm_generator_body(
    value: &Expr,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let nonlocal_py_names = expression_nonlocal_py_names(value, ctx);
    emit_nonlocal_declarations(&nonlocal_py_names, indent, out);
    writeln!(out, "{pad}if False:").expect("write to string");
    writeln!(out, "{pad}    yield None").expect("write to string");

    ctx.push_scope();
    let result = ctx.with_cooperative_yields(true, |ctx| {
        let target = ctx.fresh_temp("concurrent_value");
        if emit_expr_to_target_if_needed(value, &target, ctx, indent, out)? {
            writeln!(out, "{pad}return {target}").expect("write to string");
        } else {
            let value_py = emit_expr(value, ctx)?;
            writeln!(out, "{pad}return {value_py}").expect("write to string");
        }
        Ok(())
    });
    ctx.pop_scope();
    result
}

pub(super) fn concurrent_timeout_can_record_shape(timeout: &Expr) -> bool {
    matches!(timeout.kind, ExprKind::Duration(_))
}

pub(super) fn concurrent_timeout_ms(timeout: &Expr, ctx: &Ctx<'_>) -> Result<u64, PyEmitError> {
    let ExprKind::Duration(_) = &timeout.kind else {
        return Err(PyEmitError::unsupported("concurrent timeout").at(timeout.span));
    };
    parse_duration_milliseconds(ctx.text(timeout.span)).ok_or_else(|| {
        PyEmitError::unsupported("concurrent timeout duration overflows u64 milliseconds")
            .at(timeout.span)
    })
}

pub(super) fn expr_is_instant_concurrent_arm(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Float
        | ExprKind::Bool(_)
        | ExprKind::Int
        | ExprKind::Null
        | ExprKind::Unit
        | ExprKind::Ident => true,
        ExprKind::Paren(inner) | ExprKind::Unary { operand: inner, .. } => {
            expr_is_instant_concurrent_arm(inner)
        }
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Compose { lhs, rhs } => {
            expr_is_instant_concurrent_arm(lhs) && expr_is_instant_concurrent_arm(rhs)
        }
        ExprKind::String(lit) => lit.parts.iter().all(|part| match part {
            StringPart::Text(_) => true,
            StringPart::Interpolation(expr) => expr_is_instant_concurrent_arm(expr),
        }),
        ExprKind::Array(elements) => elements.iter().all(|element| match element {
            ArrayElement::Expr(expr) | ArrayElement::Spread(expr) => {
                expr_is_instant_concurrent_arm(expr)
            }
        }),
        ExprKind::SetLiteral(elements) => elements.iter().all(expr_is_instant_concurrent_arm),
        ExprKind::MapLiteral(entries) => entries.iter().all(|(key, value)| {
            expr_is_instant_concurrent_arm(key) && expr_is_instant_concurrent_arm(value)
        }),
        ExprKind::RecordLiteral { fields } => fields
            .iter()
            .all(|field| expr_is_instant_concurrent_arm(&field.value)),
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            expr_is_instant_concurrent_arm(base)
                && spread
                    .as_ref()
                    .is_none_or(|expr| expr_is_instant_concurrent_arm(expr))
                && fields
                    .iter()
                    .all(|field| expr_is_instant_concurrent_arm(&field.value))
        }
        ExprKind::Block(block) => block_is_instant_concurrent_arm(block),
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            expr_is_instant_concurrent_arm(cond)
                && block_is_instant_concurrent_arm(then_block)
                && else_branch
                    .as_deref()
                    .is_none_or(expr_is_instant_concurrent_arm)
        }
        ExprKind::Match { scrutinee, cases } => {
            expr_is_instant_concurrent_arm(scrutinee)
                && cases.iter().all(|case| {
                    case.guard
                        .as_ref()
                        .is_none_or(expr_is_instant_concurrent_arm)
                        && match &case.body {
                            CaseArmBody::Expr(expr) => expr_is_instant_concurrent_arm(expr),
                            CaseArmBody::Return { .. } => false,
                        }
                })
        }
        ExprKind::Member { object, .. } | ExprKind::OptionalAccess { object, .. } => {
            expr_is_instant_concurrent_arm(object)
        }
        ExprKind::Index { object, index } => {
            expr_is_instant_concurrent_arm(object) && expr_is_instant_concurrent_arm(index)
        }
        ExprKind::Range { lo, hi, step, .. } => {
            expr_is_instant_concurrent_arm(lo)
                && expr_is_instant_concurrent_arm(hi)
                && step.as_deref().is_none_or(expr_is_instant_concurrent_arm)
        }
        ExprKind::Try(_)
        | ExprKind::Duration(_)
        | ExprKind::Placeholder
        | ExprKind::For { .. }
        | ExprKind::Loop { .. }
        | ExprKind::Concurrent { .. }
        | ExprKind::Call { .. }
        | ExprKind::Pipe { .. }
        | ExprKind::Comprehension { .. }
        | ExprKind::Lambda { .. } => false,
    }
}

pub(super) fn block_is_instant_concurrent_arm(block: &Block) -> bool {
    block.stmts.iter().all(|stmt| match &stmt.kind {
        StmtKind::Let { value, .. } | StmtKind::Const { value, .. } | StmtKind::Expr(value) => {
            expr_is_instant_concurrent_arm(value)
        }
        _ => false,
    }) && block
        .tail
        .as_deref()
        .is_none_or(expr_is_instant_concurrent_arm)
}
