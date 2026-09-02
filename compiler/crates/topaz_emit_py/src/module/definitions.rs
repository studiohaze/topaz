use crate::*;

pub(super) fn exported_inner(stmt: &Stmt) -> &Stmt {
    match &stmt.kind {
        StmtKind::Export(inner) => inner.as_ref(),
        _ => stmt,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RecordShape {
    pub(super) fields: Vec<String>,
}

pub(super) fn ensure_record_shape(shapes: &mut Vec<RecordShape>, shape: RecordShape) {
    if !shapes.contains(&shape) {
        shapes.push(shape);
    }
}

pub(super) fn map_entry_record_shape() -> RecordShape {
    RecordShape {
        fields: vec!["key".to_string(), "value".to_string()],
    }
}

#[derive(Debug, Clone)]
pub(super) struct NominalRecordDef<'a> {
    pub(super) source_name: String,
    pub(super) py_class_name: String,
    pub(super) type_params: Vec<String>,
    pub(super) fields: Vec<NominalRecordField<'a>>,
    pub(super) declaration_identity: Option<String>,
    pub(super) method_identity: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct NominalRecordField<'a> {
    pub(super) source_name: String,
    pub(super) ty: &'a Type,
    pub(super) default: Option<NominalRecordDefault<'a>>,
}

#[derive(Debug, Clone)]
pub(super) struct NominalRecordDefault<'a> {
    pub(super) expr: &'a Expr,
    pub(super) const_py: Option<String>,
    pub(super) defining_py: Option<String>,
    pub(super) imported_py: Option<String>,
    pub(super) helper_py_names: Option<Rc<NominalRecordDefaultHelperNames>>,
    pub(super) callable_metadata: Option<Rc<NominalRecordDefaultCallableMetadata>>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct NominalRecordDefaultCallableMetadata {
    pub(super) cooperative_callback_target: Option<(String, bool)>,
    pub(super) callable_params: Option<Vec<FunctionParamInfo>>,
    pub(super) record_descendants: RecordDescendantMetadata,
}

#[derive(Debug)]
pub(super) struct NominalRecordDefaultHelperNames {
    pub(super) direct: String,
    pub(super) cooperative: String,
}

pub(super) struct NominalRecordDefaultHelper<'a> {
    pub(super) expr: &'a Expr,
    pub(super) names: Rc<NominalRecordDefaultHelperNames>,
}

#[derive(Debug, Clone)]
pub(super) struct NewtypeDef {
    pub(super) source_name: String,
    pub(super) type_params: Vec<String>,
    pub(super) base: Type,
    pub(super) declaration_identity: Option<String>,
    pub(super) method_identity: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct TypeAliasDef {
    pub(super) type_params: Vec<String>,
    pub(super) body: Rc<Type>,
}

#[derive(Debug, Clone)]
pub(super) struct EnumDef {
    pub(super) source_name: String,
    pub(super) type_params: Vec<String>,
    pub(super) variants: BTreeMap<String, EnumVariantDef>,
    pub(super) declaration_identity: Option<String>,
    pub(super) method_identity: Option<String>,
}

pub(super) struct ModuleDefinitions<'a> {
    pub(super) records: Rc<BTreeMap<String, NominalRecordDef<'a>>>,
    pub(super) newtypes: Rc<BTreeMap<String, NewtypeDef>>,
    pub(super) enums: Rc<BTreeMap<String, EnumDef>>,
    pub(super) schema_aliases: Rc<BTreeMap<String, TypeAliasDef>>,
    pub(super) schema_imports: Rc<JsonSchemaImportScope>,
    pub(super) protocol_names: Rc<[String]>,
    pub(super) functions: Rc<[&'a FunctionDecl]>,
    pub(super) imports: Rc<[&'a ImportItem]>,
    pub(super) exported_statements: Rc<[&'a Stmt]>,
    pub(super) receiver_impls: Rc<[&'a ImplDecl]>,
    pub(super) protocol_impls: Rc<[&'a ImplDecl]>,
}
pub(super) type ModuleDefinitionCatalog<'a> = BTreeMap<String, Rc<ModuleDefinitions<'a>>>;

#[derive(Debug, Clone, Default)]
pub(super) struct JsonSchemaImportScope {
    pub(super) namespaces: BTreeMap<String, String>,
    pub(super) selected: BTreeMap<String, (String, String)>,
}

#[derive(Debug, Clone)]
pub(super) struct JsonSchemaModule<'a> {
    pub(super) records: Rc<BTreeMap<String, NominalRecordDef<'a>>>,
    pub(super) newtypes: Rc<BTreeMap<String, NewtypeDef>>,
    pub(super) enums: Rc<BTreeMap<String, EnumDef>>,
    pub(super) aliases: Rc<BTreeMap<String, TypeAliasDef>>,
    pub(super) imports: Rc<JsonSchemaImportScope>,
}

pub(super) type JsonSchemaModules<'a> = BTreeMap<String, JsonSchemaModule<'a>>;

#[derive(Debug, Clone)]
pub(super) struct EnumVariantDef {
    pub(super) arity: usize,
    pub(super) variant_index: usize,
    pub(super) payload: Vec<Type>,
}

pub(super) fn collect_record_shapes(unit: &ResolveOutput) -> Vec<RecordShape> {
    let mut shapes = Vec::new();
    for module in &unit.modules {
        for stmt in &module.program.items {
            collect_record_shapes_stmt(stmt, &unit.map, &mut shapes);
        }
    }
    shapes
}

pub(super) fn collect_record_shapes_stmt(
    stmt: &Stmt,
    map: &SourceMap,
    shapes: &mut Vec<RecordShape>,
) {
    match &stmt.kind {
        StmtKind::Export(inner) => collect_record_shapes_stmt(inner, map, shapes),
        StmtKind::Function(decl) => collect_record_shapes_block(&decl.body, map, shapes),
        StmtKind::Let { value, .. } | StmtKind::Const { value, .. } => {
            collect_record_shapes_expr(value, map, shapes);
        }
        StmtKind::Assign { target, value, .. } => {
            collect_record_shapes_expr(target, map, shapes);
            collect_record_shapes_expr(value, map, shapes);
        }
        StmtKind::Defer(value) => {
            collect_record_shapes_expr(value, map, shapes);
        }
        StmtKind::Return(Some(value))
        | StmtKind::Break {
            value: Some(value), ..
        }
        | StmtKind::Expr(value) => {
            collect_record_shapes_expr(value, map, shapes);
        }
        StmtKind::While { cond, body } => {
            collect_record_shapes_expr(cond, map, shapes);
            collect_record_shapes_block(body, map, shapes);
        }
        StmtKind::Using { value, body, .. } => {
            collect_record_shapes_expr(value, map, shapes);
            collect_record_shapes_block(body, map, shapes);
        }
        StmtKind::Record(decl) => {
            for field in &decl.fields {
                if let Some(default) = &field.default {
                    collect_record_shapes_expr(default, map, shapes);
                }
            }
        }
        StmtKind::Return(None)
        | StmtKind::Import(_)
        | StmtKind::TypeAlias(_)
        | StmtKind::Enum(_)
        | StmtKind::Newtype(_)
        | StmtKind::Impl(_)
        | StmtKind::Protocol(_)
        | StmtKind::Break { value: None, .. }
        | StmtKind::Continue { .. } => {}
    }
}

pub(super) fn collect_record_shapes_block(
    block: &Block,
    map: &SourceMap,
    shapes: &mut Vec<RecordShape>,
) {
    for stmt in &block.stmts {
        collect_record_shapes_stmt(stmt, map, shapes);
    }
    if let Some(tail) = block.tail.as_deref() {
        collect_record_shapes_expr(tail, map, shapes);
    }
}

pub(super) fn collect_record_shapes_expr(
    expr: &Expr,
    map: &SourceMap,
    shapes: &mut Vec<RecordShape>,
) {
    // Keep this walker as a superset of every expression position the emitter can lower;
    // otherwise a newly supported record literal position can compile to an undefined class.
    match &expr.kind {
        ExprKind::Paren(inner) | ExprKind::Try(inner) => {
            collect_record_shapes_expr(inner, map, shapes)
        }
        ExprKind::Block(block) => collect_record_shapes_block(block, map, shapes),
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            collect_record_shapes_expr(cond, map, shapes);
            collect_record_shapes_block(then_block, map, shapes);
            if let Some(branch) = else_branch {
                collect_record_shapes_expr(branch, map, shapes);
            }
        }
        ExprKind::Match { scrutinee, cases } => {
            collect_record_shapes_expr(scrutinee, map, shapes);
            for case in cases {
                if let Some(guard) = &case.guard {
                    collect_record_shapes_expr(guard, map, shapes);
                }
                match &case.body {
                    CaseArmBody::Expr(body) => collect_record_shapes_expr(body, map, shapes),
                    CaseArmBody::Return {
                        value: Some(body), ..
                    } => collect_record_shapes_expr(body, map, shapes),
                    CaseArmBody::Return { value: None, .. } => {}
                }
            }
        }
        ExprKind::For { iter, body, .. } => {
            collect_record_shapes_expr(iter, map, shapes);
            collect_record_shapes_block(body, map, shapes);
        }
        ExprKind::Call { callee, args, .. } => {
            collect_record_shapes_expr(callee, map, shapes);
            for arg in args {
                match arg {
                    CallArg::Positional(expr) | CallArg::Spread(expr) => {
                        collect_record_shapes_expr(expr, map, shapes);
                    }
                    CallArg::Named { value, .. } => collect_record_shapes_expr(value, map, shapes),
                }
            }
        }
        ExprKind::Member { object, field } | ExprKind::OptionalAccess { object, field } => {
            collect_record_shapes_expr(object, map, shapes);
            if text_in_map(map, field.span) == "entries" {
                ensure_record_shape(shapes, map_entry_record_shape());
            }
        }
        ExprKind::Index { object, index } => {
            collect_record_shapes_expr(object, map, shapes);
            collect_record_shapes_expr(index, map, shapes);
        }
        ExprKind::Unary { operand, .. } => collect_record_shapes_expr(operand, map, shapes),
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Compose { lhs, rhs } => {
            collect_record_shapes_expr(lhs, map, shapes);
            collect_record_shapes_expr(rhs, map, shapes);
        }
        ExprKind::Array(elements) => {
            for element in elements {
                let expr = match element {
                    ArrayElement::Expr(expr) | ArrayElement::Spread(expr) => expr,
                };
                collect_record_shapes_expr(expr, map, shapes);
            }
        }
        ExprKind::RecordLiteral { fields } => {
            let shape = record_shape(fields, map);
            ensure_record_shape(shapes, shape);
            for field in fields {
                collect_record_shapes_expr(&field.value, map, shapes);
            }
        }
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            collect_record_shapes_expr(base, map, shapes);
            if let Some(spread) = spread {
                collect_record_shapes_expr(spread, map, shapes);
            }
            for field in fields {
                collect_record_shapes_expr(&field.value, map, shapes);
            }
        }
        ExprKind::SetLiteral(elements) => {
            for element in elements {
                collect_record_shapes_expr(element, map, shapes);
            }
        }
        ExprKind::MapLiteral(entries) => {
            for (key, value) in entries {
                collect_record_shapes_expr(key, map, shapes);
                collect_record_shapes_expr(value, map, shapes);
            }
        }
        ExprKind::Loop { body, .. } => collect_record_shapes_block(body, map, shapes),
        ExprKind::Comprehension { clauses, body, .. } => {
            for clause in clauses {
                match clause {
                    CompClause::For { iter, .. } => collect_record_shapes_expr(iter, map, shapes),
                    CompClause::If(cond) => collect_record_shapes_expr(cond, map, shapes),
                }
            }
            match body.as_ref() {
                CompBody::Elem(expr) => collect_record_shapes_expr(expr, map, shapes),
                CompBody::Entry { key, value } => {
                    collect_record_shapes_expr(key, map, shapes);
                    collect_record_shapes_expr(value, map, shapes);
                }
            }
        }
        ExprKind::Concurrent {
            timeout,
            arms,
            else_block,
        } => {
            if let Some(timeout) = timeout {
                collect_record_shapes_expr(timeout, map, shapes);
            }
            if timeout.is_none()
                || timeout
                    .as_deref()
                    .is_some_and(concurrent_timeout_can_record_shape)
            {
                let shape = concurrent_record_shape(arms, map);
                ensure_record_shape(shapes, shape);
            }
            for arm in arms {
                collect_record_shapes_expr(&arm.value, map, shapes);
            }
            if let Some(block) = else_block {
                collect_record_shapes_block(block, map, shapes);
            }
        }
        ExprKind::String(lit) => {
            for part in &lit.parts {
                if let StringPart::Interpolation(expr) = part {
                    collect_record_shapes_expr(expr, map, shapes);
                }
            }
        }
        ExprKind::Range { lo, hi, step, .. } => {
            collect_record_shapes_expr(lo, map, shapes);
            collect_record_shapes_expr(hi, map, shapes);
            if let Some(step) = step {
                collect_record_shapes_expr(step, map, shapes);
            }
        }
        ExprKind::Pipe { lhs, rhs } => {
            collect_record_shapes_expr(lhs, map, shapes);
            match rhs.as_ref() {
                PipeRhs::Field(field) => {
                    if text_in_map(map, field.span) == "entries" {
                        ensure_record_shape(shapes, map_entry_record_shape());
                    }
                }
                PipeRhs::Expr(stage) => collect_record_shapes_expr(stage, map, shapes),
            }
        }
        ExprKind::Float
        | ExprKind::Duration(_)
        | ExprKind::Bool(_)
        | ExprKind::Int
        | ExprKind::Null
        | ExprKind::Unit
        | ExprKind::Ident
        | ExprKind::Placeholder => {}
        ExprKind::Lambda { body, .. } => collect_record_shapes_expr(body, map, shapes),
    }
}

pub(super) struct RecordDefaultScopeState {
    pub(super) remaining_bound_name_counts: BTreeMap<String, usize>,
    pub(super) later_bound_names: BTreeSet<String>,
    pub(super) prior_mutable_names: BTreeSet<String>,
}

impl RecordDefaultScopeState {
    pub(super) fn new(binding_facts: &RecordDefaultRuntimeBindingFacts) -> Self {
        let remaining_bound_name_counts = binding_facts.counts.clone();
        Self {
            later_bound_names: remaining_bound_name_counts.keys().cloned().collect(),
            remaining_bound_name_counts,
            prior_mutable_names: BTreeSet::new(),
        }
    }

    pub(super) fn enter(&mut self, current_bound_names: &[String]) {
        for name in current_bound_names {
            let count = self
                .remaining_bound_name_counts
                .get_mut(name)
                .expect("current statement binding remains counted");
            *count -= 1;
            if *count == 0 {
                self.remaining_bound_name_counts.remove(name);
                self.later_bound_names.remove(name);
            }
        }
    }

    pub(super) fn leave(&mut self, mutable_bound_names: &[String]) {
        self.prior_mutable_names
            .extend(mutable_bound_names.iter().cloned());
    }
}

pub(super) fn nominal_record_definition<'a>(
    decl: &'a RecordDecl,
    map: &'a SourceMap,
    module_identity: Option<&str>,
    default_const_values: &[(String, Value)],
    default_self_runtime_values: &BTreeMap<String, Vec<(String, String)>>,
    scope: &RecordDefaultScopeState,
) -> (String, NominalRecordDef<'a>) {
    let source_name = text_in_map(map, decl.name.span).to_string();
    let self_runtime_values = default_self_runtime_values
        .get(&source_name)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let fields = decl
        .fields
        .iter()
        .map(|field| NominalRecordField {
            source_name: text_in_map(map, field.name.span).to_string(),
            ty: &field.ty,
            default: field.default.as_ref().map(|expr| {
                let const_py =
                    emit_nominal_record_const_default_expr(expr, map, default_const_values).ok();
                let defining_py = const_py.clone().or_else(|| {
                    emit_nominal_record_runtime_reference_default_expr(
                        expr,
                        map,
                        &[],
                        self_runtime_values,
                        &[],
                    )
                });
                let field_name = text_in_map(map, field.name.span);
                let mut later_references = BTreeSet::new();
                collect_expr_nested_function_body_references(
                    expr,
                    map,
                    &scope.later_bound_names,
                    &mut NestedForwardScope::default(),
                    &mut later_references,
                );
                let mut mutable_references = BTreeSet::new();
                collect_expr_nested_function_body_references(
                    expr,
                    map,
                    &scope.prior_mutable_names,
                    &mut NestedForwardScope::default(),
                    &mut mutable_references,
                );
                let helper_py_names = if defining_py.is_none()
                    && later_references.is_empty()
                    && (!mutable_references.is_empty()
                        || nominal_record_default_requires_execution_thunk(expr))
                {
                    let helper = nominal_record_default_helper_name(
                        module_identity,
                        &source_name,
                        field_name,
                    );
                    Some(Rc::new(NominalRecordDefaultHelperNames {
                        cooperative: cooperative_function_py_name(&helper),
                        direct: helper,
                    }))
                } else {
                    None
                };
                NominalRecordDefault {
                    expr,
                    const_py,
                    defining_py,
                    imported_py: None,
                    helper_py_names,
                    callable_metadata: None,
                }
            }),
        })
        .collect();
    let definition = NominalRecordDef {
        py_class_name: nominal_record_class_name(module_identity, &source_name),
        source_name: source_name.clone(),
        type_params: decl
            .type_params
            .iter()
            .map(|param| text_in_map(map, param.span).to_string())
            .collect(),
        fields,
        declaration_identity: None,
        method_identity: None,
    };
    (source_name, definition)
}

pub(super) fn newtype_definition(decl: &NewtypeDecl, map: &SourceMap) -> (String, NewtypeDef) {
    let source_name = text_in_map(map, decl.name.span).to_string();
    let definition = NewtypeDef {
        source_name: source_name.clone(),
        type_params: decl
            .type_params
            .iter()
            .map(|param| text_in_map(map, param.span).to_string())
            .collect(),
        base: decl.base.clone(),
        declaration_identity: None,
        method_identity: None,
    };
    (source_name, definition)
}

pub(super) fn enum_definition(decl: &EnumDecl, map: &SourceMap) -> (String, EnumDef) {
    let source_name = text_in_map(map, decl.name.span).to_string();
    let variants = decl
        .variants
        .iter()
        .enumerate()
        .filter_map(|(variant_index, variant)| {
            let variant_name = text_in_map(map, variant.name.span).to_string();
            if matches!(variant_name.as_str(), "None" | "Some" | "Ok" | "Err") {
                return None;
            }
            Some((
                variant_name,
                EnumVariantDef {
                    arity: variant.payload.as_ref().map_or(0, Vec::len),
                    variant_index,
                    payload: variant.payload.clone().unwrap_or_default(),
                },
            ))
        })
        .collect();
    let definition = EnumDef {
        source_name: source_name.clone(),
        type_params: decl
            .type_params
            .iter()
            .map(|param| text_in_map(map, param.span).to_string())
            .collect(),
        variants,
        declaration_identity: None,
        method_identity: None,
    };
    (source_name, definition)
}

pub(super) fn type_alias_definition(alias: &TypeAlias, map: &SourceMap) -> (String, TypeAliasDef) {
    let source_name = text_in_map(map, alias.name.span).to_string();
    let definition = TypeAliasDef {
        type_params: alias
            .type_params
            .iter()
            .map(|param| text_in_map(map, param.span).to_string())
            .collect(),
        body: alias.ty.clone(),
    };
    (source_name, definition)
}

pub(super) fn record_json_schema_import(
    import: &ImportItem,
    map: &SourceMap,
    imports: &mut JsonSchemaImportScope,
) {
    let target = import_identity_from_map(import, map);
    match &import.kind {
        ImportKind::Namespace { alias } => {
            let fallback = import.path.segments.last().expect("non-empty import path");
            let local = alias.as_ref().unwrap_or(fallback);
            imports
                .namespaces
                .insert(text_in_map(map, local.span).to_string(), target);
        }
        ImportKind::Selected { specs } => {
            for spec in specs {
                let source = text_in_map(map, spec.name.span).to_string();
                let local = spec
                    .alias
                    .as_ref()
                    .map_or(source.as_str(), |alias| text_in_map(map, alias.span));
                imports
                    .selected
                    .insert(local.to_string(), (target.clone(), source));
            }
        }
    }
}

pub(super) fn apply_receiver_method_identities(
    identities: BTreeMap<String, String>,
    records: &mut BTreeMap<String, NominalRecordDef<'_>>,
    newtypes: &mut BTreeMap<String, NewtypeDef>,
    enums: &mut BTreeMap<String, EnumDef>,
) {
    for (nominal, identity) in identities {
        if let Some(record) = records.get_mut(&nominal) {
            record.method_identity = Some(identity.clone());
        }
        if let Some(newtype) = newtypes.get_mut(&nominal) {
            newtype.method_identity = Some(identity.clone());
        }
        if let Some(enm) = enums.get_mut(&nominal) {
            enm.method_identity = Some(identity);
        }
    }
}

pub(super) fn collect_module_definitions<'a>(
    module: &'a topaz_resolve::ResolvedModule,
    map: &'a SourceMap,
    version: topaz_syntax::LangVersion,
    module_identity: Option<&str>,
    default_const_values: &[(String, Value)],
    default_self_runtime_values: &BTreeMap<String, Vec<(String, String)>>,
    runtime_bindings: &RecordDefaultRuntimeBindingFacts,
) -> ModuleDefinitions<'a> {
    let items = &module.program.items;
    let mut records = std::collections::BTreeMap::new();
    let mut newtypes = BTreeMap::new();
    let mut enums = BTreeMap::new();
    let mut schema_aliases = BTreeMap::new();
    let mut schema_imports = JsonSchemaImportScope::default();
    let mut protocol_names = Vec::new();
    let mut functions = Vec::new();
    let mut imports = Vec::new();
    let mut exported_statements = Vec::new();
    let mut receiver_impls = Vec::new();
    let mut protocol_impls = Vec::new();
    let mut receiver_method_identities = BTreeMap::new();
    let mut record_default_scope = RecordDefaultScopeState::new(runtime_bindings);
    for (stmt, statement_bindings) in items.iter().zip(&runtime_bindings.statements) {
        record_default_scope.enter(&statement_bindings.current);
        if matches!(&stmt.kind, StmtKind::Export(_)) {
            exported_statements.push(stmt);
        }
        let inner = exported_inner(stmt);
        if let StmtKind::Record(decl) = &inner.kind {
            let (name, definition) = nominal_record_definition(
                decl,
                map,
                module_identity,
                default_const_values,
                default_self_runtime_values,
                &record_default_scope,
            );
            records.insert(name, definition);
        }
        if let StmtKind::Newtype(decl) = &inner.kind {
            let (name, definition) = newtype_definition(decl, map);
            newtypes.insert(name, definition);
        }
        if let StmtKind::Enum(decl) = &inner.kind {
            let (name, definition) = enum_definition(decl, map);
            enums.insert(name, definition);
        }
        if let StmtKind::TypeAlias(alias) = &inner.kind {
            let (name, definition) = type_alias_definition(alias, map);
            schema_aliases.insert(name, definition);
        }
        if let StmtKind::Protocol(decl) = &inner.kind {
            protocol_names.push(text_in_map(map, decl.name.span).to_string());
        }
        if let StmtKind::Function(decl) = &inner.kind {
            functions.push(decl);
        }
        if let StmtKind::Impl(decl) = &inner.kind {
            if decl.target.is_some() {
                protocol_impls.push(decl);
            } else {
                receiver_impls.push(decl);
                let nominal = text_in_map(map, decl.name.span).to_string();
                receiver_method_identities.insert(
                    nominal.clone(),
                    python_receiver_method_identity(module_identity.unwrap_or(""), &nominal),
                );
            }
        }
        if let StmtKind::Import(import) = &stmt.kind {
            imports.push(import);
            record_json_schema_import(import, map, &mut schema_imports);
        }
        record_default_scope.leave(&statement_bindings.mutable);
    }
    let runtime_module = module_identity.unwrap_or("");
    stamp_nominal_declaration_identities(
        version,
        runtime_module,
        &mut records,
        &mut newtypes,
        &mut enums,
    );
    apply_receiver_method_identities(
        receiver_method_identities,
        &mut records,
        &mut newtypes,
        &mut enums,
    );
    ModuleDefinitions {
        records: Rc::new(records),
        newtypes: Rc::new(newtypes),
        enums: Rc::new(enums),
        schema_aliases: Rc::new(schema_aliases),
        schema_imports: Rc::new(schema_imports),
        protocol_names: protocol_names.into(),
        functions: functions.into(),
        imports: imports.into(),
        exported_statements: exported_statements.into(),
        receiver_impls: receiver_impls.into(),
        protocol_impls: protocol_impls.into(),
    }
}

pub(super) fn collect_module_definition_catalog<'a>(
    unit: &'a ResolveOutput,
    record_default_const_catalog: &RecordDefaultConstCatalog,
    module_defaults: &ModuleDefaultInputCatalog,
) -> ModuleDefinitionCatalog<'a> {
    unit.modules
        .iter()
        .map(|module| {
            let const_values = record_default_const_catalog
                .get(&module.identity)
                .expect("nominal record module default consts");
            let module_facts = module_defaults
                .get(&module.identity)
                .expect("nominal record module default input facts");
            let runtime_identity = (!module.is_entry).then_some(module.identity.as_str());
            let definitions = collect_module_definitions(
                module,
                &unit.map,
                unit.language_version,
                runtime_identity,
                const_values,
                module_facts.self_runtime_values.as_ref(),
                module_facts.record_default_runtime_bindings.as_ref(),
            );
            (module.identity.clone(), Rc::new(definitions))
        })
        .collect()
}

pub(super) fn stamp_nominal_declaration_identities(
    version: topaz_syntax::LangVersion,
    module_identity: &str,
    records: &mut BTreeMap<String, NominalRecordDef<'_>>,
    newtypes: &mut BTreeMap<String, NewtypeDef>,
    enums: &mut BTreeMap<String, EnumDef>,
) {
    if version < topaz_syntax::LangVersion::V5_20 {
        return;
    }
    for (name, record) in records {
        record.declaration_identity = Some(python_receiver_method_identity(module_identity, name));
    }
    for (name, newtype) in newtypes {
        newtype.declaration_identity = Some(python_receiver_method_identity(module_identity, name));
    }
    for (name, enm) in enums {
        enm.declaration_identity = Some(python_receiver_method_identity(module_identity, name));
    }
}

pub(super) fn collect_json_schema_modules<'a>(
    unit: &'a ResolveOutput,
    definition_catalog: &ModuleDefinitionCatalog<'a>,
) -> JsonSchemaModules<'a> {
    unit.modules
        .iter()
        .map(|module| {
            let module_definitions = definition_catalog
                .get(&module.identity)
                .expect("schema module definitions");
            let records = module_definitions.records.clone();
            let newtypes = module_definitions.newtypes.clone();
            let enums = module_definitions.enums.clone();
            (
                module.identity.clone(),
                JsonSchemaModule {
                    records,
                    newtypes,
                    enums,
                    aliases: module_definitions.schema_aliases.clone(),
                    imports: module_definitions.schema_imports.clone(),
                },
            )
        })
        .collect()
}

pub(super) fn collect_all_nominal_record_defs<'a>(
    unit: &'a ResolveOutput,
    definition_catalog: &ModuleDefinitionCatalog<'a>,
) -> std::collections::BTreeMap<String, NominalRecordDef<'a>> {
    let mut records = std::collections::BTreeMap::new();
    for module in &unit.modules {
        let module_definitions = definition_catalog
            .get(&module.identity)
            .expect("all-nominal module definitions");
        for record in module_definitions.records.values().cloned() {
            records.insert(record.py_class_name.clone(), record);
        }
    }
    records
}
