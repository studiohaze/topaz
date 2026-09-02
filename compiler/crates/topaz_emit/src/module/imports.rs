use crate::*;

pub(crate) fn emit_extern_namespace_module(
    identity: &str,
    program: &Program,
    src: &LoweredText,
) -> Result<(String, HashSet<String>), EmitError> {
    let mut exports = HashSet::new();
    let mut fields = Vec::new();
    for item in &program.items {
        let StmtKind::Export(inner) = &item.kind else {
            continue;
        };
        let StmtKind::Function(decl) = &inner.kind else {
            continue;
        };
        let name = text(src, decl.name.span).to_string();
        let params = decl
            .params
            .iter()
            .map(|p| format!("{:?}", text(src, p.name.span)))
            .collect::<Vec<_>>()
            .join(", ");
        fields.push(format!(
            "({name:?}.to_string(), Value::Closure(Rc::new(ExternFunction::new({identity:?}, {name:?}, &[{params}], {span}))))",
            span = emit_span(decl.name.span),
        ));
        exports.insert(name);
    }
    Ok((format!("Value::record([{}])", fields.join(", ")), exports))
}

/// §17 lower a NON-ENTRY module to a `Value::record` of its EXPORTS — the namespace
/// value a `import m` binds. The module's items lower into a fresh block scope
/// (`export`s unwrapped; module-local items kept so an export may reference them —
/// an intra-module reference therefore resolves, while a reference to a name the
/// emitter cannot see refuses as a free identifier), and the block's value is a
/// record mapping each exported name to its lowered binding. A transitive `import`
/// inside the module resolves through the canonical per-module records (namespace
/// and selected forms; see the `entry_import_plan` use below).
pub(crate) fn emit_namespace_module(
    program: &Program,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    module_export_surfaces: &std::collections::BTreeMap<String, ModuleExportSurface>,
    module_built_exports: &std::collections::BTreeMap<String, BuiltRuntimeExportSurface<'_>>,
) -> Result<String, EmitError> {
    // Same span GUARANTEE as the entry body: an unlocated coverage gap from a
    // non-entry module falls back to that module's program span (first-wins keeps
    // a tighter inner span).
    emit_namespace_module_inner(
        program,
        src,
        aliases,
        module_export_surfaces,
        module_built_exports,
    )
    .map_err(|e| e.at(program.span))
}

pub(crate) fn emit_namespace_module_inner(
    program: &Program,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    module_export_surfaces: &std::collections::BTreeMap<String, ModuleExportSurface>,
    module_built_exports: &std::collections::BTreeMap<String, BuiltRuntimeExportSurface<'_>>,
) -> Result<String, EmitError> {
    let own_export_surface = module_export_surfaces
        .get(aliases.identity)
        .ok_or_else(|| EmitError::unsupported("missing module export surface").at(program.span))?;
    // §4 the interpreter runs `const_pass` for EVERY module, evaluating ALL of its
    // top-level consts BEFORE any of the module's runtime statements (and before its
    // functions are bound). Mirror that exactly: HOIST the module's top-level consts —
    // each validated against the const-expression allow-list (`const_initializer_ok`,
    // in the textual order `const_pass` binds them) — as `let` lines emitted BEFORE the
    // rest of the module lowers. This keeps a faulting const faulting before any module
    // effect runs (as the interpreter does), and lets a function reference a const
    // declared textually later (the consts are all in scope for the rest). Exports are
    // unwrapped; module-local items are kept so an export may reference them; a
    // transitive `import` inside the module resolves (see the §17 note just below).
    // §17 a module's OWN imports (TRANSITIVE): the NAMESPACE form (`import n`, then `n.foo`)
    // binds a reference to the canonically-named record `emit_items` emitted for that module
    // earlier, in dependency order (`__mod_<identity>` — one record, shared, so a diamond
    // builds it once). A module's SELECTED import (`import n { foo }`) binds each chosen
    // export to a local off that canonical record, exactly as the entry's selected import.
    // Imports are in scope for the module's FUNCTIONS only — a const cannot reference an
    // import (the interpreter's const pass faults TPZ5001), so the consts never need them.
    // §7 a non-entry module's top-level function that shadows a prelude name has the
    // same dynamic-resolution divergence as the entry's — refuse it here too.
    refuse_prelude_named_top_functions(&program.items, src)?;
    let import_plan = entry_import_plan(program, src)?;
    let mut import_prelude = String::new();
    let mut import_locals: Vec<(String, Bind)> = Vec::new();
    for (identity, how) in &import_plan {
        match how {
            ImportPlan::Namespace(alias) => {
                import_prelude.push_str(&format!(
                    "    let {} = {}.clone();\n",
                    mangle(alias),
                    canonical_module(identity),
                ));
                import_locals.push((alias.clone(), Bind::Namespace));
            }
            ImportPlan::Selected { binds, span } => {
                // The target module is built EARLIER (dependency order), so its exports are
                // known. The resolver rejects a non-exported selected name (TPZ3009), but
                // defend against an absent field rather than emit a read that would fault.
                let export_surface = module_export_surfaces.get(identity).ok_or_else(|| {
                    EmitError::unsupported("selected import of a module built after the importer")
                        .at(*span)
                })?;
                let built_exports = module_built_exports.get(identity).ok_or_else(|| {
                    EmitError::unsupported("selected import of a module built after the importer")
                        .at(*span)
                })?;
                for (name, local) in binds {
                    if !selected_import_binds_runtime(export_surface, built_exports, name, *span)? {
                        continue;
                    }
                    import_prelude.push_str(&format!(
                        "    let {} = member_value_required(&{}, {name:?}, {})?;\n",
                        mangle(local),
                        canonical_module(identity),
                        emit_span(*span),
                    ));
                    import_locals.push((local.clone(), Bind::Imm));
                }
            }
        }
    }
    let exports = &own_export_surface.runtime_order;
    let mut const_lines = String::new();
    let mut consts: Vec<(String, Bind)> = Vec::new();
    // The folded const VALUES, for the emit-time const evaluator (Ident lookups + fault
    // detection); parallel to `consts` (which tracks the binding kind).
    let mut const_values = ConstValues::new();
    let mut rest: Vec<Stmt> = Vec::new();
    for item in &program.items {
        let inner: &Stmt = match &item.kind {
            StmtKind::Export(inner) => {
                // §3/§6 a `type` alias or nominal declaration erases at runtime:
                // it is collected for type checking, declares no env value (the
                // interpreter records the export NAME but binds nothing), and emits
                // no statement. So it contributes NO runtime export-record field —
                // skip it rather than refuse the module's lowering. (A module whose
                // ONLY exports are types therefore has empty runtime `exports` and
                // lowers to an empty `Value::record([])` below — a valid type-only
                // namespace; the resolver already gated a truly export-less import.)
                if matches!(
                    &inner.kind,
                    StmtKind::TypeAlias(_) | StmtKind::Enum(_) | StmtKind::Newtype(_)
                ) {
                    continue;
                }
                if matches!(&inner.kind, StmtKind::Record(_)) {
                    rest.push((**inner).clone());
                    continue;
                }
                inner
            }
            // Handled above — a module import is not a record field.
            StmtKind::Import(_) => continue,
            _ => item,
        };
        if let StmtKind::Const { name, value, .. } = &inner.kind {
            let var = text(src, name.span);
            // A module const colliding with an import alias is a same-scope redeclaration the
            // interpreter faults (GUARD_REDECLARE) — mirror the entry's const-vs-seed check
            // (the imports were bound above), not just const-vs-const.
            if const_values.contains_key(var) || import_locals.iter().any(|(n, _)| n == var) {
                return Err(EmitError::unsupported("same-scope redeclaration").at(inner.span));
            }
            if !const_initializer_ok(value, src, &const_values) {
                // A non-constant module const faults the interpreter's const pass
                // (TPZ5001); refuse rather than lower it as an ordinary binding.
                return Err(EmitError::unsupported("non-constant const initializer").at(inner.span));
            }
            match const_eval_emit(value, src, &const_values) {
                Ok(v) => {
                    let value_rs = emit_expr(value, src, aliases, &consts, false)?;
                    const_lines.push_str(&format!("    let {} = {value_rs};\n", mangle(var)));
                    const_values.insert(var.to_string(), v);
                }
                Err(e) => {
                    // The interpreter's const pass faults HERE; emit its const-guard fault
                    // (code+message+span) so the binary faults identically, not with a bare
                    // runtime fault. The enclosing module-record `async { Ok(...) }` wrapper
                    // (in `emit_items`) now appends the SAME module-init import-chain suffix
                    // the interpreter's `run_unit` does, so this fault matches run vs build.
                    // `let … = return …` keeps the export binding defined (type `!` coerces)
                    // for the now-dead record build.
                    const_lines.push_str(&format!(
                        "    let {}: Value = return Err(fault(codes::GUARD_TYPE, {:?}, {}));\n",
                        mangle(var),
                        e.message,
                        emit_span(e.span),
                    ));
                    const_values.insert(var.to_string(), Value::Unit);
                }
            }
            consts.push((var.to_string(), Bind::Imm));
        } else {
            rest.push(inner.clone());
        }
    }
    // §17 a module with NO RUNTIME exports but a TYPE export (`export type …`,
    // which erases) is a legitimate type-only namespace — the resolver accepted it
    // (it exports a name), the interpreter binds an empty namespace, so the emitter
    // builds an empty `Value::record([])` below (run≡build) rather than refusing.
    // A TRULY export-less / side-effect-only module never reaches here: the resolver
    // rejects its import first with TPZ3010 ("no side-effect-only imports"), the
    // single enforcement point for that v5.2 rule.
    // §7/§13 a TOP-LEVEL `return` or `?` (outside any function/lambda) in a NON-ENTRY
    // module is refused, exactly as `emit_entry_body_seeded_inner` refuses it in the
    // entry — the interpreter runtime-faults it ("return outside a function"). Without
    // this, `emit_stmt_seq`'s `Return` arm / `Try`'s `return Ok(__early)` would be
    // CAUGHT by the module-record's `async { Ok(...) }` wrapper and bind a bogus
    // module value instead of faulting, diverging run vs build. (A nested function /
    // lambda body is its own return scope — `stmt_has_bare_return` does not descend.)
    if let Some(stmt) = rest.iter().find(|s| stmt_has_bare_return(s)) {
        return Err(EmitError::unsupported("return outside a function").at(stmt.span));
    }
    // The hoisted consts + the imports + the rest are ONE module-top scope (`base = 0`, so a
    // runtime binding reusing an import/const name is a redeclaration — the resolver's
    // TPZ3008). The body (functions) sees the imports AND the consts; a const saw only
    // earlier consts (it cannot reference an import).
    let mut locals: Vec<(String, Bind)> = import_locals;
    locals.extend(consts);
    // §7 seed a forward-reference cell for each top-level function (parallel to the
    // entry, `emit_entry_body_seeded_inner`), so a non-entry module's body can name
    // a function declared later — even across a non-function statement. A
    // prelude-named function was already refused above; a duplicate is refused here.
    let mut top_fn_seed = String::new();
    for stmt in &rest {
        if let StmtKind::Function(decl) = &stmt.kind {
            let fname = text(src, decl.name.span);
            if locals
                .iter()
                .any(|(n, b)| n == fname && matches!(b, Bind::TopFnCell))
            {
                return Err(EmitError::unsupported("same-scope redeclaration").at(decl.name.span));
            }
            if locals.iter().any(|(n, _)| n == fname) {
                continue;
            }
            top_fn_seed.push_str(&format!("    let {} = top_cell();\n", mangle(fname)));
            locals.push((fname.to_string(), Bind::TopFnCell));
        }
    }
    let top_value_seed = seed_top_runtime_value_cells(&rest, src, &mut locals)?;
    let mut method_seed = String::new();
    let mut method_ids: Vec<&String> = aliases.methods.keys().collect();
    method_ids.sort_unstable();
    for type_id in method_ids {
        for method in &aliases.methods[type_id] {
            let method_name = text(src, method.decl.name.span);
            let closure = emit_method_closure(&method.decl, src, aliases, &locals)?;
            method_seed.push_str(&emitted_method_registration(
                aliases.runtime_identity(),
                type_id,
                method_name,
                &closure,
            ));
        }
    }
    let (lines, _) = emit_stmt_seq(StatementSequenceEmission {
        stmts: &rest,
        tail: None,
        src,
        aliases,
        locals: &mut locals,
        base: 0,
        in_loop: false,
        defer_scope: false,
        at_module_top: true,
    })?;
    // §7 an exported TOP-LEVEL function lives in its `TopFnCell` (filled by now, the
    // record is built after every declaration ran), so read its VALUE out of the
    // cell; any other export is a plain binding cloned directly.
    let mut fields = runtime_export_fields(exports, &locals);
    fields.extend(hidden_self_runtime_default_fields(
        aliases,
        &own_export_surface.runtime,
    ));
    fields.extend(hidden_record_default_thunk_fields(aliases));
    // const pass first (the interpreter's order), then the imports, then the
    // top-function cells, then the functions that may reference all of them.
    let self_default_seed = self_runtime_default_seed_lines(aliases);
    let record = format!(
        "{{ {const_lines}{import_prelude}{self_default_seed}{top_fn_seed}{top_value_seed}{method_seed}{lines}Value::record([{}]) }}",
        fields.join(", ")
    );
    Ok(record)
}

/// §17 the canonical local name a multi-module unit binds an imported module's record under
/// (`__mod_<identity>`), so a TRANSITIVE importer references the SAME single record the
/// enclosing scope built once, in dependency order — a diamond shares one record.
pub(crate) fn canonical_module(identity: &str) -> String {
    format!("__mod_{}", mangle(identity))
}

/// §17 the entry's imports as a map from each imported module's DOTTED-PATH identity
/// (`segments.join(".")`, e.g. `utils.strings`) to how the entry binds it. The resolver
/// rejects a duplicate import of the same module, so a module appears at most once here.
pub(crate) fn entry_import_plan(
    program: &Program,
    src: &LoweredText,
) -> Result<std::collections::BTreeMap<String, ImportPlan>, EmitError> {
    let mut map = std::collections::BTreeMap::new();
    for item in &program.items {
        if let StmtKind::Import(imp) = &item.kind {
            // The module IDENTITY is the dotted path — the resolver's
            // `segments.join(".")` and the interpreter's import target — so a
            // multi-segment `import utils.strings` keys by "utils.strings". The
            // namespace alias FALLBACK is the LAST segment, matching the resolver's
            // `segments.last()` (`import utils.strings` binds `strings`).
            let identity = render_import_path(imp, src);
            let last = text(src, imp.path.segments.last().expect("non-empty path").span);
            let plan = match &imp.kind {
                ImportKind::Namespace { alias } => {
                    let a = alias.as_ref().map(|id| text(src, id.span)).unwrap_or(last);
                    ImportPlan::Namespace(a.to_string())
                }
                ImportKind::Selected { specs } => {
                    let mut binds = Vec::with_capacity(specs.len());
                    for spec in specs {
                        let name = text(src, spec.name.span).to_string();
                        let local = match &spec.alias {
                            Some(id) => text(src, id.span).to_string(),
                            None => name.clone(),
                        };
                        binds.push((name, local));
                    }
                    ImportPlan::Selected {
                        binds,
                        span: item.span,
                    }
                }
            };
            map.insert(identity, plan);
        }
    }
    Ok(map)
}
