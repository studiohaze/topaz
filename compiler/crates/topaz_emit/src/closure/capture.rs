use crate::*;

/// The captured variables of a lambda: enclosing locals the body
/// references FREE (referenced but not bound within the body, and not a
/// param). Sound by a FLAT approximation — `referenced − bound − params`:
/// a capture is never bound anywhere in the body, so it can never collide
/// with a body binding (no shadowing hazard), and a name that is free in
/// one sub-scope but bound in another is conservatively treated as bound
/// (so it is NOT captured; the body lowering then refuses its free use as
/// a free identifier — an over-refusal, never a miscompile). Missing a
/// referenced name only over-refuses too. Returns the captures in source
/// order without checking mutability; the same non-refusing result feeds
/// escape analysis, and `push_capture_locals` later rejects any captured
/// plain `Mut` that was not lifted to a cell.
pub(crate) fn lambda_captures<'a>(
    body: &'a Expr,
    params: &[(String, Bind)],
    // The enclosing scope is only READ for membership/mutability, so its
    // lifetime is decoupled from the returned captures (which borrow `src`)
    // — the caller may keep the captures while it mutates its locals.
    enclosing: &[(String, Bind)],
    src: &'a LoweredText,
) -> Result<Vec<&'a str>, EmitError> {
    if let Some(captures) = src.captures_for_body(body.span) {
        return Ok(captures.iter().map(String::as_str).collect());
    }
    let mut referenced: Vec<&str> = Vec::new();
    let mut bound: Vec<&str> = Vec::new();
    collect_idents(body, src, &mut referenced, &mut bound);
    let mut captures = filter_captures(referenced, bound, params, enclosing)?;
    let mut scope: Vec<String> = params.iter().map(|(n, _)| n.clone()).collect();
    outer_write_captures_expr(body, src, enclosing, &mut scope, &mut captures);
    Ok(captures)
}

/// The captures of a `function` declaration's BLOCK body (same flat
/// analysis as [`lambda_captures`], walking the block).
pub(crate) fn closure_captures_block<'a>(
    block: &'a Block,
    params: &[(String, Bind)],
    enclosing: &[(String, Bind)],
    src: &'a LoweredText,
) -> Result<Vec<&'a str>, EmitError> {
    if let Some(captures) = src.captures_for_body(block.span) {
        return Ok(captures.iter().map(String::as_str).collect());
    }
    let mut referenced: Vec<&str> = Vec::new();
    let mut bound: Vec<&str> = Vec::new();
    collect_block_idents(block, src, &mut referenced, &mut bound);
    let mut captures = filter_captures(referenced, bound, params, enclosing)?;
    let mut scope: Vec<String> = params.iter().map(|(n, _)| n.clone()).collect();
    outer_write_captures_block(block, src, enclosing, &mut scope, &mut captures);
    Ok(captures)
}

pub(crate) fn function_default_captures<'a>(
    expr: &'a Expr,
    enclosing: &[(String, Bind)],
    src: &'a LoweredText,
) -> Result<Vec<&'a str>, EmitError> {
    let mut referenced: Vec<&str> = Vec::new();
    let mut bound: Vec<&str> = Vec::new();
    collect_idents(expr, src, &mut referenced, &mut bound);
    filter_captures(referenced, bound, &[], enclosing)
}

pub(crate) fn function_defaults_captures<'a>(
    decl: &'a FunctionDecl,
    enclosing: &[(String, Bind)],
    src: &'a LoweredText,
) -> Result<Vec<&'a str>, EmitError> {
    let fixed_count =
        decl.params.len() - decl.params.last().filter(|p| p.variadic).is_some() as usize;
    let mut captures = Vec::new();
    for param in &decl.params[..fixed_count] {
        if let Some(default) = &param.default {
            for capture in function_default_captures(default, enclosing, src)? {
                if !captures.contains(&capture) {
                    captures.push(capture);
                }
            }
        }
    }
    Ok(captures)
}

pub(crate) fn push_pattern_scope_names(
    pattern: &Pattern,
    src: &LoweredText,
    scope: &mut Vec<String>,
) {
    let mut names = Vec::new();
    pattern_binds(pattern, src, &mut names);
    scope.extend(names.into_iter().map(str::to_string));
}

pub(crate) fn push_outer_write_capture<'a>(
    name: &'a str,
    scope: &[String],
    enclosing: &[(String, Bind)],
    captures: &mut Vec<&'a str>,
) {
    if scope.iter().any(|n| n == name) || captures.contains(&name) {
        return;
    }
    if has_local(enclosing, name) {
        captures.push(name);
    }
}

pub(crate) fn outer_write_captures_target<'a>(
    target: &'a Expr,
    src: &'a LoweredText,
    enclosing: &[(String, Bind)],
    scope: &[String],
    captures: &mut Vec<&'a str>,
) {
    if let ExprKind::Ident = &target.kind {
        push_outer_write_capture(text(src, target.span), scope, enclosing, captures);
    }
}

pub(crate) fn outer_write_captures_expr<'a>(
    expr: &'a Expr,
    src: &'a LoweredText,
    enclosing: &[(String, Bind)],
    scope: &mut Vec<String>,
    captures: &mut Vec<&'a str>,
) {
    match &expr.kind {
        ExprKind::Paren(inner) => outer_write_captures_expr(inner, src, enclosing, scope, captures),
        ExprKind::Unary { operand, .. } => {
            outer_write_captures_expr(operand, src, enclosing, scope, captures)
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            outer_write_captures_expr(lhs, src, enclosing, scope, captures);
            outer_write_captures_expr(rhs, src, enclosing, scope, captures);
        }
        ExprKind::Range { lo, hi, step, .. } => {
            outer_write_captures_expr(lo, src, enclosing, scope, captures);
            outer_write_captures_expr(hi, src, enclosing, scope, captures);
            if let Some(step) = step {
                outer_write_captures_expr(step, src, enclosing, scope, captures);
            }
        }
        ExprKind::Array(elements) => {
            for el in elements {
                match el {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => {
                        outer_write_captures_expr(e, src, enclosing, scope, captures);
                    }
                }
            }
        }
        ExprKind::RecordLiteral { fields } => {
            for field in fields {
                outer_write_captures_expr(&field.value, src, enclosing, scope, captures);
            }
        }
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            outer_write_captures_expr(base, src, enclosing, scope, captures);
            if let Some(spread) = spread {
                outer_write_captures_expr(spread, src, enclosing, scope, captures);
            }
            for field in fields {
                outer_write_captures_expr(&field.value, src, enclosing, scope, captures);
            }
        }
        ExprKind::String(lit) => {
            for part in &lit.parts {
                if let StringPart::Interpolation(e) = part {
                    outer_write_captures_expr(e, src, enclosing, scope, captures);
                }
            }
        }
        ExprKind::Call { callee, args, .. } => {
            outer_write_captures_expr(callee, src, enclosing, scope, captures);
            for arg in args {
                match arg {
                    CallArg::Positional(e) | CallArg::Spread(e) => {
                        outer_write_captures_expr(e, src, enclosing, scope, captures);
                    }
                    CallArg::Named { value, .. } => {
                        outer_write_captures_expr(value, src, enclosing, scope, captures);
                    }
                }
            }
        }
        ExprKind::Block(block) => {
            let mut child = scope.clone();
            outer_write_captures_block(block, src, enclosing, &mut child, captures);
        }
        ExprKind::Loop { body, .. } => {
            let mut loop_scope = scope.clone();
            outer_write_captures_block(body, src, enclosing, &mut loop_scope, captures);
        }
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            outer_write_captures_expr(cond, src, enclosing, scope, captures);
            let mut then_scope = scope.clone();
            outer_write_captures_block(then_block, src, enclosing, &mut then_scope, captures);
            if let Some(else_branch) = else_branch {
                outer_write_captures_expr(else_branch, src, enclosing, scope, captures);
            }
        }
        ExprKind::For {
            pattern,
            iter,
            body,
        } => {
            outer_write_captures_expr(iter, src, enclosing, scope, captures);
            let mut body_scope = scope.clone();
            push_pattern_scope_names(pattern, src, &mut body_scope);
            outer_write_captures_block(body, src, enclosing, &mut body_scope, captures);
        }
        ExprKind::Match { scrutinee, cases } => {
            outer_write_captures_expr(scrutinee, src, enclosing, scope, captures);
            for case in cases {
                let mut case_scope = scope.clone();
                push_pattern_scope_names(&case.pattern, src, &mut case_scope);
                if let Some(guard) = &case.guard {
                    outer_write_captures_expr(guard, src, enclosing, &mut case_scope, captures);
                }
                match &case.body {
                    CaseArmBody::Expr(e) => {
                        outer_write_captures_expr(e, src, enclosing, &mut case_scope, captures);
                    }
                    CaseArmBody::Return { value: Some(e), .. } => {
                        outer_write_captures_expr(e, src, enclosing, &mut case_scope, captures);
                    }
                    CaseArmBody::Return { value: None, .. } => {}
                }
            }
        }
        ExprKind::Lambda { params, body } => {
            let mut lambda_scope = scope.clone();
            for param in params {
                lambda_scope.push(text(src, param.name.span).to_string());
            }
            outer_write_captures_expr(body, src, enclosing, &mut lambda_scope, captures);
        }
        ExprKind::Concurrent {
            arms, else_block, ..
        } => {
            for arm in arms {
                outer_write_captures_expr(&arm.value, src, enclosing, scope, captures);
            }
            if let Some(else_block) = else_block {
                let mut else_scope = scope.clone();
                outer_write_captures_block(else_block, src, enclosing, &mut else_scope, captures);
            }
        }
        ExprKind::Member { object, .. } | ExprKind::OptionalAccess { object, .. } => {
            outer_write_captures_expr(object, src, enclosing, scope, captures);
        }
        ExprKind::Index { object, index } => {
            outer_write_captures_expr(object, src, enclosing, scope, captures);
            outer_write_captures_expr(index, src, enclosing, scope, captures);
        }
        ExprKind::Try(inner) => outer_write_captures_expr(inner, src, enclosing, scope, captures),
        ExprKind::Compose { lhs, rhs } => {
            outer_write_captures_expr(lhs, src, enclosing, scope, captures);
            outer_write_captures_expr(rhs, src, enclosing, scope, captures);
        }
        ExprKind::Pipe { lhs, rhs } => {
            outer_write_captures_expr(lhs, src, enclosing, scope, captures);
            if let PipeRhs::Expr(stage) = rhs {
                outer_write_captures_expr(stage, src, enclosing, scope, captures);
            }
        }
        ExprKind::SetLiteral(elements) => {
            for element in elements {
                outer_write_captures_expr(element, src, enclosing, scope, captures);
            }
        }
        ExprKind::MapLiteral(entries) => {
            for (key, value) in entries {
                outer_write_captures_expr(key, src, enclosing, scope, captures);
                outer_write_captures_expr(value, src, enclosing, scope, captures);
            }
        }
        ExprKind::Comprehension { clauses, body, .. } => {
            let mut comp_scope = scope.clone();
            for clause in clauses {
                match clause {
                    CompClause::For { pattern, iter } => {
                        outer_write_captures_expr(iter, src, enclosing, &mut comp_scope, captures);
                        push_pattern_scope_names(pattern, src, &mut comp_scope);
                    }
                    CompClause::If(cond) => {
                        outer_write_captures_expr(cond, src, enclosing, &mut comp_scope, captures);
                    }
                }
            }
            match body {
                CompBody::Elem(e) => {
                    outer_write_captures_expr(e, src, enclosing, &mut comp_scope, captures);
                }
                CompBody::Entry { key, value } => {
                    outer_write_captures_expr(key, src, enclosing, &mut comp_scope, captures);
                    outer_write_captures_expr(value, src, enclosing, &mut comp_scope, captures);
                }
            }
        }
        _ => {}
    }
}

pub(crate) fn outer_write_captures_stmt<'a>(
    stmt: &'a Stmt,
    src: &'a LoweredText,
    enclosing: &[(String, Bind)],
    scope: &mut Vec<String>,
    captures: &mut Vec<&'a str>,
) {
    match &stmt.kind {
        StmtKind::Let { pattern, value, .. } => {
            outer_write_captures_expr(value, src, enclosing, scope, captures);
            push_pattern_scope_names(pattern, src, scope);
        }
        StmtKind::Const { name, value, .. } => {
            outer_write_captures_expr(value, src, enclosing, scope, captures);
            scope.push(text(src, name.span).to_string());
        }
        StmtKind::Expr(e) => outer_write_captures_expr(e, src, enclosing, scope, captures),
        StmtKind::Assign { target, value, .. } => {
            outer_write_captures_target(target, src, enclosing, scope, captures);
            outer_write_captures_expr(value, src, enclosing, scope, captures);
        }
        StmtKind::While { cond, body } => {
            outer_write_captures_expr(cond, src, enclosing, scope, captures);
            let mut body_scope = scope.clone();
            outer_write_captures_block(body, src, enclosing, &mut body_scope, captures);
        }
        StmtKind::Using { name, value, body } => {
            outer_write_captures_expr(value, src, enclosing, scope, captures);
            let mut body_scope = scope.clone();
            body_scope.push(text(src, name.span).to_string());
            outer_write_captures_block(body, src, enclosing, &mut body_scope, captures);
            scope.push(text(src, name.span).to_string());
        }
        StmtKind::Function(decl) => {
            let mut fn_scope = scope.clone();
            fn_scope.push(text(src, decl.name.span).to_string());
            for param in &decl.params {
                fn_scope.push(text(src, param.name.span).to_string());
            }
            outer_write_captures_block(&decl.body, src, enclosing, &mut fn_scope, captures);
            scope.push(text(src, decl.name.span).to_string());
        }
        StmtKind::Return(Some(e)) => outer_write_captures_expr(e, src, enclosing, scope, captures),
        StmtKind::Defer(action) => {
            outer_write_captures_expr(action, src, enclosing, scope, captures)
        }
        _ => {}
    }
}

pub(crate) fn outer_write_captures_block<'a>(
    block: &'a Block,
    src: &'a LoweredText,
    enclosing: &[(String, Bind)],
    scope: &mut Vec<String>,
    captures: &mut Vec<&'a str>,
) {
    for stmt in &block.stmts {
        outer_write_captures_stmt(stmt, src, enclosing, scope, captures);
    }
    if let Some(tail) = &block.tail {
        outer_write_captures_expr(tail, src, enclosing, scope, captures);
    }
}

/// From a body's referenced/bound name sets, the captures: a referenced
/// name that is neither bound in the body nor a param, and IS an enclosing
/// local. Mutability is intentionally not checked here: this non-refusing
/// walk feeds escape analysis, and `push_capture_locals` later classifies
/// each capture as `Imm`, `Cell`, or refused plain `Mut`.
pub(crate) fn filter_captures<'a>(
    referenced: Vec<&'a str>,
    bound: Vec<&'a str>,
    params: &[(String, Bind)],
    enclosing: &[(String, Bind)],
) -> Result<Vec<&'a str>, EmitError> {
    // NON-refusing: collect EVERY captured enclosing name (immutable, plain
    // mutable, or cell). The mutability classification — snapshot an `Imm`,
    // share a `Cell`'s `Rc`, or REFUSE a plain `Mut` (the safety gate) — happens
    // at the closure-emission site, which has the binding's `Bind`. This same
    // non-refusing walk drives the escape analysis (`scope_cell_set`), so it
    // must see mutable captures rather than erroring on them.
    let mut captures: Vec<&str> = Vec::new();
    for name in referenced {
        if bound.contains(&name)
            || params.iter().any(|(n, _)| n == name)
            || captures.contains(&name)
        {
            continue;
        }
        if has_local(enclosing, name) {
            captures.push(name);
        }
    }
    Ok(captures)
}

/// Push a closure's captures onto its body-locals, classifying each by the
/// ENCLOSING binding (the §5 safety gate). A capture of an immutable binding is
/// a value SNAPSHOT (`Imm`); a capture of a cell shares the `Rc` (`Cell`, so the
/// body reads through `cell_get` and writes through `cell_set`); a capture of a
/// plain `Mut` is REFUSED — only a `Cell` may be captured mutable, so any
/// escape-analysis miss declines the program rather than emitting a stale
/// snapshot that would diverge. The emitted capture code (`x.clone()`) is the
/// same for `Imm` and `Cell` — the binding's Rust TYPE makes `.clone()` snapshot
/// a `Value` or share an `Rc<RefCell<Value>>`.
pub(crate) fn push_capture_locals(
    captures: &[&str],
    enclosing: &[(String, Bind)],
    body_locals: &mut Vec<(String, Bind)>,
) -> Result<(), EmitError> {
    for cap in captures {
        let bind = match lookup_bind(enclosing, cap) {
            Some(Bind::Mut) => {
                return Err(EmitError::unsupported(
                    "closure capture of a mutable binding",
                ));
            }
            // `Cell` (captured-mutable) and `Imm` are both fine; a name not in
            // `enclosing` cannot occur (filter_captures only keeps locals).
            Some(b) => b,
            None => Bind::Imm,
        };
        body_locals.push((cap.to_string(), bind));
    }
    Ok(())
}

/// §5 ESCAPE ANALYSIS — which of THIS scope's `let mut` bindings a closure
/// CAPTURES (and so must become a rebinding cell, not a plain `let mut`).
///
/// A binding is in scope only AFTER its statement, so a closure that captures
/// `x` must come AFTER `let mut x`. This SIMULATES the scope's declarations in
/// order (mirroring `emit_stmt_seq`'s binding rules), and for each statement
/// collects the captures of every closure IN it (the non-refusing
/// `stmt_lambda_captures` walk) against the bindings in scope SO FAR; it then
/// returns this scope's `let mut` names that were captured. A MISS here is
/// caught downstream by the `push_capture_locals` safety gate (a captured plain
/// `Mut` is REFUSED, never silently snapshotted), so this only needs to be
/// complete enough to avoid over-refusing the common forms. A false positive (a
/// non-captured `let mut` made a cell) is sound — only slightly wasteful.
pub(crate) fn scope_cell_set<'a>(
    stmts: &'a [Stmt],
    tail: Option<&'a Expr>,
    src: &'a LoweredText,
    enclosing: &[(String, Bind)],
) -> Vec<&'a str> {
    let mut sim: Vec<(String, Bind)> = enclosing.to_vec();
    let mut captured: Vec<&str> = Vec::new();
    let mut muts_here: Vec<&str> = Vec::new();
    for stmt in stmts {
        // Closures in THIS statement capture bindings declared BEFORE it.
        let _ = stmt_lambda_captures(stmt, src, &sim, &mut captured);
        let record_stmt = match &stmt.kind {
            StmtKind::Export(inner) => inner.as_ref(),
            _ => stmt,
        };
        if let StmtKind::Record(decl) = &record_stmt.kind {
            for field in &decl.fields {
                if let Some(default) = &field.default
                    && !imported_nominal_record_default_is_self_contained(default)
                    && let Ok(default_captures) = lambda_captures(default, &[], &sim, src)
                {
                    for capture in default_captures {
                        if !captured.contains(&capture) {
                            captured.push(capture);
                        }
                    }
                }
            }
        }
        // Then this statement's own binding enters scope (mirroring the
        // emission: an immutable `let`/`function` can be captured by snapshot;
        // a `let mut` is a cell candidate).
        match &stmt.kind {
            StmtKind::Let {
                mutable, pattern, ..
            } => {
                if let Ok(Some(name)) = binding_name(pattern, src) {
                    sim.push((
                        name.to_string(),
                        if *mutable { Bind::Mut } else { Bind::Imm },
                    ));
                    if *mutable {
                        muts_here.push(name);
                    }
                }
            }
            StmtKind::Function(decl) => {
                sim.push((text(src, decl.name.span).to_string(), Bind::Imm));
            }
            StmtKind::Using { .. } => {}
            // §4 a `const` is an immutable binding — never a cell candidate.
            StmtKind::Const { name, .. } => {
                sim.push((text(src, name.span).to_string(), Bind::Imm));
            }
            _ => {}
        }
    }
    if let Some(t) = tail {
        let _ = expr_lambda_captures(t, src, &sim, &mut captured);
    }
    muts_here
        .into_iter()
        .filter(|n| captured.contains(n))
        .collect()
}

/// The one name owned by a simple plain or typed binding pattern. Export inventory,
/// top-level cell seeding, and record-default facts all use this projection instead of
/// expanding a general pattern into a temporary collection.
pub(crate) fn single_binding_pattern_name<'a>(
    pattern: &'a Pattern,
    src: &'a LoweredText,
) -> Option<&'a str> {
    match &pattern.kind {
        PatternKind::Binding(name) | PatternKind::Typed { name, .. } => Some(text(src, name.span)),
        _ => None,
    }
}

/// Every name a pattern BINDS, into `bound` (for [`collect_idents`], so a pattern
/// body's reference to a pattern binding is not mistaken for a free var the enclosing
/// lambda must capture). Covers the emittable binding forms: a `Binding`, a `Typed`
/// name, a constructor's subpattern bindings (recursively, e.g. `Some(x)`), a LIST
/// pattern's element + rest bindings, structural/nominal RECORD field bindings
/// (shorthand `{x}` binds `x`, `{x: p}` binds `p`'s names), and OR-pattern binding
/// agreement (unioned by name, not duplicated per alternative). Binding-free patterns
/// such as wildcards, literals, and ranges add nothing.
pub(crate) fn pattern_binds<'a>(
    pattern: &'a Pattern,
    src: &'a LoweredText,
    bound: &mut Vec<&'a str>,
) {
    match &pattern.kind {
        PatternKind::Binding(n) => bound.push(text(src, n.span)),
        PatternKind::Typed { name, .. } => bound.push(text(src, name.span)),
        PatternKind::Constructor { args, .. } => {
            for a in args {
                pattern_binds(a, src, bound);
            }
        }
        PatternKind::List(elems) => {
            for e in elems {
                match e {
                    ListPatternElem::Pattern(p) | ListPatternElem::Rest(Some(p)) => {
                        pattern_binds(p, src, bound)
                    }
                    ListPatternElem::Rest(None) => {}
                }
            }
        }
        PatternKind::Record(fields) | PatternKind::NominalRecord { fields, .. } => {
            for f in fields {
                match &f.pattern {
                    Some(p) => pattern_binds(p, src, bound),
                    None => bound.push(text(src, f.name.span)),
                }
            }
        }
        PatternKind::Or(alts) => {
            let mut names = Vec::new();
            for alt in alts {
                pattern_binds(alt, src, &mut names);
            }
            for name in names {
                if !bound.contains(&name) {
                    bound.push(name);
                }
            }
        }
        _ => {}
    }
}

/// Flat walk collecting every identifier the body REFERENCES and every
/// name it BINDS (within supported constructs). `bound` must be complete
/// for the supported binding forms (a missed bind could let a body-bound
/// name be wrongly captured); `referenced` may be incomplete (a missed
/// reference only over-refuses). Unsupported sub-constructs need no
/// handling — the body lowering refuses the lambda anyway.
pub(crate) fn collect_idents<'a>(
    expr: &'a Expr,
    src: &'a LoweredText,
    referenced: &mut Vec<&'a str>,
    bound: &mut Vec<&'a str>,
) {
    match &expr.kind {
        ExprKind::Ident => referenced.push(text(src, expr.span)),
        ExprKind::Paren(inner) => collect_idents(inner, src, referenced, bound),
        ExprKind::Unary { operand, .. } => collect_idents(operand, src, referenced, bound),
        ExprKind::Binary { lhs, rhs, .. } => {
            collect_idents(lhs, src, referenced, bound);
            collect_idents(rhs, src, referenced, bound);
        }
        ExprKind::Range { lo, hi, step, .. } => {
            collect_idents(lo, src, referenced, bound);
            collect_idents(hi, src, referenced, bound);
            if let Some(s) = step {
                collect_idents(s, src, referenced, bound);
            }
        }
        ExprKind::Array(elements) => {
            for el in elements {
                // A §9 spread operand `...e` is lowered too, so its references
                // must count (else a name used only in a spread is missed —
                // an over-refusal, or a closure under a spread escapes the
                // later-shadow guard).
                match el {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => {
                        collect_idents(e, src, referenced, bound)
                    }
                }
            }
        }
        ExprKind::RecordLiteral { fields } => {
            for f in fields {
                collect_idents(&f.value, src, referenced, bound);
            }
        }
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            collect_idents(base, src, referenced, bound);
            if let Some(spread) = spread {
                collect_idents(spread, src, referenced, bound);
            }
            for f in fields {
                collect_idents(&f.value, src, referenced, bound);
            }
        }
        ExprKind::String(lit) => {
            for part in &lit.parts {
                if let StringPart::Interpolation(e) = part {
                    collect_idents(e, src, referenced, bound);
                }
            }
        }
        ExprKind::Call { callee, args, .. } => {
            collect_idents(callee, src, referenced, bound);
            for a in args {
                match a {
                    CallArg::Positional(e) | CallArg::Spread(e) => {
                        collect_idents(e, src, referenced, bound)
                    }
                    CallArg::Named { value, .. } => collect_idents(value, src, referenced, bound),
                }
            }
        }
        ExprKind::Block(block) => collect_block_idents(block, src, referenced, bound),
        ExprKind::Loop { body, .. } => collect_block_idents(body, src, referenced, bound),
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            collect_idents(cond, src, referenced, bound);
            collect_block_idents(then_block, src, referenced, bound);
            if let Some(e) = else_branch {
                collect_idents(e, src, referenced, bound);
            }
        }
        ExprKind::For {
            pattern,
            iter,
            body,
        } => {
            collect_idents(iter, src, referenced, bound);
            // `pattern_binds` (not `binding_name`) so a TYPED loop variable
            // `for x: int in …` records its binding as bound.
            pattern_binds(pattern, src, bound);
            collect_block_idents(body, src, referenced, bound);
        }
        ExprKind::Match { scrutinee, cases } => {
            collect_idents(scrutinee, src, referenced, bound);
            for case in cases {
                // Every name the pattern BINDS (a top-level binding OR a
                // constructor subpattern binding like `Some(x)`) is `bound`, so
                // the case body's reference to it is not mistaken for a free var
                // that the enclosing lambda must capture.
                pattern_binds(&case.pattern, src, bound);
                // A §5 `case` GUARD references identifiers too (the binding —
                // already `bound` above — plus enclosing names), so it is
                // walked for the free/bound sets.
                if let Some(g) = &case.guard {
                    collect_idents(g, src, referenced, bound);
                }
                // (A §5 literal pattern is a plain constant — `emit_match`
                // refuses an interpolated one — so it carries no identifiers.)
                // BOTH arm-body shapes lower an expression: an `Expr` value or a
                // `return e` (a closure can appear in either).
                match &case.body {
                    CaseArmBody::Expr(e) => collect_idents(e, src, referenced, bound),
                    CaseArmBody::Return { value: Some(e), .. } => {
                        collect_idents(e, src, referenced, bound)
                    }
                    CaseArmBody::Return { value: None, .. } => {}
                }
            }
        }
        ExprKind::Lambda { params, body } => {
            for p in params {
                bound.push(text(src, p.name.span));
            }
            collect_idents(body, src, referenced, bound);
        }
        // §15 each `concurrent` arm — and the `else` block — is a no-parameter closure
        // body, so its references count as the enclosing scope's (they are captured). The
        // `timeout` is a duration literal with no identifiers.
        ExprKind::Concurrent {
            arms, else_block, ..
        } => {
            for arm in arms {
                collect_idents(&arm.value, src, referenced, bound);
            }
            if let Some(else_blk) = else_block {
                collect_block_idents(else_blk, src, referenced, bound);
            }
        }
        // §8/§1 member access and indexing — the emitter lowers both, so their
        // sub-expressions' references must count (the field is a member name,
        // not a variable). Missing them would over-refuse a capture used only
        // through `.f`/`[i]`.
        ExprKind::Member { object, .. } => collect_idents(object, src, referenced, bound),
        ExprKind::OptionalAccess { object, .. } => collect_idents(object, src, referenced, bound),
        ExprKind::Index { object, index } => {
            collect_idents(object, src, referenced, bound);
            collect_idents(index, src, referenced, bound);
        }
        // §13 `e?` lowers its operand, so its references count.
        ExprKind::Try(inner) => collect_idents(inner, src, referenced, bound),
        // §11 `f >> g` lowers both operands, so their references count.
        ExprKind::Compose { lhs, rhs } => {
            collect_idents(lhs, src, referenced, bound);
            collect_idents(rhs, src, referenced, bound);
        }
        // §11 `lhs |> stage` lowers the lhs and the stage expr (a field stage is
        // a member name, no expr).
        ExprKind::Pipe { lhs, rhs } => {
            collect_idents(lhs, src, referenced, bound);
            if let PipeRhs::Expr(stage) = rhs {
                collect_idents(stage, src, referenced, bound);
            }
        }
        // §11 a placeholder `_` REFERENCES the pipe-stage binding `_`, so a
        // closure that uses `_` must capture it — count `_` as a reference.
        ExprKind::Placeholder => referenced.push("_"),
        // §6 (v5.4) a set/map literal lowers each element/entry, so a name used only
        // inside one must count as referenced (else a capture is missed).
        ExprKind::SetLiteral(elements) => {
            for e in elements {
                collect_idents(e, src, referenced, bound);
            }
        }
        ExprKind::MapLiteral(entries) => {
            for (k, v) in entries {
                collect_idents(k, src, referenced, bound);
                collect_idents(v, src, referenced, bound);
            }
        }
        // §6.4 a comprehension's clauses/body reference names. Each `for` pattern
        // binds after its iterable and before later clauses/body, so those loop locals
        // are not misclassified as captures of an enclosing lambda.
        ExprKind::Comprehension { clauses, body, .. } => {
            for clause in clauses {
                match clause {
                    CompClause::For { pattern, iter } => {
                        collect_idents(iter, src, referenced, bound);
                        pattern_binds(pattern, src, bound);
                    }
                    CompClause::If(cond) => collect_idents(cond, src, referenced, bound),
                }
            }
            match body {
                CompBody::Elem(e) => collect_idents(e, src, referenced, bound),
                CompBody::Entry { key, value } => {
                    collect_idents(key, src, referenced, bound);
                    collect_idents(value, src, referenced, bound);
                }
            }
        }
        _ => {}
    }
}

/// Whether one statement contains a value reference to `name`. This is used
/// only to preserve the enclosing value of a body-local function until its
/// positional declaration executes; false positives merely add a safe capture,
/// while a false negative would turn an outer read into an unbound-cell fault.
pub(crate) fn stmt_references_name(stmt: &Stmt, src: &LoweredText, name: &str) -> bool {
    let mut referenced = Vec::new();
    let mut bound = Vec::new();
    match &stmt.kind {
        StmtKind::Export(inner) => return stmt_references_name(inner, src, name),
        StmtKind::Function(decl) => {
            for param in &decl.params {
                if let Some(default) = &param.default {
                    collect_idents(default, src, &mut referenced, &mut bound);
                }
            }
            collect_block_idents(&decl.body, src, &mut referenced, &mut bound);
        }
        StmtKind::Let { value, .. } | StmtKind::Const { value, .. } => {
            collect_idents(value, src, &mut referenced, &mut bound)
        }
        StmtKind::Assign { target, value, .. } => {
            collect_idents(target, src, &mut referenced, &mut bound);
            collect_idents(value, src, &mut referenced, &mut bound);
        }
        StmtKind::Defer(value) => collect_idents(value, src, &mut referenced, &mut bound),
        StmtKind::Return(Some(value))
        | StmtKind::Expr(value)
        | StmtKind::Break {
            value: Some(value), ..
        } => collect_idents(value, src, &mut referenced, &mut bound),
        StmtKind::Using { value, body, .. } => {
            collect_idents(value, src, &mut referenced, &mut bound);
            collect_block_idents(body, src, &mut referenced, &mut bound);
        }
        StmtKind::While { cond, body } => {
            collect_idents(cond, src, &mut referenced, &mut bound);
            collect_block_idents(body, src, &mut referenced, &mut bound);
        }
        StmtKind::Record(decl) => {
            for field in &decl.fields {
                if let Some(default) = &field.default {
                    collect_idents(default, src, &mut referenced, &mut bound);
                }
            }
        }
        StmtKind::Impl(decl) => {
            for method in &decl.methods {
                collect_block_idents(&method.decl.body, src, &mut referenced, &mut bound);
            }
        }
        StmtKind::Import(_)
        | StmtKind::TypeAlias(_)
        | StmtKind::Enum(_)
        | StmtKind::Newtype(_)
        | StmtKind::Protocol(_)
        | StmtKind::Return(None)
        | StmtKind::Break { value: None, .. }
        | StmtKind::Continue { .. } => {}
    }
    referenced.contains(&name)
}

/// A block's identifiers: each `let` binds (recorded so a later free use
/// is not a capture), each statement/tail recurses.
pub(crate) fn collect_block_idents<'a>(
    block: &'a Block,
    src: &'a LoweredText,
    referenced: &mut Vec<&'a str>,
    bound: &mut Vec<&'a str>,
) {
    for stmt in &block.stmts {
        match &stmt.kind {
            StmtKind::Let { pattern, value, .. } => {
                collect_idents(value, src, referenced, bound);
                // `pattern_binds` (not `binding_name`) so a TYPED let `let x: T
                // = v` — whose pattern `binding_name` refuses — still records its
                // binding as bound (else a body reference to it looks like a free
                // var). Covers every binding pattern form.
                pattern_binds(pattern, src, bound);
            }
            // §4 a `const` binds a single name and walks its initializer.
            StmtKind::Const { name, value, .. } => {
                collect_idents(value, src, referenced, bound);
                bound.push(text(src, name.span));
            }
            StmtKind::Expr(e) => collect_idents(e, src, referenced, bound),
            StmtKind::Assign { target, value, .. } => {
                collect_idents(target, src, referenced, bound);
                collect_idents(value, src, referenced, bound);
            }
            StmtKind::While { cond, body } => {
                collect_idents(cond, src, referenced, bound);
                collect_block_idents(body, src, referenced, bound);
            }
            StmtKind::Using { name, value, body } => {
                collect_idents(value, src, referenced, bound);
                bound.push(text(src, name.span));
                collect_block_idents(body, src, referenced, bound);
            }
            // A NESTED `function` — like a lambda (the [`collect_idents`]
            // `Lambda` arm): its NAME and PARAMS bind in this block (so a use
            // of either is not a free reference), and its body recurses so an
            // enclosing name used ONLY inside the nested function still counts
            // as referenced — otherwise the outer closure would not capture
            // it and the nested body would refuse it as a free identifier.
            StmtKind::Function(decl) => {
                bound.push(text(src, decl.name.span));
                for p in &decl.params {
                    bound.push(text(src, p.name.span));
                }
                collect_block_idents(&decl.body, src, referenced, bound);
            }
            // §5/§7 value transfers carry ordinary expressions. Their references
            // count just like a tail expression, including a closure or call that
            // reaches an enclosing name.
            StmtKind::Return(Some(e)) | StmtKind::Break { value: Some(e), .. } => {
                collect_idents(e, src, referenced, bound)
            }
            // §14 a `defer ACTION`'s references count too — an enclosing name used
            // ONLY inside a nested closure's defer must still be captured by that
            // closure (else its body refuses the name as a free identifier).
            StmtKind::Defer(action) => collect_idents(action, src, referenced, bound),
            _ => {}
        }
    }
    if let Some(tail) = &block.tail {
        collect_idents(tail, src, referenced, bound);
    }
}

/// Collect the captures (enclosing locals) of EVERY lambda appearing in
/// `expr`, NOT descending into a found lambda's body (its own analysis
/// already folds in nested lambdas). Used by [`emit_stmt_seq`] to track
/// which enclosing bindings a statement's closures snapshot, so a LATER
/// same-scope declaration that would shadow such a binding is refused —
/// the interpreter's whole-env capture would observe the new binding, but
/// the emitter froze the old value (CDR-006 §4). Capture attribution is a
/// sound OVER-approximation: an intervening block-local that shadows an
/// enclosing name is not tracked here, so such a capture may be ascribed
/// to the enclosing name and over-refuse (never under-refuse).
///
/// MUST mirror [`collect_idents`]'s traversal (a missed container would
/// hide a lambda and silently drop the shadow guard) — keep the two in
/// step when the expression grammar grows.
pub(crate) fn expr_lambda_captures<'a>(
    expr: &'a Expr,
    src: &'a LoweredText,
    // Decoupled from `'a` (see `lambda_captures`): the captures borrow `src`,
    // so the caller may record them and keep mutating its locals.
    locals: &[(String, Bind)],
    out: &mut Vec<&'a str>,
) -> Result<(), EmitError> {
    match &expr.kind {
        ExprKind::Lambda { params, body } => {
            let param_locals: Vec<(String, Bind)> = params
                .iter()
                .map(|p| (text(src, p.name.span).to_string(), Bind::Imm))
                .collect();
            // The value already lowered, so this cannot newly refuse.
            for c in lambda_captures(body, &param_locals, locals, src)? {
                if !out.contains(&c) {
                    out.push(c);
                }
            }
        }
        // §15 each `concurrent` arm — and the `else` block — is a no-parameter nested
        // closure; gather its captures so an enclosing lambda captures them too.
        ExprKind::Concurrent {
            arms, else_block, ..
        } => {
            for arm in arms {
                for c in lambda_captures(&arm.value, &[], locals, src)? {
                    if !out.contains(&c) {
                        out.push(c);
                    }
                }
            }
            if let Some(else_blk) = else_block {
                for c in closure_captures_block(else_blk, &[], locals, src)? {
                    if !out.contains(&c) {
                        out.push(c);
                    }
                }
            }
        }
        ExprKind::Paren(inner) => expr_lambda_captures(inner, src, locals, out)?,
        ExprKind::Unary { operand, .. } => expr_lambda_captures(operand, src, locals, out)?,
        ExprKind::Binary { lhs, rhs, .. } => {
            expr_lambda_captures(lhs, src, locals, out)?;
            expr_lambda_captures(rhs, src, locals, out)?;
        }
        ExprKind::Range { lo, hi, step, .. } => {
            expr_lambda_captures(lo, src, locals, out)?;
            expr_lambda_captures(hi, src, locals, out)?;
            if let Some(s) = step {
                expr_lambda_captures(s, src, locals, out)?;
            }
        }
        ExprKind::Array(elements) => {
            for el in elements {
                // A §9 spread operand `...e` is lowered too, so a closure under
                // it must be recorded for the later-shadow guard.
                match el {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => {
                        expr_lambda_captures(e, src, locals, out)?
                    }
                }
            }
        }
        ExprKind::RecordLiteral { fields } => {
            for f in fields {
                expr_lambda_captures(&f.value, src, locals, out)?;
            }
        }
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            expr_lambda_captures(base, src, locals, out)?;
            if let Some(spread) = spread {
                expr_lambda_captures(spread, src, locals, out)?;
            }
            for f in fields {
                expr_lambda_captures(&f.value, src, locals, out)?;
            }
        }
        ExprKind::String(lit) => {
            for part in &lit.parts {
                if let StringPart::Interpolation(e) = part {
                    expr_lambda_captures(e, src, locals, out)?;
                }
            }
        }
        ExprKind::Call { callee, args, .. } => {
            expr_lambda_captures(callee, src, locals, out)?;
            for a in args {
                match a {
                    CallArg::Positional(e) | CallArg::Spread(e) => {
                        expr_lambda_captures(e, src, locals, out)?
                    }
                    CallArg::Named { value, .. } => expr_lambda_captures(value, src, locals, out)?,
                }
            }
        }
        ExprKind::Block(block) => block_lambda_captures(block, src, locals, out)?,
        ExprKind::Loop { body, .. } => block_lambda_captures(body, src, locals, out)?,
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            expr_lambda_captures(cond, src, locals, out)?;
            block_lambda_captures(then_block, src, locals, out)?;
            if let Some(e) = else_branch {
                expr_lambda_captures(e, src, locals, out)?;
            }
        }
        ExprKind::For { iter, body, .. } => {
            expr_lambda_captures(iter, src, locals, out)?;
            block_lambda_captures(body, src, locals, out)?;
        }
        ExprKind::Match { scrutinee, cases } => {
            expr_lambda_captures(scrutinee, src, locals, out)?;
            for case in cases {
                // A §5 `case` GUARD can hold a closure (a later-shadow / cell
                // capture), so walk it too — against `locals`, exactly as the
                // arm body (the case binding is not an enclosing local; an
                // over-approximation there is sound).
                if let Some(g) = &case.guard {
                    expr_lambda_captures(g, src, locals, out)?;
                }
                // (A §5 literal pattern is a plain constant — `emit_match`
                // refuses an interpolated one — so it holds no closures.) BOTH
                // arm-body shapes lower an expression that can hold a closure.
                match &case.body {
                    CaseArmBody::Expr(e) => expr_lambda_captures(e, src, locals, out)?,
                    CaseArmBody::Return { value: Some(e), .. } => {
                        expr_lambda_captures(e, src, locals, out)?
                    }
                    CaseArmBody::Return { value: None, .. } => {}
                }
            }
        }
        // §8/§1 member access and indexing — the emitter lowers both, so a
        // closure under `obj.f` or `obj[i]` must be recorded for the
        // later-shadow guard.
        ExprKind::Member { object, .. } => expr_lambda_captures(object, src, locals, out)?,
        ExprKind::OptionalAccess { object, .. } => expr_lambda_captures(object, src, locals, out)?,
        ExprKind::Index { object, index } => {
            expr_lambda_captures(object, src, locals, out)?;
            expr_lambda_captures(index, src, locals, out)?;
        }
        // §13 `e?` lowers its operand, so a closure under it must be recorded.
        ExprKind::Try(inner) => expr_lambda_captures(inner, src, locals, out)?,
        // §11 `f >> g` lowers both operands, so a closure under either is recorded.
        ExprKind::Compose { lhs, rhs } => {
            expr_lambda_captures(lhs, src, locals, out)?;
            expr_lambda_captures(rhs, src, locals, out)?;
        }
        // §11 `lhs |> stage` lowers the lhs and the stage expr.
        ExprKind::Pipe { lhs, rhs } => {
            expr_lambda_captures(lhs, src, locals, out)?;
            if let PipeRhs::Expr(stage) = rhs {
                expr_lambda_captures(stage, src, locals, out)?;
            }
        }
        // §6 (v5.4) a set/map literal lowers each element/entry, so a closure under
        // one must be recorded for the later-shadow guard.
        ExprKind::SetLiteral(elements) => {
            for e in elements {
                expr_lambda_captures(e, src, locals, out)?;
            }
        }
        ExprKind::MapLiteral(entries) => {
            for (k, v) in entries {
                expr_lambda_captures(k, src, locals, out)?;
                expr_lambda_captures(v, src, locals, out)?;
            }
        }
        // §6.4 a comprehension may host a lambda in any clause or in the body. Thread
        // the clause bindings through a temporary local scope so a nested closure that
        // references a loop variable captures that loop variable, not an outer same-named
        // binding.
        ExprKind::Comprehension { clauses, body, .. } => {
            let mut scope = locals.to_vec();
            for clause in clauses {
                match clause {
                    CompClause::For { pattern, iter } => {
                        expr_lambda_captures(iter, src, &scope, out)?;
                        let mut names = Vec::new();
                        pattern_binds(pattern, src, &mut names);
                        for name in names {
                            scope.push((name.to_string(), Bind::Imm));
                        }
                    }
                    CompClause::If(cond) => expr_lambda_captures(cond, src, &scope, out)?,
                }
            }
            match body {
                CompBody::Elem(e) => expr_lambda_captures(e, src, &scope, out)?,
                CompBody::Entry { key, value } => {
                    expr_lambda_captures(key, src, &scope, out)?;
                    expr_lambda_captures(value, src, &scope, out)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// The lambda/function captures a single statement's closures snapshot of
/// `locals`, descending into nested bodies (a closure created in a loop or
/// block body can ESCAPE by assignment to an enclosing binding, so its
/// captures are recorded against the ENCLOSING scope — a sound
/// over-approximation: a non-escaping nested capture is recorded too, only
/// over-refusing). MUST stay complete as the statement grammar grows.
pub(crate) fn stmt_lambda_captures<'a>(
    stmt: &'a Stmt,
    src: &'a LoweredText,
    locals: &[(String, Bind)],
    out: &mut Vec<&'a str>,
) -> Result<(), EmitError> {
    match &stmt.kind {
        StmtKind::Let { value, .. } | StmtKind::Const { value, .. } => {
            expr_lambda_captures(value, src, locals, out)
        }
        StmtKind::Expr(e) => expr_lambda_captures(e, src, locals, out),
        StmtKind::Assign { target, value, .. } => {
            expr_lambda_captures(target, src, locals, out)?;
            expr_lambda_captures(value, src, locals, out)
        }
        StmtKind::While { cond, body } => {
            expr_lambda_captures(cond, src, locals, out)?;
            block_lambda_captures(body, src, locals, out)
        }
        StmtKind::Using { name, value, body } => {
            expr_lambda_captures(value, src, locals, out)?;
            let mut body_locals = locals.to_vec();
            body_locals.push((text(src, name.span).to_string(), Bind::Imm));
            block_lambda_captures(body, src, &body_locals, out)
        }
        // A NESTED `function`'s body captures too (the outer scope's
        // accumulator must see them — the function value can escape).
        StmtKind::Function(decl) => {
            for c in function_defaults_captures(decl, locals, src)? {
                if !out.contains(&c) {
                    out.push(c);
                }
            }
            let param_locals: Vec<(String, Bind)> = decl
                .params
                .iter()
                .map(|p| (text(src, p.name.span).to_string(), Bind::Imm))
                .collect();
            for c in closure_captures_block(&decl.body, &param_locals, locals, src)? {
                if !out.contains(&c) {
                    out.push(c);
                }
            }
            Ok(())
        }
        // §5/§7 a value transfer can carry a closure capturing an enclosing
        // binding; record it for the later-shadow guard.
        StmtKind::Return(Some(e)) | StmtKind::Break { value: Some(e), .. } => {
            expr_lambda_captures(e, src, locals, out)
        }
        // §14 a `defer ACTION` is itself a captured closure (it runs at scope exit),
        // so its OWN free variables — not just lambdas nested inside it — must join the
        // cell-discovery pass, so a `let mut` referenced ONLY by a defer becomes a
        // rebinding cell (read at drain time), not a raw `Bind::Mut` the defer closure
        // would refuse. (`defer print(x)` after `x = …` must print the LATEST value.)
        // Use `lambda_captures` (free vars of the action), as the defer emit does.
        StmtKind::Defer(action) => {
            for c in lambda_captures(action, &[], locals, src)? {
                if !out.contains(&c) {
                    out.push(c);
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// The lambda captures within a block's statements and tail (the block-body
/// analogue of [`expr_lambda_captures`]).
pub(crate) fn block_lambda_captures<'a>(
    block: &'a Block,
    src: &'a LoweredText,
    locals: &[(String, Bind)],
    out: &mut Vec<&'a str>,
) -> Result<(), EmitError> {
    for stmt in &block.stmts {
        stmt_lambda_captures(stmt, src, locals, out)?;
    }
    if let Some(tail) = &block.tail {
        expr_lambda_captures(tail, src, locals, out)?;
    }
    Ok(())
}

/// Whether a statement sequence contains a `return` reachable WITHOUT crossing
/// a function/lambda boundary (a "top-level" return). A `function`
/// declaration's body and a lambda's body are their OWN return scope, so the
/// walk does NOT descend into them. `emit_entry_body` uses this to REFUSE a
/// unit with a top-level return — which the interpreter runtime-faults
/// (TPZ "return outside a function") — so the `StmtKind::Return` arm can emit
/// `return Ok(e)` unconditionally (it is then reached only inside a
/// function/lambda body). MUST descend into every emittable construct that can
/// hold a statement sequence (a miss would emit a top-level `return` and
/// diverge) — keep in step with `emit_expr` as the grammar grows.
pub(crate) fn block_has_bare_return(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_bare_return)
        || block.tail.as_deref().is_some_and(expr_has_bare_return)
}

pub(crate) fn block_has_try_expr(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_try_expr)
        || block.tail.as_deref().is_some_and(expr_has_try_expr)
}

pub(crate) fn stmt_has_bare_return(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Return(_) => true,
        // A nested function is its OWN return scope — do not descend.
        StmtKind::Function(_) => false,
        StmtKind::Let { value, .. } | StmtKind::Expr(value) | StmtKind::Const { value, .. } => {
            expr_has_bare_return(value)
        }
        StmtKind::Assign { target, value, .. } => {
            expr_has_bare_return(target) || expr_has_bare_return(value)
        }
        StmtKind::While { cond, body } => expr_has_bare_return(cond) || block_has_bare_return(body),
        StmtKind::Using { value, body, .. } => {
            expr_has_bare_return(value) || block_has_bare_return(body)
        }
        StmtKind::Break { value, .. } => value.as_ref().is_some_and(expr_has_bare_return),
        _ => false,
    }
}

pub(crate) fn stmt_has_try_expr(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Return(value) | StmtKind::Break { value, .. } => {
            value.as_ref().is_some_and(expr_has_try_expr)
        }
        StmtKind::Function(_) => false,
        StmtKind::Defer(value) => expr_has_try_expr(value),
        StmtKind::Let { value, .. } | StmtKind::Expr(value) | StmtKind::Const { value, .. } => {
            expr_has_try_expr(value)
        }
        StmtKind::Assign { target, value, .. } => {
            expr_has_try_expr(target) || expr_has_try_expr(value)
        }
        StmtKind::While { cond, body } => expr_has_try_expr(cond) || block_has_try_expr(body),
        StmtKind::Using { value, body, .. } => expr_has_try_expr(value) || block_has_try_expr(body),
        StmtKind::Export(inner) => stmt_has_try_expr(inner),
        _ => false,
    }
}

pub(crate) fn expr_has_bare_return(expr: &Expr) -> bool {
    match &expr.kind {
        // A lambda is its OWN return scope — do not descend.
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
        ExprKind::Paren(e) | ExprKind::Unary { operand: e, .. } => expr_has_bare_return(e),
        ExprKind::Binary { lhs, rhs, .. } => expr_has_bare_return(lhs) || expr_has_bare_return(rhs),
        ExprKind::Range { lo, hi, step, .. } => {
            expr_has_bare_return(lo)
                || expr_has_bare_return(hi)
                || step.as_deref().is_some_and(expr_has_bare_return)
        }
        ExprKind::Array(els) => els.iter().any(|el| match el {
            ArrayElement::Expr(e) | ArrayElement::Spread(e) => expr_has_bare_return(e),
        }),
        ExprKind::SetLiteral(els) => els.iter().any(expr_has_bare_return),
        ExprKind::MapLiteral(entries) => entries
            .iter()
            .any(|(k, v)| expr_has_bare_return(k) || expr_has_bare_return(v)),
        ExprKind::Comprehension { clauses, body, .. } => {
            clauses.iter().any(|c| match c {
                CompClause::For { iter, .. } => expr_has_bare_return(iter),
                CompClause::If(cond) => expr_has_bare_return(cond),
            }) || match body {
                CompBody::Elem(e) => expr_has_bare_return(e),
                CompBody::Entry { key, value } => {
                    expr_has_bare_return(key) || expr_has_bare_return(value)
                }
            }
        }
        ExprKind::RecordLiteral { fields } => fields.iter().any(|f| expr_has_bare_return(&f.value)),
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            expr_has_bare_return(base)
                || spread.as_ref().is_some_and(|s| expr_has_bare_return(s))
                || fields.iter().any(|f| expr_has_bare_return(&f.value))
        }
        ExprKind::String(lit) => lit
            .parts
            .iter()
            .any(|p| matches!(p, StringPart::Interpolation(e) if expr_has_bare_return(e))),
        ExprKind::Call { callee, args, .. } => {
            expr_has_bare_return(callee)
                || args.iter().any(|a| match a {
                    CallArg::Positional(e) | CallArg::Spread(e) => expr_has_bare_return(e),
                    CallArg::Named { value, .. } => expr_has_bare_return(value),
                })
        }
        ExprKind::Block(b) => block_has_bare_return(b),
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
                || cases.iter().any(|c| {
                    // A §5 `case` GUARD is an emittable expression (a block
                    // guard `{ return 1; true }` can hold a top-level return),
                    // so it must be walked too.
                    c.guard.as_ref().is_some_and(expr_has_bare_return)
                        || match &c.body {
                            CaseArmBody::Return { .. } => true,
                            CaseArmBody::Expr(e) => expr_has_bare_return(e),
                        }
                })
        }
        ExprKind::Member { object, .. } | ExprKind::OptionalAccess { object, .. } => {
            expr_has_bare_return(object)
        }
        ExprKind::Index { object, index } => {
            expr_has_bare_return(object) || expr_has_bare_return(index)
        }
        // §13 a `?` escapes the enclosing FUNCTION boundary on its `Err` branch
        // (the interpreter unwinds a `Return`, like a bare `return`), so a
        // top-level `?` runtime-faults "return outside a function". Treat it as
        // a boundary escape here so `emit_entry_body` refuses a top-level `?`,
        // exactly as it refuses a top-level `return` (the operand cannot host a
        // separate top-level return that this misses — a true result suffices).
        ExprKind::Try(_) => true,
        // §11 `f >> g` evaluates both operands in place, so a bare return in
        // either (a degenerate `(return 1) >> g`) is a top-level escape.
        ExprKind::Compose { lhs, rhs } => expr_has_bare_return(lhs) || expr_has_bare_return(rhs),
        // §11 `lhs |> stage` evaluates the lhs and the stage expr in place.
        ExprKind::Pipe { lhs, rhs } => {
            expr_has_bare_return(lhs) || matches!(rhs, PipeRhs::Expr(s) if expr_has_bare_return(s))
        }
        _ => false,
    }
}

pub(crate) fn expr_has_try_expr(expr: &Expr) -> bool {
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
            }) || match body {
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
            expr_has_try_expr(lhs) || matches!(rhs, PipeRhs::Expr(expr) if expr_has_try_expr(expr))
        }
        _ => false,
    }
}

/// Emit a `Span` literal reconstructing this source span in the
/// generated crate, so an emitted fault carries the SAME span the
/// interpreter would (the differential comparator normalizes both to a
/// root-relative file name + byte offsets).
pub(crate) fn emit_span(span: Span) -> String {
    format!(
        "Span::new(FileId({}), {}, {})",
        span.file.0, span.lo, span.hi
    )
}
