use crate::*;

pub(super) fn lambda_callee(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Lambda { .. } => true,
        ExprKind::Paren(inner) => lambda_callee(inner),
        _ => false,
    }
}

pub(super) fn lambda_param_info(expr: &Expr, ctx: &Ctx<'_>) -> Option<Vec<FunctionParamInfo>> {
    match &expr.kind {
        ExprKind::Lambda { params, .. } => Some(
            params
                .iter()
                .map(|param| {
                    let source_name = ctx.text(param.name.span).to_string();
                    FunctionParamInfo {
                        py_name: mangle(&source_name),
                        source_name,
                        has_default: false,
                        variadic: false,
                        accepts_named_argument: true,
                    }
                })
                .collect(),
        ),
        ExprKind::Paren(inner) => lambda_param_info(inner, ctx),
        _ => None,
    }
}

pub(super) fn metadata_join_block_tail_expr(block: &Block) -> Option<&Expr> {
    if block.stmts.is_empty() {
        block.tail.as_deref()
    } else {
        None
    }
}

pub(super) fn direct_tail_metadata(
    decl: &FunctionDecl,
    ctx: &Ctx<'_>,
    module_top_bound_names: &BTreeSet<String>,
) -> DirectTailMetadata {
    let body = decl.body.as_ref();
    if !block_has_bare_return(body)
        && let Some(tail) = body.tail.as_deref()
        && let Some(return_shape) = direct_tail_expr_return_shape(tail)
    {
        return DirectTailMetadata {
            return_shape: Some(return_shape),
            result_ok_shape: None,
        };
    }
    let Some(tail) = metadata_join_block_tail_expr(decl.body.as_ref()) else {
        return DirectTailMetadata::default();
    };
    direct_tail_namespace_metadata(tail, decl, ctx, module_top_bound_names).unwrap_or_default()
}

pub(super) fn direct_tail_expr_return_shape(expr: &Expr) -> Option<ReceiverShape> {
    match &expr.kind {
        // Keep this direct-only: broader value inference can bootstrap through
        // other unannotated function return metadata in source order.
        ExprKind::String(lit) if lit.tag.is_none() => Some(ReceiverShape::String),
        ExprKind::String(_) => Some(ReceiverShape::Template),
        ExprKind::Array(_) => Some(ReceiverShape::Array),
        ExprKind::MapLiteral(_) => Some(ReceiverShape::Map),
        ExprKind::Paren(inner) => direct_tail_expr_return_shape(inner),
        _ => None,
    }
}

pub(super) fn direct_tail_namespace_metadata(
    expr: &Expr,
    decl: &FunctionDecl,
    ctx: &Ctx<'_>,
    module_top_bound_names: &BTreeSet<String>,
) -> Option<DirectTailMetadata> {
    match &expr.kind {
        ExprKind::Call { callee, args, .. } => {
            direct_namespace_builtin_tail_metadata(callee, args, decl, ctx, module_top_bound_names)
        }
        ExprKind::Paren(inner) => {
            direct_tail_namespace_metadata(inner, decl, ctx, module_top_bound_names)
        }
        _ => None,
    }
}

pub(super) fn namespace_builtin_call_metadata(namespace: &str, method: &str) -> DirectTailMetadata {
    match (namespace, method) {
        ("Bytes", "empty" | "encodeUtf8" | "concat")
        | ("Encoding", "utf8Encode")
        | ("Hash", "sha256" | "sha512" | "hmacSha256") => DirectTailMetadata {
            return_shape: Some(ReceiverShape::Bytes),
            result_ok_shape: None,
        },
        ("Hash", "crc32") => DirectTailMetadata {
            return_shape: None,
            result_ok_shape: None,
        },
        ("Map", "new" | "ofEntries") => DirectTailMetadata {
            return_shape: Some(ReceiverShape::Map),
            result_ok_shape: None,
        },
        ("JSON", "parse") => DirectTailMetadata {
            return_shape: Some(ReceiverShape::Result),
            result_ok_shape: Some(ReceiverShape::Json),
        },
        ("TOML", "toJson") => DirectTailMetadata {
            return_shape: Some(ReceiverShape::Json),
            result_ok_shape: None,
        },
        ("Bytes", "fromArray" | "fromHex" | "fromBase64")
        | ("Encoding", "hexDecode" | "base64Decode")
        | ("FS", "readBytes") => DirectTailMetadata {
            return_shape: Some(ReceiverShape::Result),
            result_ok_shape: Some(ReceiverShape::Bytes),
        },
        ("Encoding", "utf8Decode") | ("FS", "readText") => DirectTailMetadata {
            return_shape: Some(ReceiverShape::Result),
            result_ok_shape: Some(ReceiverShape::String),
        },
        ("CSV", "parse" | "parseWithHeader") => DirectTailMetadata {
            return_shape: Some(ReceiverShape::Result),
            result_ok_shape: Some(ReceiverShape::Array),
        },
        ("Regex", "compile") | ("TOML", "parse") | ("URL", "parse") => DirectTailMetadata {
            return_shape: Some(ReceiverShape::Result),
            result_ok_shape: None,
        },
        ("Cli", "option") => DirectTailMetadata {
            return_shape: Some(ReceiverShape::Option),
            result_ok_shape: None,
        },
        ("Cli", "options" | "positionals") => DirectTailMetadata {
            return_shape: Some(ReceiverShape::Array),
            result_ok_shape: None,
        },
        _ => DirectTailMetadata::default(),
    }
}

pub(super) fn namespace_builtin_option_inner_shape(
    namespace: &str,
    method: &str,
) -> Option<ReceiverShape> {
    matches!((namespace, method), ("Cli", "option")).then_some(ReceiverShape::String)
}

pub(super) fn direct_namespace_builtin_tail_metadata(
    callee: &Expr,
    args: &[CallArg],
    decl: &FunctionDecl,
    ctx: &Ctx<'_>,
    module_top_bound_names: &BTreeSet<String>,
) -> Option<DirectTailMetadata> {
    let (namespace, method) = static_member_name(callee, ctx)?;
    if direct_tail_namespace_root_shadowed(namespace, decl, ctx, module_top_bound_names) {
        return None;
    }
    match (namespace, method) {
        ("Bytes", "empty" | "encodeUtf8" | "concat") | ("Encoding", "utf8Encode") => {
            Some(DirectTailMetadata {
                return_shape: Some(ReceiverShape::Bytes),
                result_ok_shape: None,
            })
        }
        ("Map", "ofEntries") => Some(DirectTailMetadata {
            return_shape: Some(ReceiverShape::Map),
            result_ok_shape: None,
        }),
        ("Map", "new") if args.is_empty() => Some(DirectTailMetadata {
            return_shape: Some(ReceiverShape::Map),
            result_ok_shape: None,
        }),
        ("JSON", "parse") => Some(DirectTailMetadata {
            return_shape: Some(ReceiverShape::Result),
            result_ok_shape: Some(ReceiverShape::Json),
        }),
        ("Bytes", "fromArray" | "fromHex" | "fromBase64")
        | ("Encoding", "hexDecode" | "base64Decode") => Some(DirectTailMetadata {
            return_shape: Some(ReceiverShape::Result),
            result_ok_shape: Some(ReceiverShape::Bytes),
        }),
        ("Encoding", "utf8Decode") => Some(DirectTailMetadata {
            return_shape: Some(ReceiverShape::Result),
            result_ok_shape: Some(ReceiverShape::String),
        }),
        _ => None,
    }
}

pub(super) fn direct_tail_namespace_root_shadowed(
    namespace: &str,
    decl: &FunctionDecl,
    ctx: &Ctx<'_>,
    module_top_bound_names: &BTreeSet<String>,
) -> bool {
    module_top_bound_names.contains(namespace)
        || decl
            .params
            .iter()
            .any(|param| ctx.text(param.name.span) == namespace)
}

pub(super) fn metadata_join_else_expr(expr: &Expr) -> Option<&Expr> {
    match &expr.kind {
        ExprKind::Block(block) => metadata_join_block_tail_expr(block),
        _ => Some(expr),
    }
}

pub(super) struct CallableJoinExpr<'a> {
    pub(super) expr: &'a Expr,
    pub(super) statementful: bool,
}

pub(super) fn metadata_neutral_literal_expr(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Int
        | ExprKind::Float
        | ExprKind::Duration(_)
        | ExprKind::Bool(_)
        | ExprKind::Null
        | ExprKind::Unit => true,
        ExprKind::String(lit) => {
            lit.tag.is_none()
                && lit
                    .parts
                    .iter()
                    .all(|part| matches!(part, StringPart::Text(_)))
        }
        ExprKind::Paren(inner) => metadata_neutral_literal_expr(inner),
        _ => false,
    }
}

pub(super) fn single_namespace_member_root_name(expr: &Expr, ctx: &Ctx<'_>) -> Option<String> {
    match &expr.kind {
        ExprKind::Member { object, .. } => match &object.kind {
            ExprKind::Ident => Some(ctx.text(object.span).to_string()),
            ExprKind::Paren(inner) => match &inner.kind {
                ExprKind::Ident => Some(ctx.text(inner.span).to_string()),
                _ => None,
            },
            _ => None,
        },
        ExprKind::Paren(inner) => single_namespace_member_root_name(inner, ctx),
        _ => None,
    }
}

pub(super) fn callable_metadata_join_block_tail_expr<'a>(
    block: &'a Block,
    ctx: &Ctx<'_>,
) -> Option<CallableJoinExpr<'a>> {
    if block.stmts.is_empty() {
        return block.tail.as_deref().map(|expr| CallableJoinExpr {
            expr,
            statementful: false,
        });
    }

    let mut prefix_names = BTreeSet::new();
    for stmt in &block.stmts {
        let StmtKind::Let {
            mutable: false,
            pattern,
            value,
            ..
        } = &stmt.kind
        else {
            return None;
        };
        let name = direct_binding_pattern_name(pattern, ctx)?;
        if !metadata_neutral_literal_expr(value) || !prefix_names.insert(name) {
            return None;
        }
    }

    let tail = block.tail.as_deref()?;
    let root_name = single_namespace_member_root_name(tail, ctx)?;
    if prefix_names.contains(&root_name) {
        return None;
    }
    Some(CallableJoinExpr {
        expr: tail,
        statementful: true,
    })
}

pub(super) fn callable_metadata_join_else_expr<'a>(
    expr: &'a Expr,
    ctx: &Ctx<'_>,
) -> Option<CallableJoinExpr<'a>> {
    match &expr.kind {
        ExprKind::Block(block) => callable_metadata_join_block_tail_expr(block, ctx),
        _ => Some(CallableJoinExpr {
            expr,
            statementful: false,
        }),
    }
}

pub(super) fn join_identical_if_branch_callable_metadata(
    then_block: &Block,
    else_branch: Option<&Expr>,
    ctx: &Ctx<'_>,
) -> Option<Vec<FunctionParamInfo>> {
    let then_expr = callable_metadata_join_block_tail_expr(then_block, ctx)?;
    let else_expr = callable_metadata_join_else_expr(else_branch?, ctx)?;
    if (then_expr.statementful || else_expr.statementful)
        && (matches!(
            then_expr.expr.kind,
            ExprKind::If { .. } | ExprKind::Match { .. } | ExprKind::Pipe { .. }
        ) || matches!(
            else_expr.expr.kind,
            ExprKind::If { .. } | ExprKind::Match { .. } | ExprKind::Pipe { .. }
        ))
    {
        return None;
    }
    let then_metadata = callable_param_info(then_expr.expr, ctx)?;
    let else_metadata = callable_param_info(else_expr.expr, ctx)?;
    (then_metadata == else_metadata).then_some(then_metadata)
}

pub(super) fn direct_call_callable_param_info(
    expr: &Expr,
    ctx: &Ctx<'_>,
) -> Option<Vec<FunctionParamInfo>> {
    match &expr.kind {
        ExprKind::If {
            then_block,
            else_branch,
            ..
        } => join_identical_if_branch_callable_metadata(then_block, else_branch.as_deref(), ctx),
        ExprKind::Match { cases, .. } => join_match_arm_direct_call_callable_metadata(cases, ctx),
        ExprKind::Paren(inner) => direct_call_callable_param_info(inner, ctx),
        _ => None,
    }
}

pub(super) fn join_match_arm_direct_call_callable_metadata(
    cases: &[CaseClause],
    ctx: &Ctx<'_>,
) -> Option<Vec<FunctionParamInfo>> {
    if cases.is_empty() {
        return None;
    }
    let mut bodies = cases
        .iter()
        .map(|case| metadata_join_match_body_expr(&case.body));
    let first_expr = bodies.next().flatten()?;
    let metadata = callable_param_info(first_expr, ctx)?;
    for body in bodies {
        if callable_param_info(body?, ctx)? != metadata {
            return None;
        }
    }
    Some(metadata)
}

pub(super) fn join_identical_if_branch_metadata<T: PartialEq>(
    then_block: &Block,
    else_branch: Option<&Expr>,
    extract: impl Fn(&Expr) -> T,
) -> Option<T> {
    let then_expr = metadata_join_block_tail_expr(then_block)?;
    let else_expr = metadata_join_else_expr(else_branch?)?;
    let then_metadata = extract(then_expr);
    let else_metadata = extract(else_expr);
    (then_metadata == else_metadata).then_some(then_metadata)
}

pub(super) fn metadata_join_match_body_expr(body: &CaseArmBody) -> Option<&Expr> {
    match body {
        CaseArmBody::Expr(expr) => Some(expr),
        CaseArmBody::Return { .. } => None,
    }
}

pub(super) fn match_case_is_unguarded_catch_all(case: &CaseClause) -> bool {
    case.guard.is_none() && matches!(case.pattern.kind, PatternKind::Wildcard)
}

pub(super) fn join_identical_match_arm_metadata<T: PartialEq>(
    cases: &[CaseClause],
    extract: impl Fn(&Expr) -> T,
) -> Option<T> {
    if cases.is_empty() || !cases.last().is_some_and(match_case_is_unguarded_catch_all) {
        return None;
    }
    let mut bodies = cases
        .iter()
        .map(|case| metadata_join_match_body_expr(&case.body));
    let first_expr = bodies.next().flatten()?;
    let metadata = extract(first_expr);
    for body in bodies {
        if extract(body?) != metadata {
            return None;
        }
    }
    Some(metadata)
}

pub(super) fn callable_param_info(expr: &Expr, ctx: &Ctx<'_>) -> Option<Vec<FunctionParamInfo>> {
    match &expr.kind {
        ExprKind::Ident => {
            let name = ctx.text(expr.span);
            if !ctx.binding_is_bound(name)
                && let Some(info) = ctx.function_info(name)
            {
                return Some(info.params.clone());
            }
            ctx.binding_flow_callable_info(name).map(|info| info.params)
        }
        ExprKind::Lambda { .. } => lambda_param_info(expr, ctx),
        ExprKind::Member { .. } => ctx.namespace_value_callable_params_for_member_expr(expr),
        ExprKind::Paren(inner) => callable_param_info(inner, ctx),
        ExprKind::Compose { lhs, .. } => callable_param_info(lhs, ctx),
        ExprKind::If {
            then_block,
            else_branch,
            ..
        } => join_identical_if_branch_metadata(then_block, else_branch.as_deref(), |branch| {
            callable_param_info(branch, ctx)
        })
        .flatten(),
        ExprKind::Match { cases, .. } => {
            join_identical_match_arm_metadata(cases, |arm| callable_param_info(arm, ctx)).flatten()
        }
        _ => None,
    }
}

pub(super) fn static_string_literal_value(expr: &Expr, ctx: &Ctx<'_>) -> Option<String> {
    match &expr.kind {
        ExprKind::String(lit) if lit.tag.is_none() => decode_string_parts(&lit.parts, ctx.map).ok(),
        ExprKind::Paren(inner) => static_string_literal_value(inner, ctx),
        _ => None,
    }
}

pub(super) fn direct_binding_pattern_name(pattern: &Pattern, ctx: &Ctx<'_>) -> Option<String> {
    match &pattern.kind {
        PatternKind::Binding(name) | PatternKind::Typed { name, .. } => {
            Some(ctx.text(name.span).to_string())
        }
        _ => None,
    }
}

pub(super) fn some_pattern_binding_name(pattern: &Pattern, ctx: &Ctx<'_>) -> Option<String> {
    match &pattern.kind {
        PatternKind::Constructor { name, args }
            if ctx.text(name.span) == "Some" && args.len() == 1 =>
        {
            direct_binding_pattern_name(&args[0], ctx)
        }
        _ => None,
    }
}

pub(super) fn ok_pattern_binding_name(pattern: &Pattern, ctx: &Ctx<'_>) -> Option<String> {
    match &pattern.kind {
        PatternKind::Constructor { name, args }
            if ctx.text(name.span) == "Ok" && args.len() == 1 =>
        {
            direct_binding_pattern_name(&args[0], ctx)
        }
        _ => None,
    }
}

pub(super) fn emit_compose_expr(
    lhs: &Expr,
    rhs: &Expr,
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let first = emit_callable_value_expr(lhs, ctx)?;
    let second = emit_callable_value_expr(rhs, ctx)?;
    Ok(format!("tpz_compose({first}, {second}, {})", py_span(span)))
}

pub(super) fn emit_callable_value_expr(expr: &Expr, ctx: &Ctx<'_>) -> Result<String, PyEmitError> {
    match &expr.kind {
        ExprKind::Ident => {
            let name = ctx.text(expr.span);
            if !ctx.binding_is_bound(name)
                && let Some(info) = ctx.function_info(name)
            {
                return Ok(render_host_callable(info));
            }
            if let Some(py_name) = ctx.binding_py_name(name) {
                return Ok(py_name.to_string());
            }
            emit_expr(expr, ctx)
        }
        ExprKind::Lambda { .. } => emit_expr(expr, ctx),
        ExprKind::Paren(inner) => emit_callable_value_expr(inner, ctx),
        ExprKind::Compose { .. } => emit_expr(expr, ctx),
        _ => emit_expr(expr, ctx),
    }
}

pub(super) fn compose_value(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Compose { .. } => true,
        ExprKind::Paren(inner) => compose_value(inner),
        _ => false,
    }
}

pub(super) fn compose_binding_value(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    if compose_value(expr) {
        return true;
    }
    match &expr.kind {
        ExprKind::Ident => {
            let name = ctx.text(expr.span);
            ctx.binding_is_bound(name) && ctx.binding_is_composed(name)
        }
        ExprKind::Paren(inner) => compose_binding_value(inner, ctx),
        _ => false,
    }
}

pub(super) fn render_host_callable(info: &FunctionInfo) -> String {
    let variadic_py_name = info
        .params
        .last()
        .filter(|param| param.variadic)
        .map(|param| py_string(&param.py_name));
    match (info.cooperative_py_name.as_deref(), variadic_py_name) {
        (Some(cooperative_py_name), Some(variadic_py_name)) => format!(
            "tpz_host_callable({}, host, {cooperative_py_name}, {variadic_py_name})",
            info.py_name
        ),
        (Some(cooperative_py_name), None) => {
            format!(
                "tpz_host_callable({}, host, {cooperative_py_name})",
                info.py_name
            )
        }
        (None, Some(variadic_py_name)) => {
            format!(
                "tpz_host_callable({}, host, None, {variadic_py_name})",
                info.py_name
            )
        }
        (None, None) => format!("tpz_host_callable({}, host)", info.py_name),
    }
}

pub(super) fn receiver_shape_from_type(ty: &Type, ctx: &Ctx<'_>) -> Option<ReceiverShape> {
    if let Some(alias) = checked_alias_for_ast_type(ty, ctx) {
        return receiver_shape_from_checked_type(&alias.body);
    }
    match &ty.kind {
        TypeKind::Named { name, args } => match ctx.text(name.span) {
            "string" if args.is_empty() => Some(ReceiverShape::String),
            "Array" if args.len() == 1 => Some(ReceiverShape::Array),
            "Map" if args.len() == 2 => Some(ReceiverShape::Map),
            "Bytes" if args.is_empty() => Some(ReceiverShape::Bytes),
            "ByteBuffer" if args.is_empty() => Some(ReceiverShape::ByteBuffer),
            "JSONValue" if args.is_empty() => Some(ReceiverShape::Json),
            "Option" if args.len() == 1 => Some(ReceiverShape::Option),
            "Result" if args.len() == 2 => Some(ReceiverShape::Result),
            _ => None,
        },
        TypeKind::Qualified { .. }
        | TypeKind::Literal
        | TypeKind::Record(_)
        | TypeKind::Function { .. }
        | TypeKind::Unit
        | TypeKind::Union(_) => None,
    }
}

pub(super) fn receiver_shape_from_checked_type(ty: &CheckType) -> Option<ReceiverShape> {
    match ty {
        CheckType::Prim(CheckPrim::String) => Some(ReceiverShape::String),
        CheckType::Ctor(CheckCtor::Array, args) if args.len() == 1 => Some(ReceiverShape::Array),
        CheckType::Ctor(CheckCtor::Map, args) if args.len() == 2 => Some(ReceiverShape::Map),
        CheckType::Bytes => Some(ReceiverShape::Bytes),
        CheckType::ByteBuffer => Some(ReceiverShape::ByteBuffer),
        CheckType::JsonValue => Some(ReceiverShape::Json),
        CheckType::Ctor(CheckCtor::Option, args) if args.len() == 1 => Some(ReceiverShape::Option),
        CheckType::Ctor(CheckCtor::Result, args) if args.len() == 2 => Some(ReceiverShape::Result),
        _ => None,
    }
}

pub(super) fn array_element_metadata_from_type(ty: &Type, ctx: &Ctx<'_>) -> ArrayElementMetadata {
    if let Some(alias) = checked_alias_for_ast_type(ty, ctx) {
        return array_element_metadata_from_checked_type(&alias.body);
    }
    let TypeKind::Named { name, args } = &ty.kind else {
        return ArrayElementMetadata::default();
    };
    if ctx.text(name.span) != "Array" || args.len() != 1 {
        return ArrayElementMetadata::default();
    }
    let element_ty = &args[0];
    ArrayElementMetadata {
        receiver_shape: receiver_shape_from_type(element_ty, ctx),
        declared_callable_params: match &element_ty.kind {
            TypeKind::Function { params, .. } => Some(function_type_param_info(params)),
            _ => None,
        },
        declared_wrapped_value_metadata: wrapped_value_metadata_catalog_from_type(element_ty, ctx),
        declared_map_value: map_value_metadata_from_type(element_ty, ctx),
        declared_record_descendants: record_descendant_catalog_from_type(element_ty, ctx),
        ..ArrayElementMetadata::default()
    }
}

pub(super) fn array_element_metadata_from_checked_type(ty: &CheckType) -> ArrayElementMetadata {
    let CheckType::Ctor(CheckCtor::Array, args) = ty else {
        return ArrayElementMetadata::default();
    };
    let Some(element_ty) = args.first() else {
        return ArrayElementMetadata::default();
    };
    ArrayElementMetadata {
        receiver_shape: receiver_shape_from_checked_type(element_ty),
        declared_callable_params: match element_ty {
            CheckType::Func {
                params, variadic, ..
            } => Some(checked_function_type_param_info(
                params,
                variadic.as_deref(),
            )),
            _ => None,
        },
        declared_wrapped_value_metadata: wrapped_value_metadata_catalog_from_checked_type(
            element_ty,
        ),
        declared_map_value: map_value_metadata_from_checked_type(element_ty),
        declared_record_descendants: record_descendant_catalog_from_checked_type(element_ty),
        ..ArrayElementMetadata::default()
    }
}

pub(super) fn record_descendant_metadata_from_type(
    ty: &Type,
    ctx: &Ctx<'_>,
) -> RecordDescendantMetadata {
    let mut metadata = RecordDescendantMetadata::default();
    collect_record_descendant_metadata_from_ast_record(ty, "", ctx, &mut metadata);
    metadata
}

pub(super) fn collect_record_descendant_metadata_from_ast_record(
    ty: &Type,
    prefix: &str,
    ctx: &Ctx<'_>,
    metadata: &mut RecordDescendantMetadata,
) {
    if let Some(alias) = checked_alias_for_ast_type(ty, ctx) {
        collect_record_descendant_metadata_from_checked_record(&alias.body, prefix, metadata);
        return;
    }
    let TypeKind::Record(fields) = &ty.kind else {
        return;
    };
    for field in fields {
        let field_name = ctx.text(field.name.span);
        let path = record_field_path(prefix, field_name);
        if let Some(shape) = receiver_shape_from_type(&field.ty, ctx) {
            metadata.receiver_shapes.insert(path.clone(), shape);
        }
        if let TypeKind::Function { params, .. } = &field.ty.kind {
            metadata
                .callable_params
                .insert(path.clone(), function_type_param_info(params));
        }
        collect_record_descendant_metadata_from_ast_record(&field.ty, &path, ctx, metadata);
    }
}

pub(super) fn collect_record_descendant_metadata_from_checked_record(
    ty: &CheckType,
    prefix: &str,
    metadata: &mut RecordDescendantMetadata,
) {
    let CheckType::Record(fields) = ty else {
        return;
    };
    for (field_name, field_ty) in fields {
        let path = record_field_path(prefix, field_name);
        if let Some(shape) = receiver_shape_from_checked_type(field_ty) {
            metadata.receiver_shapes.insert(path.clone(), shape);
        }
        if let CheckType::Func {
            params, variadic, ..
        } = field_ty
        {
            metadata.callable_params.insert(
                path.clone(),
                checked_function_type_param_info(params, variadic.as_deref()),
            );
        }
        collect_record_descendant_metadata_from_checked_record(field_ty, &path, metadata);
    }
}

pub(super) fn record_field_path(prefix: &str, field: &str) -> String {
    if prefix.is_empty() {
        field.to_string()
    } else {
        format!("{prefix}.{field}")
    }
}

pub(super) fn record_descendant_catalog_from_type(
    ty: &Type,
    ctx: &Ctx<'_>,
) -> RecordDescendantCatalog {
    let mut catalog = RecordDescendantCatalog::default();
    collect_record_descendant_catalog_from_type(ty, ctx, &mut Vec::new(), &mut catalog);
    catalog
}

pub(super) fn record_descendant_catalog_from_checked_type(
    ty: &CheckType,
) -> RecordDescendantCatalog {
    let mut catalog = RecordDescendantCatalog::default();
    collect_record_descendant_catalog_from_checked_type(ty, &mut Vec::new(), &mut catalog);
    catalog
}

pub(super) fn collect_record_descendant_catalog_from_type(
    ty: &Type,
    ctx: &Ctx<'_>,
    path: &mut Vec<RecordWrapper>,
    catalog: &mut RecordDescendantCatalog,
) {
    if let Some(alias) = checked_alias_for_ast_type(ty, ctx) {
        collect_record_descendant_catalog_from_checked_type(&alias.body, path, catalog);
        return;
    }
    match &ty.kind {
        TypeKind::Record(_) => {
            catalog.insert(path.clone(), record_descendant_metadata_from_type(ty, ctx))
        }
        TypeKind::Named { name, args } if ctx.text(name.span) == "Option" && args.len() == 1 => {
            path.push(RecordWrapper::Option);
            collect_record_descendant_catalog_from_type(&args[0], ctx, path, catalog);
            path.pop();
        }
        TypeKind::Named { name, args } if ctx.text(name.span) == "Result" && args.len() == 2 => {
            path.push(RecordWrapper::ResultOk);
            collect_record_descendant_catalog_from_type(&args[0], ctx, path, catalog);
            path.pop();
        }
        TypeKind::Named { name, args } if ctx.text(name.span) == "Map" && args.len() == 2 => {
            path.push(RecordWrapper::MapValue);
            collect_record_descendant_catalog_from_type(&args[1], ctx, path, catalog);
            path.pop();
        }
        _ => {}
    }
}

pub(super) fn collect_record_descendant_catalog_from_checked_type(
    ty: &CheckType,
    path: &mut Vec<RecordWrapper>,
    catalog: &mut RecordDescendantCatalog,
) {
    match ty {
        CheckType::Record(_) => {
            let mut metadata = RecordDescendantMetadata::default();
            collect_record_descendant_metadata_from_checked_record(ty, "", &mut metadata);
            catalog.insert(path.clone(), metadata);
        }
        CheckType::Ctor(CheckCtor::Option, args) if args.len() == 1 => {
            path.push(RecordWrapper::Option);
            collect_record_descendant_catalog_from_checked_type(&args[0], path, catalog);
            path.pop();
        }
        CheckType::Ctor(CheckCtor::Result, args) if args.len() == 2 => {
            path.push(RecordWrapper::ResultOk);
            collect_record_descendant_catalog_from_checked_type(&args[0], path, catalog);
            path.pop();
        }
        CheckType::Ctor(CheckCtor::Map, args) if args.len() == 2 => {
            path.push(RecordWrapper::MapValue);
            collect_record_descendant_catalog_from_checked_type(&args[1], path, catalog);
            path.pop();
        }
        _ => {}
    }
}

pub(super) fn receiver_shape_from_value(expr: &Expr, ctx: &Ctx<'_>) -> Option<ReceiverShape> {
    if string_value(expr, ctx) {
        Some(ReceiverShape::String)
    } else if template_value(expr, ctx) {
        Some(ReceiverShape::Template)
    } else if array_value(expr, ctx) {
        Some(ReceiverShape::Array)
    } else if map_value(expr, ctx) {
        Some(ReceiverShape::Map)
    } else if bytes_value(expr, ctx) {
        Some(ReceiverShape::Bytes)
    } else if byte_buffer_value(expr, ctx) {
        Some(ReceiverShape::ByteBuffer)
    } else if json_value(expr, ctx) {
        Some(ReceiverShape::Json)
    } else if option_value(expr, ctx) {
        Some(ReceiverShape::Option)
    } else if result_value(expr, ctx) {
        Some(ReceiverShape::Result)
    } else if let ExprKind::If {
        then_block,
        else_branch,
        ..
    } = &expr.kind
    {
        join_identical_if_branch_metadata(then_block, else_branch.as_deref(), |branch| {
            receiver_shape_from_value(branch, ctx)
        })
        .flatten()
    } else if let ExprKind::Match { cases, .. } = &expr.kind {
        join_identical_match_arm_metadata(cases, |arm| receiver_shape_from_value(arm, ctx))
            .flatten()
    } else if let ExprKind::Pipe { rhs, .. } = &expr.kind {
        pipe_return_shape(rhs, ctx)
    } else {
        None
    }
}

pub(super) fn wrapped_value_metadata_catalog_from_type(
    ty: &Type,
    ctx: &Ctx<'_>,
) -> WrappedValueMetadataCatalog {
    if let Some(alias) = checked_alias_for_ast_type(ty, ctx) {
        return wrapped_value_metadata_catalog_from_checked_type(&alias.body);
    }
    let TypeKind::Named { name, args } = &ty.kind else {
        return WrappedValueMetadataCatalog::default();
    };
    let (wrapper, inner) = match ctx.text(name.span) {
        "Option" if args.len() == 1 => (RecordWrapper::Option, &args[0]),
        "Result" if args.len() == 2 => (RecordWrapper::ResultOk, &args[0]),
        _ => return WrappedValueMetadataCatalog::default(),
    };
    let mut catalog = wrapped_value_metadata_catalog_from_type(inner, ctx).prepended(wrapper);
    catalog.insert_root(
        wrapper,
        WrappedValueMetadata {
            receiver_shape: receiver_shape_from_type(inner, ctx),
            callable_params: function_callable_params_from_type(inner, ctx),
        },
    );
    catalog
}

pub(super) fn wrapped_value_metadata_catalog_from_checked_type(
    ty: &CheckType,
) -> WrappedValueMetadataCatalog {
    let (wrapper, inner) = match ty {
        CheckType::Ctor(CheckCtor::Option, args) if args.len() == 1 => {
            (RecordWrapper::Option, &args[0])
        }
        CheckType::Ctor(CheckCtor::Result, args) if args.len() == 2 => {
            (RecordWrapper::ResultOk, &args[0])
        }
        _ => return WrappedValueMetadataCatalog::default(),
    };
    let mut catalog = wrapped_value_metadata_catalog_from_checked_type(inner).prepended(wrapper);
    catalog.insert_root(
        wrapper,
        WrappedValueMetadata {
            receiver_shape: receiver_shape_from_checked_type(inner),
            callable_params: match inner {
                CheckType::Func {
                    params, variadic, ..
                } => Some(checked_function_type_param_info(
                    params,
                    variadic.as_deref(),
                )),
                _ => None,
            },
        },
    );
    catalog
}

pub(super) fn call_return_shape(expr: &Expr, ctx: &Ctx<'_>) -> Option<ReceiverShape> {
    let ExprKind::Call { callee, .. } = &expr.kind else {
        return None;
    };
    match &callee.kind {
        ExprKind::Ident => {
            let name = ctx.text(callee.span);
            if ctx.binding_is_bound(name) {
                None
            } else {
                ctx.function_info(name).and_then(|info| info.return_shape)
            }
        }
        ExprKind::Member { object, field } => {
            let method = ctx.text(field.span);
            if let ExprKind::Ident = &object.kind {
                let namespace = ctx.text(object.span);
                if let Some(ModuleRuntimeExport::Function { info }) =
                    ctx.namespace_export(namespace, method)
                {
                    return info.return_shape;
                }
                if !ctx.binding_is_bound(namespace) {
                    return namespace_builtin_call_metadata(namespace, method).return_shape;
                }
            }
            builtin_receiver_return_shape(object, method, ctx)
        }
        ExprKind::Paren(inner) => call_return_shape(inner, ctx),
        _ => None,
    }
}

pub(super) fn pipe_return_shape(rhs: &PipeRhs, ctx: &Ctx<'_>) -> Option<ReceiverShape> {
    match rhs {
        PipeRhs::Expr(stage) => expr_return_shape(stage, ctx),
        PipeRhs::Field(_) => None,
    }
}

pub(super) fn expr_return_shape(expr: &Expr, ctx: &Ctx<'_>) -> Option<ReceiverShape> {
    match &expr.kind {
        ExprKind::Call { .. } => call_return_shape(expr, ctx),
        ExprKind::Try(_) => try_result_ok_shape(expr, ctx),
        ExprKind::Pipe { rhs, .. } => pipe_return_shape(rhs, ctx),
        ExprKind::Paren(inner) => expr_return_shape(inner, ctx),
        _ => receiver_shape_from_value(expr, ctx),
    }
}

pub(super) fn builtin_receiver_return_shape(
    object: &Expr,
    method: &str,
    ctx: &Ctx<'_>,
) -> Option<ReceiverShape> {
    let receiver_shape = receiver_shape_from_value(object, ctx)
        .or_else(|| ctx.receiver_shape_for_member_expr(object))
        .or_else(|| ctx.receiver_shape_for_index_expr(object));
    if method == "get"
        || method == "at"
            && matches!(
                receiver_shape,
                Some(ReceiverShape::Array | ReceiverShape::Json)
            )
    {
        return Some(ReceiverShape::Option);
    }
    if receiver_is_array_value(object, ctx) {
        match method {
            "slice" | "sorted" | "map" | "filter" => return Some(ReceiverShape::Array),
            "join" => return Some(ReceiverShape::String),
            _ => {}
        }
    }
    if string_value(object, ctx) {
        match method {
            "trim" | "trimStart" | "trimEnd" | "slice" | "replace" => {
                return Some(ReceiverShape::String);
            }
            "split" | "scalars" => return Some(ReceiverShape::Array),
            _ => {}
        }
    }
    None
}

pub(super) fn call_result_ok_shape(expr: &Expr, ctx: &Ctx<'_>) -> Option<ReceiverShape> {
    let ExprKind::Call { callee, .. } = &expr.kind else {
        return None;
    };
    match &callee.kind {
        ExprKind::Ident => {
            let name = ctx.text(callee.span);
            if ctx.binding_is_bound(name) {
                None
            } else {
                ctx.function_info(name).and_then(|info| {
                    info.return_wrapped_metadata
                        .root(RecordWrapper::ResultOk)
                        .receiver_shape
                })
            }
        }
        ExprKind::Member { object, field } => {
            let ExprKind::Ident = &object.kind else {
                return None;
            };
            let namespace = ctx.text(object.span);
            let method = ctx.text(field.span);
            match ctx.namespace_export(namespace, method) {
                Some(ModuleRuntimeExport::Function { info }) => {
                    info.return_wrapped_metadata
                        .root(RecordWrapper::ResultOk)
                        .receiver_shape
                }
                _ if !ctx.binding_is_bound(namespace) => {
                    namespace_builtin_call_metadata(namespace, method).result_ok_shape
                }
                _ => None,
            }
        }
        ExprKind::Paren(inner) => call_result_ok_shape(inner, ctx),
        _ => None,
    }
}

pub(super) fn try_result_ok_shape(expr: &Expr, ctx: &Ctx<'_>) -> Option<ReceiverShape> {
    match &expr.kind {
        ExprKind::Try(inner) => call_result_ok_shape(inner, ctx),
        ExprKind::Paren(inner) => try_result_ok_shape(inner, ctx),
        _ => None,
    }
}

pub(super) fn projected_receiver_shape(expr: &Expr, ctx: &Ctx<'_>) -> Option<ReceiverShape> {
    ctx.receiver_shape_for_member_expr(expr)
        .or_else(|| ctx.receiver_shape_for_index_expr(expr))
}

pub(super) fn string_value(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    match &expr.kind {
        ExprKind::String(lit) => lit.tag.is_none(),
        ExprKind::Ident => ctx.binding_is_string(ctx.text(expr.span)),
        ExprKind::Call { .. } => call_return_shape(expr, ctx) == Some(ReceiverShape::String),
        ExprKind::Try(_) => try_result_ok_shape(expr, ctx) == Some(ReceiverShape::String),
        ExprKind::Pipe { rhs, .. } => pipe_return_shape(rhs, ctx) == Some(ReceiverShape::String),
        ExprKind::Member { .. } | ExprKind::Index { .. } => {
            projected_receiver_shape(expr, ctx) == Some(ReceiverShape::String)
        }
        ExprKind::Paren(inner) => string_value(inner, ctx),
        _ => false,
    }
}

pub(super) fn template_value(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    match &expr.kind {
        ExprKind::String(lit) => lit.tag.is_some(),
        ExprKind::Ident => ctx.binding_is_template(ctx.text(expr.span)),
        ExprKind::Call { .. } => call_return_shape(expr, ctx) == Some(ReceiverShape::Template),
        ExprKind::Pipe { rhs, .. } => pipe_return_shape(rhs, ctx) == Some(ReceiverShape::Template),
        ExprKind::Member { .. } | ExprKind::Index { .. } => {
            projected_receiver_shape(expr, ctx) == Some(ReceiverShape::Template)
        }
        ExprKind::Paren(inner) => template_value(inner, ctx),
        _ => false,
    }
}

pub(super) fn array_value(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    match &expr.kind {
        ExprKind::Array(_) => true,
        ExprKind::Ident => ctx.binding_is_array(ctx.text(expr.span)),
        ExprKind::Comprehension {
            kind: CompKind::Array,
            ..
        } => true,
        ExprKind::Call { callee, .. } => {
            call_return_shape(expr, ctx) == Some(ReceiverShape::Array)
                || matches!(static_member_name(callee, ctx), Some(("Array", "of")))
        }
        ExprKind::Try(_) => try_result_ok_shape(expr, ctx) == Some(ReceiverShape::Array),
        ExprKind::Pipe { rhs, .. } => pipe_return_shape(rhs, ctx) == Some(ReceiverShape::Array),
        ExprKind::Member { .. } | ExprKind::Index { .. } => {
            projected_receiver_shape(expr, ctx) == Some(ReceiverShape::Array)
        }
        ExprKind::Paren(inner) => array_value(inner, ctx),
        _ => false,
    }
}

pub(super) fn map_value(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    match &expr.kind {
        ExprKind::MapLiteral(_) => true,
        ExprKind::Ident => ctx.binding_is_map(ctx.text(expr.span)),
        ExprKind::Comprehension {
            kind: CompKind::Map,
            ..
        } => true,
        ExprKind::Call { callee, args, .. } => {
            call_return_shape(expr, ctx) == Some(ReceiverShape::Map)
                || matches!(static_member_name(callee, ctx), Some(("Map", "ofEntries")))
                || (matches!(static_member_name(callee, ctx), Some(("Map", "new")))
                    && args.is_empty())
        }
        ExprKind::Try(_) => try_result_ok_shape(expr, ctx) == Some(ReceiverShape::Map),
        ExprKind::Pipe { rhs, .. } => pipe_return_shape(rhs, ctx) == Some(ReceiverShape::Map),
        ExprKind::Member { .. } | ExprKind::Index { .. } => {
            projected_receiver_shape(expr, ctx) == Some(ReceiverShape::Map)
        }
        ExprKind::Paren(inner) => map_value(inner, ctx),
        _ => false,
    }
}

pub(super) fn bytes_value(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    match &expr.kind {
        ExprKind::Ident => ctx.binding_is_bytes(ctx.text(expr.span)),
        ExprKind::Call { callee, .. } => {
            matches!(
                static_member_name(callee, ctx),
                Some(("Bytes", "empty" | "encodeUtf8" | "concat"))
                    | Some(("Encoding", "utf8Encode"))
            ) || call_return_shape(expr, ctx) == Some(ReceiverShape::Bytes)
        }
        ExprKind::Try(inner) => {
            try_result_ok_shape(expr, ctx) == Some(ReceiverShape::Bytes)
                || matches!(
                    &inner.kind,
                    ExprKind::Call { callee, .. }
                        if matches!(
                            static_member_name(callee, ctx),
                            Some(("Bytes", "fromArray" | "fromHex" | "fromBase64"))
                                | Some(("Encoding", "hexDecode" | "base64Decode"))
                        )
                )
        }
        ExprKind::Pipe { rhs, .. } => pipe_return_shape(rhs, ctx) == Some(ReceiverShape::Bytes),
        ExprKind::Member { .. } | ExprKind::Index { .. } => {
            projected_receiver_shape(expr, ctx) == Some(ReceiverShape::Bytes)
        }
        ExprKind::Paren(inner) => bytes_value(inner, ctx),
        _ => false,
    }
}

pub(super) fn byte_buffer_value(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    match &expr.kind {
        ExprKind::Ident => ctx.binding_is_byte_buffer(ctx.text(expr.span)),
        ExprKind::Call { callee, .. } => {
            matches!(
                static_member_name(callee, ctx),
                Some(("ByteBuffer", "allocate" | "fromBytes"))
            ) || call_return_shape(expr, ctx) == Some(ReceiverShape::ByteBuffer)
        }
        ExprKind::Pipe { rhs, .. } => {
            pipe_return_shape(rhs, ctx) == Some(ReceiverShape::ByteBuffer)
        }
        ExprKind::Member { .. } | ExprKind::Index { .. } => {
            projected_receiver_shape(expr, ctx) == Some(ReceiverShape::ByteBuffer)
        }
        ExprKind::Paren(inner) => byte_buffer_value(inner, ctx),
        _ => false,
    }
}

pub(super) fn json_value(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    match &expr.kind {
        ExprKind::Ident => ctx.binding_is_json(ctx.text(expr.span)),
        ExprKind::Call { .. } => call_return_shape(expr, ctx) == Some(ReceiverShape::Json),
        ExprKind::Try(inner) => {
            try_result_ok_shape(expr, ctx) == Some(ReceiverShape::Json)
                || matches!(
                    &inner.kind,
                    ExprKind::Call { callee, .. }
                        if matches!(static_member_name(callee, ctx), Some(("JSON", "parse")))
                )
        }
        ExprKind::Pipe { rhs, .. } => pipe_return_shape(rhs, ctx) == Some(ReceiverShape::Json),
        ExprKind::Member { .. } | ExprKind::Index { .. } => {
            projected_receiver_shape(expr, ctx) == Some(ReceiverShape::Json)
        }
        ExprKind::Paren(inner) => json_value(inner, ctx),
        _ => false,
    }
}

pub(super) fn option_value(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    match &expr.kind {
        ExprKind::Ident => {
            ctx.text(expr.span) == "None" || ctx.binding_is_option(ctx.text(expr.span))
        }
        ExprKind::Member { .. } => {
            ctx.receiver_shape_for_member_expr(expr) == Some(ReceiverShape::Option)
        }
        ExprKind::Index { .. } => {
            ctx.receiver_shape_for_index_expr(expr) == Some(ReceiverShape::Option)
        }
        ExprKind::Call { callee, args, .. } => {
            call_return_shape(expr, ctx) == Some(ReceiverShape::Option)
                || (matches!(constructor_name(callee, ctx), Some("Some") | Some("None"))
                    && args.len() <= 1)
        }
        ExprKind::Pipe { rhs, .. } => pipe_return_shape(rhs, ctx) == Some(ReceiverShape::Option),
        ExprKind::Paren(inner) => option_value(inner, ctx),
        _ => false,
    }
}

pub(super) fn result_value(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    match &expr.kind {
        ExprKind::Ident => ctx.binding_is_result(ctx.text(expr.span)),
        ExprKind::Member { .. } => {
            ctx.receiver_shape_for_member_expr(expr) == Some(ReceiverShape::Result)
        }
        ExprKind::Index { .. } => {
            ctx.receiver_shape_for_index_expr(expr) == Some(ReceiverShape::Result)
        }
        ExprKind::Call { callee, args, .. } => {
            call_return_shape(expr, ctx) == Some(ReceiverShape::Result)
                || (matches!(constructor_name(callee, ctx), Some("Ok") | Some("Err"))
                    && args.len() == 1)
        }
        ExprKind::Pipe { rhs, .. } => pipe_return_shape(rhs, ctx) == Some(ReceiverShape::Result),
        ExprKind::Paren(inner) => result_value(inner, ctx),
        _ => false,
    }
}

pub(super) fn constructor_name<'a>(expr: &Expr, ctx: &'a Ctx<'_>) -> Option<&'a str> {
    match &expr.kind {
        ExprKind::Ident => {
            let name = ctx.text(expr.span);
            if ctx.binding_is_bound(name) {
                None
            } else {
                Some(name)
            }
        }
        ExprKind::Paren(inner) => constructor_name(inner, ctx),
        _ => None,
    }
}

pub(super) fn constructor_single_value_arg<'a>(
    args: &'a [CallArg],
    ctx: &Ctx<'_>,
) -> Option<&'a Expr> {
    match args {
        [CallArg::Positional(value)] => Some(value),
        [CallArg::Named { name, value }] if ctx.text(name.span) == "value" => Some(value),
        _ => None,
    }
}

pub(super) fn static_member_name<'a>(expr: &Expr, ctx: &'a Ctx<'_>) -> Option<(&'a str, &'a str)> {
    match &expr.kind {
        ExprKind::Member { object, field } => {
            if let ExprKind::Ident = &object.kind {
                let namespace = ctx.text(object.span);
                if ctx.binding_is_bound(namespace) {
                    None
                } else {
                    Some((namespace, ctx.text(field.span)))
                }
            } else {
                None
            }
        }
        ExprKind::Paren(inner) => static_member_name(inner, ctx),
        _ => None,
    }
}

pub(super) fn receiver_is_array_value(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    match &expr.kind {
        ExprKind::Array(_) => true,
        ExprKind::Ident => ctx.binding_is_array(ctx.text(expr.span)),
        ExprKind::Call { .. }
        | ExprKind::Try(_)
        | ExprKind::Index { .. }
        | ExprKind::Member { .. } => array_value(expr, ctx),
        ExprKind::Paren(inner) => receiver_is_array_value(inner, ctx),
        _ => false,
    }
}

pub(super) fn receiver_is_map_value(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    match &expr.kind {
        ExprKind::MapLiteral(_) => true,
        ExprKind::Ident => ctx.binding_is_map(ctx.text(expr.span)),
        ExprKind::Call { .. }
        | ExprKind::Try(_)
        | ExprKind::Index { .. }
        | ExprKind::Member { .. } => map_value(expr, ctx),
        ExprKind::Paren(inner) => receiver_is_map_value(inner, ctx),
        _ => false,
    }
}

pub(super) fn receiver_is_bytes_value(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    match &expr.kind {
        ExprKind::Ident => ctx.binding_is_bytes(ctx.text(expr.span)),
        ExprKind::Call { .. }
        | ExprKind::Try(_)
        | ExprKind::Index { .. }
        | ExprKind::Member { .. } => bytes_value(expr, ctx),
        ExprKind::Paren(inner) => receiver_is_bytes_value(inner, ctx),
        _ => false,
    }
}

pub(super) fn receiver_is_byte_buffer_value(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    match &expr.kind {
        ExprKind::Ident => ctx.binding_is_byte_buffer(ctx.text(expr.span)),
        ExprKind::Call { .. } | ExprKind::Index { .. } | ExprKind::Member { .. } => {
            byte_buffer_value(expr, ctx)
        }
        ExprKind::Paren(inner) => receiver_is_byte_buffer_value(inner, ctx),
        _ => false,
    }
}

pub(super) fn receiver_is_json_value(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    match &expr.kind {
        ExprKind::Ident => ctx.binding_is_json(ctx.text(expr.span)),
        ExprKind::Call { .. }
        | ExprKind::Try(_)
        | ExprKind::Index { .. }
        | ExprKind::Member { .. } => json_value(expr, ctx),
        ExprKind::Paren(inner) => receiver_is_json_value(inner, ctx),
        _ => false,
    }
}

pub(super) fn receiver_is_option_value(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    match &expr.kind {
        ExprKind::Ident => {
            let name = ctx.text(expr.span);
            name == "None" || ctx.binding_is_option(name)
        }
        ExprKind::Member { .. } => {
            ctx.receiver_shape_for_member_expr(expr) == Some(ReceiverShape::Option)
        }
        ExprKind::Index { .. } => {
            ctx.receiver_shape_for_index_expr(expr) == Some(ReceiverShape::Option)
        }
        ExprKind::Call { .. } => option_value(expr, ctx),
        ExprKind::Paren(inner) => receiver_is_option_value(inner, ctx),
        _ => false,
    }
}

pub(super) fn receiver_is_result_value(expr: &Expr, ctx: &Ctx<'_>) -> bool {
    match &expr.kind {
        ExprKind::Ident => ctx.binding_is_result(ctx.text(expr.span)),
        ExprKind::Member { .. } => {
            ctx.receiver_shape_for_member_expr(expr) == Some(ReceiverShape::Result)
        }
        ExprKind::Index { .. } => {
            ctx.receiver_shape_for_index_expr(expr) == Some(ReceiverShape::Result)
        }
        ExprKind::Call { .. } => result_value(expr, ctx),
        ExprKind::Paren(inner) => receiver_is_result_value(inner, ctx),
        _ => false,
    }
}

impl<'a> Ctx<'a> {
    pub(super) fn map_value_metadata_for_value(
        &self,
        value: &Expr,
        mutable: bool,
    ) -> MapValueMetadata {
        match &value.kind {
            ExprKind::MapLiteral(entries) => {
                let observed_by_key = self.map_observed_values_by_key_for_value(value, false);
                let known_present_keys = observed_by_key.keys().cloned().collect();
                MapValueMetadata {
                    observed_by_key,
                    known_present_keys,
                    observed_keys_complete: entries
                        .iter()
                        .all(|(key, _)| static_string_literal_value(key, self).is_some()),
                    ..MapValueMetadata::default()
                }
            }
            ExprKind::Ident => {
                let name = self.text(value.span);
                self.binding_lookup(name)
                    .filter(|(_, info)| {
                        self.binding_allows_value_static_metadata(name, info)
                            && ((!mutable && !info.mutable)
                                || info.namespace_member_value_metadata
                                || collection_storage_is_local(info))
                    })
                    .map(|(_, info)| info.map_value.clone())
                    .unwrap_or_default()
            }
            ExprKind::Member { .. } if !mutable => self
                .namespace_value_metadata_for_member_expr(value)
                .map(|metadata| metadata.map_value.clone())
                .unwrap_or_default(),
            ExprKind::Paren(inner) => self.map_value_metadata_for_value(inner, mutable),
            ExprKind::If {
                then_block,
                else_branch,
                ..
            } => join_identical_if_branch_metadata(then_block, else_branch.as_deref(), |branch| {
                self.map_value_metadata_for_value(branch, mutable)
            })
            .unwrap_or_default(),
            ExprKind::Match { cases, .. } => join_identical_match_arm_metadata(cases, |arm| {
                self.map_value_metadata_for_value(arm, mutable)
            })
            .unwrap_or_default(),
            _ => MapValueMetadata::default(),
        }
    }

    pub(super) fn map_observed_values_by_key_for_value(
        &self,
        value: &Expr,
        mutable: bool,
    ) -> BTreeMap<String, StaticMapValueMetadata> {
        if mutable {
            return BTreeMap::new();
        }
        match &value.kind {
            ExprKind::MapLiteral(entries) => {
                let mut observed = BTreeMap::new();
                for (key, value) in entries {
                    let Some(key) = static_string_literal_value(key, self) else {
                        continue;
                    };
                    let metadata = static_map_value_metadata_for_value(value, self);
                    observed.insert(key, metadata);
                }
                observed
            }
            ExprKind::Ident => {
                let name = self.text(value.span);
                self.binding_lookup(name)
                    .filter(|(_, info)| !info.mutable)
                    .map(|(_, info)| info.map_value.observed_by_key.clone())
                    .unwrap_or_default()
            }
            ExprKind::Member { .. } => self
                .namespace_value_metadata_for_member_expr(value)
                .map(|metadata| metadata.map_value.observed_by_key.clone())
                .unwrap_or_default(),
            ExprKind::Paren(inner) => self.map_observed_values_by_key_for_value(inner, mutable),
            ExprKind::If {
                then_block,
                else_branch,
                ..
            } => join_identical_if_branch_metadata(then_block, else_branch.as_deref(), |branch| {
                self.map_observed_values_by_key_for_value(branch, mutable)
            })
            .unwrap_or_default(),
            ExprKind::Match { cases, .. } => join_identical_match_arm_metadata(cases, |arm| {
                self.map_observed_values_by_key_for_value(arm, mutable)
            })
            .unwrap_or_default(),
            _ => BTreeMap::new(),
        }
    }

    pub(super) fn typed_rebind_callable_params_for_value(
        &self,
        value: &Expr,
    ) -> Option<Vec<FunctionParamInfo>> {
        match &value.kind {
            ExprKind::Ident => {
                let name = self.text(value.span);
                self.binding_lookup(name)
                    .and_then(|(_, info)| info.typed_rebind_callable_params.clone())
            }
            ExprKind::Paren(inner) => self.typed_rebind_callable_params_for_value(inner),
            _ => None,
        }
    }

    pub(super) fn map_value_pattern_projection<'b>(
        &'b self,
        object: &Expr,
        key: &Expr,
    ) -> Option<MapValuePatternProjection<'b>> {
        if let ExprKind::Paren(inner) = &object.kind {
            return self.map_value_pattern_projection(inner, key);
        }
        let static_key = static_string_literal_value(key, self);
        match &object.kind {
            ExprKind::Ident => {
                let source_name = self.text(object.span);
                let (_, info) = self.binding_lookup(source_name)?;
                let observed = if self.binding_allows_value_static_metadata(source_name, info)
                    && (!info.mutable
                        || info.namespace_member_value_metadata
                        || collection_storage_is_local(info))
                {
                    observed_map_value_metadata(&info.map_value, static_key.as_deref())
                } else {
                    None
                };
                Some(MapValuePatternProjection {
                    metadata: &info.map_value,
                    observed,
                    record_descendants: self.binding_record_descendant_catalog_under(
                        source_name,
                        info,
                        RecordWrapper::MapValue,
                    ),
                })
            }
            ExprKind::Member { .. } => {
                let metadata = self.namespace_value_metadata_for_member_expr(object)?;
                let observed =
                    observed_map_value_metadata(&metadata.map_value, static_key.as_deref());
                Some(MapValuePatternProjection {
                    metadata: &metadata.map_value,
                    observed,
                    record_descendants: metadata
                        .record_descendants
                        .project(RecordWrapper::MapValue),
                })
            }
            ExprKind::Index { object, index } => self
                .array_element_projection_for_index(object, index)
                .map(|projection| projection.map_value_pattern_projection(static_key.as_deref())),
            ExprKind::Paren(_) => unreachable!("parenthesized Map object handled above"),
            _ => None,
        }
    }

    pub(super) fn nominal_record_default_callable_metadata(
        &self,
        value: &Expr,
    ) -> NominalRecordDefaultCallableMetadata {
        NominalRecordDefaultCallableMetadata {
            cooperative_callback_target: self.cooperative_callback_target_for_value(value, false),
            callable_params: callable_param_info(value, self),
            record_descendants: self.record_descendant_metadata_for_value(value, false),
        }
    }

    pub(super) fn apply_record_field_value_metadata(
        &self,
        metadata: &mut RecordDescendantMetadata,
        field_name: &str,
        value: &Expr,
        mutable: bool,
    ) {
        let projection = self.record_field_value_projection(value, mutable);
        if let Some(shape) = projection.receiver_shape {
            metadata
                .receiver_shapes
                .insert(field_name.to_string(), shape);
        }
        if let Some(target) = projection.cooperative_callback_target {
            metadata
                .cooperative_callback_targets
                .insert(field_name.to_string(), target);
        }
        if let Some(callable_params) = projection.callable_params {
            metadata
                .callable_params
                .insert(field_name.to_string(), callable_params);
        }
        let nested = self.record_descendant_metadata_for_value(value, false);
        for (nested_path, shape) in nested.receiver_shapes {
            metadata
                .receiver_shapes
                .insert(format!("{field_name}.{nested_path}"), shape);
        }
        for (nested_path, callable_params) in nested.callable_params {
            metadata
                .callable_params
                .insert(format!("{field_name}.{nested_path}"), callable_params);
        }
        for (nested_path, target) in nested.cooperative_callback_targets {
            metadata
                .cooperative_callback_targets
                .insert(format!("{field_name}.{nested_path}"), target);
        }
    }

    pub(super) fn record_field_value_projection(
        &self,
        value: &Expr,
        mutable: bool,
    ) -> RecordFieldProjection {
        RecordFieldProjection {
            receiver_shape: receiver_shape_from_value(value, self),
            callable_params: if mutable
                && self.namespace_member_value_metadata_origin_for_value(value)
            {
                None
            } else {
                callable_param_info(value, self)
            },
            cooperative_callback_target: self.cooperative_callback_target_for_value(value, false),
        }
    }

    pub(super) fn record_descendant_metadata_for_value(
        &self,
        value: &Expr,
        mutable: bool,
    ) -> RecordDescendantMetadata {
        match &value.kind {
            ExprKind::RecordLiteral { fields } => {
                let mut metadata = RecordDescendantMetadata::default();
                for field in fields {
                    let field_name = self.text(field.name.span).to_string();
                    self.apply_record_field_value_metadata(
                        &mut metadata,
                        &field_name,
                        &field.value,
                        mutable,
                    );
                }
                metadata
            }
            ExprKind::RecordUpdate { base, fields, .. } => {
                let nominal = nominal_record_for_construct_base(base, self);
                let mut metadata = if let Some(record) = nominal {
                    let mut declared = RecordDescendantMetadata::default();
                    for field in &record.fields {
                        if let Some(shape) = receiver_shape_from_type(field.ty, self) {
                            declared
                                .receiver_shapes
                                .insert(field.source_name.clone(), shape);
                        }
                        if let Some(params) = function_callable_params_from_type(field.ty, self) {
                            declared
                                .callable_params
                                .insert(field.source_name.clone(), params);
                        }
                        let nested = record_descendant_metadata_from_type(field.ty, self);
                        for (nested_path, shape) in nested.receiver_shapes {
                            declared
                                .receiver_shapes
                                .insert(format!("{}.{}", field.source_name, nested_path), shape);
                        }
                        for (nested_path, params) in nested.callable_params {
                            declared
                                .callable_params
                                .insert(format!("{}.{}", field.source_name, nested_path), params);
                        }
                        let Some(default) = &field.default else {
                            continue;
                        };
                        let synthesized_metadata;
                        let callable_metadata = if let Some(metadata) = &default.callable_metadata {
                            metadata.as_ref()
                        } else {
                            synthesized_metadata =
                                self.nominal_record_default_callable_metadata(default.expr);
                            &synthesized_metadata
                        };
                        if let Some(target) = &callable_metadata.cooperative_callback_target {
                            declared
                                .cooperative_callback_targets
                                .insert(field.source_name.clone(), target.clone());
                        }
                        if let Some(params) = &callable_metadata.callable_params {
                            declared
                                .callable_params
                                .insert(field.source_name.clone(), params.clone());
                        }
                        for (nested_path, params) in
                            &callable_metadata.record_descendants.callable_params
                        {
                            declared.callable_params.insert(
                                format!("{}.{}", field.source_name, nested_path),
                                params.clone(),
                            );
                        }
                        for (nested_path, target) in &callable_metadata
                            .record_descendants
                            .cooperative_callback_targets
                        {
                            declared.cooperative_callback_targets.insert(
                                format!("{}.{}", field.source_name, nested_path),
                                target.clone(),
                            );
                        }
                    }
                    declared
                } else {
                    self.record_descendant_metadata_for_value(base, mutable)
                };
                for field in fields {
                    let field_name = self.text(field.name.span).to_string();
                    let mut replacement = RecordDescendantMetadata::default();
                    self.apply_record_field_value_metadata(
                        &mut replacement,
                        &field_name,
                        &field.value,
                        mutable,
                    );
                    metadata.replace_field_subtree(
                        &field_name,
                        replacement,
                        if nominal.is_some() {
                            RecordDescendantReplacement::PreserveReceiverShapes
                        } else {
                            RecordDescendantReplacement::AllAxes
                        },
                    );
                }
                metadata
            }
            ExprKind::Ident => {
                let name = self.text(value.span);
                self.binding_lookup(name)
                    .filter(|(_, info)| {
                        let loses_mutable_flow = info.mutable && mutable;
                        let loses_namespace_flow =
                            info.namespace_member_value_metadata && (info.mutable || mutable);
                        self.binding_allows_flow_static_metadata(name, info)
                            && !loses_mutable_flow
                            && !loses_namespace_flow
                    })
                    .map(|(_, info)| self.binding_record_descendant_metadata(name, info, &[]))
                    .unwrap_or_default()
            }
            ExprKind::Member { .. } => {
                let mut metadata = self
                    .namespace_value_metadata_for_member_expr(value)
                    .map(|metadata| metadata.record_descendants.cloned_metadata(&[]))
                    .unwrap_or_default();
                if mutable {
                    metadata.receiver_shapes.clear();
                    metadata.cooperative_callback_targets.clear();
                }
                metadata
            }
            ExprKind::Call { callee, .. } => self
                .function_info_for_call_callee(callee)
                .map(|info| info.return_record_descendants.cloned_metadata(&[]))
                .unwrap_or_default(),
            ExprKind::Paren(inner) => self.record_descendant_metadata_for_value(inner, mutable),
            ExprKind::If {
                then_block,
                else_branch,
                ..
            } => {
                let Some(then_expr) = metadata_join_block_tail_expr(then_block) else {
                    return RecordDescendantMetadata::default();
                };
                let Some(else_expr) = else_branch.as_deref().and_then(metadata_join_else_expr)
                else {
                    return RecordDescendantMetadata::default();
                };
                join_record_descendant_metadata([
                    self.record_descendant_metadata_for_value(then_expr, mutable),
                    self.record_descendant_metadata_for_value(else_expr, mutable),
                ])
            }
            ExprKind::Match { cases, .. }
                if !cases.is_empty()
                    && cases.last().is_some_and(match_case_is_unguarded_catch_all) =>
            {
                let metadata = cases.iter().map(|case| {
                    metadata_join_match_body_expr(&case.body)
                        .map(|arm| self.record_descendant_metadata_for_value(arm, mutable))
                });
                join_optional_record_descendant_metadata(metadata)
            }
            _ => RecordDescendantMetadata::default(),
        }
    }

    pub(super) fn binding_record_descendant_catalog(
        &self,
        source_name: &str,
        info: &BindingInfo,
    ) -> RecordDescendantCatalog {
        let mut catalog = info.declared_record_descendants.clone();
        if self.binding_allows_value_static_metadata(source_name, info) {
            if !info.mutable || info.namespace_member_value_metadata {
                catalog.extend(info.record_descendants.clone());
            } else {
                catalog.insert(Vec::new(), info.record_descendants.cloned_metadata(&[]));
            }
        }
        catalog
    }

    pub(super) fn binding_record_descendant_catalog_under(
        &self,
        source_name: &str,
        info: &BindingInfo,
        wrapper: RecordWrapper,
    ) -> RecordDescendantCatalog {
        let mut catalog = info.declared_record_descendants.project(wrapper);
        if self.binding_allows_value_static_metadata(source_name, info)
            && (!info.mutable || info.namespace_member_value_metadata)
        {
            catalog.extend(info.record_descendants.project(wrapper));
        }
        catalog
    }

    pub(super) fn binding_record_descendant_metadata(
        &self,
        source_name: &str,
        info: &BindingInfo,
        path: &[RecordWrapper],
    ) -> RecordDescendantMetadata {
        let mut metadata = info.declared_record_descendants.cloned_metadata(path);
        if self.binding_allows_value_static_metadata(source_name, info)
            && (!info.mutable || info.namespace_member_value_metadata || path.is_empty())
            && let Some(observed) = info.record_descendants.metadata(path)
        {
            metadata.extend_from(observed);
        }
        metadata
    }

    pub(super) fn binding_record_descendant_field_projection(
        &self,
        source_name: &str,
        info: &BindingInfo,
        path: &[RecordWrapper],
        field_path: &str,
    ) -> RecordFieldProjection {
        let mut projection = info
            .declared_record_descendants
            .metadata(path)
            .map(|metadata| metadata.field_projection(field_path))
            .unwrap_or_default();
        if self.binding_allows_value_static_metadata(source_name, info)
            && (!info.mutable || info.namespace_member_value_metadata || path.is_empty())
            && let Some(observed) = info.record_descendants.metadata(path)
        {
            projection.overlay(observed.field_projection(field_path));
        }
        projection
    }

    pub(super) fn record_descendant_catalog_for_value(
        &self,
        value: &Expr,
        mutable: bool,
    ) -> RecordDescendantCatalog {
        if mutable {
            let mut catalog = RecordDescendantCatalog::default();
            catalog.insert(
                Vec::new(),
                self.record_descendant_metadata_for_value(value, true),
            );
            return catalog;
        }
        match &value.kind {
            ExprKind::RecordLiteral { .. } | ExprKind::RecordUpdate { .. } => {
                let mut catalog = RecordDescendantCatalog::default();
                catalog.insert(
                    Vec::new(),
                    self.record_descendant_metadata_for_value(value, false),
                );
                catalog
            }
            ExprKind::Call { callee, args, .. }
                if matches!(constructor_name(callee, self), Some("Some")) && args.len() == 1 =>
            {
                constructor_single_value_arg(args, self)
                    .map(|inner| {
                        self.record_descendant_catalog_for_value(inner, false)
                            .prepended(RecordWrapper::Option)
                    })
                    .unwrap_or_default()
            }
            ExprKind::Call { callee, args, .. }
                if matches!(constructor_name(callee, self), Some("Ok")) && args.len() == 1 =>
            {
                constructor_single_value_arg(args, self)
                    .map(|ok| {
                        self.record_descendant_catalog_for_value(ok, false)
                            .prepended(RecordWrapper::ResultOk)
                    })
                    .unwrap_or_default()
            }
            ExprKind::MapLiteral(entries) => {
                let mut catalogs = entries
                    .iter()
                    .map(|(_, value)| self.record_descendant_catalog_for_value(value, false));
                let Some(first) = catalogs.next() else {
                    return RecordDescendantCatalog::default();
                };
                if catalogs.all(|catalog| catalog == first) {
                    first.prepended(RecordWrapper::MapValue)
                } else {
                    RecordDescendantCatalog::default()
                }
            }
            ExprKind::Ident => {
                let name = self.text(value.span);
                self.binding_lookup(name)
                    .map(|(_, info)| self.binding_record_descendant_catalog(name, info))
                    .unwrap_or_default()
            }
            ExprKind::Member { .. } => self
                .namespace_value_metadata_for_member_expr(value)
                .map(|metadata| metadata.record_descendants.clone())
                .unwrap_or_default(),
            ExprKind::Index { object, index } => self
                .array_element_projection_for_index(object, index)
                .map(ArrayElementProjection::record_descendant_catalog)
                .unwrap_or_default(),
            ExprKind::Call { callee, .. } => self
                .function_info_for_call_callee(callee)
                .map(|info| info.return_record_descendants.clone())
                .unwrap_or_default(),
            ExprKind::Paren(inner) => self.record_descendant_catalog_for_value(inner, mutable),
            ExprKind::If {
                then_block,
                else_branch,
                ..
            } => join_identical_if_branch_metadata(then_block, else_branch.as_deref(), |branch| {
                self.record_descendant_catalog_for_value(branch, mutable)
            })
            .unwrap_or_default(),
            ExprKind::Match { cases, .. } => join_identical_match_arm_metadata(cases, |arm| {
                self.record_descendant_catalog_for_value(arm, mutable)
            })
            .unwrap_or_default(),
            _ => RecordDescendantCatalog::default(),
        }
    }

    pub(super) fn record_descendant_catalog_for_value_under(
        &self,
        value: &Expr,
        wrapper: RecordWrapper,
    ) -> RecordDescendantCatalog {
        match &value.kind {
            ExprKind::Call { callee, args, .. }
                if matches!(constructor_name(callee, self), Some("Some")) && args.len() == 1 =>
            {
                if wrapper != RecordWrapper::Option {
                    return RecordDescendantCatalog::default();
                }
                constructor_single_value_arg(args, self)
                    .map(|inner| self.record_descendant_catalog_for_value(inner, false))
                    .unwrap_or_default()
            }
            ExprKind::Call { callee, args, .. }
                if matches!(constructor_name(callee, self), Some("Ok")) && args.len() == 1 =>
            {
                if wrapper != RecordWrapper::ResultOk {
                    return RecordDescendantCatalog::default();
                }
                constructor_single_value_arg(args, self)
                    .map(|ok| self.record_descendant_catalog_for_value(ok, false))
                    .unwrap_or_default()
            }
            ExprKind::MapLiteral(_) => self
                .record_descendant_catalog_for_value(value, false)
                .project(wrapper),
            ExprKind::Ident => {
                let source_name = self.text(value.span);
                self.binding_lookup(source_name)
                    .map(|(_, info)| {
                        self.binding_record_descendant_catalog_under(source_name, info, wrapper)
                    })
                    .unwrap_or_default()
            }
            ExprKind::Member { .. } => self
                .namespace_value_metadata_for_member_expr(value)
                .map(|metadata| metadata.record_descendants.project(wrapper))
                .unwrap_or_default(),
            ExprKind::Index { object, index } => self
                .array_element_projection_for_index(object, index)
                .map(|projection| projection.record_descendant_catalog_under(wrapper))
                .unwrap_or_default(),
            ExprKind::Call { callee, .. } => self
                .function_info_for_call_callee(callee)
                .map(|info| info.return_record_descendants.project(wrapper))
                .unwrap_or_default(),
            ExprKind::Paren(inner) => {
                self.record_descendant_catalog_for_value_under(inner, wrapper)
            }
            ExprKind::If {
                then_block,
                else_branch,
                ..
            } => join_identical_if_branch_metadata(then_block, else_branch.as_deref(), |branch| {
                self.record_descendant_catalog_for_value_under(branch, wrapper)
            })
            .unwrap_or_default(),
            ExprKind::Match { cases, .. } => join_identical_match_arm_metadata(cases, |arm| {
                self.record_descendant_catalog_for_value_under(arm, wrapper)
            })
            .unwrap_or_default(),
            _ => RecordDescendantCatalog::default(),
        }
    }

    pub(super) fn wrapped_value_metadata_catalog_for_value(
        &self,
        value: &Expr,
    ) -> WrappedValueMetadataCatalog {
        match &value.kind {
            ExprKind::Call { callee, args, .. }
                if matches!(constructor_name(callee, self), Some("Some") | Some("Ok"))
                    && args.len() == 1 =>
            {
                let Some(inner) = constructor_single_value_arg(args, self) else {
                    return WrappedValueMetadataCatalog::default();
                };
                let wrapper = if matches!(constructor_name(callee, self), Some("Some")) {
                    RecordWrapper::Option
                } else {
                    RecordWrapper::ResultOk
                };
                let mut catalog = self
                    .wrapped_value_metadata_catalog_for_value(inner)
                    .prepended(wrapper);
                catalog.insert_root(
                    wrapper,
                    WrappedValueMetadata {
                        callable_params: callable_param_info(inner, self),
                        receiver_shape: receiver_shape_from_value(inner, self),
                    },
                );
                catalog
            }
            ExprKind::Call { callee, .. } => {
                if let Some(info) = self.function_info_for_call_callee(callee) {
                    return info.return_wrapped_metadata.clone();
                }
                let Some((namespace, method)) = static_member_name(callee, self) else {
                    return WrappedValueMetadataCatalog::default();
                };
                let mut catalog = WrappedValueMetadataCatalog::default();
                catalog.insert_root(
                    RecordWrapper::Option,
                    WrappedValueMetadata {
                        receiver_shape: namespace_builtin_option_inner_shape(namespace, method),
                        callable_params: None,
                    },
                );
                catalog.insert_root(
                    RecordWrapper::ResultOk,
                    WrappedValueMetadata {
                        receiver_shape: namespace_builtin_call_metadata(namespace, method)
                            .result_ok_shape,
                        callable_params: None,
                    },
                );
                catalog
            }
            ExprKind::Ident => self
                .binding_lookup(self.text(value.span))
                .map(|(_, info)| info.wrapped_value_metadata.clone())
                .unwrap_or_default(),
            ExprKind::Member { .. } => self
                .namespace_value_metadata_for_member_expr(value)
                .map(|metadata| metadata.wrapped_value_metadata.clone())
                .unwrap_or_default(),
            ExprKind::Index { object, index } => self
                .array_element_projection_for_index(object, index)
                .map(ArrayElementProjection::wrapped_value_metadata_catalog)
                .unwrap_or_default(),
            ExprKind::Paren(inner) => self.wrapped_value_metadata_catalog_for_value(inner),
            ExprKind::If {
                then_block,
                else_branch,
                ..
            } => join_identical_if_branch_metadata(then_block, else_branch.as_deref(), |branch| {
                self.wrapped_value_metadata_catalog_for_value(branch)
            })
            .unwrap_or_default(),
            ExprKind::Match { cases, .. } => join_identical_match_arm_metadata(cases, |arm| {
                self.wrapped_value_metadata_catalog_for_value(arm)
            })
            .unwrap_or_default(),
            _ => WrappedValueMetadataCatalog::default(),
        }
    }

    pub(super) fn wrapped_pattern_value_projection(
        &self,
        value: &Expr,
        wrapper: RecordWrapper,
    ) -> WrappedPatternValueProjection {
        WrappedPatternValueProjection::from_catalog(
            &self.wrapped_value_metadata_catalog_for_value(value),
            wrapper,
            self.record_descendant_catalog_for_value_under(value, wrapper),
        )
    }

    pub(super) fn option_record_field_projection(
        &self,
        object: &Expr,
        field: &Ident,
    ) -> RecordFieldProjection {
        self.option_record_field_projection_for_path(object, self.text(field.span))
    }

    pub(super) fn option_record_field_projection_for_path(
        &self,
        value: &Expr,
        field_path: &str,
    ) -> RecordFieldProjection {
        match &value.kind {
            ExprKind::OptionalAccess { object, field } => {
                let nested_path = format!("{}.{field_path}", self.text(field.span));
                self.option_record_field_projection_for_path(object, &nested_path)
            }
            ExprKind::Ident => {
                let source_name = self.text(value.span);
                self.binding_lookup(source_name)
                    .map(|(_, info)| {
                        self.binding_record_descendant_field_projection(
                            source_name,
                            info,
                            &[RecordWrapper::Option],
                            field_path,
                        )
                    })
                    .unwrap_or_default()
            }
            ExprKind::Member { .. } => self
                .namespace_value_metadata_for_member_expr(value)
                .and_then(|metadata| {
                    metadata
                        .record_descendants
                        .metadata(&[RecordWrapper::Option])
                })
                .map(|metadata| metadata.field_projection(field_path))
                .unwrap_or_default(),
            ExprKind::Call { callee, .. } => {
                if let Some(info) = self.function_info_for_call_callee(callee) {
                    return info
                        .return_record_descendants
                        .metadata(&[RecordWrapper::Option])
                        .map(|metadata| metadata.field_projection(field_path))
                        .unwrap_or_default();
                }
                self.record_descendant_catalog_for_value_under(value, RecordWrapper::Option)
                    .metadata(&[])
                    .map(|metadata| metadata.field_projection(field_path))
                    .unwrap_or_default()
            }
            ExprKind::Paren(inner) => {
                self.option_record_field_projection_for_path(inner, field_path)
            }
            ExprKind::If {
                then_block,
                else_branch,
                ..
            } => join_identical_if_branch_metadata(then_block, else_branch.as_deref(), |branch| {
                self.option_record_field_projection_for_path(branch, field_path)
            })
            .unwrap_or_default(),
            ExprKind::Match { cases, .. } => join_identical_match_arm_metadata(cases, |arm| {
                self.option_record_field_projection_for_path(arm, field_path)
            })
            .unwrap_or_default(),
            _ => self
                .record_descendant_catalog_for_value_under(value, RecordWrapper::Option)
                .metadata(&[])
                .map(|metadata| metadata.field_projection(field_path))
                .unwrap_or_default(),
        }
    }
}
