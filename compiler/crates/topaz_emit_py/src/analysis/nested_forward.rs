use crate::*;

pub(super) fn reject_nested_function_forward_references(
    block: &Block,
    map: &SourceMap,
    candidates: &[(usize, String, &FunctionDecl)],
) -> Result<(), NestedForwardSignal> {
    if candidates.is_empty() {
        return Ok(());
    }
    let def_indexes = candidates
        .iter()
        .map(|(index, name, _)| (name.clone(), *index))
        .collect::<BTreeMap<_, _>>();
    let direct_refs = collect_nested_function_body_reference_graph(map, candidates);
    let transitive_refs = transitive_nested_function_body_references(&direct_refs);
    let analysis = NestedForwardAnalysis {
        def_indexes,
        transitive_refs,
    };
    let mut scope = NestedForwardScope::default();
    scope.push_frame();
    for (index, stmt) in block.stmts.iter().enumerate() {
        scan_stmt_for_nested_forward_reference(stmt, map, &analysis, index, &mut scope)?;
    }
    scope.pop_frame();
    Ok(())
}

pub(super) struct NestedForwardAnalysis {
    pub(super) def_indexes: BTreeMap<String, usize>,
    pub(super) transitive_refs: BTreeMap<String, BTreeSet<String>>,
}

impl NestedForwardAnalysis {
    pub(super) fn is_direct_forward_reference(&self, name: &str, current_index: usize) -> bool {
        self.def_indexes
            .get(name)
            .is_some_and(|def_index| *def_index > current_index)
    }

    pub(super) fn is_candidate_defined_by(&self, name: &str, current_index: usize) -> bool {
        self.def_indexes
            .get(name)
            .is_some_and(|def_index| *def_index <= current_index)
    }

    pub(super) fn has_later_body_dependency(&self, name: &str, current_index: usize) -> bool {
        self.transitive_refs.get(name).is_some_and(|refs| {
            refs.iter().any(|dep| {
                self.def_indexes
                    .get(dep)
                    .is_some_and(|def_index| *def_index > current_index)
            })
        })
    }
}

pub(super) fn collect_nested_function_body_reference_graph(
    map: &SourceMap,
    candidates: &[(usize, String, &FunctionDecl)],
) -> BTreeMap<String, BTreeSet<String>> {
    let candidate_names = candidates
        .iter()
        .map(|(_, name, _)| name.clone())
        .collect::<BTreeSet<_>>();
    let mut graph = BTreeMap::new();
    for (_, name, decl) in candidates {
        let mut refs = BTreeSet::new();
        let mut scope = NestedForwardScope::default();
        scope.push_frame();
        for param in &decl.params {
            scope.insert_binding(text_in_map(map, param.name.span).to_string());
        }
        collect_block_nested_function_body_references(
            decl.body.as_ref(),
            map,
            &candidate_names,
            &mut scope,
            &mut refs,
        );
        scope.pop_frame();
        graph.insert(name.clone(), refs);
    }
    graph
}

pub(super) fn transitive_nested_function_body_references(
    direct_refs: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut transitive_refs = BTreeMap::new();
    for name in direct_refs.keys() {
        let mut refs = BTreeSet::new();
        collect_transitive_nested_function_body_references(name, direct_refs, &mut refs);
        refs.remove(name);
        transitive_refs.insert(name.clone(), refs);
    }
    transitive_refs
}

pub(super) fn collect_transitive_nested_function_body_references(
    name: &str,
    direct_refs: &BTreeMap<String, BTreeSet<String>>,
    out: &mut BTreeSet<String>,
) {
    if let Some(refs) = direct_refs.get(name) {
        for dep in refs {
            if out.insert(dep.clone()) {
                collect_transitive_nested_function_body_references(dep, direct_refs, out);
            }
        }
    }
}

#[derive(Default)]
pub(super) struct NestedForwardScope {
    pub(super) frames: Vec<NestedForwardFrame>,
}

#[derive(Default)]
pub(super) struct NestedForwardFrame {
    pub(super) bindings: BTreeSet<String>,
    pub(super) functions: BTreeSet<String>,
}

impl NestedForwardScope {
    pub(super) fn push_frame(&mut self) {
        self.frames.push(NestedForwardFrame::default());
    }

    pub(super) fn push_binding_frame(&mut self, bindings: BTreeSet<String>) {
        self.frames.push(NestedForwardFrame {
            bindings,
            functions: BTreeSet::new(),
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

    pub(super) fn contains_binding(&self, name: &str) -> bool {
        self.frames
            .iter()
            .rev()
            .any(|frame| frame.bindings.contains(name))
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
}

pub(super) fn nested_function_nonlocal_py_names(decl: &FunctionDecl, ctx: &Ctx<'_>) -> Vec<String> {
    let mut scope = NestedForwardScope::default();
    let mut names = BTreeSet::new();
    scope.push_frame();
    for param in &decl.params {
        scope.insert_binding(text_in_map(ctx.map, param.name.span).to_string());
    }
    collect_nonlocal_assignments_in_block(decl.body.as_ref(), ctx, &mut scope, &mut names);
    scope.pop_frame();
    names.into_iter().collect()
}

pub(super) fn collecting_for_body_nonlocal_py_names(
    pattern: &Pattern,
    body: &Block,
    ctx: &Ctx<'_>,
) -> Vec<String> {
    let mut scope = NestedForwardScope::default();
    let mut names = BTreeSet::new();
    scope.push_frame();
    scope.insert_pattern(pattern, ctx.map);
    collect_nonlocal_assignments_in_block(body, ctx, &mut scope, &mut names);
    scope.pop_frame();
    names.into_iter().collect()
}

pub(super) fn comprehension_body_nonlocal_py_names(
    body: &CompBody,
    captures: &[(String, String)],
    ctx: &Ctx<'_>,
) -> Vec<String> {
    let mut scope = NestedForwardScope::default();
    let mut names = BTreeSet::new();
    scope.push_frame();
    for (source_name, _) in captures {
        scope.insert_binding(source_name.clone());
    }
    match body {
        CompBody::Elem(value) => {
            collect_nonlocal_assignments_in_expr(value, ctx, &mut scope, &mut names)
        }
        CompBody::Entry { key, value } => {
            collect_nonlocal_assignments_in_expr(key, ctx, &mut scope, &mut names);
            collect_nonlocal_assignments_in_expr(value, ctx, &mut scope, &mut names);
        }
    }
    scope.pop_frame();
    names.into_iter().collect()
}

pub(super) fn lambda_body_nonlocal_py_names(
    params: &[LambdaParam],
    body: &Expr,
    ctx: &Ctx<'_>,
) -> Vec<String> {
    let mut scope = NestedForwardScope::default();
    let mut names = BTreeSet::new();
    scope.push_frame();
    for param in params {
        scope.insert_binding(ctx.text(param.name.span).to_string());
    }
    collect_nonlocal_assignments_in_expr(body, ctx, &mut scope, &mut names);
    scope.pop_frame();
    names.into_iter().collect()
}

pub(super) fn expression_nonlocal_py_names(expr: &Expr, ctx: &Ctx<'_>) -> Vec<String> {
    let mut scope = NestedForwardScope::default();
    let mut names = BTreeSet::new();
    scope.push_frame();
    collect_nonlocal_assignments_in_expr(expr, ctx, &mut scope, &mut names);
    scope.pop_frame();
    names.into_iter().collect()
}

pub(super) fn collect_nonlocal_assignments_in_block(
    block: &Block,
    ctx: &Ctx<'_>,
    scope: &mut NestedForwardScope,
    out: &mut BTreeSet<String>,
) {
    scope.push_frame();
    for stmt in &block.stmts {
        collect_nonlocal_assignments_in_stmt(stmt, ctx, scope, out);
    }
    if let Some(tail) = block.tail.as_deref() {
        collect_nonlocal_assignments_in_expr(tail, ctx, scope, out);
    }
    scope.pop_frame();
}

pub(super) fn collect_nonlocal_assignments_in_stmt(
    stmt: &Stmt,
    ctx: &Ctx<'_>,
    scope: &mut NestedForwardScope,
    out: &mut BTreeSet<String>,
) {
    match &stmt.kind {
        StmtKind::Export(inner) => collect_nonlocal_assignments_in_stmt(inner, ctx, scope, out),
        StmtKind::Function(decl) => {
            scope.insert_function(text_in_map(ctx.map, decl.name.span).to_string());
        }
        StmtKind::Let { pattern, value, .. } => {
            collect_nonlocal_assignments_in_expr(value, ctx, scope, out);
            scope.insert_pattern(pattern, ctx.map);
        }
        StmtKind::Const { name, value, .. } => {
            collect_nonlocal_assignments_in_expr(value, ctx, scope, out);
            scope.insert_binding(text_in_map(ctx.map, name.span).to_string());
        }
        StmtKind::Assign { target, value, .. } => {
            if let Some(root) = nonlocal_rebinding_root_name(target, ctx.map)
                && !scope.contains(&root)
                && let Some(py_name) = ctx.nonlocal_py_name_for_assignment(&root)
            {
                out.insert(py_name);
            }
            collect_nonlocal_assignments_in_expr(target, ctx, scope, out);
            collect_nonlocal_assignments_in_expr(value, ctx, scope, out);
        }
        StmtKind::Defer(value) => {
            collect_nonlocal_assignments_in_expr(value, ctx, scope, out);
        }
        StmtKind::Return(Some(value))
        | StmtKind::Break {
            value: Some(value), ..
        }
        | StmtKind::Expr(value) => {
            collect_nonlocal_assignments_in_expr(value, ctx, scope, out);
        }
        StmtKind::Using { name, value, body } => {
            collect_nonlocal_assignments_in_expr(value, ctx, scope, out);
            let mut frame = BTreeSet::new();
            frame.insert(text_in_map(ctx.map, name.span).to_string());
            scope.push_binding_frame(frame);
            collect_nonlocal_assignments_in_block(body, ctx, scope, out);
            scope.pop_frame();
        }
        StmtKind::While { cond, body } => {
            collect_nonlocal_assignments_in_expr(cond, ctx, scope, out);
            collect_nonlocal_assignments_in_block(body, ctx, scope, out);
        }
        StmtKind::Import(_)
        | StmtKind::TypeAlias(_)
        | StmtKind::Enum(_)
        | StmtKind::Record(_)
        | StmtKind::Newtype(_)
        | StmtKind::Impl(_)
        | StmtKind::Protocol(_)
        | StmtKind::Return(None)
        | StmtKind::Break { value: None, .. }
        | StmtKind::Continue { .. } => {}
    }
}

pub(super) fn collect_nonlocal_assignments_in_expr(
    expr: &Expr,
    ctx: &Ctx<'_>,
    scope: &mut NestedForwardScope,
    out: &mut BTreeSet<String>,
) {
    match &expr.kind {
        ExprKind::Paren(inner) | ExprKind::Try(inner) | ExprKind::Unary { operand: inner, .. } => {
            collect_nonlocal_assignments_in_expr(inner, ctx, scope, out);
        }
        ExprKind::Block(block) => collect_nonlocal_assignments_in_block(block, ctx, scope, out),
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            collect_nonlocal_assignments_in_expr(cond, ctx, scope, out);
            collect_nonlocal_assignments_in_block(then_block, ctx, scope, out);
            if let Some(else_branch) = else_branch.as_deref() {
                collect_nonlocal_assignments_in_expr(else_branch, ctx, scope, out);
            }
        }
        ExprKind::Match { scrutinee, cases } => {
            collect_nonlocal_assignments_in_expr(scrutinee, ctx, scope, out);
            for case in cases {
                let mut frame = BTreeSet::new();
                collect_pattern_binding_names(&case.pattern, ctx.map, &mut frame);
                scope.push_binding_frame(frame);
                if let Some(guard) = &case.guard {
                    collect_nonlocal_assignments_in_expr(guard, ctx, scope, out);
                }
                match &case.body {
                    CaseArmBody::Expr(value)
                    | CaseArmBody::Return {
                        value: Some(value), ..
                    } => collect_nonlocal_assignments_in_expr(value, ctx, scope, out),
                    CaseArmBody::Return { value: None, .. } => {}
                }
                scope.pop_frame();
            }
        }
        ExprKind::For {
            pattern,
            iter,
            body,
        } => {
            collect_nonlocal_assignments_in_expr(iter, ctx, scope, out);
            let mut frame = BTreeSet::new();
            collect_pattern_binding_names(pattern, ctx.map, &mut frame);
            scope.push_binding_frame(frame);
            collect_nonlocal_assignments_in_block(body, ctx, scope, out);
            scope.pop_frame();
        }
        ExprKind::Loop { body, .. } => collect_nonlocal_assignments_in_block(body, ctx, scope, out),
        ExprKind::Concurrent {
            timeout,
            arms,
            else_block,
        } => {
            if let Some(timeout) = timeout.as_deref() {
                collect_nonlocal_assignments_in_expr(timeout, ctx, scope, out);
            }
            for arm in arms {
                collect_nonlocal_assignments_in_expr(&arm.value, ctx, scope, out);
            }
            if let Some(else_block) = else_block.as_deref() {
                collect_nonlocal_assignments_in_block(else_block, ctx, scope, out);
            }
        }
        ExprKind::Call { callee, args, .. } => {
            collect_nonlocal_assignments_in_expr(callee, ctx, scope, out);
            for arg in args {
                match arg {
                    CallArg::Positional(value) | CallArg::Spread(value) => {
                        collect_nonlocal_assignments_in_expr(value, ctx, scope, out);
                    }
                    CallArg::Named { value, .. } => {
                        collect_nonlocal_assignments_in_expr(value, ctx, scope, out);
                    }
                }
            }
        }
        ExprKind::Member { object, .. } | ExprKind::OptionalAccess { object, .. } => {
            collect_nonlocal_assignments_in_expr(object, ctx, scope, out);
        }
        ExprKind::Index { object, index } => {
            collect_nonlocal_assignments_in_expr(object, ctx, scope, out);
            collect_nonlocal_assignments_in_expr(index, ctx, scope, out);
        }
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Compose { lhs, rhs } => {
            collect_nonlocal_assignments_in_expr(lhs, ctx, scope, out);
            collect_nonlocal_assignments_in_expr(rhs, ctx, scope, out);
        }
        ExprKind::Range { lo, hi, step, .. } => {
            collect_nonlocal_assignments_in_expr(lo, ctx, scope, out);
            collect_nonlocal_assignments_in_expr(hi, ctx, scope, out);
            if let Some(step) = step.as_deref() {
                collect_nonlocal_assignments_in_expr(step, ctx, scope, out);
            }
        }
        ExprKind::Pipe { lhs, rhs } => {
            collect_nonlocal_assignments_in_expr(lhs, ctx, scope, out);
            if let PipeRhs::Expr(stage) = rhs.as_ref() {
                collect_nonlocal_assignments_in_expr(stage, ctx, scope, out);
            }
        }
        ExprKind::RecordLiteral { fields } => {
            for field in fields {
                collect_nonlocal_assignments_in_expr(&field.value, ctx, scope, out);
            }
        }
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            collect_nonlocal_assignments_in_expr(base, ctx, scope, out);
            if let Some(spread) = spread.as_deref() {
                collect_nonlocal_assignments_in_expr(spread, ctx, scope, out);
            }
            for field in fields {
                collect_nonlocal_assignments_in_expr(&field.value, ctx, scope, out);
            }
        }
        ExprKind::Array(elements) => {
            for element in elements {
                match element {
                    ArrayElement::Expr(value) | ArrayElement::Spread(value) => {
                        collect_nonlocal_assignments_in_expr(value, ctx, scope, out);
                    }
                }
            }
        }
        ExprKind::SetLiteral(elements) => {
            for value in elements {
                collect_nonlocal_assignments_in_expr(value, ctx, scope, out);
            }
        }
        ExprKind::MapLiteral(entries) => {
            for (key, value) in entries {
                collect_nonlocal_assignments_in_expr(key, ctx, scope, out);
                collect_nonlocal_assignments_in_expr(value, ctx, scope, out);
            }
        }
        ExprKind::Comprehension { clauses, body, .. } => {
            scope.push_frame();
            for clause in clauses {
                match clause {
                    CompClause::For { pattern, iter } => {
                        collect_nonlocal_assignments_in_expr(iter, ctx, scope, out);
                        scope.insert_pattern(pattern, ctx.map);
                    }
                    CompClause::If(cond) => {
                        collect_nonlocal_assignments_in_expr(cond, ctx, scope, out);
                    }
                }
            }
            match body.as_ref() {
                CompBody::Elem(value) => {
                    collect_nonlocal_assignments_in_expr(value, ctx, scope, out)
                }
                CompBody::Entry { key, value } => {
                    collect_nonlocal_assignments_in_expr(key, ctx, scope, out);
                    collect_nonlocal_assignments_in_expr(value, ctx, scope, out);
                }
            }
            scope.pop_frame();
        }
        ExprKind::String(lit) => {
            for part in &lit.parts {
                if let StringPart::Interpolation(value) = part {
                    collect_nonlocal_assignments_in_expr(value, ctx, scope, out);
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

pub(super) fn nonlocal_rebinding_root_name(target: &Expr, map: &SourceMap) -> Option<String> {
    match &target.kind {
        ExprKind::Ident => Some(text_in_map(map, target.span).to_string()),
        ExprKind::Paren(inner) => nonlocal_rebinding_root_name(inner, map),
        ExprKind::Member { .. } | ExprKind::OptionalAccess { .. } | ExprKind::Index { .. } => None,
        _ => None,
    }
}

pub(super) fn collect_stmt_nested_function_body_references(
    stmt: &Stmt,
    map: &SourceMap,
    candidate_names: &BTreeSet<String>,
    scope: &mut NestedForwardScope,
    out: &mut BTreeSet<String>,
) {
    match &stmt.kind {
        StmtKind::Export(inner) => {
            collect_stmt_nested_function_body_references(inner, map, candidate_names, scope, out)
        }
        StmtKind::Function(decl) => {
            scope.insert_function(text_in_map(map, decl.name.span).to_string());
            scope.push_frame();
            for param in &decl.params {
                scope.insert_binding(text_in_map(map, param.name.span).to_string());
            }
            collect_block_nested_function_body_references(
                decl.body.as_ref(),
                map,
                candidate_names,
                scope,
                out,
            );
            scope.pop_frame();
        }
        StmtKind::Let { pattern, value, .. } => {
            collect_expr_nested_function_body_references(value, map, candidate_names, scope, out);
            scope.insert_pattern(pattern, map);
        }
        StmtKind::Const { name, value, .. } => {
            collect_expr_nested_function_body_references(value, map, candidate_names, scope, out);
            scope.insert_binding(text_in_map(map, name.span).to_string());
        }
        StmtKind::Assign { target, value, .. } => {
            collect_expr_nested_function_body_references(target, map, candidate_names, scope, out);
            collect_expr_nested_function_body_references(value, map, candidate_names, scope, out);
        }
        StmtKind::Defer(value) => {
            collect_expr_nested_function_body_references(value, map, candidate_names, scope, out);
        }
        StmtKind::Return(Some(value))
        | StmtKind::Break {
            value: Some(value), ..
        }
        | StmtKind::Expr(value) => {
            collect_expr_nested_function_body_references(value, map, candidate_names, scope, out);
        }
        StmtKind::Using { name, value, body } => {
            collect_expr_nested_function_body_references(value, map, candidate_names, scope, out);
            let mut frame = BTreeSet::new();
            frame.insert(text_in_map(map, name.span).to_string());
            scope.push_binding_frame(frame);
            collect_block_nested_function_body_references(body, map, candidate_names, scope, out);
            scope.pop_frame();
        }
        StmtKind::While { cond, body } => {
            collect_expr_nested_function_body_references(cond, map, candidate_names, scope, out);
            collect_block_nested_function_body_references(body, map, candidate_names, scope, out);
        }
        StmtKind::Import(_)
        | StmtKind::TypeAlias(_)
        | StmtKind::Enum(_)
        | StmtKind::Record(_)
        | StmtKind::Newtype(_)
        | StmtKind::Impl(_)
        | StmtKind::Protocol(_)
        | StmtKind::Return(None)
        | StmtKind::Break { value: None, .. }
        | StmtKind::Continue { .. } => {}
    }
}

pub(super) fn collect_block_nested_function_body_references(
    block: &Block,
    map: &SourceMap,
    candidate_names: &BTreeSet<String>,
    scope: &mut NestedForwardScope,
    out: &mut BTreeSet<String>,
) {
    scope.push_frame();
    for stmt in &block.stmts {
        collect_stmt_nested_function_body_references(stmt, map, candidate_names, scope, out);
    }
    if let Some(tail) = block.tail.as_deref() {
        collect_expr_nested_function_body_references(tail, map, candidate_names, scope, out);
    }
    scope.pop_frame();
}

pub(super) fn collect_expr_nested_function_body_references(
    expr: &Expr,
    map: &SourceMap,
    candidate_names: &BTreeSet<String>,
    scope: &mut NestedForwardScope,
    out: &mut BTreeSet<String>,
) {
    if let ExprKind::Ident = &expr.kind {
        let name = text_in_map(map, expr.span);
        if candidate_names.contains(name) && !scope.contains(name) {
            out.insert(name.to_string());
        }
    }
    match &expr.kind {
        ExprKind::Paren(inner) | ExprKind::Try(inner) => {
            collect_expr_nested_function_body_references(inner, map, candidate_names, scope, out);
        }
        ExprKind::Block(block) => {
            collect_block_nested_function_body_references(block, map, candidate_names, scope, out);
        }
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            collect_expr_nested_function_body_references(cond, map, candidate_names, scope, out);
            collect_block_nested_function_body_references(
                then_block,
                map,
                candidate_names,
                scope,
                out,
            );
            if let Some(else_branch) = else_branch.as_deref() {
                collect_expr_nested_function_body_references(
                    else_branch,
                    map,
                    candidate_names,
                    scope,
                    out,
                );
            }
        }
        ExprKind::Match { scrutinee, cases } => {
            collect_expr_nested_function_body_references(
                scrutinee,
                map,
                candidate_names,
                scope,
                out,
            );
            for case in cases {
                if let Some(guard) = &case.guard {
                    collect_expr_nested_function_body_references(
                        guard,
                        map,
                        candidate_names,
                        scope,
                        out,
                    );
                }
                match &case.body {
                    CaseArmBody::Expr(value)
                    | CaseArmBody::Return {
                        value: Some(value), ..
                    } => collect_expr_nested_function_body_references(
                        value,
                        map,
                        candidate_names,
                        scope,
                        out,
                    ),
                    CaseArmBody::Return { value: None, .. } => {}
                }
            }
        }
        ExprKind::For { iter, body, .. } => {
            collect_expr_nested_function_body_references(iter, map, candidate_names, scope, out);
            collect_block_nested_function_body_references(body, map, candidate_names, scope, out);
        }
        ExprKind::Loop { body, .. } => {
            collect_block_nested_function_body_references(body, map, candidate_names, scope, out);
        }
        ExprKind::Concurrent {
            timeout,
            arms,
            else_block,
        } => {
            if let Some(timeout) = timeout.as_deref() {
                collect_expr_nested_function_body_references(
                    timeout,
                    map,
                    candidate_names,
                    scope,
                    out,
                );
            }
            for arm in arms {
                collect_expr_nested_function_body_references(
                    &arm.value,
                    map,
                    candidate_names,
                    scope,
                    out,
                );
            }
            if let Some(else_block) = else_block.as_deref() {
                collect_block_nested_function_body_references(
                    else_block,
                    map,
                    candidate_names,
                    scope,
                    out,
                );
            }
        }
        ExprKind::Call { callee, args, .. } => {
            if let Some((params, body)) = immediate_lambda_callee(callee) {
                collect_lambda_body_nested_function_references(
                    params,
                    body,
                    map,
                    candidate_names,
                    scope,
                    out,
                );
            } else {
                collect_expr_nested_function_body_references(
                    callee,
                    map,
                    candidate_names,
                    scope,
                    out,
                );
            }
            for arg in args {
                match arg {
                    CallArg::Positional(value) | CallArg::Spread(value) => {
                        collect_expr_nested_function_body_references(
                            value,
                            map,
                            candidate_names,
                            scope,
                            out,
                        );
                    }
                    CallArg::Named { value, .. } => {
                        collect_expr_nested_function_body_references(
                            value,
                            map,
                            candidate_names,
                            scope,
                            out,
                        );
                    }
                }
            }
        }
        ExprKind::Member { object, .. } | ExprKind::OptionalAccess { object, .. } => {
            collect_expr_nested_function_body_references(object, map, candidate_names, scope, out);
        }
        ExprKind::Index { object, index } => {
            collect_expr_nested_function_body_references(object, map, candidate_names, scope, out);
            collect_expr_nested_function_body_references(index, map, candidate_names, scope, out);
        }
        ExprKind::Unary { operand, .. } => {
            collect_expr_nested_function_body_references(operand, map, candidate_names, scope, out);
        }
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Compose { lhs, rhs } => {
            collect_expr_nested_function_body_references(lhs, map, candidate_names, scope, out);
            collect_expr_nested_function_body_references(rhs, map, candidate_names, scope, out);
        }
        ExprKind::Range { lo, hi, step, .. } => {
            collect_expr_nested_function_body_references(lo, map, candidate_names, scope, out);
            collect_expr_nested_function_body_references(hi, map, candidate_names, scope, out);
            if let Some(step) = step.as_deref() {
                collect_expr_nested_function_body_references(
                    step,
                    map,
                    candidate_names,
                    scope,
                    out,
                );
            }
        }
        ExprKind::Pipe { lhs, rhs } => {
            collect_expr_nested_function_body_references(lhs, map, candidate_names, scope, out);
            if let PipeRhs::Expr(stage) = rhs.as_ref() {
                collect_expr_nested_function_body_references(
                    stage,
                    map,
                    candidate_names,
                    scope,
                    out,
                );
            }
        }
        ExprKind::RecordLiteral { fields } => {
            for field in fields {
                collect_expr_nested_function_body_references(
                    &field.value,
                    map,
                    candidate_names,
                    scope,
                    out,
                );
            }
        }
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            collect_expr_nested_function_body_references(base, map, candidate_names, scope, out);
            if let Some(spread) = spread.as_deref() {
                collect_expr_nested_function_body_references(
                    spread,
                    map,
                    candidate_names,
                    scope,
                    out,
                );
            }
            for field in fields {
                collect_expr_nested_function_body_references(
                    &field.value,
                    map,
                    candidate_names,
                    scope,
                    out,
                );
            }
        }
        ExprKind::Array(elements) => {
            for element in elements {
                match element {
                    ArrayElement::Expr(value) | ArrayElement::Spread(value) => {
                        collect_expr_nested_function_body_references(
                            value,
                            map,
                            candidate_names,
                            scope,
                            out,
                        );
                    }
                }
            }
        }
        ExprKind::SetLiteral(elements) => {
            for value in elements {
                collect_expr_nested_function_body_references(
                    value,
                    map,
                    candidate_names,
                    scope,
                    out,
                );
            }
        }
        ExprKind::MapLiteral(entries) => {
            for (key, value) in entries {
                collect_expr_nested_function_body_references(key, map, candidate_names, scope, out);
                collect_expr_nested_function_body_references(
                    value,
                    map,
                    candidate_names,
                    scope,
                    out,
                );
            }
        }
        ExprKind::Comprehension { clauses, body, .. } => {
            scope.push_frame();
            for clause in clauses {
                match clause {
                    CompClause::For { pattern, iter } => {
                        collect_expr_nested_function_body_references(
                            iter,
                            map,
                            candidate_names,
                            scope,
                            out,
                        );
                        scope.insert_pattern(pattern, map);
                    }
                    CompClause::If(cond) => collect_expr_nested_function_body_references(
                        cond,
                        map,
                        candidate_names,
                        scope,
                        out,
                    ),
                }
            }
            match body.as_ref() {
                CompBody::Elem(value) => collect_expr_nested_function_body_references(
                    value,
                    map,
                    candidate_names,
                    scope,
                    out,
                ),
                CompBody::Entry { key, value } => {
                    collect_expr_nested_function_body_references(
                        key,
                        map,
                        candidate_names,
                        scope,
                        out,
                    );
                    collect_expr_nested_function_body_references(
                        value,
                        map,
                        candidate_names,
                        scope,
                        out,
                    );
                }
            }
            scope.pop_frame();
        }
        ExprKind::String(lit) => {
            for part in &lit.parts {
                if let StringPart::Interpolation(value) = part {
                    collect_expr_nested_function_body_references(
                        value,
                        map,
                        candidate_names,
                        scope,
                        out,
                    );
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
        | ExprKind::Placeholder
        | ExprKind::Lambda { .. } => {}
    }
}

pub(super) fn collect_lambda_body_nested_function_references(
    params: &[LambdaParam],
    body: &Expr,
    map: &SourceMap,
    candidate_names: &BTreeSet<String>,
    scope: &mut NestedForwardScope,
    out: &mut BTreeSet<String>,
) {
    let mut frame = BTreeSet::new();
    for param in params {
        frame.insert(text_in_map(map, param.name.span).to_string());
    }
    scope.push_binding_frame(frame);
    collect_expr_nested_function_body_references(body, map, candidate_names, scope, out);
    scope.pop_frame();
}

pub(super) fn scan_stmt_for_nested_forward_reference(
    stmt: &Stmt,
    map: &SourceMap,
    analysis: &NestedForwardAnalysis,
    current_index: usize,
    scope: &mut NestedForwardScope,
) -> Result<(), NestedForwardSignal> {
    match &stmt.kind {
        StmtKind::Export(inner) => {
            scan_stmt_for_nested_forward_reference(inner, map, analysis, current_index, scope)
        }
        StmtKind::Function(decl) => {
            scope.insert_function(text_in_map(map, decl.name.span).to_string());
            Ok(())
        }
        StmtKind::Let { pattern, value, .. } => {
            scan_expr_for_nested_forward_reference(value, map, analysis, current_index, scope)?;
            scope.insert_pattern(pattern, map);
            Ok(())
        }
        StmtKind::Const { name, value, .. } => {
            scan_expr_for_nested_forward_reference(value, map, analysis, current_index, scope)?;
            scope.insert_binding(text_in_map(map, name.span).to_string());
            Ok(())
        }
        StmtKind::Assign { target, value, .. } => {
            scan_expr_for_nested_forward_reference(target, map, analysis, current_index, scope)?;
            scan_expr_for_nested_forward_reference(value, map, analysis, current_index, scope)
        }
        StmtKind::Defer(value) => {
            scan_expr_for_nested_forward_reference(value, map, analysis, current_index, scope)
        }
        StmtKind::Return(Some(value))
        | StmtKind::Break {
            value: Some(value), ..
        }
        | StmtKind::Expr(value) => {
            scan_expr_for_nested_forward_reference(value, map, analysis, current_index, scope)
        }
        StmtKind::Using { name, value, body } => {
            scan_expr_for_nested_forward_reference(value, map, analysis, current_index, scope)?;
            let mut frame = BTreeSet::new();
            frame.insert(text_in_map(map, name.span).to_string());
            scope.push_binding_frame(frame);
            let result =
                scan_block_for_nested_forward_reference(body, map, analysis, current_index, scope);
            scope.pop_frame();
            result
        }
        StmtKind::While { cond, body } => {
            scan_expr_for_nested_forward_reference(cond, map, analysis, current_index, scope)?;
            scan_block_for_nested_forward_reference(body, map, analysis, current_index, scope)
        }
        StmtKind::Import(_)
        | StmtKind::TypeAlias(_)
        | StmtKind::Enum(_)
        | StmtKind::Record(_)
        | StmtKind::Newtype(_)
        | StmtKind::Impl(_)
        | StmtKind::Protocol(_)
        | StmtKind::Return(None)
        | StmtKind::Break { value: None, .. }
        | StmtKind::Continue { .. } => Ok(()),
    }
}

pub(super) fn scan_block_for_nested_forward_reference(
    block: &Block,
    map: &SourceMap,
    analysis: &NestedForwardAnalysis,
    current_index: usize,
    scope: &mut NestedForwardScope,
) -> Result<(), NestedForwardSignal> {
    scope.push_frame();
    let result = (|| -> Result<(), NestedForwardSignal> {
        for stmt in &block.stmts {
            scan_stmt_for_nested_forward_reference(stmt, map, analysis, current_index, scope)?;
        }
        if let Some(tail) = block.tail.as_deref() {
            scan_expr_for_nested_forward_reference(tail, map, analysis, current_index, scope)?;
        }
        Ok(())
    })();
    scope.pop_frame();
    result
}

pub(super) fn scan_expr_for_nested_forward_reference(
    expr: &Expr,
    map: &SourceMap,
    analysis: &NestedForwardAnalysis,
    current_index: usize,
    scope: &mut NestedForwardScope,
) -> Result<(), NestedForwardSignal> {
    if let ExprKind::Ident = &expr.kind {
        let name = text_in_map(map, expr.span);
        let direct_forward =
            !scope.contains(name) && analysis.is_direct_forward_reference(name, current_index);
        let transitive_forward = !scope.contains_binding(name)
            && analysis.is_candidate_defined_by(name, current_index)
            && analysis.has_later_body_dependency(name, current_index);
        if direct_forward || transitive_forward {
            return Err(NestedForwardSignal);
        }
    }
    match &expr.kind {
        ExprKind::Paren(inner) | ExprKind::Try(inner) => {
            scan_expr_for_nested_forward_reference(inner, map, analysis, current_index, scope)
        }
        ExprKind::Block(block) => {
            scan_block_for_nested_forward_reference(block, map, analysis, current_index, scope)
        }
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            scan_expr_for_nested_forward_reference(cond, map, analysis, current_index, scope)?;
            scan_block_for_nested_forward_reference(
                then_block,
                map,
                analysis,
                current_index,
                scope,
            )?;
            if let Some(else_branch) = else_branch.as_deref() {
                scan_expr_for_nested_forward_reference(
                    else_branch,
                    map,
                    analysis,
                    current_index,
                    scope,
                )?;
            }
            Ok(())
        }
        ExprKind::Match { scrutinee, cases } => {
            scan_expr_for_nested_forward_reference(scrutinee, map, analysis, current_index, scope)?;
            for case in cases {
                if let Some(guard) = &case.guard {
                    scan_expr_for_nested_forward_reference(
                        guard,
                        map,
                        analysis,
                        current_index,
                        scope,
                    )?;
                }
                match &case.body {
                    CaseArmBody::Expr(value) => scan_expr_for_nested_forward_reference(
                        value,
                        map,
                        analysis,
                        current_index,
                        scope,
                    )?,
                    CaseArmBody::Return {
                        value: Some(value), ..
                    } => scan_expr_for_nested_forward_reference(
                        value,
                        map,
                        analysis,
                        current_index,
                        scope,
                    )?,
                    CaseArmBody::Return { value: None, .. } => {}
                }
            }
            Ok(())
        }
        ExprKind::For { iter, body, .. } => {
            scan_expr_for_nested_forward_reference(iter, map, analysis, current_index, scope)?;
            scan_block_for_nested_forward_reference(body, map, analysis, current_index, scope)
        }
        ExprKind::Loop { body, .. } => {
            scan_block_for_nested_forward_reference(body, map, analysis, current_index, scope)
        }
        ExprKind::Concurrent {
            timeout,
            arms,
            else_block,
        } => {
            if let Some(timeout) = timeout.as_deref() {
                scan_expr_for_nested_forward_reference(
                    timeout,
                    map,
                    analysis,
                    current_index,
                    scope,
                )?;
            }
            for arm in arms {
                scan_expr_for_nested_forward_reference(
                    &arm.value,
                    map,
                    analysis,
                    current_index,
                    scope,
                )?;
            }
            if let Some(else_block) = else_block.as_deref() {
                scan_block_for_nested_forward_reference(
                    else_block,
                    map,
                    analysis,
                    current_index,
                    scope,
                )?;
            }
            Ok(())
        }
        ExprKind::Call { callee, args, .. } => {
            if let Some((params, body)) = immediate_lambda_callee(callee) {
                scan_lambda_body_for_nested_forward_reference(
                    params,
                    body,
                    map,
                    analysis,
                    current_index,
                    scope,
                )?;
            } else {
                scan_expr_for_nested_forward_reference(
                    callee,
                    map,
                    analysis,
                    current_index,
                    scope,
                )?;
            }
            for arg in args {
                match arg {
                    CallArg::Positional(value) | CallArg::Spread(value) => {
                        scan_expr_for_nested_forward_reference(
                            value,
                            map,
                            analysis,
                            current_index,
                            scope,
                        )?;
                    }
                    CallArg::Named { value, .. } => {
                        scan_expr_for_nested_forward_reference(
                            value,
                            map,
                            analysis,
                            current_index,
                            scope,
                        )?;
                    }
                }
            }
            Ok(())
        }
        ExprKind::Member { object, .. } | ExprKind::OptionalAccess { object, .. } => {
            scan_expr_for_nested_forward_reference(object, map, analysis, current_index, scope)
        }
        ExprKind::Index { object, index } => {
            scan_expr_for_nested_forward_reference(object, map, analysis, current_index, scope)?;
            scan_expr_for_nested_forward_reference(index, map, analysis, current_index, scope)
        }
        ExprKind::Unary { operand, .. } => {
            scan_expr_for_nested_forward_reference(operand, map, analysis, current_index, scope)
        }
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Compose { lhs, rhs } => {
            scan_expr_for_nested_forward_reference(lhs, map, analysis, current_index, scope)?;
            scan_expr_for_nested_forward_reference(rhs, map, analysis, current_index, scope)
        }
        ExprKind::Range { lo, hi, step, .. } => {
            scan_expr_for_nested_forward_reference(lo, map, analysis, current_index, scope)?;
            scan_expr_for_nested_forward_reference(hi, map, analysis, current_index, scope)?;
            if let Some(step) = step.as_deref() {
                scan_expr_for_nested_forward_reference(step, map, analysis, current_index, scope)?;
            }
            Ok(())
        }
        ExprKind::Pipe { lhs, rhs } => {
            scan_expr_for_nested_forward_reference(lhs, map, analysis, current_index, scope)?;
            if let PipeRhs::Expr(stage) = rhs.as_ref() {
                scan_expr_for_nested_forward_reference(stage, map, analysis, current_index, scope)?;
            }
            Ok(())
        }
        ExprKind::RecordLiteral { fields } => {
            for field in fields {
                scan_expr_for_nested_forward_reference(
                    &field.value,
                    map,
                    analysis,
                    current_index,
                    scope,
                )?;
            }
            Ok(())
        }
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            scan_expr_for_nested_forward_reference(base, map, analysis, current_index, scope)?;
            if let Some(spread) = spread.as_deref() {
                scan_expr_for_nested_forward_reference(
                    spread,
                    map,
                    analysis,
                    current_index,
                    scope,
                )?;
            }
            for field in fields {
                scan_expr_for_nested_forward_reference(
                    &field.value,
                    map,
                    analysis,
                    current_index,
                    scope,
                )?;
            }
            Ok(())
        }
        ExprKind::Array(elements) => {
            for element in elements {
                match element {
                    ArrayElement::Expr(value) | ArrayElement::Spread(value) => {
                        scan_expr_for_nested_forward_reference(
                            value,
                            map,
                            analysis,
                            current_index,
                            scope,
                        )?;
                    }
                }
            }
            Ok(())
        }
        ExprKind::SetLiteral(elements) => {
            for value in elements {
                scan_expr_for_nested_forward_reference(value, map, analysis, current_index, scope)?;
            }
            Ok(())
        }
        ExprKind::MapLiteral(entries) => {
            for (key, value) in entries {
                scan_expr_for_nested_forward_reference(key, map, analysis, current_index, scope)?;
                scan_expr_for_nested_forward_reference(value, map, analysis, current_index, scope)?;
            }
            Ok(())
        }
        ExprKind::Comprehension { clauses, body, .. } => {
            scope.push_frame();
            let result = (|| -> Result<(), NestedForwardSignal> {
                for clause in clauses {
                    match clause {
                        CompClause::For { pattern, iter } => {
                            scan_expr_for_nested_forward_reference(
                                iter,
                                map,
                                analysis,
                                current_index,
                                scope,
                            )?;
                            scope.insert_pattern(pattern, map);
                        }
                        CompClause::If(cond) => scan_expr_for_nested_forward_reference(
                            cond,
                            map,
                            analysis,
                            current_index,
                            scope,
                        )?,
                    }
                }
                match body.as_ref() {
                    CompBody::Elem(value) => scan_expr_for_nested_forward_reference(
                        value,
                        map,
                        analysis,
                        current_index,
                        scope,
                    ),
                    CompBody::Entry { key, value } => {
                        scan_expr_for_nested_forward_reference(
                            key,
                            map,
                            analysis,
                            current_index,
                            scope,
                        )?;
                        scan_expr_for_nested_forward_reference(
                            value,
                            map,
                            analysis,
                            current_index,
                            scope,
                        )
                    }
                }
            })();
            scope.pop_frame();
            result
        }
        ExprKind::String(lit) => {
            for part in &lit.parts {
                if let StringPart::Interpolation(value) = part {
                    scan_expr_for_nested_forward_reference(
                        value,
                        map,
                        analysis,
                        current_index,
                        scope,
                    )?;
                }
            }
            Ok(())
        }
        ExprKind::Int
        | ExprKind::Float
        | ExprKind::Duration(_)
        | ExprKind::Bool(_)
        | ExprKind::Null
        | ExprKind::Unit
        | ExprKind::Ident
        | ExprKind::Placeholder
        | ExprKind::Lambda { .. } => Ok(()),
    }
}

pub(super) fn scan_lambda_body_for_nested_forward_reference(
    params: &[LambdaParam],
    body: &Expr,
    map: &SourceMap,
    analysis: &NestedForwardAnalysis,
    current_index: usize,
    scope: &mut NestedForwardScope,
) -> Result<(), NestedForwardSignal> {
    let mut frame = BTreeSet::new();
    for param in params {
        frame.insert(text_in_map(map, param.name.span).to_string());
    }
    scope.push_binding_frame(frame);
    let result = scan_expr_for_nested_forward_reference(body, map, analysis, current_index, scope);
    scope.pop_frame();
    result
}

pub(super) fn immediate_lambda_callee(expr: &Expr) -> Option<(&[LambdaParam], &Expr)> {
    match &expr.kind {
        ExprKind::Lambda { params, body } => Some((params.as_slice(), body.as_ref())),
        ExprKind::Paren(inner) => immediate_lambda_callee(inner),
        _ => None,
    }
}

pub(super) fn render_function_default_value(value: &Value) -> Option<String> {
    match value {
        Value::Int(value) => Some(value.to_string()),
        Value::Float(value) => Some(format!("tpz_f64_from_bits(0x{:016x})", value.to_bits())),
        Value::Bool(value) => Some(if *value { "True" } else { "False" }.to_string()),
        Value::Null => Some("TPZ_NULL".to_string()),
        Value::Unit => Some("TPZ_UNIT".to_string()),
        Value::Str(value) => Some(py_string(value.as_ref())),
        _ => None,
    }
}
