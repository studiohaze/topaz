use crate::*;

pub(crate) fn receiver_method_identity(module: &str, nominal: &str) -> String {
    let module = if module.is_empty() {
        "__entry__"
    } else {
        module
    };
    format!("{module}::{nominal}")
}

pub(crate) fn emitted_method_registration(
    module: &str,
    key: &str,
    method: &str,
    closure: &str,
) -> String {
    let module = if module.is_empty() {
        "__entry__"
    } else {
        module
    };
    if let Some((protocol, nominal)) = key.split_once('<')
        && let Some(nominal) = nominal.strip_suffix('>')
    {
        format!(
            "    __protocol_method_register({module:?}, {protocol:?}, {nominal:?}, {method:?}, {closure});\n"
        )
    } else {
        let registry_id = receiver_method_identity(module, key);
        format!("    __method_register({registry_id:?}, {method:?}, {closure});\n")
    }
}

pub(crate) fn seed_top_runtime_value_cells(
    items: &[Stmt],
    src: &LoweredText,
    locals: &mut Vec<(String, Bind)>,
) -> Result<String, EmitError> {
    let mut candidates = Vec::new();
    for stmt in items {
        let StmtKind::Let {
            mutable, pattern, ..
        } = &stmt.kind
        else {
            continue;
        };
        let Some(name) = single_binding_pattern_name(pattern, src) else {
            continue;
        };
        if name != "_" {
            candidates.push((
                name.to_string(),
                if *mutable {
                    Bind::TopMutValueCell
                } else {
                    Bind::TopValueCell
                },
                stmt.span,
            ));
        }
    }
    let mut enclosing = locals.clone();
    enclosing.extend(
        candidates
            .iter()
            .map(|(name, bind, _)| (name.clone(), *bind)),
    );
    let mut captured = HashSet::new();
    for stmt in items {
        let item = match &stmt.kind {
            StmtKind::Export(inner) => inner.as_ref(),
            _ => stmt,
        };
        let StmtKind::Impl(decl) = &item.kind else {
            continue;
        };
        for method in &decl.methods {
            let params = method
                .decl
                .params
                .iter()
                .map(|param| (text(src, param.name.span).to_string(), Bind::Imm))
                .collect::<Vec<_>>();
            captured.extend(closure_captures_block(
                &method.decl.body,
                &params,
                &enclosing,
                src,
            )?);
        }
    }
    let mut seed = String::new();
    for (name, bind, span) in candidates {
        if !captured.contains(name.as_str()) {
            continue;
        }
        if locals.iter().any(|(existing, _)| existing == &name) {
            return Err(EmitError::unsupported("same-scope redeclaration").at(span));
        }
        seed.push_str(&format!("    let {} = top_cell();\n", mangle(&name)));
        locals.push((name, bind));
    }
    Ok(seed)
}

impl<'a, 'c> Aliases<'a, 'c> {
    /// Runtime nominal and method identities use `__entry__` for the entry module,
    /// independently of its resolver identity (normally `main`). Imported modules
    /// retain their canonical module identity. All emitted method registration and
    /// lookup paths obtain that choice from the same `TypeCtx` fact.
    pub(crate) fn runtime_identity(&self) -> &str {
        self.type_ctx
            .module(self.identity)
            .map(|module| module.emission.runtime_identity.as_str())
            .unwrap_or(self.identity)
    }

    /// A child view for a function / lambda / defer-action BODY: the type-param
    /// scope is replaced (named fn: its decl params; lambda/defer: `&[]`), the
    /// table/poison/context are shared, and the qualified-type `in_nested` is set
    /// EXPLICITLY by the caller — `false` only for a function declared directly at
    /// the flat module top (its sole non-captured enclosing is module-top, which the
    /// namespace filtering handles); `true` for a lambda, a defer action, or any
    /// function declared inside a block / loop / match / enclosing body.
    pub(crate) fn with_body(&self, type_params: &'a [Ident], in_nested: bool) -> Aliases<'a, 'c> {
        Aliases {
            table: self.table.clone(),
            generic_table: self.generic_table.clone(),
            poison: self.poison.clone(),
            type_params,
            type_ctx: self.type_ctx,
            identity: self.identity,
            in_nested,
            flow: self.flow.clone(),
            enums: self.enums.clone(),
            records: self.records.clone(),
            newtypes: self.newtypes.clone(),
            methods: self.methods.clone(),
            method_names: self.method_names.clone(),
            protocols: self.protocols.clone(),
            schema_aliases: self.schema_aliases.clone(),
            schema_records: self.schema_records.clone(),
            schema_enums: self.schema_enums.clone(),
            schema_newtypes: self.schema_newtypes.clone(),
            imported_schema_record_modules: self.imported_schema_record_modules.clone(),
            imported_schema_enum_modules: self.imported_schema_enum_modules.clone(),
            imported_schema_newtype_modules: self.imported_schema_newtype_modules.clone(),
        }
    }

    /// §17 a child view for expanding a QUALIFIED alias body in its DEFINING
    /// (target) module: the target's table/poison drive Named lookups in the body
    /// (mirroring the interpreter matching the body under the defining source),
    /// UNIONED with the consumer's poison so a name the consumer could shadow with
    /// a block-local `type` (the interpreter's scope-chain-first `lookup_alias_in`)
    /// refuses rather than resolves the wrong body. The consuming `identity`,
    /// `type_ctx`, and body-nesting are unchanged; type params reset (monomorphic).
    pub(crate) fn with_def_module(&self, target: &ModuleTypeCtx<'a>) -> Aliases<'a, 'c> {
        let target_aliases = &target.local_aliases;
        let mut poison = target_aliases.poison.as_ref().clone();
        poison.extend(self.poison.iter().copied());
        let local_types = &target.local_types;
        Aliases {
            table: target_aliases.table.clone(),
            generic_table: target_aliases.generic_table.clone(),
            poison: Rc::new(poison),
            type_params: &[],
            type_ctx: self.type_ctx,
            identity: self.identity,
            in_nested: self.in_nested,
            flow: self.flow.clone(),
            enums: local_types.enum_defs.clone(),
            records: local_types.record_defs.clone(),
            newtypes: local_types.newtype_defs.clone(),
            methods: self.methods.clone(),
            method_names: self.method_names.clone(),
            protocols: self.protocols.clone(),
            schema_aliases: local_types.schema_aliases.clone(),
            schema_records: local_types.schema_records.clone(),
            schema_enums: local_types.schema_enums.clone(),
            schema_newtypes: local_types.schema_newtypes.clone(),
            imported_schema_record_modules: Rc::new(HashMap::new()),
            imported_schema_enum_modules: Rc::new(HashMap::new()),
            imported_schema_newtype_modules: Rc::new(HashMap::new()),
        }
    }

    /// Build a module's alias view from the complete local and imported facts in
    /// the shared cross-module `type_ctx`.
    pub(crate) fn collect(type_ctx: &'c TypeCtx<'a>, identity: &'a str) -> Aliases<'a, 'c> {
        let module = type_ctx
            .module(identity)
            .expect("current module has type context");
        let local_aliases = &module.local_aliases;
        let local_methods = &module.local_methods;
        let projection = &module.alias_projection;
        Aliases {
            table: local_aliases.table.clone(),
            generic_table: local_aliases.generic_table.clone(),
            poison: local_aliases.poison.clone(),
            type_params: &[],
            type_ctx,
            identity,
            in_nested: false,
            flow: Rc::new(RefCell::new(FlowCtx::default())),
            enums: projection.enum_defs.clone(),
            records: projection.record_defs.clone(),
            newtypes: projection.newtype_defs.clone(),
            methods: local_methods.definitions.clone(),
            method_names: projection.method_names.clone(),
            protocols: local_methods.protocols.clone(),
            schema_aliases: module.local_types.schema_aliases.clone(),
            schema_records: projection.schema_records.clone(),
            schema_enums: projection.schema_enums.clone(),
            schema_newtypes: projection.schema_newtypes.clone(),
            imported_schema_record_modules: projection.schema_record_modules.clone(),
            imported_schema_enum_modules: projection.schema_enum_modules.clone(),
            imported_schema_newtype_modules: projection.schema_newtype_modules.clone(),
        }
    }
}

pub(crate) fn emit_nominal_record_runtime_reference_root(
    expr: &Expr,
    src: &LoweredText,
    runtime_refs: &[(String, String, String)],
    self_runtime_refs: &[(String, String)],
    hidden_runtime_refs: &[(String, String, String)],
) -> Option<String> {
    match &expr.kind {
        ExprKind::Ident => {
            let name = text(src, expr.span);
            if let Some((_, cell)) = self_runtime_refs
                .iter()
                .rev()
                .find(|(local, _)| local == name)
            {
                return Some(format!(
                    "top_cell_get(&{cell}, {name:?}, {})?",
                    emit_span(expr.span)
                ));
            }
            if let Some((_, identity, hidden_field)) = hidden_runtime_refs
                .iter()
                .rev()
                .find(|(local, _, _)| local == name)
            {
                return Some(format!(
                    "member_value_required(&{}, {hidden_field:?}, {})?",
                    canonical_module(identity),
                    emit_span(expr.span)
                ));
            }
            runtime_refs
                .iter()
                .rev()
                .find(|(local, _, _)| local == name)
                .map(|(_, identity, exported)| {
                    format!(
                        "member_value_required(&{}, {exported:?}, {})?",
                        canonical_module(identity),
                        emit_span(expr.span)
                    )
                })
        }
        ExprKind::Member { object, field } => {
            let ExprKind::Ident = object.kind else {
                return None;
            };
            let key = namespace_const_key(text(src, object.span), text(src, field.span));
            if let Some((_, identity, hidden_field)) = hidden_runtime_refs
                .iter()
                .rev()
                .find(|(local, _, _)| local == &key)
            {
                return Some(format!(
                    "member_value_required(&{}, {hidden_field:?}, {})?",
                    canonical_module(identity),
                    emit_span(expr.span)
                ));
            }
            runtime_refs
                .iter()
                .rev()
                .find(|(local, _, _)| local == &key)
                .map(|(_, identity, exported)| {
                    format!(
                        "member_value_required(&{}, {exported:?}, {})?",
                        canonical_module(identity),
                        emit_span(expr.span)
                    )
                })
        }
        _ => None,
    }
}

pub(crate) fn project_alias_import<'a>(
    imp: &'a ImportItem,
    src: &'a LoweredText,
    target_identity: &str,
    target: &ModuleTypeCtx<'a>,
    projection: &mut ModuleAliasProjection<'a>,
) {
    let exported_types = &target.exported_type_surface;
    let ModuleAliasProjection {
        enum_defs: enums,
        record_defs: records,
        newtype_defs: newtypes,
        schema_records,
        schema_enums,
        schema_newtypes,
        schema_record_modules,
        schema_enum_modules,
        schema_newtype_modules,
        method_names,
    } = projection;
    let ImportKind::Selected { specs } = &imp.kind else {
        Rc::make_mut(method_names).extend(
            exported_types
                .receiver_methods
                .values()
                .flat_map(|methods| methods.iter().copied()),
        );
        return;
    };
    for spec in specs {
        let imported = text(src, spec.name.span);
        let local = spec
            .alias
            .as_ref()
            .map(|id| text(src, id.span))
            .unwrap_or(imported);
        if let Some(methods) = exported_types.receiver_methods.get(imported) {
            Rc::make_mut(method_names).extend(methods.iter().copied());
        }
        if let Some(def) = exported_types.enum_defs.get(imported) {
            Rc::make_mut(enums)
                .entry(local)
                .or_insert_with(|| def.clone());
            if let Some(decl) = target.local_types.schema_enums.get(imported) {
                Rc::make_mut(schema_enums).entry(local).or_insert(*decl);
                Rc::make_mut(schema_enum_modules)
                    .entry(local)
                    .or_insert_with(|| target_identity.to_string());
            }
        }
        if let Some(def) = exported_types.record_defs.get(imported) {
            Rc::make_mut(records)
                .entry(local)
                .or_insert_with(|| def.clone());
            if let Some(decl) = target.local_types.schema_records.get(imported) {
                Rc::make_mut(schema_records).entry(local).or_insert(*decl);
                Rc::make_mut(schema_record_modules)
                    .entry(local)
                    .or_insert_with(|| target_identity.to_string());
            }
        }
        if let Some(id) = exported_types.newtype_defs.get(imported) {
            Rc::make_mut(newtypes)
                .entry(local)
                .or_insert_with(|| id.clone());
            if let Some(decl) = target.local_types.schema_newtypes.get(imported) {
                Rc::make_mut(schema_newtypes).entry(local).or_insert(*decl);
                Rc::make_mut(schema_newtype_modules)
                    .entry(local)
                    .or_insert_with(|| target_identity.to_string());
            }
        }
    }
}

/// Collect one module's method, protocol, alias, and nominal declarations in one
/// source-order pass. Runtime nominal definitions are projected after every
/// inherent method target is known.
pub(crate) fn collect_local_declaration_inventory<'a>(
    items: &'a [Stmt],
    src: &'a LoweredText,
    origin_identity: &'a str,
    runtime_identity: &str,
    stable_nominal_identity: bool,
) -> LocalDeclarationInventory<'a> {
    let mut inventory = LocalDeclarationInventory {
        has_method_declarations: false,
        top_binding_cardinality: HashMap::new(),
        method_targets: HashSet::new(),
        exported_method_names: HashMap::new(),
        method_definitions: HashMap::new(),
        method_names: HashSet::new(),
        protocols: HashSet::from(["Show", "Eq", "Order"]),
        table: HashMap::new(),
        generic_table: HashMap::new(),
        poison: HashSet::new(),
        schema_aliases: HashMap::new(),
        schema_records: HashMap::new(),
        schema_enums: HashMap::new(),
        schema_newtypes: HashMap::new(),
        enum_defs: HashMap::new(),
        record_defs: HashMap::new(),
        newtype_defs: HashMap::new(),
    };
    let mut top_bound_names = Vec::new();
    for stmt in items {
        let first_new_name = top_bound_names.len();
        append_module_top_bound_names(stmt, src, &mut top_bound_names);
        for name in &top_bound_names[first_new_name..] {
            *inventory.top_binding_cardinality.entry(*name).or_insert(0) += 1;
        }
        let inner = match &stmt.kind {
            StmtKind::Export(inner) => &**inner,
            _ => stmt,
        };
        match &inner.kind {
            StmtKind::TypeAlias(decl) => {
                let name = text(src, decl.name.span);
                inventory.schema_aliases.insert(name, decl);
                if inventory.table.contains_key(name)
                    || inventory.generic_table.contains_key(name)
                    || inventory.poison.contains(name)
                {
                    inventory.table.remove(name);
                    inventory.generic_table.remove(name);
                    inventory.poison.insert(name);
                } else if decl.type_params.is_empty() {
                    inventory.table.insert(name, &decl.ty);
                } else {
                    inventory
                        .generic_table
                        .insert(name, (decl.type_params.as_slice(), &decl.ty));
                }
            }
            StmtKind::Record(decl) => {
                let name = text(src, decl.name.span);
                inventory.schema_records.insert(name, decl);
            }
            StmtKind::Enum(decl) => {
                let name = text(src, decl.name.span);
                inventory.schema_enums.insert(name, decl);
            }
            StmtKind::Newtype(decl) => {
                let name = text(src, decl.name.span);
                inventory.schema_newtypes.insert(name, decl);
            }
            StmtKind::Protocol(decl) => {
                inventory.protocols.insert(text(src, decl.name.span));
            }
            StmtKind::Impl(decl) => {
                inventory.has_method_declarations = true;
                let key = match decl.target {
                    Some(target) => {
                        format!("{}<{}>", text(src, decl.name.span), text(src, target.span))
                    }
                    None => {
                        let target = text(src, decl.name.span);
                        inventory.method_targets.insert(target);
                        for method in &decl.methods {
                            let name = text(src, method.decl.name.span);
                            inventory.method_names.insert(name);
                            if method.exported {
                                inventory
                                    .exported_method_names
                                    .entry(target)
                                    .or_default()
                                    .insert(name);
                            }
                        }
                        target.to_string()
                    }
                };
                inventory
                    .method_definitions
                    .entry(key)
                    .or_default()
                    .extend(decl.methods.iter());
            }
            _ => {}
        }
    }
    for (name, decl) in &inventory.schema_records {
        let name = *name;
        inventory.record_defs.insert(
            name,
            RecordDef {
                id: name,
                origin_identity,
                declaration_identity: stable_nominal_identity
                    .then(|| receiver_method_identity(runtime_identity, name)),
                method_identity: inventory
                    .method_targets
                    .contains(name)
                    .then(|| receiver_method_identity(runtime_identity, name)),
                fields: decl
                    .fields
                    .iter()
                    .map(|field| {
                        (
                            text(src, field.name.span),
                            field.default.as_ref().map(|default| (src, default)),
                        )
                    })
                    .collect(),
            },
        );
    }
    for (name, decl) in &inventory.schema_enums {
        let name = *name;
        inventory.enum_defs.insert(
            name,
            EnumDef {
                id: name,
                declaration_identity: stable_nominal_identity
                    .then(|| receiver_method_identity(runtime_identity, name)),
                method_identity: inventory
                    .method_targets
                    .contains(name)
                    .then(|| receiver_method_identity(runtime_identity, name)),
                variants: decl
                    .variants
                    .iter()
                    .enumerate()
                    .map(|(index, variant)| {
                        let arity = variant.payload.as_ref().map_or(0, |types| types.len());
                        (text(src, variant.name.span), (arity, index as u32))
                    })
                    .filter(|(variant, _)| !matches!(*variant, "None" | "Some" | "Ok" | "Err"))
                    .collect(),
            },
        );
    }
    for name in inventory.schema_newtypes.keys().copied() {
        inventory.newtype_defs.insert(
            name,
            NewtypeDef {
                id: name,
                declaration_identity: stable_nominal_identity
                    .then(|| receiver_method_identity(runtime_identity, name)),
                method_identity: inventory
                    .method_targets
                    .contains(name)
                    .then(|| receiver_method_identity(runtime_identity, name)),
            },
        );
    }
    inventory
}

/// §3 (v5.3): the declared enum names whose variant set includes `name`, as a
/// Rust array-literal of string literals (e.g. `["Color", "Hue"]`). Empty when
/// `name` is not any enum's variant. Used to emit a refutable runtime test for a
/// bare/`Constructor` variant pattern — mirroring the interpreter, which checks
/// the MATCHED value's own enum against `name`.
pub(crate) fn enums_declaring_variant(aliases: &Aliases<'_, '_>, name: &str) -> Vec<String> {
    let mut owners: Vec<String> = aliases
        .enums
        .iter()
        .filter(|(_, def)| def.variants.contains_key(name))
        .map(|(_, def)| {
            format!(
                "{:?}",
                nominal_declaration_identity(def.id, def.declaration_identity.as_deref())
            )
        })
        .collect();
    owners.extend(
        aliases
            .type_ctx
            .modules
            .values()
            .flat_map(|module| module.exported_type_surface.enum_defs.values())
            .filter(|def| def.variants.contains_key(name))
            .map(|def| {
                format!(
                    "{:?}",
                    nominal_declaration_identity(def.id, def.declaration_identity.as_deref())
                )
            }),
    );
    owners.sort(); // deterministic emit
    owners.dedup();
    owners
}

/// Finalize one module's alias-resolution product after applying block-local
/// alias shadowing. The structural walk descends every block-bearing AST
/// position rather than collecting only module declarations.
pub(crate) fn build_module_local_alias_resolution<'a>(
    items: &'a [Stmt],
    src: &'a LoweredText,
    mut table: HashMap<&'a str, &'a Type>,
    mut generic_table: GenericAliasTable<'a>,
    mut poison: HashSet<&'a str>,
) -> ModuleLocalAliasResolution<'a> {
    let mut nested: HashSet<&'a str> = HashSet::new();
    for stmt in items {
        walk_stmt_children_for_aliases(stmt, src, &mut nested);
    }
    for name in nested {
        table.remove(name);
        generic_table.remove(name);
        poison.insert(name);
    }
    ModuleLocalAliasResolution {
        table: Rc::new(table),
        generic_table: Rc::new(generic_table),
        poison: Rc::new(poison),
    }
}

impl<'a> TypeCtx<'a> {
    pub(crate) fn module(&self, identity: &str) -> Option<&ModuleTypeCtx<'a>> {
        self.modules.get(identity)
    }
}

pub(crate) fn lispex_intrinsic_kind(aliases: &Aliases<'_, '_>, name: &str) -> Option<&'static str> {
    let module = aliases.type_ctx.module(aliases.identity)?;
    if !module.emission.is_generated_std {
        return None;
    }
    match (aliases.identity, name) {
        ("std.lispex.rules", "__lispexRule") => Some("LispexRule"),
        ("std.lispex", "__lispexValueFromCanonical") => Some("LispexValueFromCanonical"),
        ("std.lispex", "__lispexCanonicalBytes") => Some("LispexCanonicalBytes"),
        ("std.lispex", "__lispexDefaultLimits") => Some("LispexDefaultLimits"),
        ("std.lispex", "__lispexInspectRule") => Some("LispexInspectRule"),
        ("std.lispex", "__lispexEvaluate") => Some("LispexEvaluate"),
        ("std.lispex", "__lispexEvaluateWithEvidence") => Some("LispexEvaluateWithEvidence"),
        ("std.lispex", "__lispexConsumerArtifactFromBytes") => {
            Some("LispexConsumerArtifactFromBytes")
        }
        ("std.lispex", "__lispexConsumerArtifactBytes") => Some("LispexConsumerArtifactBytes"),
        ("std.lispex", "__lispexPortableCoreBytes") => Some("LispexPortableCoreBytes"),
        ("std.lispex", "__lispexInspectConsumerArtifact") => Some("LispexInspectConsumerArtifact"),
        ("std.lispex", "__lispexVerifyConsumerArtifact") => Some("LispexVerifyConsumerArtifact"),
        ("std.lispex", "__lispexFreshReplay") => Some("LispexFreshReplay"),
        _ => None,
    }
}

/// Append every name bound by one module-top statement (unwrapping a single
/// `export`). Keeping this statement projection shared lets declaration
/// collection record binding cardinality in its existing source-order pass.
pub(crate) fn append_module_top_bound_names<'a>(
    stmt: &'a Stmt,
    src: &'a LoweredText,
    names: &mut Vec<&'a str>,
) {
    let kind = match &stmt.kind {
        StmtKind::Export(inner) => &inner.kind,
        other => other,
    };
    match kind {
        StmtKind::Import(imp) => match &imp.kind {
            ImportKind::Namespace { alias } => {
                let last = text(src, imp.path.segments.last().expect("non-empty path").span);
                names.push(alias.as_ref().map(|id| text(src, id.span)).unwrap_or(last));
            }
            ImportKind::Selected { specs } => {
                for spec in specs {
                    names.push(
                        spec.alias
                            .as_ref()
                            .map(|id| text(src, id.span))
                            .unwrap_or_else(|| text(src, spec.name.span)),
                    );
                }
            }
        },
        StmtKind::Function(decl) => names.push(text(src, decl.name.span)),
        StmtKind::TypeAlias(a) => names.push(text(src, a.name.span)),
        StmtKind::Enum(decl) => names.push(text(src, decl.name.span)),
        StmtKind::Record(decl) => names.push(text(src, decl.name.span)),
        StmtKind::Newtype(decl) => names.push(text(src, decl.name.span)),
        StmtKind::Const { name, .. } => names.push(text(src, name.span)),
        StmtKind::Let { pattern, .. } => pattern_binds(pattern, src, names),
        _ => {}
    }
}

pub(crate) fn record_default_thunk_cell(identity: &str, record: &str, field: &str) -> String {
    format!(
        "__topaz_record_default_{}_{}_{}",
        mangle(identity),
        mangle(record),
        mangle(field)
    )
}

pub(crate) fn record_default_thunk_hidden_field(
    identity: &str,
    record: &str,
    field: &str,
) -> String {
    format!("__topaz_record_default::{identity}::{record}::{field}")
}

pub(crate) fn record_default_requires_execution_thunk(expr: &Expr) -> bool {
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
            record_default_requires_execution_thunk(inner)
        }
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Compose { lhs, rhs } => {
            record_default_requires_execution_thunk(lhs)
                || record_default_requires_execution_thunk(rhs)
        }
        ExprKind::Range { lo, hi, step, .. } => {
            record_default_requires_execution_thunk(lo)
                || record_default_requires_execution_thunk(hi)
                || step
                    .as_deref()
                    .is_some_and(record_default_requires_execution_thunk)
        }
        ExprKind::Array(elements) => elements.iter().any(|element| match element {
            ArrayElement::Expr(expr) | ArrayElement::Spread(expr) => {
                record_default_requires_execution_thunk(expr)
            }
        }),
        ExprKind::SetLiteral(elements) => {
            elements.iter().any(record_default_requires_execution_thunk)
        }
        ExprKind::MapLiteral(entries) => entries.iter().any(|(key, value)| {
            record_default_requires_execution_thunk(key)
                || record_default_requires_execution_thunk(value)
        }),
        ExprKind::String(lit) => lit.parts.iter().any(|part| {
            matches!(part, StringPart::Interpolation(expr)
                if record_default_requires_execution_thunk(expr))
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

pub(crate) fn record_default_has_mutable_reference(
    referenced: &[&str],
    bound: &[&str],
    prior_mutable_names: &HashSet<&str>,
) -> bool {
    referenced.iter().any(|name| {
        prior_mutable_names.contains(*name) && !bound.iter().any(|bound_name| bound_name == name)
    })
}

pub(crate) fn collect_module_default_input_facts<'a>(
    items: &'a [Stmt],
    src: &'a LoweredText,
    identity: &str,
    modules: &std::collections::BTreeMap<String, ModuleTypeCtx<'a>>,
    namespaces: &std::collections::BTreeMap<String, String>,
    top_binding_cardinality: &HashMap<&'a str, usize>,
) -> ModuleDefaultInputFacts<'a> {
    let mut const_values = ConstValues::new();
    let mut exported_const_values = Vec::new();
    let mut export_names = HashSet::new();
    let mut immutable_let_names = HashSet::new();
    let mut own_exported_runtime_refs = Vec::new();
    let mut available_self_runtime_names = HashSet::new();
    let mut self_runtime_refs: HashMap<String, Vec<RuntimeTargetRef>> = HashMap::new();
    let mut self_runtime_ref_names = HashMap::<&str, HashSet<&str>>::new();
    let mut hidden_runtime_refs = RuntimeRefsByRecord::new();
    let mut external_hidden_runtime_refs = RuntimeRefsByTarget::new();
    let mut thunks = HashMap::new();
    let mut remaining_bound_names = top_binding_cardinality.clone();
    let mut prior_mutable_names = HashSet::new();
    let mut current_bound_names = Vec::new();
    for item in items {
        current_bound_names.clear();
        append_module_top_bound_names(item, src, &mut current_bound_names);
        for name in &current_bound_names {
            if let std::collections::hash_map::Entry::Occupied(mut entry) =
                remaining_bound_names.entry(*name)
            {
                if *entry.get() == 1 {
                    entry.remove();
                } else {
                    *entry.get_mut() -= 1;
                }
            }
        }
        let exported = matches!(&item.kind, StmtKind::Export(_));
        let inner = match &item.kind {
            StmtKind::Export(inner) => inner.as_ref(),
            _ => item,
        };
        match &inner.kind {
            StmtKind::Const { name, value, .. } => {
                let var = text(src, name.span);
                if exported {
                    export_names.insert(var);
                }
                if const_values.contains_key(var)
                    || !const_initializer_ok(value, src, &const_values)
                {
                    continue;
                }
                if let Ok(value) = const_eval_emit(value, src, &const_values) {
                    if exported {
                        exported_const_values.push((var.to_string(), value.clone()));
                    }
                    const_values.insert(var.to_string(), value);
                }
            }
            StmtKind::Let {
                mutable, pattern, ..
            } => {
                let local = single_binding_pattern_name(pattern, src);
                if exported && let Some(local) = local {
                    export_names.insert(local);
                }
                if *mutable {
                    prior_mutable_names.extend(current_bound_names.iter().copied());
                    continue;
                }
                let Some(local) = local else {
                    continue;
                };
                if top_binding_cardinality.get(local) != Some(&1) {
                    continue;
                }
                immutable_let_names.insert(local);
                available_self_runtime_names.insert(local);
                if exported {
                    own_exported_runtime_refs.push((
                        local.to_string(),
                        identity.to_string(),
                        local.to_string(),
                    ));
                }
            }
            StmtKind::Record(decl) => {
                let record = text(src, decl.name.span);
                let record_refs = self_runtime_refs.entry(record.to_string()).or_default();
                let record_ref_names = self_runtime_ref_names.entry(record).or_default();
                let mut record_hidden_runtime_refs = Vec::new();
                let mut record_thunks = Vec::new();
                for field in &decl.fields {
                    let Some(default) = &field.default else {
                        continue;
                    };
                    collect_self_runtime_default_refs_from_expr(
                        default,
                        src,
                        identity,
                        &available_self_runtime_names,
                        record_ref_names,
                        record_refs,
                    );
                    collect_namespace_private_runtime_refs_from_expr(
                        default,
                        src,
                        modules,
                        namespaces,
                        &mut record_hidden_runtime_refs,
                        &mut external_hidden_runtime_refs,
                    );
                    if imported_nominal_record_default_is_self_contained(default) {
                        continue;
                    }
                    let mut referenced = Vec::new();
                    let mut bound = Vec::new();
                    collect_idents(default, src, &mut referenced, &mut bound);
                    let mutable_reference = record_default_has_mutable_reference(
                        &referenced,
                        &bound,
                        &prior_mutable_names,
                    );
                    if !mutable_reference && !record_default_requires_execution_thunk(default) {
                        continue;
                    }
                    let has_forward_reference = referenced.iter().any(|name| {
                        remaining_bound_names.contains_key(name)
                            && !bound.iter().any(|bound_name| bound_name == name)
                    });
                    if has_forward_reference {
                        continue;
                    }
                    let field_name = text(src, field.name.span);
                    record_thunks.push(RecordDefaultThunk {
                        field: field_name.to_string(),
                        cell: record_default_thunk_cell(identity, record, field_name),
                        hidden_field: record_default_thunk_hidden_field(
                            identity, record, field_name,
                        ),
                        label: format!("{record}.{field_name} default"),
                        span: default.span,
                    });
                }
                if !record_thunks.is_empty() {
                    thunks.insert(record.to_string(), record_thunks);
                }
                record_hidden_runtime_refs.sort();
                record_hidden_runtime_refs.dedup();
                if !record_hidden_runtime_refs.is_empty() {
                    hidden_runtime_refs.insert(record.to_string(), record_hidden_runtime_refs);
                }
            }
            _ => {}
        }
    }
    self_runtime_refs.retain(|_, refs| !refs.is_empty());
    for refs in external_hidden_runtime_refs.values_mut() {
        refs.sort();
        refs.dedup();
    }
    ModuleDefaultInputFacts {
        const_values,
        exported_const_values,
        export_names,
        immutable_let_names,
        own_exported_runtime_refs,
        self_runtime_refs,
        hidden_runtime_refs,
        external_hidden_runtime_refs,
        thunks,
    }
}

pub(crate) fn build_module_type_projection<'a>(
    items: &'a [Stmt],
    src: &'a LoweredText,
    modules: &std::collections::BTreeMap<String, ModuleTypeCtx<'a>>,
    local_types: &ModuleLocalTypeDeclarations<'a>,
    method_names: HashSet<&'a str>,
    exported_method_names: HashMap<&'a str, HashSet<&'a str>>,
    top_binding_cardinality: &HashMap<&'a str, usize>,
) -> BuiltModuleTypeProjection<'a> {
    let mut alias_projection = ModuleAliasProjection {
        enum_defs: local_types.enum_defs.clone(),
        record_defs: local_types.record_defs.clone(),
        newtype_defs: local_types.newtype_defs.clone(),
        schema_records: local_types.schema_records.clone(),
        schema_enums: local_types.schema_enums.clone(),
        schema_newtypes: local_types.schema_newtypes.clone(),
        method_names: Rc::new(method_names),
        schema_record_modules: Rc::new(HashMap::new()),
        schema_enum_modules: Rc::new(HashMap::new()),
        schema_newtype_modules: Rc::new(HashMap::new()),
    };
    let mut exported_type_surface = ModuleExportedTypeSurface {
        names: HashSet::new(),
        enum_defs: HashMap::new(),
        record_defs: HashMap::new(),
        newtype_defs: HashMap::new(),
        receiver_methods: HashMap::new(),
    };
    let mut namespaces = std::collections::BTreeMap::new();
    let mut selected_types = std::collections::BTreeMap::new();
    for item in items {
        match &item.kind {
            StmtKind::Export(inner) => match &inner.kind {
                StmtKind::TypeAlias(alias) => {
                    exported_type_surface
                        .names
                        .insert(text(src, alias.name.span));
                }
                StmtKind::Enum(decl) => {
                    let name = text(src, decl.name.span);
                    exported_type_surface.names.insert(name);
                    if let Some(definition) = local_types.enum_defs.get(name) {
                        exported_type_surface
                            .enum_defs
                            .insert(name, definition.clone());
                    }
                }
                StmtKind::Record(decl) => {
                    let name = text(src, decl.name.span);
                    exported_type_surface.names.insert(name);
                    if let Some(definition) = local_types.record_defs.get(name) {
                        exported_type_surface
                            .record_defs
                            .insert(name, definition.clone());
                    }
                }
                StmtKind::Newtype(decl) => {
                    let name = text(src, decl.name.span);
                    exported_type_surface.names.insert(name);
                    if let Some(definition) = local_types.newtype_defs.get(name) {
                        exported_type_surface
                            .newtype_defs
                            .insert(name, definition.clone());
                    }
                }
                _ => {}
            },
            StmtKind::Import(import) => {
                let identity = render_import_path(import, src);
                if let Some(target) = modules.get(&identity) {
                    project_alias_import(import, src, &identity, target, &mut alias_projection);
                }
                match &import.kind {
                    ImportKind::Namespace { alias } => {
                        let last = text(
                            src,
                            import.path.segments.last().expect("non-empty path").span,
                        );
                        let local = alias
                            .as_ref()
                            .map(|identifier| text(src, identifier.span))
                            .unwrap_or(last);
                        if top_binding_cardinality.get(local) == Some(&1) {
                            namespaces.insert(local.to_string(), identity);
                        }
                    }
                    ImportKind::Selected { specs } => {
                        for spec in specs {
                            let imported = text(src, spec.name.span);
                            let local = spec
                                .alias
                                .as_ref()
                                .map(|alias| text(src, alias.span))
                                .unwrap_or(imported);
                            if top_binding_cardinality.get(local) == Some(&1) {
                                selected_types.insert(
                                    local.to_string(),
                                    (identity.clone(), imported.to_string()),
                                );
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for (nominal, methods) in exported_method_names {
        if exported_type_surface.enum_defs.contains_key(nominal)
            || exported_type_surface.record_defs.contains_key(nominal)
            || exported_type_surface.newtype_defs.contains_key(nominal)
        {
            exported_type_surface
                .receiver_methods
                .insert(nominal, methods);
        }
    }
    BuiltModuleTypeProjection {
        alias_projection,
        exported_type_surface,
        type_imports: ModuleTypeImportBindings {
            namespaces,
            selected_types,
        },
    }
}

pub(crate) fn build_module_default_facts<'a>(
    items: &'a [Stmt],
    src: &'a LoweredText,
    identity: &str,
    modules: &std::collections::BTreeMap<String, ModuleTypeCtx<'a>>,
    type_imports: &ModuleTypeImportBindings,
    top_binding_cardinality: &HashMap<&'a str, usize>,
) -> BuiltModuleDefaultFacts<'a> {
    let ModuleDefaultInputFacts {
        const_values,
        exported_const_values,
        export_names,
        immutable_let_names,
        own_exported_runtime_refs,
        self_runtime_refs,
        hidden_runtime_refs,
        external_hidden_runtime_refs,
        thunks,
    } = collect_module_default_input_facts(
        items,
        src,
        identity,
        modules,
        &type_imports.namespaces,
        top_binding_cardinality,
    );
    let ModuleDefaultImportFacts {
        selected_const_values,
        selected_runtime_refs,
    } = collect_selected_default_import_facts(items, src, modules, top_binding_cardinality);
    let ModuleNamespaceDefaultImportFacts {
        const_values: namespace_const_values,
        runtime_refs: namespace_runtime_refs,
    } = collect_namespace_default_import_facts(modules, &type_imports.namespaces);
    let mut imported_const_values = ConstValues::new();
    imported_const_values.extend(selected_const_values);
    imported_const_values.extend(namespace_const_values);
    let mut runtime_refs = selected_runtime_refs;
    runtime_refs.extend(own_exported_runtime_refs);
    runtime_refs.extend(namespace_runtime_refs);
    imported_const_values.extend(const_values);
    BuiltModuleDefaultFacts {
        runtime_values: ModuleRuntimeValueSurface {
            exported_const_values,
            export_names,
            immutable_let_names,
        },
        record_defaults: ModuleRecordDefaultFacts {
            const_values: imported_const_values,
            runtime_refs,
            self_runtime_refs,
            hidden_runtime_refs,
            thunks,
            external_hidden_runtime_refs: Vec::new(),
        },
        external_hidden_runtime_refs,
    }
}

pub(crate) fn build_module_local_declarations<'a>(
    module: &'a LoweredModule,
    runtime_identity: &str,
    stable_nominal_identity: bool,
) -> BuiltModuleLocalDeclarations<'a> {
    let src = &module.text;
    let LocalDeclarationInventory {
        has_method_declarations,
        top_binding_cardinality,
        exported_method_names,
        method_definitions,
        method_names,
        protocols,
        table,
        generic_table,
        poison,
        schema_aliases,
        schema_records,
        schema_enums,
        schema_newtypes,
        enum_defs,
        record_defs,
        newtype_defs,
        ..
    } = collect_local_declaration_inventory(
        &module.program.items,
        src,
        &module.identity,
        runtime_identity,
        stable_nominal_identity,
    );
    BuiltModuleLocalDeclarations {
        has_method_declarations,
        top_binding_cardinality,
        method_names,
        exported_method_names,
        local_aliases: build_module_local_alias_resolution(
            &module.program.items,
            src,
            table,
            generic_table,
            poison,
        ),
        local_types: ModuleLocalTypeDeclarations {
            enum_defs: Rc::new(enum_defs),
            record_defs: Rc::new(record_defs),
            newtype_defs: Rc::new(newtype_defs),
            schema_aliases: Rc::new(schema_aliases),
            schema_records: Rc::new(schema_records),
            schema_enums: Rc::new(schema_enums),
            schema_newtypes: Rc::new(schema_newtypes),
        },
        local_methods: ModuleLocalMethodDeclarations {
            definitions: Rc::new(method_definitions),
            protocols: Rc::new(protocols),
        },
    }
}

pub(crate) fn build_module_type_context<'a>(
    module: &'a LoweredModule,
    stable_nominal_identity: bool,
    modules: &std::collections::BTreeMap<String, ModuleTypeCtx<'a>>,
) -> BuiltModuleTypeContext<'a> {
    let src = &module.text;
    let runtime_identity = if module.is_entry {
        ""
    } else {
        module.identity.as_str()
    };
    let BuiltModuleLocalDeclarations {
        has_method_declarations,
        top_binding_cardinality,
        exported_method_names,
        method_names,
        local_aliases,
        local_types,
        local_methods,
    } = build_module_local_declarations(module, runtime_identity, stable_nominal_identity);
    let BuiltModuleTypeProjection {
        alias_projection,
        exported_type_surface,
        type_imports,
    } = build_module_type_projection(
        &module.program.items,
        src,
        modules,
        &local_types,
        method_names,
        exported_method_names,
        &top_binding_cardinality,
    );
    let BuiltModuleDefaultFacts {
        runtime_values,
        record_defaults,
        external_hidden_runtime_refs,
    } = build_module_default_facts(
        &module.program.items,
        src,
        &module.identity,
        modules,
        &type_imports,
        &top_binding_cardinality,
    );
    BuiltModuleTypeContext {
        context: ModuleTypeCtx {
            emission: ModuleEmissionIdentity {
                src,
                runtime_identity: runtime_identity.to_string(),
                is_generated_std: module.is_generated_std,
            },
            local_aliases,
            runtime_values,
            record_defaults,
            local_types,
            local_methods,
            alias_projection,
            exported_type_surface,
            type_imports,
        },
        has_method_declarations,
        external_hidden_runtime_refs,
    }
}

pub(crate) fn merge_external_hidden_runtime_refs(
    modules: &mut std::collections::BTreeMap<String, ModuleTypeCtx<'_>>,
    references_by_target: RuntimeRefsByTarget,
) {
    for (target_identity, refs) in references_by_target {
        if let Some(target) = modules.get_mut(&target_identity) {
            target
                .record_defaults
                .external_hidden_runtime_refs
                .extend(refs);
            target.record_defaults.external_hidden_runtime_refs.sort();
            target.record_defaults.external_hidden_runtime_refs.dedup();
        }
    }
}

/// Build the cross-module `TypeCtx` once for every module (entry included).
pub(crate) fn build_type_ctx<'a>(unit: &'a LoweredUnit, hybrid: Option<HybridPlan>) -> TypeCtx<'a> {
    let mut modules = std::collections::BTreeMap::new();
    let mut has_method_declarations = false;
    let stable_nominal_identity = unit.language_version >= topaz_syntax::LangVersion::V5_20;
    for module in &unit.modules {
        let BuiltModuleTypeContext {
            context,
            has_method_declarations: module_has_method_declarations,
            external_hidden_runtime_refs,
        } = build_module_type_context(module, stable_nominal_identity, &modules);
        has_method_declarations |= module_has_method_declarations;
        merge_external_hidden_runtime_refs(&mut modules, external_hidden_runtime_refs);
        modules.insert(module.identity.clone(), context);
    }
    TypeCtx {
        has_method_declarations,
        modules,
        hybrid,
        closure_factories: RefCell::new(String::new()),
    }
}

/// The `TypeAlias` a statement declares, unwrapping a single `export` wrapper
/// (mirrors the checker's per-frame alias hoist). `None` for any other kind.
pub(crate) fn stmt_type_alias(stmt: &Stmt) -> Option<&TypeAlias> {
    match &stmt.kind {
        StmtKind::TypeAlias(a) => Some(a),
        StmtKind::Export(inner) => match &inner.kind {
            StmtKind::TypeAlias(a) => Some(a),
            _ => None,
        },
        _ => None,
    }
}

/// Collect every `type` NAME declared block-locally anywhere under `stmt`'s
/// Children (not `stmt` itself) — the complete-AST poison walk. Exhaustive
/// over every block-bearing construct so a nested alias is never missed.
pub(crate) fn walk_stmt_children_for_aliases<'a>(
    stmt: &Stmt,
    src: &'a LoweredText,
    out: &mut HashSet<&'a str>,
) {
    match &stmt.kind {
        StmtKind::Import(_)
        | StmtKind::TypeAlias(_)
        | StmtKind::Enum(_)
        | StmtKind::Record(_)
        | StmtKind::Newtype(_)
        // §4 (v5.4) a protocol declares method signatures (empty bodies) — no
        // block-local `type` to poison.
        | StmtKind::Protocol(_)
        | StmtKind::Continue { .. } => {}
        // A `break <value>` value can contain a block-local `type`.
        StmtKind::Break { value, .. } => {
            if let Some(e) = value {
                walk_expr_for_aliases(e, src, out);
            }
        }
        StmtKind::Export(inner) => walk_stmt_children_for_aliases(inner, src, out),
        StmtKind::Function(decl) => walk_block_for_aliases(&decl.body, src, out),
        // §4 (v5.4) impl method bodies may declare block-local type aliases.
        StmtKind::Impl(decl) => {
            for m in &decl.methods {
                walk_block_for_aliases(&m.decl.body, src, out);
            }
        }
        StmtKind::Let { value, .. } | StmtKind::Const { value, .. } | StmtKind::Expr(value) => {
            walk_expr_for_aliases(value, src, out)
        }
        StmtKind::Using { value, body, .. } => {
            walk_expr_for_aliases(value, src, out);
            walk_block_for_aliases(body, src, out);
        }
        StmtKind::Assign { target, value, .. } => {
            walk_expr_for_aliases(target, src, out);
            walk_expr_for_aliases(value, src, out);
        }
        StmtKind::Return(e) => {
            if let Some(e) = e {
                walk_expr_for_aliases(e, src, out);
            }
        }
        StmtKind::Defer(e) => walk_expr_for_aliases(e, src, out),
        StmtKind::While { cond, body } => {
            walk_expr_for_aliases(cond, src, out);
            walk_block_for_aliases(body, src, out);
        }
    }
}

/// Walk a block: collect any `type` it declares (it IS nested), then descend.
pub(crate) fn walk_block_for_aliases<'a>(
    block: &Block,
    src: &'a LoweredText,
    out: &mut HashSet<&'a str>,
) {
    for stmt in &block.stmts {
        if let Some(a) = stmt_type_alias(stmt) {
            out.insert(text(src, a.name.span));
        }
        walk_stmt_children_for_aliases(stmt, src, out);
    }
    if let Some(tail) = &block.tail {
        walk_expr_for_aliases(tail, src, out);
    }
}

/// Descend an expression into every block-bearing position. EXHAUSTIVE over
/// `ExprKind` (no `_` arm) — and unlike `expr_has_bare_return`, it DOES descend
/// into Lambda bodies and Concurrent arms, since a block-local `type` there
/// still lexically shadows.
pub(crate) fn walk_expr_for_aliases<'a>(
    expr: &Expr,
    src: &'a LoweredText,
    out: &mut HashSet<&'a str>,
) {
    match &expr.kind {
        ExprKind::Int
        | ExprKind::Float
        | ExprKind::Duration(_)
        | ExprKind::Bool(_)
        | ExprKind::Null
        | ExprKind::Unit
        | ExprKind::Ident
        | ExprKind::Placeholder => {}
        ExprKind::String(lit) => {
            for p in &lit.parts {
                if let StringPart::Interpolation(e) = p {
                    walk_expr_for_aliases(e, src, out);
                }
            }
        }
        ExprKind::Paren(e) | ExprKind::Try(e) | ExprKind::Unary { operand: e, .. } => {
            walk_expr_for_aliases(e, src, out)
        }
        ExprKind::Block(b) => walk_block_for_aliases(b, src, out),
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            walk_expr_for_aliases(cond, src, out);
            walk_block_for_aliases(then_block, src, out);
            if let Some(e) = else_branch {
                walk_expr_for_aliases(e, src, out);
            }
        }
        ExprKind::Match { scrutinee, cases } => {
            walk_expr_for_aliases(scrutinee, src, out);
            for c in cases {
                if let Some(g) = &c.guard {
                    walk_expr_for_aliases(g, src, out);
                }
                match &c.body {
                    CaseArmBody::Return { value, .. } => {
                        if let Some(e) = value {
                            walk_expr_for_aliases(e, src, out);
                        }
                    }
                    CaseArmBody::Expr(e) => walk_expr_for_aliases(e, src, out),
                }
            }
        }
        ExprKind::For { iter, body, .. } => {
            walk_expr_for_aliases(iter, src, out);
            walk_block_for_aliases(body, src, out);
        }
        // A `loop` body may declare a block-local `type`.
        ExprKind::Loop { body, .. } => walk_block_for_aliases(body, src, out),
        ExprKind::Concurrent {
            timeout,
            arms,
            else_block,
        } => {
            if let Some(t) = timeout {
                walk_expr_for_aliases(t, src, out);
            }
            for arm in arms {
                walk_expr_for_aliases(&arm.value, src, out);
            }
            if let Some(b) = else_block {
                walk_block_for_aliases(b, src, out);
            }
        }
        ExprKind::Call { callee, args, .. } => {
            walk_expr_for_aliases(callee, src, out);
            for a in args {
                match a {
                    CallArg::Positional(e) | CallArg::Spread(e) => {
                        walk_expr_for_aliases(e, src, out)
                    }
                    CallArg::Named { value, .. } => walk_expr_for_aliases(value, src, out),
                }
            }
        }
        ExprKind::Member { object, .. } | ExprKind::OptionalAccess { object, .. } => {
            walk_expr_for_aliases(object, src, out)
        }
        ExprKind::Index { object, index } => {
            walk_expr_for_aliases(object, src, out);
            walk_expr_for_aliases(index, src, out);
        }
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Compose { lhs, rhs } => {
            walk_expr_for_aliases(lhs, src, out);
            walk_expr_for_aliases(rhs, src, out);
        }
        ExprKind::Range { lo, hi, step, .. } => {
            walk_expr_for_aliases(lo, src, out);
            walk_expr_for_aliases(hi, src, out);
            if let Some(s) = step {
                walk_expr_for_aliases(s, src, out);
            }
        }
        ExprKind::Pipe { lhs, rhs } => {
            walk_expr_for_aliases(lhs, src, out);
            if let PipeRhs::Expr(s) = rhs {
                walk_expr_for_aliases(s, src, out);
            }
        }
        ExprKind::Lambda { body, .. } => walk_expr_for_aliases(body, src, out),
        ExprKind::RecordLiteral { fields } => {
            for f in fields {
                walk_expr_for_aliases(&f.value, src, out);
            }
        }
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            walk_expr_for_aliases(base, src, out);
            if let Some(spread) = spread {
                walk_expr_for_aliases(spread, src, out);
            }
            for f in fields {
                walk_expr_for_aliases(&f.value, src, out);
            }
        }
        ExprKind::Array(els) => {
            for el in els {
                match el {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => {
                        walk_expr_for_aliases(e, src, out)
                    }
                }
            }
        }
        ExprKind::SetLiteral(els) => {
            for e in els {
                walk_expr_for_aliases(e, src, out);
            }
        }
        ExprKind::MapLiteral(entries) => {
            for (k, v) in entries {
                walk_expr_for_aliases(k, src, out);
                walk_expr_for_aliases(v, src, out);
            }
        }
        ExprKind::Comprehension { clauses, body, .. } => {
            for clause in clauses {
                match clause {
                    CompClause::For { iter, .. } => walk_expr_for_aliases(iter, src, out),
                    CompClause::If(cond) => walk_expr_for_aliases(cond, src, out),
                }
            }
            match body {
                CompBody::Elem(e) => walk_expr_for_aliases(e, src, out),
                CompBody::Entry { key, value } => {
                    walk_expr_for_aliases(key, src, out);
                    walk_expr_for_aliases(value, src, out);
                }
            }
        }
    }
}

/// A Rust BOOLEAN expression testing whether the value at `access` (an
/// expression of type `&Value`) conforms to `ty`, mirroring the interpreter's
/// `type_matches` — or `None` for any type the emitter cannot decide locally.
///
/// Handled: the scalars (`int`/`float`/`string`/`bool`) + `()` (a single
/// `matches!`), a UNION of handled members (a `||` disjunction), the structural
/// containers `Option<T>` / `Result<T, E>` / `Array<T>` / `Set<T>` / `Map<K, V>`
/// (each recursively payload-/key-/value-/element-checked), a RECORD type
/// `{ f: T, … }` (an exact field set, each field recursively checked), a FUNCTION
/// type `(P…) -> R` (SHAPE-only via `callable_shape_matches` — arity range, not the
/// erased P/R), and a resolvable QUALIFIED type `m.Id` — exactly the interpreter's
/// `type_matches` arms. A LITERAL type, a generic type param that is also a
/// resolvable/poisoned alias miss, a poisoned / self-recursive alias, a container
/// or function type with an UNDECIDABLE inner type, or an undecidable qualified type
/// returns `None` — the caller refuses it (TPZ6001). A TOP-LEVEL alias does NOT
/// return `None`: monomorphic aliases expand directly; generic aliases first
/// substitute their type arguments into the definition body, then reuse this same
/// test generator.
///
/// `counter` mints fresh `__tt{K}` / `__v{K}` / `__m{K}` temporaries so nested
/// containers never collide. Every form is emitted at its natural Rust
/// precedence; unions use a block to bind their shared input exactly once.
pub(crate) fn substitute_alias_type_args(
    body: &Type,
    params: &[Ident],
    args: &[Type],
    src: &LoweredText,
) -> Option<Type> {
    if params.len() != args.len() {
        return None;
    }
    match &body.kind {
        TypeKind::Named {
            name,
            args: named_args,
        } => {
            let n = text(src, name.span);
            if let Some(i) = params.iter().position(|p| text(src, p.span) == n) {
                if !named_args.is_empty() {
                    return None;
                }
                return Some(args[i].clone());
            }
            Some(Type {
                kind: TypeKind::Named {
                    name: *name,
                    args: named_args
                        .iter()
                        .map(|arg| substitute_alias_type_args(arg, params, args, src))
                        .collect::<Option<Vec<_>>>()?,
                },
                span: body.span,
            })
        }
        TypeKind::Qualified {
            ns,
            name,
            args: named_args,
        } => Some(Type {
            kind: TypeKind::Qualified {
                ns: *ns,
                name: *name,
                args: named_args
                    .iter()
                    .map(|arg| substitute_alias_type_args(arg, params, args, src))
                    .collect::<Option<Vec<_>>>()?,
            },
            span: body.span,
        }),
        TypeKind::Record(fields) => Some(Type {
            kind: TypeKind::Record(
                fields
                    .iter()
                    .map(|field| {
                        Some(FieldType {
                            name: field.name,
                            ty: substitute_alias_type_args(&field.ty, params, args, src)?,
                            span: field.span,
                        })
                    })
                    .collect::<Option<Vec<_>>>()?,
            ),
            span: body.span,
        }),
        TypeKind::Function {
            params: fn_params,
            ret,
        } => Some(Type {
            kind: TypeKind::Function {
                params: fn_params
                    .iter()
                    .map(|param| {
                        Some(FunctionTypeParam {
                            ty: substitute_alias_type_args(&param.ty, params, args, src)?,
                            variadic: param.variadic,
                        })
                    })
                    .collect::<Option<Vec<_>>>()?,
                ret: Box::new(substitute_alias_type_args(ret, params, args, src)?),
            },
            span: body.span,
        }),
        TypeKind::Union(members) => Some(Type {
            kind: TypeKind::Union(
                members
                    .iter()
                    .map(|member| substitute_alias_type_args(member, params, args, src))
                    .collect::<Option<Vec<_>>>()?,
            ),
            span: body.span,
        }),
        TypeKind::Literal | TypeKind::Unit => Some(body.clone()),
    }
}
