use crate::*;

pub(super) fn register_module_functions(
    functions: &[&FunctionDecl],
    identity: Option<&str>,
    module_top_bound_names: &BTreeSet<String>,
    ctx: &mut Ctx<'_>,
) {
    for decl in functions {
        let source_name = ctx.text(decl.name.span).to_string();
        let py_name = match identity {
            Some(identity) => module_value_name(identity, &source_name),
            None => mangle(&source_name),
        };
        let info = function_info(decl, py_name, ctx, module_top_bound_names);
        ctx.register_function_info(&source_name, info);
    }
}

#[cfg(test)]
pub(super) fn module_top_bound_names_for_direct_tail(
    items: &[Stmt],
    map: &SourceMap,
) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for stmt in items {
        let inner = exported_inner(stmt);
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
                    names.insert(
                        alias
                            .as_ref()
                            .map_or(last, |alias| text_in_map(map, alias.span))
                            .to_string(),
                    );
                }
                ImportKind::Selected { specs } => {
                    for spec in specs {
                        let local = spec.alias.as_ref().map_or_else(
                            || text_in_map(map, spec.name.span),
                            |alias| text_in_map(map, alias.span),
                        );
                        names.insert(local.to_string());
                    }
                }
            },
            StmtKind::Function(decl) => {
                names.insert(text_in_map(map, decl.name.span).to_string());
            }
            StmtKind::TypeAlias(alias) => {
                names.insert(text_in_map(map, alias.name.span).to_string());
            }
            StmtKind::Enum(decl) => {
                names.insert(text_in_map(map, decl.name.span).to_string());
            }
            StmtKind::Record(decl) => {
                names.insert(text_in_map(map, decl.name.span).to_string());
            }
            StmtKind::Newtype(decl) => {
                names.insert(text_in_map(map, decl.name.span).to_string());
            }
            StmtKind::Const { name, .. } => {
                names.insert(text_in_map(map, name.span).to_string());
            }
            StmtKind::Let { pattern, .. } => {
                collect_pattern_binding_names(pattern, map, &mut names)
            }
            _ => {}
        }
    }
    names
}

pub(super) fn enrich_module_function_return_metadata(
    functions: &[&FunctionDecl],
    ctx: &mut Ctx<'_>,
) {
    loop {
        let mut changed = false;
        for decl in functions {
            let source_name = ctx.text(decl.name.span).to_string();
            changed |= ctx.enrich_function_return_metadata(&source_name, decl);
        }
        if !changed {
            break;
        }
    }
}

pub(super) fn enrich_nominal_record_default_callable_metadata(ctx: &mut Ctx<'_>) {
    let mut discovered = Vec::new();
    for (record_name, record) in ctx.records.iter() {
        for (field_index, field) in record.fields.iter().enumerate() {
            let Some(default) = &field.default else {
                continue;
            };
            discovered.push((
                record_name.clone(),
                field_index,
                ctx.nominal_record_default_callable_metadata(default.expr),
            ));
        }
    }
    let records = Rc::make_mut(&mut ctx.records);
    for (record_name, field_index, metadata) in discovered {
        let default = records
            .get_mut(&record_name)
            .and_then(|record| record.fields.get_mut(field_index))
            .and_then(|field| field.default.as_mut())
            .expect("record default remains available during metadata enrichment");
        default.callable_metadata = Some(Rc::new(metadata));
    }
}

pub(super) fn enrich_module_function_mutation_metadata(
    functions: &[&FunctionDecl],
    ctx: &mut Ctx<'_>,
) {
    loop {
        let discovered = functions
            .iter()
            .map(|decl| {
                (
                    ctx.text(decl.name.span).to_string(),
                    mutated_collection_parameter_indices(decl, ctx, true),
                )
            })
            .collect::<Vec<_>>();
        let infos = Rc::make_mut(&mut ctx.functions);
        let mut changed = false;
        for (name, params) in discovered {
            let Some(info) = infos.get_mut(&name) else {
                continue;
            };
            let previous_len = info.mutated_collection_params.len();
            info.mutated_collection_params.extend(params);
            changed |= info.mutated_collection_params.len() != previous_len;
        }
        if !changed {
            break;
        }
    }
}

pub(super) fn function_info(
    decl: &FunctionDecl,
    py_name: String,
    ctx: &Ctx<'_>,
    module_top_bound_names: &BTreeSet<String>,
) -> FunctionInfo {
    let params = function_param_info(decl, ctx);
    let cooperative_py_name = Some(cooperative_function_py_name(&py_name));
    let return_record_descendants = decl
        .return_type
        .as_ref()
        .map(|ty| record_descendant_catalog_from_type(ty, ctx))
        .unwrap_or_default();
    let direct_tail = decl.return_type.as_ref().map_or_else(
        || direct_tail_metadata(decl, ctx, module_top_bound_names),
        |_| DirectTailMetadata::default(),
    );
    FunctionInfo {
        py_name,
        cooperative_py_name,
        params,
        return_shape: decl.return_type.as_ref().map_or_else(
            || direct_tail.return_shape,
            |ty| receiver_shape_from_type(ty, ctx),
        ),
        return_wrapped_metadata: decl.return_type.as_ref().map_or_else(
            || {
                let mut metadata = WrappedValueMetadataCatalog::default();
                metadata.insert_root(
                    RecordWrapper::ResultOk,
                    WrappedValueMetadata {
                        receiver_shape: direct_tail.result_ok_shape,
                        callable_params: None,
                    },
                );
                metadata
            },
            |ty| wrapped_value_metadata_catalog_from_type(ty, ctx),
        ),
        return_record_descendants,
        mutated_collection_params: directly_mutated_collection_parameter_indices(decl, ctx),
        needs_host: true,
    }
}

pub(super) fn function_param_info(decl: &FunctionDecl, ctx: &Ctx<'_>) -> Vec<FunctionParamInfo> {
    decl.params
        .iter()
        .map(|param| {
            let source_name = ctx.text(param.name.span).to_string();
            FunctionParamInfo {
                py_name: mangle(&source_name),
                source_name,
                has_default: param.default.is_some(),
                variadic: param.variadic,
                accepts_named_argument: true,
            }
        })
        .collect()
}

pub(super) fn function_type_param_info(params: &[FunctionTypeParam]) -> Vec<FunctionParamInfo> {
    params
        .iter()
        .enumerate()
        .map(|(index, param)| {
            let source_name = format!("__tpz_type_param_{index}");
            FunctionParamInfo {
                py_name: if param.variadic {
                    ANONYMOUS_VARIADIC_TAIL_KW.to_string()
                } else {
                    mangle(&source_name)
                },
                source_name,
                has_default: false,
                variadic: param.variadic,
                accepts_named_argument: false,
            }
        })
        .collect()
}

pub(super) fn checked_function_type_param_info(
    params: &[CheckType],
    variadic: Option<&CheckType>,
) -> Vec<FunctionParamInfo> {
    let mut infos = params
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let source_name = format!("__tpz_type_param_{index}");
            FunctionParamInfo {
                py_name: mangle(&source_name),
                source_name,
                has_default: false,
                variadic: false,
                accepts_named_argument: false,
            }
        })
        .collect::<Vec<_>>();
    if variadic.is_some() {
        let source_name = format!("__tpz_type_param_{}", params.len());
        infos.push(FunctionParamInfo {
            py_name: ANONYMOUS_VARIADIC_TAIL_KW.to_string(),
            source_name,
            has_default: false,
            variadic: true,
            accepts_named_argument: false,
        });
    }
    infos
}

pub(super) fn named_callable_param_index(
    params: &[FunctionParamInfo],
    source_name: &str,
) -> Option<usize> {
    params
        .iter()
        .position(|param| param.accepts_named_argument && param.source_name == source_name)
}

pub(super) fn push_known_variadic_fixed_arg(
    call_args: &mut Vec<String>,
    param: &FunctionParamInfo,
    value: String,
) {
    if param.accepts_named_argument {
        call_args.push(format!("{}={value}", param.py_name));
    } else {
        call_args.push(value);
    }
}

pub(super) fn checked_alias_for_ast_type<'a>(
    ty: &Type,
    ctx: &'a Ctx<'_>,
) -> Option<&'a topaz_check::ExportedAlias> {
    let TypeKind::Named { name, args } = &ty.kind else {
        return None;
    };
    if !args.is_empty() {
        return None;
    }
    let alias = ctx.type_alias(ctx.text(name.span))?;
    (alias.params == 0).then_some(alias)
}

pub(super) fn function_callable_params_from_type(
    ty: &Type,
    ctx: &Ctx<'_>,
) -> Option<Vec<FunctionParamInfo>> {
    if let Some(alias) = checked_alias_for_ast_type(ty, ctx)
        && let CheckType::Func {
            params, variadic, ..
        } = &alias.body
    {
        return Some(checked_function_type_param_info(
            params,
            variadic.as_deref(),
        ));
    }
    let TypeKind::Function { params, .. } = &ty.kind else {
        return None;
    };
    Some(function_type_param_info(params))
}

pub(super) fn map_value_metadata_from_type(ty: &Type, ctx: &Ctx<'_>) -> MapValueMetadata {
    if let Some(alias) = checked_alias_for_ast_type(ty, ctx) {
        return map_value_metadata_from_checked_type(&alias.body);
    }
    let TypeKind::Named { name, args } = &ty.kind else {
        return MapValueMetadata::default();
    };
    if ctx.text(name.span) != "Map" || args.len() != 2 {
        return MapValueMetadata::default();
    }
    let value_ty = &args[1];
    MapValueMetadata {
        receiver_shape: receiver_shape_from_type(value_ty, ctx),
        wrapped_value_metadata: wrapped_value_metadata_catalog_from_type(value_ty, ctx),
        declared_callable_params: match &value_ty.kind {
            TypeKind::Function { params, .. } => Some(function_type_param_info(params)),
            _ => None,
        },
        observed_by_key: BTreeMap::new(),
        known_present_keys: BTreeSet::new(),
        observed_keys_complete: false,
    }
}

pub(super) fn map_value_metadata_from_checked_type(ty: &CheckType) -> MapValueMetadata {
    let CheckType::Ctor(CheckCtor::Map, args) = ty else {
        return MapValueMetadata::default();
    };
    let Some(value_ty) = args.get(1) else {
        return MapValueMetadata::default();
    };
    MapValueMetadata {
        receiver_shape: receiver_shape_from_checked_type(value_ty),
        wrapped_value_metadata: wrapped_value_metadata_catalog_from_checked_type(value_ty),
        declared_callable_params: match value_ty {
            CheckType::Func {
                params, variadic, ..
            } => Some(checked_function_type_param_info(
                params,
                variadic.as_deref(),
            )),
            _ => None,
        },
        observed_by_key: BTreeMap::new(),
        known_present_keys: BTreeSet::new(),
        observed_keys_complete: false,
    }
}

pub(super) fn known_function_py_name<'a>(info: &'a FunctionInfo, ctx: &Ctx<'_>) -> &'a str {
    if ctx.cooperative_yields
        && let Some(py_name) = info.cooperative_py_name.as_deref()
    {
        py_name
    } else {
        &info.py_name
    }
}

pub(super) fn known_function_call_expr(
    info: &FunctionInfo,
    call_args: &[String],
    ctx: &Ctx<'_>,
) -> String {
    let py_name = known_function_py_name(info, ctx);
    if ctx.cooperative_yields && info.cooperative_py_name.is_some() {
        format!("(yield from {py_name}({}))", call_args.join(", "))
    } else {
        format!("{py_name}({})", call_args.join(", "))
    }
}

pub(super) fn write_known_function_call_to_target(
    out: &mut String,
    indent: usize,
    target_py: &str,
    info: &FunctionInfo,
    call_args: &[String],
    ctx: &Ctx<'_>,
) {
    let pad = " ".repeat(indent);
    let py_name = known_function_py_name(info, ctx);
    if ctx.cooperative_yields && info.cooperative_py_name.is_some() {
        writeln!(
            out,
            "{pad}{target_py} = yield from {py_name}({})",
            call_args.join(", ")
        )
        .expect("write to string");
    } else {
        writeln!(
            out,
            "{pad}{target_py} = {py_name}({})",
            call_args.join(", ")
        )
        .expect("write to string");
    }
}

pub(super) struct FunctionDefaultThunk<'a> {
    pub(super) helper_py_name: String,
    pub(super) default: &'a Expr,
}

pub(super) enum FunctionParamPrelude {
    Default {
        py_name: String,
        helper_py_name: String,
    },
    RequiredAfterDefault {
        py_name: String,
        source_name: String,
        span: Span,
    },
}

pub(super) struct FunctionSignatureParts<'a> {
    pub(super) param_parts: Vec<String>,
    pub(super) original_param_names: Vec<String>,
    pub(super) prelude: Vec<FunctionParamPrelude>,
    pub(super) default_thunks: Vec<FunctionDefaultThunk<'a>>,
}

pub(super) fn function_default_thunk_name(
    function_py_name: &str,
    index: usize,
    param_py_name: &str,
) -> String {
    format!("__tpz_default_{function_py_name}_{index}_{param_py_name}")
}

pub(super) fn emit_function_signature_parts<'a>(
    decl: &'a FunctionDecl,
    ctx: &Ctx<'_>,
    function_py_name: &str,
) -> Result<FunctionSignatureParts<'a>, PyEmitError> {
    let mut param_parts = Vec::with_capacity(decl.params.len());
    let mut original_param_names = Vec::with_capacity(decl.params.len());
    let mut saw_default = false;
    let mut prelude = Vec::new();
    let mut default_thunks = Vec::new();
    for (index, param) in decl.params.iter().enumerate() {
        let raw = ctx.text(param.name.span);
        let mut py_param = mangle(raw);
        let param_py_name = py_param.clone();
        if param.variadic {
            // Accepted programs are stopped by TPZ5024 before Python emission; this
            // remains a defense-in-depth boundary for unchecked emitter callers.
            if index + 1 != decl.params.len() {
                return Err(PyEmitError::unsupported("function variadic parameter").at(param.span));
            }
            py_param.push_str("=None");
        } else {
            match &param.default {
                Some(default) => {
                    let helper_py_name =
                        function_default_thunk_name(function_py_name, index, &param_py_name);
                    py_param.push_str("=__tpz_missing");
                    prelude.push(FunctionParamPrelude::Default {
                        py_name: param_py_name,
                        helper_py_name: helper_py_name.clone(),
                    });
                    default_thunks.push(FunctionDefaultThunk {
                        helper_py_name,
                        default,
                    });
                    saw_default = true;
                }
                None if saw_default => {
                    py_param.push_str("=__tpz_missing");
                    prelude.push(FunctionParamPrelude::RequiredAfterDefault {
                        py_name: param_py_name,
                        source_name: raw.to_string(),
                        span: param.span,
                    });
                }
                None => {}
            }
        }
        param_parts.push(py_param);
        original_param_names.push(py_comment_name(raw));
    }
    Ok(FunctionSignatureParts {
        param_parts,
        original_param_names,
        prelude,
        default_thunks,
    })
}

pub(super) fn emit_function_default_thunks(
    default_thunks: Vec<FunctionDefaultThunk<'_>>,
    ctx: &Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let inner_pad = " ".repeat(indent + 4);
    let error_pad = " ".repeat(indent + 8);
    for thunk in default_thunks {
        if !function_default_const_shape(thunk.default) {
            return Err(PyEmitError::unsupported("function default shape").at(thunk.default.span));
        }
        if function_default_static_int_fault(thunk.default, ctx) {
            return Err(PyEmitError::unsupported("function default shape").at(thunk.default.span));
        }
        let default_py = emit_expr(thunk.default, ctx)?;
        writeln!(out, "{pad}def {}(host):", thunk.helper_py_name).expect("write to string");
        writeln!(out, "{inner_pad}try:").expect("write to string");
        writeln!(out, "{error_pad}return {default_py}").expect("write to string");
        writeln!(out, "{inner_pad}except NameError:").expect("write to string");
        writeln!(
            out,
            "{error_pad}raise TpzFault(\"TPZ5002\", {}, {}) from None",
            py_string("name is not bound while evaluating a function default (§7)"),
            py_span(thunk.default.span)
        )
        .expect("write to string");
    }
    Ok(())
}

pub(super) fn function_default_const_shape(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Int
        | ExprKind::Float
        | ExprKind::Bool(_)
        | ExprKind::Null
        | ExprKind::Unit
        | ExprKind::Ident => true,
        ExprKind::String(lit) => {
            lit.tag.is_none()
                && lit
                    .parts
                    .iter()
                    .all(|part| matches!(part, StringPart::Text(_)))
        }
        ExprKind::Paren(inner) => function_default_const_shape(inner),
        ExprKind::Unary { operand, .. } => function_default_const_shape(operand),
        ExprKind::Binary { op, lhs, rhs }
            if !matches!(op, BinaryOp::And | BinaryOp::Or | BinaryOp::Coalesce) =>
        {
            function_default_const_shape(lhs) && function_default_const_shape(rhs)
        }
        _ => false,
    }
}

pub(super) enum FunctionDefaultIntFold {
    Value(i64),
    Dynamic,
    Fault,
}

pub(super) fn function_default_static_int_fault(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    matches!(
        function_default_int_fold(expr, ctx),
        FunctionDefaultIntFold::Fault
    )
}

pub(super) fn function_default_int_fold(expr: &Expr, ctx: &Ctx<'_>) -> FunctionDefaultIntFold {
    match &expr.kind {
        ExprKind::Int => ctx
            .text(expr.span)
            .replace('_', "")
            .parse::<i64>()
            .map(FunctionDefaultIntFold::Value)
            .unwrap_or(FunctionDefaultIntFold::Dynamic),
        ExprKind::Paren(inner) => function_default_int_fold(inner, ctx),
        ExprKind::Unary { op, operand } => match (op, function_default_int_fold(operand, ctx)) {
            (_, FunctionDefaultIntFold::Fault) => FunctionDefaultIntFold::Fault,
            (UnaryOp::Plus, FunctionDefaultIntFold::Value(value)) => {
                FunctionDefaultIntFold::Value(value)
            }
            (UnaryOp::Minus, FunctionDefaultIntFold::Value(value)) => value
                .checked_neg()
                .map(FunctionDefaultIntFold::Value)
                .unwrap_or(FunctionDefaultIntFold::Fault),
            _ => FunctionDefaultIntFold::Dynamic,
        },
        ExprKind::Binary { op, lhs, rhs }
            if !matches!(op, BinaryOp::And | BinaryOp::Or | BinaryOp::Coalesce) =>
        {
            let left = function_default_int_fold(lhs, ctx);
            let right = function_default_int_fold(rhs, ctx);
            match (left, right) {
                (FunctionDefaultIntFold::Fault, _) | (_, FunctionDefaultIntFold::Fault) => {
                    FunctionDefaultIntFold::Fault
                }
                (FunctionDefaultIntFold::Value(left), FunctionDefaultIntFold::Value(right)) => {
                    let value = match op {
                        BinaryOp::Add => left.checked_add(right),
                        BinaryOp::Sub => left.checked_sub(right),
                        BinaryOp::Mul => left.checked_mul(right),
                        BinaryOp::Div => {
                            if right == 0 {
                                None
                            } else {
                                left.checked_div(right)
                            }
                        }
                        BinaryOp::Rem => {
                            if right == 0 {
                                None
                            } else {
                                left.checked_rem(right)
                            }
                        }
                        BinaryOp::Pow => u32::try_from(right)
                            .ok()
                            .and_then(|power| left.checked_pow(power)),
                        _ => return FunctionDefaultIntFold::Dynamic,
                    };
                    value
                        .map(FunctionDefaultIntFold::Value)
                        .unwrap_or(FunctionDefaultIntFold::Fault)
                }
                _ => FunctionDefaultIntFold::Dynamic,
            }
        }
        _ => FunctionDefaultIntFold::Dynamic,
    }
}

pub(super) fn emit_function_parameter_prelude(
    prelude: Vec<FunctionParamPrelude>,
    indent: usize,
    out: &mut String,
) {
    let pad = " ".repeat(indent);
    let inner_pad = " ".repeat(indent + 4);
    for action in prelude {
        match action {
            FunctionParamPrelude::Default {
                py_name,
                helper_py_name,
            } => {
                writeln!(out, "{pad}if {py_name} is __tpz_missing:").expect("write to string");
                writeln!(out, "{inner_pad}{py_name} = {helper_py_name}(host)")
                    .expect("write to string");
            }
            FunctionParamPrelude::RequiredAfterDefault {
                py_name,
                source_name,
                span,
            } => {
                writeln!(out, "{pad}if {py_name} is __tpz_missing:").expect("write to string");
                writeln!(
                    out,
                    "{inner_pad}tpz_call_order_fault([], {}, {})",
                    py_string(&format!(
                        "missing argument for parameter `{source_name}` (§5)"
                    )),
                    py_span(span)
                )
                .expect("write to string");
            }
        }
    }
}

pub(super) fn emit_function_body(
    decl: &FunctionDecl,
    ctx: &mut Ctx<'_>,
    body_indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(body_indent);
    writeln!(out, "{pad}__tpz_defers = []").expect("write to string");
    emit_defer_helpers(out, body_indent);
    if ctx.cooperative_yields {
        writeln!(out, "{pad}if False:").expect("write to string");
        writeln!(out, "{pad}    yield None").expect("write to string");
    }
    writeln!(out, "{pad}try:").expect("write to string");
    for param in &decl.params {
        if param.variadic {
            let raw = ctx.text(param.name.span);
            let py_name = mangle(raw);
            writeln!(out, "{}if {py_name} is None:", " ".repeat(body_indent + 4))
                .expect("write to string");
            writeln!(out, "{}{py_name} = []", " ".repeat(body_indent + 8))
                .expect("write to string");
        }
    }
    ctx.push_scope();
    for param in &decl.params {
        let raw = ctx.text(param.name.span);
        if param.variadic {
            ctx.register_array_binding(raw, false);
        } else {
            ctx.register_typed_binding(raw, false, &param.ty);
        }
    }
    let body_result = emit_block_as_function(&decl.body, ctx, body_indent + 4, out);
    ctx.pop_scope();
    body_result?;
    writeln!(out, "{pad}except TpzReturn as __tpz_return:").expect("write to string");
    writeln!(
        out,
        "{}__tpz_result = __tpz_return.value",
        " ".repeat(body_indent + 4)
    )
    .expect("write to string");
    writeln!(out, "{pad}except TpzFault:").expect("write to string");
    writeln!(out, "{}raise", " ".repeat(body_indent + 4)).expect("write to string");
    writeln!(out, "{pad}__tpz_run_defers()").expect("write to string");
    writeln!(out, "{pad}return __tpz_result").expect("write to string");
    Ok(())
}

pub(super) fn emit_nonlocal_declarations(names: &[String], indent: usize, out: &mut String) {
    if names.is_empty() {
        return;
    }
    let pad = " ".repeat(indent);
    writeln!(out, "{pad}nonlocal {}", names.join(", ")).expect("write to string");
}

pub(super) fn emit_function(
    decl: &FunctionDecl,
    ctx: &mut Ctx<'_>,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let name = ctx.text(decl.name.span);
    let py_name = ctx
        .function_py_name(name)
        .map(str::to_string)
        .unwrap_or_else(|| mangle(name));
    if ctx.function_info(name).is_none() {
        let module_top_bound_names = BTreeSet::new();
        ctx.register_function_info(
            name,
            function_info(decl, py_name.clone(), ctx, &module_top_bound_names),
        );
        ctx.enrich_function_return_metadata(name, decl);
    }
    emit_function_variant(decl, ctx, out, &py_name, false)?;
    if let Some(cooperative_py_name) = ctx
        .function_info(name)
        .and_then(|info| info.cooperative_py_name.clone())
    {
        emit_function_variant(decl, ctx, out, &cooperative_py_name, true)?;
    }
    Ok(())
}

#[derive(Clone)]
pub(super) struct ReceiverMethodRegistration {
    pub(super) dispatch_id: String,
    pub(super) nominal: String,
    pub(super) source_name: String,
    pub(super) exported: bool,
    pub(super) info: FunctionInfo,
    pub(super) decl: FunctionDecl,
}

#[derive(Clone)]
pub(super) struct ProtocolMethodRegistration {
    pub(super) dispatch_id: String,
    pub(super) source_name: String,
    pub(super) info: FunctionInfo,
    pub(super) decl: FunctionDecl,
}

pub(super) fn python_receiver_method_identity(module: &str, nominal: &str) -> String {
    let module = if module.is_empty() {
        "__entry__"
    } else {
        module
    };
    format!("{module}::{nominal}")
}

pub(super) fn receiver_method_py_name(module: &str, nominal: &str, method: &str) -> String {
    format!(
        "__tpz_method_{}_{}_{}",
        mangle(module),
        mangle(nominal),
        mangle(method)
    )
}

pub(super) fn protocol_method_py_name(
    module: &str,
    protocol: &str,
    nominal: &str,
    method: &str,
) -> String {
    format!(
        "__tpz_protocol_{}_{}_{}_{}",
        mangle(module),
        mangle(protocol),
        mangle(nominal),
        mangle(method)
    )
}

pub(super) fn python_protocol_method_identity(
    module: &str,
    protocol: &str,
    nominal: &str,
) -> String {
    let module = if module.is_empty() {
        "__entry__"
    } else {
        module
    };
    format!("{module}::{protocol}<{nominal}>")
}

pub(super) fn register_protocols(protocol_names: &[String], ctx: &mut Ctx<'_>) {
    Rc::make_mut(&mut ctx.protocols).extend(protocol_names.iter().cloned());
}

pub(super) fn prepare_receiver_methods(
    impls: &[&ImplDecl],
    module_identity: &str,
    ctx: &mut Ctx<'_>,
) -> Vec<ReceiverMethodRegistration> {
    let mut registrations = Vec::new();
    let module_top_bound_names = BTreeSet::new();
    for decl in impls {
        let nominal = ctx.text(decl.name.span).to_string();
        for method in &decl.methods {
            let source_name = ctx.text(method.decl.name.span).to_string();
            let py_name = receiver_method_py_name(module_identity, &nominal, &source_name);
            let info = function_info(&method.decl, py_name, ctx, &module_top_bound_names);
            ctx.register_receiver_method_info(&source_name, info.clone());
            registrations.push(ReceiverMethodRegistration {
                dispatch_id: python_receiver_method_identity(module_identity, &nominal),
                nominal: nominal.clone(),
                source_name,
                exported: method.exported,
                info,
                decl: method.decl.clone(),
            });
        }
    }
    registrations
}

pub(super) fn prepare_protocol_methods(
    impls: &[&ImplDecl],
    module_identity: &str,
    ctx: &Ctx<'_>,
) -> Vec<ProtocolMethodRegistration> {
    let mut registrations = Vec::new();
    let module_top_bound_names = BTreeSet::new();
    for decl in impls {
        let target = decl.target.expect("protocol impl target");
        let protocol = ctx.text(decl.name.span);
        let nominal = ctx.text(target.span);
        for method in &decl.methods {
            let source_name = ctx.text(method.decl.name.span).to_string();
            let py_name = protocol_method_py_name(module_identity, protocol, nominal, &source_name);
            registrations.push(ProtocolMethodRegistration {
                dispatch_id: python_protocol_method_identity(module_identity, protocol, nominal),
                source_name,
                info: function_info(&method.decl, py_name, ctx, &module_top_bound_names),
                decl: method.decl.clone(),
            });
        }
    }
    registrations
}

pub(super) fn emit_receiver_method_functions(
    registrations: &[ReceiverMethodRegistration],
    module_values: &Rc<BTreeMap<String, String>>,
    ctx: &mut Ctx<'_>,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let previous = std::mem::replace(
        &mut ctx.receiver_method_module_values,
        Rc::clone(module_values),
    );
    for registration in registrations {
        let result = emit_function_variant(
            &registration.decl,
            ctx,
            out,
            &registration.info.py_name,
            false,
        );
        if let Err(error) = result {
            ctx.receiver_method_module_values = previous;
            return Err(error);
        }
        let cooperative = registration
            .info
            .cooperative_py_name
            .as_deref()
            .expect("receiver methods always have a cooperative sibling");
        if let Err(error) = emit_function_variant(&registration.decl, ctx, out, cooperative, true) {
            ctx.receiver_method_module_values = previous;
            return Err(error);
        }
        out.push('\n');
    }
    ctx.receiver_method_module_values = previous;
    Ok(())
}

pub(super) fn emit_protocol_method_functions(
    registrations: &[ProtocolMethodRegistration],
    module_values: &Rc<BTreeMap<String, String>>,
    ctx: &mut Ctx<'_>,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let previous = std::mem::replace(
        &mut ctx.receiver_method_module_values,
        Rc::clone(module_values),
    );
    for registration in registrations {
        let result = emit_function_variant(
            &registration.decl,
            ctx,
            out,
            &registration.info.py_name,
            false,
        );
        if let Err(error) = result {
            ctx.receiver_method_module_values = previous;
            return Err(error);
        }
        let cooperative = registration
            .info
            .cooperative_py_name
            .as_deref()
            .expect("protocol methods always have a cooperative sibling");
        if let Err(error) = emit_function_variant(&registration.decl, ctx, out, cooperative, true) {
            ctx.receiver_method_module_values = previous;
            return Err(error);
        }
        out.push('\n');
    }
    ctx.receiver_method_module_values = previous;
    Ok(())
}

pub(super) fn receiver_method_module_value_names(
    source_names: &[String],
    module_identity: Option<&str>,
) -> BTreeMap<String, String> {
    source_names
        .iter()
        .map(|name| {
            let py_name = module_identity
                .map(|identity| module_value_name(identity, name))
                .unwrap_or_else(|| mangle(name));
            (name.clone(), py_name)
        })
        .collect()
}

pub(super) fn write_receiver_method_module_value_seeds(
    out: &mut String,
    indent: usize,
    names: &BTreeMap<String, String>,
) {
    for py_name in names.values() {
        writeln!(
            out,
            "{}globals()[{}] = __tpz_missing",
            " ".repeat(indent),
            py_string(py_name)
        )
        .expect("write to string");
    }
}

pub(super) fn emit_receiver_method_registrations(
    registrations: &[ReceiverMethodRegistration],
    indent: usize,
    out: &mut String,
) {
    let pad = " ".repeat(indent);
    for registration in registrations {
        writeln!(
            out,
            "{pad}__tpz_methods[({}, {})] = {}",
            py_string(&registration.dispatch_id),
            py_string(&registration.source_name),
            render_host_callable(&registration.info),
        )
        .expect("write to string");
    }
}

pub(super) fn emit_protocol_method_registrations(
    registrations: &[ProtocolMethodRegistration],
    indent: usize,
    out: &mut String,
) {
    let pad = " ".repeat(indent);
    for registration in registrations {
        writeln!(
            out,
            "{pad}__tpz_methods[({}, {})] = {}",
            py_string(&registration.dispatch_id),
            py_string(&registration.source_name),
            render_host_callable(&registration.info),
        )
        .expect("write to string");
    }
}

pub(super) fn exported_receiver_methods_for_nominal(
    registrations: &[ReceiverMethodRegistration],
    nominal: &str,
) -> BTreeMap<String, FunctionInfo> {
    registrations
        .iter()
        .filter(|registration| registration.exported && registration.nominal == nominal)
        .map(|registration| (registration.source_name.clone(), registration.info.clone()))
        .collect()
}

pub(super) fn emit_function_variant(
    decl: &FunctionDecl,
    ctx: &mut Ctx<'_>,
    out: &mut String,
    py_name: &str,
    cooperative: bool,
) -> Result<(), PyEmitError> {
    let name = ctx.text(decl.name.span);
    let signature = emit_function_signature_parts(decl, ctx, py_name)?;
    emit_function_default_thunks(signature.default_thunks, ctx, 0, out)?;
    let params = signature.param_parts.join(", ");
    let original_params = signature
        .original_param_names
        .iter()
        .map(|raw| raw.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    if params.is_empty() {
        writeln!(out, "def {}(host):  # {}", py_name, py_comment_name(name))
            .expect("write to string");
    } else {
        writeln!(
            out,
            "def {}(host, {params}):  # {}({original_params})",
            py_name,
            py_comment_name(name)
        )
        .expect("write to string");
    }
    emit_function_parameter_prelude(signature.prelude, 4, out);
    ctx.with_cooperative_yields(cooperative, |ctx| emit_function_body(decl, ctx, 4, out))
}

pub(super) fn emit_nested_function(
    decl: &FunctionDecl,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let name = ctx.text(decl.name.span).to_string();
    let py_name = ctx
        .binding_py_name(&name)
        .map(str::to_string)
        .unwrap_or_else(|| mangle(&name));
    let cooperative_py_name = ctx
        .binding_callable_info(&name)
        .and_then(|info| info.cooperative_py_name);
    let forward_cell = ctx.binding_is_forward_function_cell(&name);
    let implementation_py_name = if forward_cell {
        format!("{py_name}__impl")
    } else {
        py_name.clone()
    };
    emit_nested_function_variant(decl, ctx, indent, out, &implementation_py_name, false)?;
    if forward_cell {
        writeln!(
            out,
            "{}{}[0] = {implementation_py_name}",
            " ".repeat(indent),
            py_name
        )
        .expect("write to string");
    }
    if let Some(cooperative_py_name) = cooperative_py_name {
        let implementation_cooperative_py_name = if forward_cell {
            format!("{cooperative_py_name}__impl")
        } else {
            cooperative_py_name.clone()
        };
        emit_nested_function_variant(
            decl,
            ctx,
            indent,
            out,
            &implementation_cooperative_py_name,
            true,
        )?;
        if forward_cell {
            writeln!(
                out,
                "{}{}[0] = {implementation_cooperative_py_name}",
                " ".repeat(indent),
                cooperative_py_name
            )
            .expect("write to string");
        }
    }
    Ok(())
}

pub(super) fn emit_nested_function_variant(
    decl: &FunctionDecl,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
    py_name: &str,
    cooperative: bool,
) -> Result<(), PyEmitError> {
    let name = ctx.text(decl.name.span).to_string();
    let nonlocal_py_names = nested_function_nonlocal_py_names(decl, ctx);
    let signature = emit_function_signature_parts(decl, ctx, py_name)?;
    emit_function_default_thunks(signature.default_thunks, ctx, indent, out)?;
    let params = signature.param_parts.join(", ");
    let original_params = signature
        .original_param_names
        .iter()
        .map(|raw| raw.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let pad = " ".repeat(indent);
    if params.is_empty() {
        writeln!(out, "{pad}def {}():  # {}", py_name, py_comment_name(&name))
            .expect("write to string");
    } else {
        writeln!(
            out,
            "{pad}def {}({params}):  # {}({original_params})",
            py_name,
            py_comment_name(&name)
        )
        .expect("write to string");
    }
    emit_nonlocal_declarations(&nonlocal_py_names, indent + 4, out);
    emit_function_parameter_prelude(signature.prelude, indent + 4, out);
    ctx.with_cooperative_yields(cooperative, |ctx| {
        emit_function_body(decl, ctx, indent + 4, out)
    })
}

#[derive(Default)]
pub(super) struct NestedFunctionRegistrationSnapshot {
    pub(super) saved: Vec<(String, Option<BindingInfo>)>,
}

impl NestedFunctionRegistrationSnapshot {
    pub(super) fn restore(self, ctx: &mut Ctx<'_>) {
        for (name, previous) in self.saved.into_iter().rev() {
            ctx.restore_current_binding(name, previous);
        }
    }
}

pub(super) fn pre_register_nested_functions(
    block: &Block,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<NestedFunctionRegistrationSnapshot, PyEmitError> {
    let mut candidates = Vec::new();
    let mut prior_names = BTreeSet::new();
    for (index, stmt) in block.stmts.iter().enumerate() {
        match &stmt.kind {
            StmtKind::Function(decl) => {
                let name = ctx.text(decl.name.span).to_string();
                if ctx.current_scope_contains(&name) || prior_names.contains(&name) {
                    return Err(
                        PyEmitError::unsupported("nested function shadowing").at(decl.name.span)
                    );
                }
                prior_names.insert(name.clone());
                candidates.push((index, name, decl));
            }
            _ => collect_stmt_binding_names(stmt, ctx.map, &mut prior_names),
        }
    }
    let forward_candidates = candidates
        .iter()
        .map(|(index, name, decl)| (*index, name.clone(), *decl))
        .collect::<Vec<_>>();
    let needs_positional_cells =
        reject_nested_function_forward_references(block, ctx.map, &forward_candidates).is_err();
    let mut snapshot = NestedFunctionRegistrationSnapshot::default();
    for (_, name, decl) in &candidates {
        let outer_callable = if needs_positional_cells {
            ctx.binding_callable_info(name)
                .or_else(|| ctx.function_info(name).cloned())
        } else {
            None
        };
        snapshot
            .saved
            .push((name.clone(), ctx.current_binding_info(name)));
        let py_name = ctx.new_binding_py_name(name);
        let cooperative_py_name = Some(cooperative_function_py_name(&py_name));
        ctx.register_callable_binding(
            name,
            function_param_info(decl, ctx),
            directly_mutated_collection_parameter_indices(decl, ctx),
            cooperative_py_name.clone(),
            needs_positional_cells,
        );
        ctx.set_binding_py_name(name, py_name);
        if !needs_positional_cells {
            continue;
        }
        let pad = " ".repeat(indent);
        let binding_py_name = ctx
            .binding_py_name(name)
            .expect("registered nested function binding");
        let normal_seed = match &outer_callable {
            Some(info) if info.needs_host => render_host_callable(info),
            Some(info) => info.py_name.clone(),
            None => "__tpz_missing".to_string(),
        };
        writeln!(out, "{pad}{binding_py_name} = [{normal_seed}]").expect("write to string");
        if let Some(cooperative_py_name) = cooperative_py_name {
            let cooperative_seed = match &outer_callable {
                Some(info) if info.needs_host => info
                    .cooperative_py_name
                    .as_deref()
                    .map(|py_name| format!("lambda *args: {py_name}(host, *args)")),
                Some(info) => info.cooperative_py_name.clone(),
                None => None,
            }
            .unwrap_or_else(|| "__tpz_missing".to_string());
            writeln!(out, "{pad}{cooperative_py_name} = [{cooperative_seed}]")
                .expect("write to string");
        }
    }
    enrich_nested_function_mutation_metadata(&candidates, ctx);
    Ok(snapshot)
}

pub(super) fn enrich_nested_function_mutation_metadata(
    functions: &[(usize, String, &FunctionDecl)],
    ctx: &mut Ctx<'_>,
) {
    loop {
        let discovered = functions
            .iter()
            .map(|(_, name, decl)| {
                (
                    name.clone(),
                    mutated_collection_parameter_indices(decl, ctx, true),
                )
            })
            .collect::<Vec<_>>();
        let mut changed = false;
        for (name, params) in discovered {
            changed |= ctx.extend_callable_binding_mutated_collection_params(&name, params);
        }
        if !changed {
            break;
        }
    }
}

pub(super) fn collect_stmt_binding_names(stmt: &Stmt, map: &SourceMap, out: &mut BTreeSet<String>) {
    match &stmt.kind {
        StmtKind::Function(decl) => {
            out.insert(text_in_map(map, decl.name.span).to_string());
        }
        StmtKind::Const { name, .. } => {
            out.insert(text_in_map(map, name.span).to_string());
        }
        StmtKind::Let { pattern, .. } => collect_pattern_binding_names(pattern, map, out),
        StmtKind::Export(inner) => collect_stmt_binding_names(inner, map, out),
        _ => {}
    }
}

pub(super) fn collect_pattern_binding_names(
    pattern: &Pattern,
    map: &SourceMap,
    out: &mut BTreeSet<String>,
) {
    match &pattern.kind {
        PatternKind::Binding(name) | PatternKind::Typed { name, .. } => {
            out.insert(text_in_map(map, name.span).to_string());
        }
        PatternKind::Constructor { args, .. } | PatternKind::Or(args) => {
            for pattern in args {
                collect_pattern_binding_names(pattern, map, out);
            }
        }
        PatternKind::List(elements) => {
            for element in elements {
                match element {
                    ListPatternElem::Pattern(pattern) | ListPatternElem::Rest(Some(pattern)) => {
                        collect_pattern_binding_names(pattern, map, out);
                    }
                    ListPatternElem::Rest(None) => {}
                }
            }
        }
        PatternKind::Record(fields) | PatternKind::NominalRecord { fields, .. } => {
            for field in fields {
                if let Some(pattern) = &field.pattern {
                    collect_pattern_binding_names(pattern, map, out);
                } else {
                    out.insert(text_in_map(map, field.name.span).to_string());
                }
            }
        }
        PatternKind::Wildcard | PatternKind::Literal(_) | PatternKind::Range { .. } => {}
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct NestedForwardSignal;
