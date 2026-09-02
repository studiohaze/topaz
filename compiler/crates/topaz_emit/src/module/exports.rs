use crate::*;

/// The bound name of an `export`ed declaration (§17): a function / const / a
/// simple `let`. A destructuring `let` export is refused; exported type-only
/// declarations never reach here — the caller skips them (they erase, no runtime
/// field).
pub(crate) fn export_name<'a>(stmt: &'a Stmt, src: &'a LoweredText) -> Result<&'a str, EmitError> {
    // Locate any export-shape refusal at the offending statement (first-wins, so a
    // tighter inner span — e.g. from `binding_name` — is preserved).
    export_name_inner(stmt, src).map_err(|e| e.at(stmt.span))
}

pub(crate) fn export_name_inner<'a>(
    stmt: &'a Stmt,
    src: &'a LoweredText,
) -> Result<&'a str, EmitError> {
    match &stmt.kind {
        StmtKind::Function(decl) => Ok(text(src, decl.name.span)),
        StmtKind::Const { name, .. } => Ok(text(src, name.span)),
        // A typed immutable `let` is still one simple runtime binding. Its
        // conformance guard belongs to `emit_let_statement`; export inventory
        // only needs the bound name and must not route it through the helper
        // that deliberately rejects typed patterns in unguarded contexts.
        StmtKind::Let { pattern, .. } => {
            single_binding_pattern_name(pattern, src).ok_or(EmitError::unsupported("export shape"))
        }
        _ => Err(EmitError::unsupported("export shape")),
    }
}

impl BuiltRuntimeExportSurface<'_> {
    pub(crate) fn contains(&self, name: &str) -> bool {
        match self {
            Self::Module(exports) => exports.contains(name),
            Self::Extern(exports) => exports.contains(name),
        }
    }
}

pub(crate) fn export_surface_names(
    program: &Program,
    src: &LoweredText,
) -> Result<ModuleExportSurface, EmitError> {
    let mut all = HashSet::new();
    let mut runtime = HashSet::new();
    let mut runtime_order = Vec::new();
    for item in &program.items {
        let StmtKind::Export(inner) = &item.kind else {
            continue;
        };
        match &inner.kind {
            StmtKind::TypeAlias(alias) => {
                all.insert(text(src, alias.name.span).to_string());
            }
            StmtKind::Record(decl) => {
                all.insert(text(src, decl.name.span).to_string());
            }
            StmtKind::Enum(decl) => {
                all.insert(text(src, decl.name.span).to_string());
            }
            StmtKind::Newtype(decl) => {
                all.insert(text(src, decl.name.span).to_string());
            }
            _ => {
                let name = export_name(inner, src)?.to_string();
                all.insert(name.clone());
                runtime.insert(name.clone());
                runtime_order.push(name);
            }
        }
    }
    Ok(ModuleExportSurface {
        all,
        runtime,
        runtime_order,
    })
}

pub(crate) fn selected_import_binds_runtime(
    surface: &ModuleExportSurface,
    built: &BuiltRuntimeExportSurface<'_>,
    name: &str,
    span: Span,
) -> Result<bool, EmitError> {
    if !surface.all.contains(name) {
        return Err(EmitError::unsupported("selected import of a non-exported name").at(span));
    }
    if !surface.runtime.contains(name) {
        return Ok(false);
    }
    if !built.contains(name) {
        return Err(
            EmitError::unsupported("selected import of an unavailable runtime export").at(span),
        );
    }
    Ok(true)
}

pub(crate) fn runtime_export_fields(exports: &[String], locals: &[(String, Bind)]) -> Vec<String> {
    let top_cell_names = locals
        .iter()
        .filter_map(|(name, bind)| {
            matches!(
                bind,
                Bind::TopFnCell | Bind::TopValueCell | Bind::TopMutValueCell
            )
            .then_some(name.as_str())
        })
        .collect::<HashSet<_>>();
    exports
        .iter()
        .map(|n| {
            if top_cell_names.contains(n.as_str()) {
                format!(
                    "({n:?}.to_string(), top_cell_value(&{}, {n:?})?)",
                    mangle(n)
                )
            } else {
                format!("({n:?}.to_string(), {}.clone())", mangle(n))
            }
        })
        .collect()
}

pub(crate) fn hidden_self_runtime_default_fields(
    aliases: &Aliases<'_, '_>,
    runtime_exports: &HashSet<String>,
) -> Vec<String> {
    let Some(module) = aliases.type_ctx.module(aliases.identity) else {
        return Vec::new();
    };
    let mut refs: Vec<(String, String)> = module
        .record_defaults
        .self_runtime_refs
        .values()
        .flat_map(|record_refs| {
            record_refs
                .iter()
                .filter(|(source_name, _)| !runtime_exports.contains(source_name))
                .map(|(source_name, _)| {
                    (
                        source_name.clone(),
                        hidden_self_runtime_default_field(aliases.identity, source_name),
                    )
                })
        })
        .collect();
    refs.extend(
        module
            .record_defaults
            .external_hidden_runtime_refs
            .iter()
            .cloned(),
    );
    refs.sort();
    refs.dedup();
    refs.into_iter()
        .map(|(source_name, field)| {
            format!("({field:?}.to_string(), {}.clone())", mangle(&source_name))
        })
        .collect()
}

pub(crate) fn hidden_record_default_thunk_fields(aliases: &Aliases<'_, '_>) -> Vec<String> {
    let Some(module) = aliases.type_ctx.module(aliases.identity) else {
        return Vec::new();
    };
    let mut thunks = module
        .record_defaults
        .thunks
        .values()
        .flat_map(|items| items.iter())
        .collect::<Vec<_>>();
    thunks.sort_by(|left, right| left.hidden_field.cmp(&right.hidden_field));
    thunks
        .into_iter()
        .map(|thunk| {
            format!(
                "({:?}.to_string(), top_cell_get(&{}, {:?}, {})?)",
                thunk.hidden_field,
                thunk.cell,
                thunk.label,
                emit_span(thunk.span),
            )
        })
        .collect()
}
