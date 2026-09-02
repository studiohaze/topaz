use crate::*;

pub(crate) fn emit_entry_body(
    program: &Program,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    final_mode: EntryFinal<'_>,
) -> Result<String, EmitError> {
    emit_entry_body_seeded(program, src, aliases, "", &[], final_mode)
}

pub(crate) fn self_runtime_default_seed_lines(aliases: &Aliases<'_, '_>) -> String {
    let Some(module) = aliases.type_ctx.module(aliases.identity) else {
        return String::new();
    };
    let mut cells: Vec<String> = module
        .record_defaults
        .self_runtime_refs
        .values()
        .flat_map(|refs| refs.iter().map(|(_, cell)| cell.clone()))
        .collect();
    cells.extend(
        module
            .record_defaults
            .thunks
            .values()
            .flat_map(|thunks| thunks.iter().map(|thunk| thunk.cell.clone())),
    );
    cells.sort();
    cells.dedup();
    let mut lines = String::new();
    for cell in cells {
        lines.push_str(&format!("    let {cell} = top_cell();\n"));
    }
    lines
}

/// §4 whether `expr` is a CONSTANT expression the interpreter's load-time const
/// pass (`const_eval`) would accept for a TOP-LEVEL `const`: a scalar literal, a
/// parenthesized const, a reference to an EARLIER const, or a unary/binary
/// operation over consts — with the short-circuit `&&`/`||`/`??` excluded, exactly
/// as `const_eval` excludes them. This mirrors `const_eval` arm-for-arm, so it
/// accepts precisely the top-level consts the interpreter does (no valid program is
/// over-refused) and refuses the rest — a member access, call, index, aggregate,
/// constructor, an interpolated/tagged string, or an identifier that is NOT an
/// earlier const (a prelude builtin, or an imported namespace alias whose name
/// happens to match a prelude value). Without this the emitter would fall back to
/// the prelude for such an identifier and lower a member/call the interpreter
/// rejects (TPZ5001), compiling a binary that diverges. (Block-local consts are NOT
/// gated: the interpreter evaluates them normally, not through `const_eval`, so the
/// emitter's ordinary lowering already matches.) `consts` is the const-only scope
/// built so far (the earlier consts).
///
/// This checks the SHAPE, not the value: a constant-shaped initializer that always
/// faults (`1 / 0`, `1 + true`) is accepted and faults at runtime through the SAME
/// shared §2 leaf `const_eval` uses, so BOTH engines fault on it (the interpreter at
/// load time, the binary at the hoisted const line) with the same underlying error.
/// The fault FRAMING differs — the interpreter wraps it as a const-pass error at the
/// const span, the binary surfaces the raw leaf fault — a pre-existing limitation
/// (it predates multi-module, for entry consts); a const-pass-exact fault would need
/// emit-time const folding (deferred). Such an always-faulting const is pathological.
pub(crate) fn const_initializer_ok(expr: &Expr, src: &LoweredText, consts: &ConstValues) -> bool {
    match &expr.kind {
        ExprKind::Int | ExprKind::Float | ExprKind::Bool(_) | ExprKind::Null | ExprKind::Unit => {
            true
        }
        ExprKind::String(s) => {
            s.tag.is_none()
                && !s
                    .parts
                    .iter()
                    .any(|p| matches!(p, StringPart::Interpolation(_)))
        }
        ExprKind::Paren(inner) => const_initializer_ok(inner, src, consts),
        ExprKind::Ident => {
            let name = text(src, expr.span);
            consts.contains_key(name)
        }
        ExprKind::Member { object, field } => {
            let ExprKind::Ident = object.kind else {
                return false;
            };
            let key = namespace_const_key(text(src, object.span), text(src, field.span));
            consts.contains_key(key.as_str())
        }
        ExprKind::Unary { operand, .. } => const_initializer_ok(operand, src, consts),
        ExprKind::Binary { op, lhs, rhs } => {
            !matches!(op, BinaryOp::And | BinaryOp::Or | BinaryOp::Coalesce)
                && const_initializer_ok(lhs, src, consts)
                && const_initializer_ok(rhs, src, consts)
        }
        _ => false,
    }
}

/// §4 emit-time CONST evaluation, mirroring the interpreter's `const_eval` arm-for-arm, so a
/// FAULTING top-level const (`const X = 1 / 0`, `const Y = 1 + "x"`) is detected HERE and the
/// caller can lower it to the interpreter's const-guard fault (`GUARD_TYPE`, "constant
/// expression error: …" at the faulting sub-expression's span) instead of a bare runtime fault
/// (a different code/message). `consts` holds the already-evaluated EARLIER consts (for an
/// `Ident`). Assumes `const_initializer_ok` accepted `expr`.
pub(crate) fn const_eval_emit(
    expr: &Expr,
    src: &LoweredText,
    consts: &ConstValues,
) -> Result<Value, RtError> {
    // The interpreter's const-pass `non_const` message verbatim — a malformed const literal
    // (an oversized int the lexer admits but `i64::parse` rejects) must fault identically.
    let non_const = || {
        fault(
            topaz_value::codes::GUARD_TYPE,
            "`const` initializers must be constant expressions (§4)",
            expr.span,
        )
    };
    match &expr.kind {
        ExprKind::Int => text(src, expr.span)
            .parse::<i64>()
            .map(Value::Int)
            .map_err(|_| non_const()),
        ExprKind::Float => text(src, expr.span)
            .parse::<f64>()
            .map(Value::Float)
            .map_err(|_| non_const()),
        ExprKind::Bool(b) => Ok(Value::Bool(*b)),
        ExprKind::Null => Ok(Value::Null),
        ExprKind::Unit => Ok(Value::Unit),
        ExprKind::String(lit) => {
            let mut decoded = String::new();
            for part in &lit.parts {
                match part {
                    StringPart::Text(span) => decode_escapes(text(src, *span), &mut decoded, *span)
                        .map_err(|_| non_const())?,
                    // `const_initializer_ok` rejects interpolation; defend anyway.
                    StringPart::Interpolation(_) => return Err(non_const()),
                }
            }
            Ok(Value::str(decoded))
        }
        ExprKind::Paren(inner) => const_eval_emit(inner, src, consts),
        ExprKind::Ident => {
            let name = text(src, expr.span);
            consts.get(name).cloned().ok_or_else(non_const)
        }
        ExprKind::Member { object, field } => {
            let ExprKind::Ident = object.kind else {
                return Err(non_const());
            };
            let key = namespace_const_key(text(src, object.span), text(src, field.span));
            consts.get(key.as_str()).cloned().ok_or_else(non_const)
        }
        ExprKind::Unary { op, operand } => {
            let v = const_eval_emit(operand, src, consts)?;
            const_guarded_emit(unary_value(value_unary_op(*op), v, expr.span), expr.span)
        }
        ExprKind::Binary { op, lhs, rhs }
            if !matches!(op, BinaryOp::And | BinaryOp::Or | BinaryOp::Coalesce) =>
        {
            let l = const_eval_emit(lhs, src, consts)?;
            let r = const_eval_emit(rhs, src, consts)?;
            const_guarded_emit(
                binary_value(value_binary_op(*op), l, r, expr.span),
                expr.span,
            )
        }
        _ => Err(non_const()),
    }
}

pub(crate) fn namespace_const_key(namespace: &str, field: &str) -> String {
    format!("{namespace}.{field}")
}

/// Canonical dotted identity of an import target. All generated-Rust import
/// consumers append source-order segments directly into one owned string.
pub(crate) fn render_import_path(import: &ImportItem, src: &LoweredText) -> String {
    let mut path = String::new();
    for segment in &import.path.segments {
        if !path.is_empty() {
            path.push('.');
        }
        path.push_str(text(src, segment.span));
    }
    path
}

pub(crate) fn self_runtime_default_name(identity: &str, name: &str) -> String {
    format!("__topaz_self_default_{}_{}", mangle(identity), mangle(name))
}

pub(crate) fn hidden_self_runtime_default_field(identity: &str, name: &str) -> String {
    format!("\0topaz.self-default.{identity}.{name}")
}

pub(crate) fn collect_selected_default_import_facts<'a>(
    items: &[Stmt],
    src: &LoweredText,
    modules: &std::collections::BTreeMap<String, ModuleTypeCtx<'a>>,
    top_binding_cardinality: &HashMap<&str, usize>,
) -> ModuleDefaultImportFacts {
    let mut selected_const_values = Vec::new();
    let mut selected_runtime_refs = Vec::new();
    let mut const_local_names = HashSet::new();
    let mut runtime_local_names = HashSet::new();
    for item in items {
        let StmtKind::Import(imp) = &item.kind else {
            continue;
        };
        let ImportKind::Selected { specs } = &imp.kind else {
            continue;
        };
        let identity = render_import_path(imp, src);
        let Some(target) = modules.get(&identity) else {
            continue;
        };
        for spec in specs {
            let imported = text(src, spec.name.span);
            let local = spec
                .alias
                .as_ref()
                .map(|id| text(src, id.span))
                .unwrap_or(imported);
            if !const_local_names.contains(local)
                && let Some((_, value)) = target
                    .runtime_values
                    .exported_const_values
                    .iter()
                    .rev()
                    .find(|(name, _)| name.as_str() == imported)
            {
                const_local_names.insert(local);
                selected_const_values.push((local.to_string(), value.clone()));
            }
            if top_binding_cardinality.get(local) == Some(&1)
                && target.runtime_values.export_names.contains(imported)
                && runtime_local_names.insert(local)
            {
                selected_runtime_refs.push((
                    local.to_string(),
                    identity.clone(),
                    imported.to_string(),
                ));
            }
        }
    }
    ModuleDefaultImportFacts {
        selected_const_values,
        selected_runtime_refs,
    }
}

pub(crate) fn collect_self_runtime_default_refs_from_expr<'a>(
    expr: &Expr,
    src: &'a LoweredText,
    identity: &str,
    available: &HashSet<&'a str>,
    seen: &mut HashSet<&'a str>,
    refs: &mut Vec<(String, String)>,
) {
    match &expr.kind {
        ExprKind::Ident => {
            let local = text(src, expr.span);
            if available.contains(local) && seen.insert(local) {
                refs.push((
                    local.to_string(),
                    self_runtime_default_name(identity, local),
                ));
            }
        }
        ExprKind::Paren(inner) => {
            collect_self_runtime_default_refs_from_expr(
                inner, src, identity, available, seen, refs,
            );
        }
        _ => {}
    }
}

pub(crate) fn collect_namespace_default_import_facts<'a>(
    modules: &std::collections::BTreeMap<String, ModuleTypeCtx<'a>>,
    namespaces: &std::collections::BTreeMap<String, String>,
) -> ModuleNamespaceDefaultImportFacts {
    let mut const_values = Vec::new();
    let mut runtime_refs = Vec::new();
    for (local, identity) in namespaces {
        // LoweredUnit is sorted by ADR-078 dependency post-order before emit.
        // Therefore a real imported namespace dependency is already present here.
        // If that invariant ever changes, missing entries simply leave the default
        // unsupported and preserve the existing loud-decline fallback.
        let Some(target) = modules.get(identity) else {
            continue;
        };
        for (exported, value) in &target.runtime_values.exported_const_values {
            const_values.push((namespace_const_key(local, exported), value.clone()));
        }
        let mut exported = target
            .runtime_values
            .export_names
            .iter()
            .copied()
            .collect::<Vec<_>>();
        exported.sort_unstable();
        for name in exported {
            runtime_refs.push((
                namespace_const_key(local, name),
                identity.clone(),
                name.to_string(),
            ));
        }
    }
    ModuleNamespaceDefaultImportFacts {
        const_values,
        runtime_refs,
    }
}

pub(crate) fn collect_namespace_private_runtime_refs_from_expr<'a>(
    expr: &Expr,
    src: &LoweredText,
    modules: &std::collections::BTreeMap<String, ModuleTypeCtx<'a>>,
    namespaces: &std::collections::BTreeMap<String, String>,
    refs: &mut Vec<(String, String, String)>,
    by_target: &mut std::collections::BTreeMap<String, Vec<(String, String)>>,
) {
    match &expr.kind {
        ExprKind::Member { object, field } => {
            let ExprKind::Ident = object.kind else {
                collect_namespace_private_runtime_refs_from_expr(
                    object, src, modules, namespaces, refs, by_target,
                );
                return;
            };
            let namespace = text(src, object.span);
            let source_name = text(src, field.span);
            let Some(target_identity) = namespaces.get(namespace) else {
                return;
            };
            let Some(target) = modules.get(target_identity) else {
                return;
            };
            if !target
                .runtime_values
                .immutable_let_names
                .contains(source_name)
                || target.runtime_values.export_names.contains(source_name)
            {
                return;
            }
            let key = namespace_const_key(namespace, source_name);
            let hidden_field = hidden_self_runtime_default_field(target_identity, source_name);
            refs.push((key, target_identity.clone(), hidden_field.clone()));
            by_target
                .entry(target_identity.clone())
                .or_default()
                .push((source_name.to_string(), hidden_field));
        }
        ExprKind::Paren(inner) => collect_namespace_private_runtime_refs_from_expr(
            inner, src, modules, namespaces, refs, by_target,
        ),
        _ => {}
    }
}

/// The interpreter's `const_guarded`: a FAULT from a const-expression operation is re-framed as
/// a `GUARD_TYPE` "constant expression error: …" at the const-expression span.
pub(crate) fn const_guarded_emit(
    result: Result<Value, RtError>,
    span: Span,
) -> Result<Value, RtError> {
    result.map_err(|e| {
        if e.is_fault() {
            fault(
                topaz_value::codes::GUARD_TYPE,
                format!("constant expression error: {}", e.message),
                span,
            )
        } else {
            e
        }
    })
}

/// Lower a statement sequence (the program body, or a block body) to
/// `(statement lines, tail-value expression)`. `locals` carries the
/// bindings visible here: on entry it holds exactly the enclosing
/// scope's bindings, and this routine APPENDS its own `let`s. A binding
/// declared in THIS sequence (index `>= base`) may not be redeclared,
/// but it may shadow an enclosing one — so a nested block is lowered on
/// a COPY (see [`emit_block`]) and the appended locals are discarded
/// with it.
pub(crate) fn emit_record_default_thunk_initializers(
    decl: &RecordDecl,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    locals: &[(String, Bind)],
) -> Result<String, EmitError> {
    let record_name = text(src, decl.name.span);
    let Some(module) = aliases.type_ctx.module(aliases.identity) else {
        return Ok(String::new());
    };
    let Some(thunks) = module.record_defaults.thunks.get(record_name) else {
        return Ok(String::new());
    };
    let mut lines = String::new();
    for thunk in thunks {
        let Some(default) = decl.fields.iter().find_map(|field| {
            (text(src, field.name.span) == thunk.field)
                .then_some(field.default.as_ref())
                .flatten()
        }) else {
            continue;
        };
        let captures = lambda_captures(default, &[], locals, src)?;
        let mut body_locals = Vec::with_capacity(captures.len());
        push_capture_locals(&captures, locals, &mut body_locals).map_err(|e| e.at(default.span))?;
        let body_aliases = aliases.with_body(&[], false);
        let body_rs = with_reset_flow(&body_aliases, |aliases| {
            emit_expr(default, src, aliases, &body_locals, false)
        })?;
        let closure = emit_closure_value(ClosureEmission {
            param_names: &[],
            captures: &captures,
            defaults: &[],
            variadic: None,
            variadic_guard: None,
            param_guards: "",
            body: &body_rs,
            return_guard: None,
            has_defers: false,
        });
        lines.push_str(&format!("    top_cell_set(&{}, {closure});\n", thunk.cell));
    }
    Ok(lines)
}

pub(crate) fn imported_nominal_record_default_is_self_contained(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Int | ExprKind::Float | ExprKind::Bool(_) | ExprKind::Null | ExprKind::Unit => {
            true
        }
        ExprKind::String(lit) if lit.tag.is_none() => lit
            .parts
            .iter()
            .all(|part| matches!(part, StringPart::Text(_))),
        ExprKind::Paren(inner) => imported_nominal_record_default_is_self_contained(inner),
        ExprKind::Unary { operand, .. } => {
            imported_nominal_record_default_is_self_contained(operand)
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            imported_nominal_record_default_is_self_contained(lhs)
                && imported_nominal_record_default_is_self_contained(rhs)
        }
        ExprKind::Array(elements) => elements.iter().all(|element| match element {
            ArrayElement::Expr(expr) => imported_nominal_record_default_is_self_contained(expr),
            ArrayElement::Spread(_) => false,
        }),
        ExprKind::SetLiteral(elements) => elements
            .iter()
            .all(imported_nominal_record_default_is_self_contained),
        ExprKind::MapLiteral(entries) => entries.iter().all(|(key, value)| {
            imported_nominal_record_default_is_self_contained(key)
                && imported_nominal_record_default_is_self_contained(value)
        }),
        ExprKind::Range { lo, hi, step, .. } => {
            imported_nominal_record_default_is_self_contained(lo)
                && imported_nominal_record_default_is_self_contained(hi)
                && step
                    .as_deref()
                    .is_none_or(imported_nominal_record_default_is_self_contained)
        }
        _ => false,
    }
}

pub(crate) fn emit_nominal_record_reference_default(
    expr: &Expr,
    src: &LoweredText,
    const_values: &ConstValues,
    runtime_refs: &[(String, String, String)],
    self_runtime_refs: &[(String, String)],
    hidden_runtime_refs: &[(String, String, String)],
) -> Result<String, EmitError> {
    if const_initializer_ok(expr, src, const_values) {
        let value = const_eval_emit(expr, src, const_values).map_err(|_| {
            EmitError::unsupported("imported nominal record reference default").at(expr.span)
        })?;
        return match value {
            Value::Int(_)
            | Value::Float(_)
            | Value::Bool(_)
            | Value::Null
            | Value::Unit
            | Value::Str(_) => Ok(render_value_rust(&value)),
            _ => Err(
                EmitError::unsupported("imported nominal record reference default").at(expr.span),
            ),
        };
    }
    emit_nominal_record_runtime_reference_default(
        expr,
        src,
        runtime_refs,
        self_runtime_refs,
        hidden_runtime_refs,
    )
    .ok_or_else(|| {
        EmitError::unsupported("imported nominal record reference default").at(expr.span)
    })
}

pub(crate) fn emit_nominal_record_runtime_reference_default(
    expr: &Expr,
    src: &LoweredText,
    runtime_refs: &[(String, String, String)],
    self_runtime_refs: &[(String, String)],
    hidden_runtime_refs: &[(String, String, String)],
) -> Option<String> {
    if let Some(root) = emit_nominal_record_runtime_reference_root(
        expr,
        src,
        runtime_refs,
        self_runtime_refs,
        hidden_runtime_refs,
    ) {
        return Some(root);
    }
    match &expr.kind {
        ExprKind::Member { object, field } => {
            let object_rs = emit_nominal_record_runtime_reference_default(
                object,
                src,
                runtime_refs,
                self_runtime_refs,
                hidden_runtime_refs,
            )?;
            Some(format!(
                "member_value_required(&({object_rs}), {:?}, {})?",
                text(src, field.span),
                emit_span(expr.span)
            ))
        }
        ExprKind::Paren(inner) => emit_nominal_record_runtime_reference_default(
            inner,
            src,
            runtime_refs,
            self_runtime_refs,
            hidden_runtime_refs,
        )
        .map(|inner| format!("({inner})")),
        _ => None,
    }
}
