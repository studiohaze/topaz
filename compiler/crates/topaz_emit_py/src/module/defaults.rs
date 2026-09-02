use crate::*;

pub(super) fn imported_nominal_record_default_is_self_contained(expr: &Expr) -> bool {
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

pub(super) fn nominal_record_default_requires_execution_thunk(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Block(_)
        | ExprKind::If { .. }
        | ExprKind::Match { .. }
        | ExprKind::For { .. }
        | ExprKind::Loop { .. }
        | ExprKind::Concurrent { .. }
        | ExprKind::Call { .. }
        | ExprKind::Index { .. }
        | ExprKind::Try(_)
        | ExprKind::Pipe { .. }
        | ExprKind::Lambda { .. }
        | ExprKind::RecordLiteral { .. }
        | ExprKind::RecordUpdate { .. }
        | ExprKind::Comprehension { .. } => true,
        ExprKind::Paren(inner)
        | ExprKind::Unary { operand: inner, .. }
        | ExprKind::Member { object: inner, .. }
        | ExprKind::OptionalAccess { object: inner, .. } => {
            nominal_record_default_requires_execution_thunk(inner)
        }
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Compose { lhs, rhs } => {
            nominal_record_default_requires_execution_thunk(lhs)
                || nominal_record_default_requires_execution_thunk(rhs)
        }
        ExprKind::Range { lo, hi, step, .. } => {
            nominal_record_default_requires_execution_thunk(lo)
                || nominal_record_default_requires_execution_thunk(hi)
                || step
                    .as_deref()
                    .is_some_and(nominal_record_default_requires_execution_thunk)
        }
        ExprKind::Array(elements) => elements.iter().any(|element| match element {
            ArrayElement::Expr(expr) | ArrayElement::Spread(expr) => {
                nominal_record_default_requires_execution_thunk(expr)
            }
        }),
        ExprKind::SetLiteral(elements) => elements
            .iter()
            .any(nominal_record_default_requires_execution_thunk),
        ExprKind::MapLiteral(entries) => entries.iter().any(|(key, value)| {
            nominal_record_default_requires_execution_thunk(key)
                || nominal_record_default_requires_execution_thunk(value)
        }),
        ExprKind::String(lit) => lit.parts.iter().any(|part| {
            matches!(part, StringPart::Interpolation(expr)
                if nominal_record_default_requires_execution_thunk(expr))
        }),
        ExprKind::Int
        | ExprKind::Float
        | ExprKind::Duration(_)
        | ExprKind::Bool(_)
        | ExprKind::Null
        | ExprKind::Unit
        | ExprKind::Ident
        | ExprKind::Placeholder => false,
    }
}

pub(super) fn const_initializer_ok_for_defaults(
    expr: &Expr,
    map: &SourceMap,
    consts: &[(String, Value)],
) -> bool {
    match &expr.kind {
        ExprKind::Int | ExprKind::Float | ExprKind::Bool(_) | ExprKind::Null | ExprKind::Unit => {
            true
        }
        ExprKind::String(lit) => {
            lit.tag.is_none()
                && lit
                    .parts
                    .iter()
                    .all(|part| matches!(part, StringPart::Text(_)))
        }
        ExprKind::Paren(inner) => const_initializer_ok_for_defaults(inner, map, consts),
        ExprKind::Ident => {
            let name = text_in_map(map, expr.span);
            consts.iter().any(|(candidate, _)| candidate == name)
        }
        ExprKind::Member { object, field } => {
            let ExprKind::Ident = object.kind else {
                return false;
            };
            let key =
                namespace_const_key(text_in_map(map, object.span), text_in_map(map, field.span));
            consts.iter().any(|(candidate, _)| candidate == &key)
        }
        ExprKind::Unary { operand, .. } => const_initializer_ok_for_defaults(operand, map, consts),
        ExprKind::Binary { op, lhs, rhs }
            if !matches!(op, BinaryOp::And | BinaryOp::Or | BinaryOp::Coalesce) =>
        {
            const_initializer_ok_for_defaults(lhs, map, consts)
                && const_initializer_ok_for_defaults(rhs, map, consts)
        }
        _ => false,
    }
}

pub(super) fn eval_nominal_record_default_const_value(
    expr: &Expr,
    map: &SourceMap,
    consts: &[(String, Value)],
) -> Result<Value, ()> {
    match &expr.kind {
        ExprKind::Int => text_in_map(map, expr.span)
            .parse::<i64>()
            .map(Value::Int)
            .map_err(|_| ()),
        ExprKind::Float => text_in_map(map, expr.span)
            .parse::<f64>()
            .map(Value::Float)
            .map_err(|_| ()),
        ExprKind::Bool(value) => Ok(Value::Bool(*value)),
        ExprKind::Null => Ok(Value::Null),
        ExprKind::Unit => Ok(Value::Unit),
        ExprKind::String(lit) => {
            if lit.tag.is_some() {
                return Err(());
            }
            let mut decoded = String::new();
            for part in &lit.parts {
                match part {
                    StringPart::Text(span) => {
                        decode_escapes(text_in_map(map, *span), &mut decoded, *span)
                            .map_err(|_| ())?;
                    }
                    StringPart::Interpolation(_) => return Err(()),
                }
            }
            Ok(Value::str(decoded))
        }
        ExprKind::Ident => {
            let name = text_in_map(map, expr.span);
            consts
                .iter()
                .rev()
                .find(|(candidate, _)| candidate == name)
                .map(|(_, value)| value.clone())
                .ok_or(())
        }
        ExprKind::Member { object, field } => {
            let ExprKind::Ident = object.kind else {
                return Err(());
            };
            let key =
                namespace_const_key(text_in_map(map, object.span), text_in_map(map, field.span));
            consts
                .iter()
                .rev()
                .find(|(candidate, _)| candidate == &key)
                .map(|(_, value)| value.clone())
                .ok_or(())
        }
        ExprKind::Paren(inner) => eval_nominal_record_default_const_value(inner, map, consts),
        ExprKind::Unary { op, operand } => {
            let value = eval_nominal_record_default_const_value(operand, map, consts)?;
            unary_value(*op, value, expr.span).map_err(|_| ())
        }
        ExprKind::Binary { op, lhs, rhs }
            if !matches!(op, BinaryOp::And | BinaryOp::Or | BinaryOp::Coalesce) =>
        {
            let left = eval_nominal_record_default_const_value(lhs, map, consts)?;
            let right = eval_nominal_record_default_const_value(rhs, map, consts)?;
            binary_value(*op, left, right, expr.span).map_err(|_| ())
        }
        _ => Err(()),
    }
}

pub(super) fn emit_nominal_record_const_default_expr(
    expr: &Expr,
    map: &SourceMap,
    consts: &[(String, Value)],
) -> Result<String, PyEmitError> {
    if imported_nominal_record_default_is_self_contained(expr) {
        let ctx = Ctx::new(
            map,
            std::collections::BTreeMap::new(),
            std::collections::BTreeMap::new(),
            std::collections::BTreeMap::new(),
            None,
        );
        return emit_expr(expr, &ctx);
    }
    if !const_initializer_ok_for_defaults(expr, map, consts) {
        return Err(
            PyEmitError::unsupported("imported nominal record reference default").at(expr.span),
        );
    }
    let value = eval_nominal_record_default_const_value(expr, map, consts).map_err(|_| {
        PyEmitError::unsupported("imported nominal record reference default").at(expr.span)
    })?;
    render_function_default_value(&value).ok_or_else(|| {
        PyEmitError::unsupported("imported nominal record reference default").at(expr.span)
    })
}

pub(super) fn emit_nominal_record_runtime_reference_root_expr(
    expr: &Expr,
    map: &SourceMap,
    runtime_values: &[(String, String)],
    self_runtime_values: &[(String, String)],
    hidden_runtime_values: &[(String, String)],
) -> Option<String> {
    match &expr.kind {
        ExprKind::Ident => {
            let name = text_in_map(map, expr.span);
            if let Some((_, py_name)) = self_runtime_values
                .iter()
                .rev()
                .find(|(candidate, _)| candidate == name)
            {
                return Some(format!(
                    "__tpz_self_default({py_name}, {}, {})",
                    py_string(name),
                    py_span(expr.span)
                ));
            }
            runtime_values
                .iter()
                .rev()
                .find(|(candidate, _)| candidate == name)
                .map(|(_, py_name)| py_name.clone())
        }
        ExprKind::Member { object, field } => {
            let ExprKind::Ident = object.kind else {
                return None;
            };
            let key =
                namespace_const_key(text_in_map(map, object.span), text_in_map(map, field.span));
            if let Some((_, py_name)) = hidden_runtime_values
                .iter()
                .rev()
                .find(|(candidate, _)| candidate == &key)
            {
                return Some(format!(
                    "__tpz_self_default({py_name}, {}, {})",
                    py_string(&key),
                    py_span(expr.span)
                ));
            }
            runtime_values
                .iter()
                .rev()
                .find(|(candidate, _)| candidate == &key)
                .map(|(_, py_name)| py_name.clone())
        }
        _ => None,
    }
}

pub(super) fn emit_nominal_record_runtime_reference_default_expr(
    expr: &Expr,
    map: &SourceMap,
    runtime_values: &[(String, String)],
    self_runtime_values: &[(String, String)],
    hidden_runtime_values: &[(String, String)],
) -> Option<String> {
    if let Some(root) = emit_nominal_record_runtime_reference_root_expr(
        expr,
        map,
        runtime_values,
        self_runtime_values,
        hidden_runtime_values,
    ) {
        return Some(root);
    }
    match &expr.kind {
        ExprKind::Member { object, field } => {
            let object_py = emit_nominal_record_runtime_reference_default_expr(
                object,
                map,
                runtime_values,
                self_runtime_values,
                hidden_runtime_values,
            )?;
            let source_field = text_in_map(map, field.span);
            Some(format!(
                "tpz_member({object_py}, {}, {}, {})",
                py_string(&mangle(source_field)),
                py_string(source_field),
                py_span(expr.span)
            ))
        }
        ExprKind::Paren(inner) => emit_nominal_record_runtime_reference_default_expr(
            inner,
            map,
            runtime_values,
            self_runtime_values,
            hidden_runtime_values,
        )
        .map(|inner| format!("({inner})")),
        _ => None,
    }
}

pub(super) fn emit_nominal_record_exported_default_expr(
    expr: &Expr,
    map: &SourceMap,
    consts: &[(String, Value)],
    runtime_values: &[(String, String)],
    self_runtime_values: &[(String, String)],
    hidden_runtime_values: &[(String, String)],
) -> Result<String, PyEmitError> {
    match emit_nominal_record_const_default_expr(expr, map, consts) {
        Ok(default_py) => Ok(default_py),
        Err(error) => emit_nominal_record_runtime_reference_default_expr(
            expr,
            map,
            runtime_values,
            self_runtime_values,
            hidden_runtime_values,
        )
        .ok_or(error),
    }
}

pub(super) fn namespace_const_key(namespace: &str, field: &str) -> String {
    format!("{namespace}.{field}")
}

pub(super) fn self_runtime_default_py_name(identity: &str, name: &str) -> String {
    format!("__topaz_self_default_{}_{}", mangle(identity), mangle(name))
}

pub(super) fn collect_module_default_input_facts(
    module: &topaz_resolve::ResolvedModule,
    map: &SourceMap,
    binding_facts: ModuleBindingFacts,
) -> ModuleDefaultInputFacts {
    let bound = &binding_facts.top_bound_names;
    let mut facts = ModuleDefaultInputFacts::default();
    let mut own_const_values = Vec::new();
    let mut exported_const_values = Vec::new();
    let mut self_runtime_available = Vec::new();
    let mut self_runtime_values = RecordDefaultSelfRuntimeValues::new();
    let mut namespace_runtime_candidates = NamespaceRuntimeDefaultCandidates::new();
    for stmt in &module.program.items {
        let exported = matches!(&stmt.kind, StmtKind::Export(_));
        let inner = exported_inner(stmt);
        match &inner.kind {
            StmtKind::Import(import) => {
                collect_module_default_import_bindings(import, map, bound, &mut facts.imports);
            }
            StmtKind::Const { name, value, .. } => {
                let source_name = text_in_map(map, name.span).to_string();
                if exported {
                    facts
                        .runtime_names
                        .exported_values
                        .insert(source_name.clone());
                }
                if !const_initializer_ok_for_defaults(value, map, &own_const_values) {
                    continue;
                }
                let Ok(evaluated) =
                    eval_nominal_record_default_const_value(value, map, &own_const_values)
                else {
                    continue;
                };
                if exported {
                    exported_const_values.push((source_name.clone(), evaluated.clone()));
                }
                own_const_values.push((source_name, evaluated));
            }
            StmtKind::Let {
                mutable, pattern, ..
            } => {
                if exported {
                    let mut names = BTreeSet::new();
                    collect_pattern_binding_names(pattern, map, &mut names);
                    if names.len() == 1 {
                        facts
                            .runtime_names
                            .exported_values
                            .insert(names.into_iter().next().expect("one binding"));
                    }
                }
                if !mutable {
                    let source_name = match &pattern.kind {
                        PatternKind::Binding(name) | PatternKind::Typed { name, .. } => {
                            text_in_map(map, name.span)
                        }
                        _ => continue,
                    };
                    if top_bound_name_is_unique(bound, source_name) {
                        facts
                            .runtime_names
                            .immutable_lets
                            .insert(source_name.to_string());
                        if !self_runtime_available
                            .iter()
                            .any(|existing| existing == source_name)
                        {
                            self_runtime_available.push(source_name.to_string());
                        }
                    }
                }
            }
            StmtKind::Record(decl) => {
                let record_name = text_in_map(map, decl.name.span).to_string();
                let self_refs = self_runtime_values.entry(record_name.clone()).or_default();
                let namespace_candidates =
                    namespace_runtime_candidates.entry(record_name).or_default();
                for field in &decl.fields {
                    let Some(default) = &field.default else {
                        continue;
                    };
                    collect_self_runtime_default_py_refs_from_expr(
                        default,
                        map,
                        &module.identity,
                        &self_runtime_available,
                        self_refs,
                    );
                    collect_namespace_runtime_default_candidates_from_expr(
                        default,
                        map,
                        namespace_candidates,
                    );
                }
            }
            _ => {}
        }
    }
    facts.const_values = ModuleDefaultConstFacts {
        own: own_const_values.into(),
        exported: exported_const_values.into(),
    };
    self_runtime_values.retain(|_, refs| !refs.is_empty());
    facts.self_runtime_values = Rc::new(self_runtime_values);
    facts.record_default_runtime_bindings = Rc::new(binding_facts.record_default_runtime_bindings);
    facts.module_value_source_names = binding_facts
        .module_value_source_names
        .into_iter()
        .collect::<Vec<_>>()
        .into();
    facts.module_top_bound_names = Rc::new(binding_facts.top_bound_names.into_keys().collect());
    for candidates in namespace_runtime_candidates.values_mut() {
        candidates.sort();
        candidates.dedup();
    }
    namespace_runtime_candidates.retain(|_, candidates| !candidates.is_empty());
    facts.namespace_runtime_candidates = Rc::new(namespace_runtime_candidates);
    facts
}

pub(super) fn collect_module_default_input_catalog(
    unit: &ResolveOutput,
) -> ModuleDefaultInputCatalog {
    unit.modules
        .iter()
        .map(|module| {
            let binding_facts = collect_module_binding_facts(module, &unit.map);
            (
                module.identity.clone(),
                Rc::new(collect_module_default_input_facts(
                    module,
                    &unit.map,
                    binding_facts,
                )),
            )
        })
        .collect()
}

pub(super) fn collect_module_default_import_bindings(
    import: &ImportItem,
    map: &SourceMap,
    bound: &ModuleTopBoundNameCounts,
    bindings: &mut ModuleDefaultImportBindings,
) {
    let identity = import_identity_from_map(import, map);
    match &import.kind {
        ImportKind::Namespace { alias } => {
            let last = text_in_map(
                map,
                import
                    .path
                    .segments
                    .last()
                    .expect("non-empty import path")
                    .span,
            );
            let local = alias
                .as_ref()
                .map_or(last, |alias| text_in_map(map, alias.span));
            if top_bound_name_is_unique(bound, local) {
                let index = bindings.namespaces.len();
                let local: Rc<str> = Rc::from(local);
                bindings.namespace_by_local.insert(Rc::clone(&local), index);
                bindings
                    .namespaces
                    .push(NamespaceDefaultImportBinding { identity, local });
            }
        }
        ImportKind::Selected { specs } => {
            let mut selected = Vec::new();
            for spec in specs {
                let imported = text_in_map(map, spec.name.span);
                let local = spec
                    .alias
                    .as_ref()
                    .map_or(imported, |alias| text_in_map(map, alias.span));
                if top_bound_name_is_unique(bound, local) {
                    selected.push(SelectedDefaultImportBinding {
                        imported: imported.to_string(),
                        local: local.to_string(),
                    });
                }
            }
            if !selected.is_empty() {
                bindings.selected.push(SelectedDefaultImport {
                    identity,
                    bindings: selected,
                });
            }
        }
    }
}

impl ModuleDefaultImportBindings {
    pub(super) fn namespace_identity(&self, local: &str) -> Option<&str> {
        let index = *self.namespace_by_local.get(local)?;
        Some(&self.namespaces[index].identity)
    }
}

pub(super) type HiddenPyRefsByRecord = BTreeMap<String, BTreeMap<String, Vec<(String, String)>>>;
pub(super) type HiddenPyRefsByModule = BTreeMap<String, Vec<(String, String)>>;

pub(super) fn collect_namespace_private_runtime_default_py_refs(
    module_defaults: &ModuleDefaultInputCatalog,
) -> (HiddenPyRefsByRecord, HiddenPyRefsByModule) {
    let mut by_record: HiddenPyRefsByRecord = BTreeMap::new();
    let mut by_target: HiddenPyRefsByModule = BTreeMap::new();
    for (module_identity, module_facts) in module_defaults {
        for (record_name, candidates) in module_facts.namespace_runtime_candidates.iter() {
            let mut refs = Vec::new();
            for (namespace, source_name) in candidates {
                let Some(target_identity) = module_facts.imports.namespace_identity(namespace)
                else {
                    continue;
                };
                let Some(target_facts) = module_defaults.get(target_identity) else {
                    continue;
                };
                let target_names = &target_facts.runtime_names;
                if !target_names.immutable_lets.contains(source_name)
                    || target_names.exported_values.contains(source_name)
                {
                    continue;
                }
                let py_name = self_runtime_default_py_name(target_identity, source_name);
                refs.push((namespace_const_key(namespace, source_name), py_name.clone()));
                by_target
                    .entry(target_identity.to_string())
                    .or_default()
                    .push((source_name.clone(), py_name));
            }
            if refs.is_empty() {
                continue;
            }
            refs.sort();
            refs.dedup();
            by_record
                .entry(module_identity.clone())
                .or_default()
                .insert(record_name.clone(), refs);
        }
    }
    for refs in by_target.values_mut() {
        refs.sort();
        refs.dedup();
    }
    (by_record, by_target)
}

pub(super) fn collect_namespace_runtime_default_candidates_from_expr(
    expr: &Expr,
    map: &SourceMap,
    candidates: &mut Vec<(String, String)>,
) {
    match &expr.kind {
        ExprKind::Member { object, field } => {
            let ExprKind::Ident = object.kind else {
                collect_namespace_runtime_default_candidates_from_expr(object, map, candidates);
                return;
            };
            let namespace = text_in_map(map, object.span);
            let source_name = text_in_map(map, field.span);
            candidates.push((namespace.to_string(), source_name.to_string()));
        }
        ExprKind::Paren(inner) => {
            collect_namespace_runtime_default_candidates_from_expr(inner, map, candidates);
        }
        _ => {}
    }
}

pub(super) fn collect_selected_imported_const_values_for_defaults(
    import_bindings: &ModuleDefaultImportBindings,
    module_defaults: &ModuleDefaultInputCatalog,
) -> Vec<(String, Value)> {
    let mut values: Vec<(String, Value)> = Vec::new();
    for selected_import in &import_bindings.selected {
        let Some(target_facts) = module_defaults.get(&selected_import.identity) else {
            continue;
        };
        let exported_consts = &target_facts.const_values.exported;
        for binding in &selected_import.bindings {
            if values.iter().any(|(name, _)| name == &binding.local) {
                continue;
            }
            if let Some((_, value)) = exported_consts
                .iter()
                .rev()
                .find(|(name, _)| name == &binding.imported)
            {
                values.push((binding.local.clone(), value.clone()));
            }
        }
    }
    values
}

pub(super) fn collect_selected_imported_runtime_value_py_names_for_defaults(
    import_bindings: &ModuleDefaultImportBindings,
    module_exports: &std::collections::BTreeMap<String, ModuleRuntimeExports<'_>>,
    module_defaults: &ModuleDefaultInputCatalog,
) -> Vec<(String, String)> {
    let mut values: Vec<(String, String)> = Vec::new();
    for selected_import in &import_bindings.selected {
        let Some(exports) = module_exports.get(&selected_import.identity) else {
            continue;
        };
        let Some(target_facts) = module_defaults.get(&selected_import.identity) else {
            continue;
        };
        let runtime_names = &target_facts.runtime_names;
        for binding in &selected_import.bindings {
            if values.iter().any(|(name, _)| name == &binding.local)
                || !runtime_names.immutable_lets.contains(&binding.imported)
            {
                continue;
            }
            if let Some(ModuleRuntimeExport::Value { py_name, .. }) = exports.get(&binding.imported)
            {
                values.push((binding.local.clone(), py_name.clone()));
            }
        }
    }
    values
}

pub(super) fn collect_namespace_imported_runtime_value_py_names_for_defaults(
    import_bindings: &ModuleDefaultImportBindings,
    module_exports: &std::collections::BTreeMap<String, ModuleRuntimeExports<'_>>,
    module_defaults: &ModuleDefaultInputCatalog,
) -> Vec<(String, String)> {
    let mut values: Vec<(String, String)> = Vec::new();
    for binding in &import_bindings.namespaces {
        let Some(exports) = module_exports.get(&binding.identity) else {
            continue;
        };
        let Some(target_facts) = module_defaults.get(&binding.identity) else {
            continue;
        };
        let runtime_names = &target_facts.runtime_names;
        for (exported, export) in exports.iter() {
            if !runtime_names.immutable_lets.contains(exported) {
                continue;
            }
            if let ModuleRuntimeExport::Value { py_name, .. } = export {
                values.push((
                    namespace_const_key(&binding.local, exported),
                    py_name.clone(),
                ));
            }
        }
    }
    values
}

pub(super) fn own_exported_runtime_let_py_names_for_defaults(
    module_identity: &str,
    runtime_names: &RuntimeDefaultNameFacts,
) -> Vec<(String, String)> {
    runtime_names
        .exported_values
        .intersection(&runtime_names.immutable_lets)
        .map(|source_name| {
            (
                source_name.clone(),
                module_value_name(module_identity, source_name),
            )
        })
        .collect()
}

pub(super) fn collect_self_runtime_default_py_refs_from_expr(
    expr: &Expr,
    map: &SourceMap,
    identity: &str,
    available: &[String],
    refs: &mut Vec<(String, String)>,
) {
    match &expr.kind {
        ExprKind::Ident => {
            let source_name = text_in_map(map, expr.span);
            if available.iter().any(|candidate| candidate == source_name)
                && !refs
                    .iter()
                    .any(|(existing, _)| existing.as_str() == source_name)
            {
                refs.push((
                    source_name.to_string(),
                    self_runtime_default_py_name(identity, source_name),
                ));
            }
        }
        ExprKind::Paren(inner) => {
            collect_self_runtime_default_py_refs_from_expr(inner, map, identity, available, refs);
        }
        _ => {}
    }
}

pub(super) fn collect_module_binding_facts(
    module: &topaz_resolve::ResolvedModule,
    map: &SourceMap,
) -> ModuleBindingFacts {
    let mut facts = ModuleBindingFacts::default();
    for stmt in &module.program.items {
        let inner = exported_inner(stmt);
        let mut runtime_binding_names = BTreeSet::new();
        let mut mutable_binding_names = BTreeSet::new();
        match &inner.kind {
            StmtKind::Import(import) => match &import.kind {
                ImportKind::Namespace { alias } => {
                    let last = text_in_map(
                        map,
                        import
                            .path
                            .segments
                            .last()
                            .expect("non-empty import path")
                            .span,
                    );
                    count_top_bound_name(
                        &mut facts.top_bound_names,
                        alias
                            .as_ref()
                            .map_or(last, |alias| text_in_map(map, alias.span)),
                    );
                }
                ImportKind::Selected { specs } => {
                    for spec in specs {
                        count_top_bound_name(
                            &mut facts.top_bound_names,
                            spec.alias.as_ref().map_or_else(
                                || text_in_map(map, spec.name.span),
                                |alias| text_in_map(map, alias.span),
                            ),
                        );
                    }
                }
            },
            StmtKind::Function(decl) => {
                let name = text_in_map(map, decl.name.span);
                count_top_bound_name(&mut facts.top_bound_names, name);
                runtime_binding_names.insert(name.to_string());
            }
            StmtKind::TypeAlias(alias) => {
                count_top_bound_name(
                    &mut facts.top_bound_names,
                    text_in_map(map, alias.name.span),
                );
            }
            StmtKind::Enum(decl) => {
                count_top_bound_name(&mut facts.top_bound_names, text_in_map(map, decl.name.span));
            }
            StmtKind::Record(decl) => {
                count_top_bound_name(&mut facts.top_bound_names, text_in_map(map, decl.name.span));
            }
            StmtKind::Newtype(decl) => {
                count_top_bound_name(&mut facts.top_bound_names, text_in_map(map, decl.name.span));
            }
            StmtKind::Const { name, .. } => {
                let name = text_in_map(map, name.span);
                count_top_bound_name(&mut facts.top_bound_names, name);
                runtime_binding_names.insert(name.to_string());
                facts.module_value_source_names.insert(name.to_string());
            }
            StmtKind::Let {
                mutable, pattern, ..
            } => {
                collect_pattern_binding_names(pattern, map, &mut runtime_binding_names);
                facts
                    .module_value_source_names
                    .extend(runtime_binding_names.iter().cloned());
                for name in &runtime_binding_names {
                    count_top_bound_name(&mut facts.top_bound_names, name);
                }
                if *mutable {
                    mutable_binding_names.clone_from(&runtime_binding_names);
                }
            }
            _ => {}
        }
        for name in &runtime_binding_names {
            *facts
                .record_default_runtime_bindings
                .counts
                .entry(name.clone())
                .or_default() += 1;
        }
        facts
            .record_default_runtime_bindings
            .statements
            .push(RecordDefaultStatementBindingFacts {
                current: runtime_binding_names.into_iter().collect::<Vec<_>>().into(),
                mutable: mutable_binding_names.into_iter().collect::<Vec<_>>().into(),
            });
    }
    facts
}

pub(super) fn count_top_bound_name(counts: &mut ModuleTopBoundNameCounts, name: &str) {
    *counts.entry(name.to_string()).or_default() += 1;
}

pub(super) fn top_bound_name_is_unique(counts: &ModuleTopBoundNameCounts, name: &str) -> bool {
    counts.get(name).copied() == Some(1)
}

pub(super) fn collect_namespace_imported_const_values_for_defaults(
    import_bindings: &ModuleDefaultImportBindings,
    module_defaults: &ModuleDefaultInputCatalog,
) -> Vec<(String, Value)> {
    let mut values: Vec<(String, Value)> = Vec::new();
    for binding in &import_bindings.namespaces {
        let Some(target_facts) = module_defaults.get(&binding.identity) else {
            continue;
        };
        let exported_consts = &target_facts.const_values.exported;
        for (exported, value) in exported_consts.iter() {
            values.push((namespace_const_key(&binding.local, exported), value.clone()));
        }
    }
    values
}

pub(super) fn collect_record_default_const_catalog(
    unit: &ResolveOutput,
    module_defaults: &ModuleDefaultInputCatalog,
) -> RecordDefaultConstCatalog {
    unit.modules
        .iter()
        .map(|module| {
            let module_facts = module_defaults
                .get(&module.identity)
                .expect("module default input facts");
            let import_bindings = &module_facts.imports;
            let mut consts = collect_selected_imported_const_values_for_defaults(
                import_bindings,
                module_defaults,
            );
            consts.extend(collect_namespace_imported_const_values_for_defaults(
                import_bindings,
                module_defaults,
            ));
            consts.extend(module_facts.const_values.own.iter().cloned());
            (module.identity.clone(), consts.into())
        })
        .collect()
}

pub(super) fn record_shape(fields: &[FieldInit], map: &SourceMap) -> RecordShape {
    RecordShape {
        fields: fields
            .iter()
            .map(|field| text_in_map(map, field.name.span).to_string())
            .collect(),
    }
}

pub(super) fn concurrent_record_shape(arms: &[ConcurrentArm], map: &SourceMap) -> RecordShape {
    RecordShape {
        fields: arms
            .iter()
            .map(|arm| text_in_map(map, arm.name.span).to_string())
            .collect(),
    }
}

pub(super) fn record_class_name(shape: &RecordShape) -> String {
    let mut name = String::from("_tr");
    for field in &shape.fields {
        name.push('_');
        for byte in field.bytes() {
            write!(name, "{byte:02x}").expect("write to string");
        }
    }
    name
}

pub(super) fn nominal_record_class_name(module_identity: Option<&str>, name: &str) -> String {
    match module_identity {
        Some(identity) => format!("_tnr{}__{}", mangle(identity), mangle(name)),
        None => format!("_tnr{}", mangle(name)),
    }
}

pub(super) fn emit_record_classes(shapes: &[RecordShape], out: &mut String) {
    for shape in shapes {
        out.push_str("@dataclass(frozen=True, slots=True)\n");
        writeln!(out, "class {}:", record_class_name(shape)).expect("write to string");
        let metadata = shape
            .fields
            .iter()
            .map(|field| format!("({}, {})", py_string(&mangle(field)), py_string(field)))
            .collect::<Vec<_>>()
            .join(", ");
        let comma = if shape.fields.len() == 1 { "," } else { "" };
        writeln!(out, "    __topaz_record_fields__ = ({metadata}{comma})")
            .expect("write to string");
        for field in &shape.fields {
            writeln!(
                out,
                "    {}: object  # {}",
                mangle(field),
                py_comment_name(field)
            )
            .expect("write to string");
        }
        out.push('\n');
    }
}

pub(super) fn emit_nominal_record_classes(
    records: &std::collections::BTreeMap<String, NominalRecordDef<'_>>,
    out: &mut String,
) {
    for record in records.values() {
        out.push_str("@dataclass(frozen=True, slots=True)\n");
        writeln!(out, "class {}:", record.py_class_name).expect("write to string");
        writeln!(
            out,
            "    __topaz_record_id__ = {}",
            py_string(&record.source_name)
        )
        .expect("write to string");
        if let Some(declaration_identity) = &record.declaration_identity {
            writeln!(
                out,
                "    __topaz_declaration_identity__ = {}",
                py_string(declaration_identity)
            )
            .expect("write to string");
        }
        if let Some(method_identity) = &record.method_identity {
            writeln!(
                out,
                "    __topaz_method_identity__ = {}",
                py_string(method_identity)
            )
            .expect("write to string");
        }
        let metadata = record
            .fields
            .iter()
            .map(|field| {
                format!(
                    "({}, {})",
                    py_string(&mangle(&field.source_name)),
                    py_string(&field.source_name)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let comma = if record.fields.len() == 1 { "," } else { "" };
        writeln!(out, "    __topaz_record_fields__ = ({metadata}{comma})")
            .expect("write to string");
        for field in &record.fields {
            writeln!(
                out,
                "    {}: object  # {}",
                mangle(&field.source_name),
                py_comment_name(&field.source_name)
            )
            .expect("write to string");
        }
        out.push('\n');
    }
}
