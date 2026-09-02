use crate::*;

pub(crate) fn build_schema_emit(
    ty: &Type,
    aliases: &Aliases<'_, '_>,
    src: &LoweredText,
) -> Result<Schema, String> {
    let decls = EmitSchemaDecls {
        type_ctx: aliases.type_ctx,
    };
    schema_of_emit(
        ty,
        &decls,
        &EmitSchemaScope {
            module: aliases.identity.to_string(),
            src,
        },
        &EmitSchemaEnv::new(),
        &mut Vec::new(),
        &mut Vec::new(),
    )
}

pub(crate) fn schema_param_env_emit<'a>(
    params: &[Ident],
    args: &'a [Type],
    caller: &EmitSchemaEnv<'a>,
    param_src: &LoweredText,
    caller_scope: &EmitSchemaScope<'a>,
) -> EmitSchemaEnv<'a> {
    let captured = Rc::new(caller.clone());
    params
        .iter()
        .zip(args)
        .map(|(param, ty)| {
            (
                text(param_src, param.span).to_string(),
                EmitSchemaSubstitution {
                    ty,
                    env: captured.clone(),
                    scope: caller_scope.clone(),
                },
            )
        })
        .collect()
}

pub(crate) fn resolve_emit_schema_decl<'a, 'ctx, 'borrow>(
    decls: &'borrow EmitSchemaDecls<'a, 'ctx>,
    current_module: &str,
    namespace: Option<&str>,
    head: &str,
) -> Option<(String, String, &'borrow ModuleTypeCtx<'a>)> {
    let current = decls.type_ctx.module(current_module)?;
    let type_imports = &current.type_imports;
    let (target_module, target_name) = match namespace {
        Some(namespace) => (
            type_imports.namespaces.get(namespace)?.clone(),
            head.to_string(),
        ),
        None => type_imports
            .selected_types
            .get(head)
            .cloned()
            .unwrap_or_else(|| (current_module.to_string(), head.to_string())),
    };
    let target = decls.type_ctx.module(&target_module)?;
    Some((target_module, target_name, target))
}

pub(crate) fn schema_named_primitive_emit(name: &str) -> Option<Schema> {
    match name {
        "int" => Some(Schema::Int),
        "string" => Some(Schema::Str),
        "bool" => Some(Schema::Bool),
        "JSONValue" => Some(Schema::Json),
        _ => None,
    }
}

pub(crate) fn schema_user_named_emit<'a>(
    emission: NamedSchemaEmission<'a, '_, '_>,
    seen: &mut Vec<String>,
    resolving: &mut Vec<EmitSchemaResolution>,
) -> Result<Schema, String> {
    let NamedSchemaEmission {
        ty,
        head,
        namespace,
        display,
        args,
        decls,
        scope,
        env,
    } = emission;
    let instance = schema_type_key_emit(ty, scope, env, resolving)?;
    if seen.contains(&instance) {
        return Err(format!(
            "JSON_LIMIT: a recursive type `{display}` has no finite JSON decode schema"
        ));
    }
    if seen.len() >= STRUCT_DEPTH {
        return Err("JSON_LIMIT: the type does not resolve to a finite JSON decode schema".into());
    }
    let Some((target_module, target_name, target)) =
        resolve_emit_schema_decl(decls, &scope.module, namespace, head)
    else {
        return Err(format!("`{display}` is not JSON-decodable"));
    };
    let target_scope = EmitSchemaScope {
        module: target_module,
        src: target.emission.src,
    };
    if let Some(alias) = target.local_types.schema_aliases.get(target_name.as_str()) {
        if alias.type_params.len() != args.len() {
            return Err(format!(
                "type alias `{display}` expects {} type argument{}, found {}",
                alias.type_params.len(),
                if alias.type_params.len() == 1 {
                    ""
                } else {
                    "s"
                },
                args.len()
            ));
        }
        let nested =
            schema_param_env_emit(&alias.type_params, args, env, target.emission.src, scope);
        seen.push(instance);
        let result = schema_of_emit(&alias.ty, decls, &target_scope, &nested, seen, resolving);
        seen.pop();
        return result;
    }
    if let Some(record) = target.local_types.schema_records.get(target_name.as_str()) {
        if record.type_params.len() != args.len() {
            return Err(format!(
                "record `{display}` expects {} type argument{}, found {}",
                record.type_params.len(),
                if record.type_params.len() == 1 {
                    ""
                } else {
                    "s"
                },
                args.len()
            ));
        }
        let nested =
            schema_param_env_emit(&record.type_params, args, env, target.emission.src, scope);
        seen.push(instance);
        let mut fields = Vec::with_capacity(record.fields.len());
        for field in &record.fields {
            let schema = schema_of_emit(&field.ty, decls, &target_scope, &nested, seen, resolving)?;
            let folded = field
                .default
                .as_ref()
                .and_then(|value| const_fold_default_emit(value, target.emission.src));
            fields.push((
                Rc::from(text(target.emission.src, field.name.span)),
                schema,
                folded,
            ));
        }
        seen.pop();
        let record_id = text(target.emission.src, record.name.span);
        let declaration_identity = target
            .local_types
            .record_defs
            .get(record_id)
            .and_then(|definition| definition.declaration_identity.clone());
        return Ok(Schema::Record {
            record_id: Rc::from(record_id),
            declaration_identity: declaration_identity.map(Rc::from),
            fields: Rc::from(fields),
        });
    }
    if let Some(enumeration) = target.local_types.schema_enums.get(target_name.as_str()) {
        if enumeration.type_params.len() != args.len() {
            return Err(format!(
                "enum `{display}` expects {} type argument{}, found {}",
                enumeration.type_params.len(),
                if enumeration.type_params.len() == 1 {
                    ""
                } else {
                    "s"
                },
                args.len()
            ));
        }
        let nested = schema_param_env_emit(
            &enumeration.type_params,
            args,
            env,
            target.emission.src,
            scope,
        );
        seen.push(instance);
        let mut variants = Vec::with_capacity(enumeration.variants.len());
        for (ordinal, variant) in enumeration.variants.iter().enumerate() {
            let mut payloads = Vec::new();
            if let Some(types) = &variant.payload {
                for payload in types {
                    payloads.push(schema_of_emit(
                        payload,
                        decls,
                        &target_scope,
                        &nested,
                        seen,
                        resolving,
                    )?);
                }
            }
            variants.push((
                Rc::from(text(target.emission.src, variant.name.span)),
                ordinal as u32,
                Rc::from(payloads),
            ));
        }
        seen.pop();
        let enum_id = text(target.emission.src, enumeration.name.span);
        let declaration_identity = target
            .local_types
            .enum_defs
            .get(enum_id)
            .and_then(|definition| definition.declaration_identity.clone());
        return Ok(Schema::Enum {
            enum_id: Rc::from(enum_id),
            declaration_identity: declaration_identity.map(Rc::from),
            variants: Rc::from(variants),
        });
    }
    if let Some(newtype) = target.local_types.schema_newtypes.get(target_name.as_str()) {
        if newtype.type_params.len() != args.len() {
            return Err(format!(
                "newtype `{display}` expects {} type argument{}, found {}",
                newtype.type_params.len(),
                if newtype.type_params.len() == 1 {
                    ""
                } else {
                    "s"
                },
                args.len()
            ));
        }
        let nested =
            schema_param_env_emit(&newtype.type_params, args, env, target.emission.src, scope);
        seen.push(instance);
        let base = schema_of_emit(
            &newtype.base,
            decls,
            &target_scope,
            &nested,
            seen,
            resolving,
        )?;
        seen.pop();
        let newtype_id = text(target.emission.src, newtype.name.span);
        let declaration_identity = target
            .local_types
            .newtype_defs
            .get(newtype_id)
            .and_then(|definition| definition.declaration_identity.clone());
        return Ok(Schema::Newtype {
            newtype_id: Rc::from(newtype_id),
            declaration_identity: declaration_identity.map(Rc::from),
            base: Rc::new(base),
        });
    }
    Err(format!("`{display}` is not JSON-decodable"))
}

pub(crate) fn schema_of_emit<'a>(
    ty: &'a Type,
    decls: &EmitSchemaDecls<'a, '_>,
    scope: &EmitSchemaScope<'a>,
    env: &EmitSchemaEnv<'a>,
    seen: &mut Vec<String>,
    resolving: &mut Vec<EmitSchemaResolution>,
) -> Result<Schema, String> {
    match &ty.kind {
        TypeKind::Unit => Ok(Schema::Unit),
        TypeKind::Literal => match text(scope.src, ty.span) {
            "null" => Ok(Schema::Null),
            "true" | "false" => Ok(Schema::Bool),
            value if value.starts_with('"') => Ok(Schema::Str),
            value
                if value
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_ascii_digit() || character == '-') =>
            {
                if value.contains('.') || value.contains('e') || value.contains('E') {
                    Err("float is not JSON-decodable".to_string())
                } else {
                    Ok(Schema::Int)
                }
            }
            _ => Err("this literal type is not JSON-decodable".to_string()),
        },
        TypeKind::Named { name, args } => {
            let head = text(scope.src, name.span);
            if let Some(replacement) = env.get(head).cloned() {
                if !args.is_empty() {
                    return Err(format!(
                        "type parameter `{head}` cannot take type arguments"
                    ));
                }
                let identity = (
                    head.to_string(),
                    replacement.ty.span.lo,
                    replacement.ty.span.hi,
                    Rc::as_ptr(&replacement.env) as usize,
                );
                if resolving.contains(&identity) {
                    return Err(format!(
                        "JSON_LIMIT: type parameter `{head}` does not resolve to a finite schema"
                    ));
                }
                resolving.push(identity);
                let result = schema_of_emit(
                    replacement.ty,
                    decls,
                    &replacement.scope,
                    &replacement.env,
                    seen,
                    resolving,
                );
                resolving.pop();
                return result;
            }
            match head {
                "Option" if args.len() == 1 => Ok(Schema::Option(Rc::new(schema_of_emit(
                    &args[0], decls, scope, env, seen, resolving,
                )?))),
                "Array" if args.len() == 1 => Ok(Schema::Array(Rc::new(schema_of_emit(
                    &args[0], decls, scope, env, seen, resolving,
                )?))),
                "Map" if args.len() == 2 => {
                    if !matches!(
                        schema_of_emit(&args[0], decls, scope, env, seen, resolving)?,
                        Schema::Str
                    ) {
                        return Err("a Map with a non-string key is not JSON-decodable".to_string());
                    }
                    Ok(Schema::Map(Rc::new(schema_of_emit(
                        &args[1], decls, scope, env, seen, resolving,
                    )?)))
                }
                "Result" | "Set" => Err(format!("`{head}` has no JSON form (not decodable)")),
                "float" => Err("float is not JSON-decodable".to_string()),
                _ => schema_named_primitive_emit(head).map_or_else(
                    || {
                        schema_user_named_emit(
                            NamedSchemaEmission {
                                ty,
                                head,
                                namespace: None,
                                display: head,
                                args,
                                decls,
                                scope,
                                env,
                            },
                            seen,
                            resolving,
                        )
                    },
                    Ok,
                ),
            }
        }
        TypeKind::Record(fields) => {
            let mut output = Vec::with_capacity(fields.len());
            for field in fields {
                output.push((
                    Rc::from(text(scope.src, field.name.span)),
                    schema_of_emit(&field.ty, decls, scope, env, seen, resolving)?,
                    None,
                ));
            }
            Ok(Schema::StructRecord {
                fields: Rc::from(output),
            })
        }
        TypeKind::Function { .. } => Err("a function is not JSON-decodable".to_string()),
        TypeKind::Union(_) => Err("a union has no single JSON shape".to_string()),
        TypeKind::Qualified { ns, name, args } => {
            let namespace = text(scope.src, ns.span);
            let head = text(scope.src, name.span);
            let display = format!("{namespace}.{head}");
            schema_user_named_emit(
                NamedSchemaEmission {
                    ty,
                    head,
                    namespace: Some(namespace),
                    display: &display,
                    args,
                    decls,
                    scope,
                    env,
                },
                seen,
                resolving,
            )
        }
    }
}

pub(crate) fn schema_type_key_emit<'a>(
    ty: &'a Type,
    scope: &EmitSchemaScope<'a>,
    env: &EmitSchemaEnv<'a>,
    resolving: &mut Vec<EmitSchemaResolution>,
) -> Result<String, String> {
    match &ty.kind {
        TypeKind::Named { name, args } => {
            let head = text(scope.src, name.span);
            if let Some(replacement) = env.get(head).cloned() {
                if !args.is_empty() {
                    return Err(format!(
                        "type parameter `{head}` cannot take type arguments"
                    ));
                }
                let identity = (
                    head.to_string(),
                    replacement.ty.span.lo,
                    replacement.ty.span.hi,
                    Rc::as_ptr(&replacement.env) as usize,
                );
                if resolving.contains(&identity) {
                    return Err(format!(
                        "JSON_LIMIT: type parameter `{head}` does not resolve to a finite schema"
                    ));
                }
                resolving.push(identity);
                let result = schema_type_key_emit(
                    replacement.ty,
                    &replacement.scope,
                    &replacement.env,
                    resolving,
                );
                resolving.pop();
                return result;
            }
            let arguments = args
                .iter()
                .map(|value| schema_type_key_emit(value, scope, env, resolving))
                .collect::<Result<Vec<_>, _>>()?;
            if arguments.is_empty() {
                Ok(format!("{}::{head}", scope.module))
            } else {
                Ok(format!("{}::{head}<{}>", scope.module, arguments.join(",")))
            }
        }
        TypeKind::Record(fields) => {
            let fields = fields
                .iter()
                .map(|field| {
                    Ok(format!(
                        "{}:{}",
                        text(scope.src, field.name.span),
                        schema_type_key_emit(&field.ty, scope, env, resolving)?
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(format!("{{{}}}", fields.join(",")))
        }
        TypeKind::Union(members) => Ok(members
            .iter()
            .map(|member| schema_type_key_emit(member, scope, env, resolving))
            .collect::<Result<Vec<_>, _>>()?
            .join("|")),
        TypeKind::Qualified { ns, name, args } => Ok(format!(
            "{}::{}.{}<{}>",
            scope.module,
            text(scope.src, ns.span),
            text(scope.src, name.span),
            args.iter()
                .map(|argument| schema_type_key_emit(argument, scope, env, resolving))
                .collect::<Result<Vec<_>, _>>()?
                .join(",")
        )),
        TypeKind::Literal | TypeKind::Function { .. } | TypeKind::Unit => {
            Ok(text(scope.src, ty.span).to_string())
        }
    }
}

pub(crate) fn const_fold_default_emit(expr: &Expr, src: &LoweredText) -> Option<Value> {
    match &expr.kind {
        ExprKind::Int => text(src, expr.span)
            .replace('_', "")
            .parse::<i64>()
            .ok()
            .map(Value::Int),
        ExprKind::Float => text(src, expr.span)
            .replace('_', "")
            .parse::<f64>()
            .ok()
            .map(Value::Float),
        ExprKind::Bool(value) => Some(Value::Bool(*value)),
        ExprKind::Null => Some(Value::Null),
        ExprKind::Unit => Some(Value::Unit),
        ExprKind::String(literal) if literal.tag.is_none() => {
            let mut buffer = String::new();
            for part in &literal.parts {
                match part {
                    StringPart::Text(span) => {
                        decode_escapes(text(src, *span), &mut buffer, *span).ok()?;
                    }
                    StringPart::Interpolation(_) => return None,
                }
            }
            Some(Value::str(buffer))
        }
        ExprKind::Paren(inner) => const_fold_default_emit(inner, src),
        ExprKind::Unary { op, operand } => unary_value(
            value_unary_op(*op),
            const_fold_default_emit(operand, src)?,
            expr.span,
        )
        .ok(),
        ExprKind::Binary { op, lhs, rhs }
            if !matches!(op, BinaryOp::And | BinaryOp::Or | BinaryOp::Coalesce) =>
        {
            binary_value(
                value_binary_op(*op),
                const_fold_default_emit(lhs, src)?,
                const_fold_default_emit(rhs, src)?,
                expr.span,
            )
            .ok()
        }
        _ => None,
    }
}

pub(crate) fn render_schema_rust(schema: &Schema) -> String {
    match schema {
        Schema::Int => "Schema::Int".to_string(),
        Schema::Str => "Schema::Str".to_string(),
        Schema::Bool => "Schema::Bool".to_string(),
        Schema::Unit => "Schema::Unit".to_string(),
        Schema::Null => "Schema::Null".to_string(),
        Schema::Json => "Schema::Json".to_string(),
        Schema::Array(inner) => format!("Schema::Array(Rc::new({}))", render_schema_rust(inner)),
        Schema::Option(inner) => {
            format!("Schema::Option(Rc::new({}))", render_schema_rust(inner))
        }
        Schema::Map(inner) => format!("Schema::Map(Rc::new({}))", render_schema_rust(inner)),
        Schema::StructRecord { fields } => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(name, fschema, _)| {
                    format!(
                        "(Rc::from({:?}), {}, None)",
                        name.as_ref(),
                        render_schema_rust(fschema)
                    )
                })
                .collect();
            format!(
                "Schema::StructRecord {{ fields: Rc::from(vec![{}]) }}",
                parts.join(", ")
            )
        }
        Schema::Newtype {
            newtype_id,
            declaration_identity,
            base,
        } => {
            let identity = declaration_identity
                .as_ref()
                .map_or("None".to_string(), |identity| {
                    format!("Some(Rc::from({:?}))", identity.as_ref())
                });
            format!(
                "Schema::Newtype {{ newtype_id: Rc::from({:?}), declaration_identity: {identity}, base: Rc::new({}) }}",
                newtype_id.as_ref(),
                render_schema_rust(base)
            )
        }
        Schema::Record {
            record_id,
            declaration_identity,
            fields,
        } => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(name, fschema, fdefault)| {
                    let def = match fdefault {
                        Some(v) => format!("Some({})", render_value_rust(v)),
                        None => "None".to_string(),
                    };
                    format!(
                        "(Rc::from({:?}), {}, {})",
                        name.as_ref(),
                        render_schema_rust(fschema),
                        def
                    )
                })
                .collect();
            let identity = declaration_identity
                .as_ref()
                .map_or("None".to_string(), |identity| {
                    format!("Some(Rc::from({:?}))", identity.as_ref())
                });
            format!(
                "Schema::Record {{ record_id: Rc::from({:?}), declaration_identity: {identity}, fields: Rc::from(vec![{}]) }}",
                record_id.as_ref(),
                parts.join(", ")
            )
        }
        Schema::Enum {
            enum_id,
            declaration_identity,
            variants,
        } => {
            let parts: Vec<String> = variants
                .iter()
                .map(|(name, idx, payloads)| {
                    let ps: Vec<String> = payloads.iter().map(render_schema_rust).collect();
                    format!(
                        "(Rc::from({:?}), {}u32, Rc::from(vec![{}]))",
                        name.as_ref(),
                        idx,
                        ps.join(", ")
                    )
                })
                .collect();
            let identity = declaration_identity
                .as_ref()
                .map_or("None".to_string(), |identity| {
                    format!("Some(Rc::from({:?}))", identity.as_ref())
                });
            format!(
                "Schema::Enum {{ enum_id: Rc::from({:?}), declaration_identity: {identity}, variants: Rc::from(vec![{}]) }}",
                enum_id.as_ref(),
                parts.join(", ")
            )
        }
    }
}

pub(crate) fn render_value_rust(value: &Value) -> String {
    match value {
        Value::Int(n) => format!("Value::Int({n})"),
        Value::Float(x) => {
            if x.is_nan() {
                "Value::Float(f64::NAN)".to_string()
            } else if x.is_infinite() && *x > 0.0 {
                "Value::Float(f64::INFINITY)".to_string()
            } else if x.is_infinite() {
                "Value::Float(f64::NEG_INFINITY)".to_string()
            } else {
                format!("Value::Float({x:?})")
            }
        }
        Value::Bool(b) => format!("Value::Bool({b})"),
        Value::Null => "Value::Null".to_string(),
        Value::Unit => "Value::Unit".to_string(),
        Value::Str(s) => format!("Value::str({:?})", s.as_ref()),
        _ => "Value::Unit".to_string(),
    }
}
