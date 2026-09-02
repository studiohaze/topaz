use super::super::*;

/// The nominal declarations `schema_of` consults while lowering a source type
/// argument to a runtime JSON decode schema.
pub type SchemaDeclLookup<'a, Declaration> =
    dyn Fn(&str, Option<&str>, &str) -> Option<Declaration> + 'a;

pub struct SchemaDecls<'a> {
    pub src: &'a str,
    pub module: String,
    pub aliases: &'a SchemaDeclLookup<'a, SchemaAliasDecl<'a>>,
    pub records: &'a SchemaDeclLookup<'a, SchemaRecordDecl<'a>>,
    pub enums: &'a SchemaDeclLookup<'a, SchemaEnumDecl<'a>>,
    pub newtypes: &'a SchemaDeclLookup<'a, SchemaNewtypeDecl<'a>>,
}

pub struct SchemaAliasDecl<'a> {
    pub module: String,
    pub src: &'a str,
    pub type_params: Vec<String>,
    pub body: &'a ast::Type,
}

pub struct SchemaRecordDecl<'a> {
    pub module: String,
    pub src: &'a str,
    pub name: String,
    pub declaration_identity: Option<String>,
    pub type_params: Vec<String>,
    pub fields: Vec<(String, &'a ast::Type, Option<&'a Expr>)>,
}

pub struct SchemaEnumDecl<'a> {
    pub module: String,
    pub src: &'a str,
    pub name: String,
    pub declaration_identity: Option<String>,
    pub type_params: Vec<String>,
    pub variants: Vec<(String, u32, Vec<&'a ast::Type>)>,
}

pub struct SchemaNewtypeDecl<'a> {
    pub module: String,
    pub src: &'a str,
    pub name: String,
    pub declaration_identity: Option<String>,
    pub type_params: Vec<String>,
    pub base: &'a ast::Type,
}

/// One capture-safe nominal type-parameter substitution. The replacement AST
/// belongs to the caller declaration, so it must keep the caller environment
/// in which names inside that AST were written. A flat `name -> AST` map would
/// capture common compositions such as `Wrap<T> { inner: Box<T> }` when `Box`
/// also names its own parameter `T`.
#[derive(Clone)]
pub(in crate::value) struct SchemaSubstitution<'a> {
    ty: &'a ast::Type,
    env: Rc<SchemaEnv<'a>>,
    scope: SchemaScope<'a>,
}

#[derive(Clone)]
pub(in crate::value) struct SchemaScope<'a> {
    module: String,
    src: &'a str,
}

pub(in crate::value) type SchemaEnv<'a> = std::collections::HashMap<String, SchemaSubstitution<'a>>;
pub(in crate::value) type SchemaResolution = (String, u32, u32, usize);

pub(in crate::value) fn schema_param_env<'a>(
    params: &[String],
    args: &'a [Rc<ast::Type>],
    caller: &SchemaEnv<'a>,
    caller_scope: &SchemaScope<'a>,
) -> SchemaEnv<'a> {
    let captured = Rc::new(caller.clone());
    params
        .iter()
        .zip(args)
        .map(|(param, ty)| {
            (
                param.clone(),
                SchemaSubstitution {
                    ty: ty.as_ref(),
                    env: captured.clone(),
                    scope: caller_scope.clone(),
                },
            )
        })
        .collect()
}

pub(in crate::value) fn schema_named_prim(name: &str) -> Option<Schema> {
    match name {
        "int" => Some(Schema::Int),
        "string" => Some(Schema::Str),
        "bool" => Some(Schema::Bool),
        "JSONValue" => Some(Schema::Json),
        _ => None,
    }
}

/// Lower a fully-known Topaz type AST into the shared JSON decode descriptor.
pub fn schema_of<'a>(
    ty: &'a ast::Type,
    decls: &SchemaDecls<'a>,
    seen: &mut Vec<String>,
) -> Result<Schema, String> {
    schema_of_with_env(
        ty,
        decls,
        &SchemaScope {
            module: decls.module.clone(),
            src: decls.src,
        },
        &SchemaEnv::new(),
        seen,
        &mut Vec::new(),
    )
}

pub(in crate::value) fn schema_of_with_env<'a>(
    ty: &'a ast::Type,
    decls: &SchemaDecls<'a>,
    scope: &SchemaScope<'a>,
    env: &SchemaEnv<'a>,
    seen: &mut Vec<String>,
    resolving_params: &mut Vec<SchemaResolution>,
) -> Result<Schema, String> {
    match &ty.kind {
        TypeKind::Unit => Ok(Schema::Unit),
        TypeKind::Literal => {
            let lex = scope.src.get(ty.span.lo as usize..ty.span.hi as usize);
            match lex {
                Some("null") => Ok(Schema::Null),
                Some("true") | Some("false") => Ok(Schema::Bool),
                Some(s) if s.starts_with('"') => Ok(Schema::Str),
                Some(s)
                    if s.chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_digit() || c == '-') =>
                {
                    if s.contains('.') || s.contains('e') || s.contains('E') {
                        Err("float is not JSON-decodable".to_string())
                    } else {
                        Ok(Schema::Int)
                    }
                }
                _ => Err("this literal type is not JSON-decodable".to_string()),
            }
        }
        TypeKind::Named { name, args } => {
            let head = scope
                .src
                .get(name.span.lo as usize..name.span.hi as usize)
                .ok_or_else(|| "unresolved type name".to_string())?;
            if let Some(replacement) = env.get(head).cloned() {
                if !args.is_empty() {
                    return Err(format!(
                        "type parameter `{head}` cannot take type arguments"
                    ));
                }
                let resolution = (
                    head.to_string(),
                    replacement.ty.span.lo,
                    replacement.ty.span.hi,
                    Rc::as_ptr(&replacement.env) as usize,
                );
                if resolving_params.iter().any(|entry| entry == &resolution) {
                    return Err(format!(
                        "JSON_LIMIT: type parameter `{head}` does not resolve to a finite schema"
                    ));
                }
                resolving_params.push(resolution);
                let out = schema_of_with_env(
                    replacement.ty,
                    decls,
                    &replacement.scope,
                    replacement.env.as_ref(),
                    seen,
                    resolving_params,
                );
                resolving_params.pop();
                return out;
            }
            match head {
                "Option" if args.len() == 1 => Ok(Schema::Option(Rc::new(schema_of_with_env(
                    &args[0],
                    decls,
                    scope,
                    env,
                    seen,
                    resolving_params,
                )?))),
                "Array" if args.len() == 1 => Ok(Schema::Array(Rc::new(schema_of_with_env(
                    &args[0],
                    decls,
                    scope,
                    env,
                    seen,
                    resolving_params,
                )?))),
                "Map" if args.len() == 2 => {
                    let key_ok = matches!(
                        schema_of_with_env(&args[0], decls, scope, env, seen, resolving_params,)?,
                        Schema::Str
                    );
                    if !key_ok {
                        return Err("a Map with a non-string key is not JSON-decodable".to_string());
                    }
                    Ok(Schema::Map(Rc::new(schema_of_with_env(
                        &args[1],
                        decls,
                        scope,
                        env,
                        seen,
                        resolving_params,
                    )?)))
                }
                "Result" | "Set" => Err(format!("`{head}` has no JSON form (not decodable)")),
                "float" => Err("float is not JSON-decodable".to_string()),
                _ => {
                    if let Some(prim) = schema_named_prim(head) {
                        return Ok(prim);
                    }
                    let instance_key = schema_type_key(ty, scope, env, resolving_params)?;
                    if seen.iter().any(|seen_key| seen_key == &instance_key) {
                        return Err(format!(
                            "JSON_LIMIT: a recursive type `{head}` has no finite JSON decode schema"
                        ));
                    }
                    if seen.len() >= STRUCT_DEPTH {
                        return Err(
                            "JSON_LIMIT: the type does not resolve to a finite JSON decode schema"
                                .to_string(),
                        );
                    }
                    if let Some(alias) = (decls.aliases)(&scope.module, None, head) {
                        if alias.type_params.len() != args.len() {
                            return Err(format!(
                                "type alias `{head}` expects {} type argument{}, found {}",
                                alias.type_params.len(),
                                if alias.type_params.len() == 1 {
                                    ""
                                } else {
                                    "s"
                                },
                                args.len()
                            ));
                        }
                        let nested_env = schema_param_env(&alias.type_params, args, env, scope);
                        let alias_scope = SchemaScope {
                            module: alias.module,
                            src: alias.src,
                        };
                        seen.push(instance_key);
                        let out = schema_of_with_env(
                            alias.body,
                            decls,
                            &alias_scope,
                            &nested_env,
                            seen,
                            resolving_params,
                        );
                        seen.pop();
                        return out;
                    }
                    if let Some(record) = (decls.records)(&scope.module, None, head) {
                        if record.type_params.len() != args.len() {
                            return Err(format!(
                                "record `{head}` expects {} type argument{}, found {}",
                                record.type_params.len(),
                                if record.type_params.len() == 1 {
                                    ""
                                } else {
                                    "s"
                                },
                                args.len()
                            ));
                        }
                        let nested_env = schema_param_env(&record.type_params, args, env, scope);
                        let record_scope = SchemaScope {
                            module: record.module.clone(),
                            src: record.src,
                        };
                        seen.push(instance_key.clone());
                        let mut out = Vec::with_capacity(record.fields.len());
                        for (fname, fty, fdefault) in &record.fields {
                            let fschema = schema_of_with_env(
                                fty,
                                decls,
                                &record_scope,
                                &nested_env,
                                seen,
                                resolving_params,
                            )?;
                            let folded = fdefault.and_then(|d| const_fold_default(d, record.src));
                            out.push((Rc::from(fname.as_str()), fschema, folded));
                        }
                        seen.pop();
                        return Ok(Schema::Record {
                            record_id: Rc::from(record.name),
                            declaration_identity: record.declaration_identity.map(Rc::from),
                            fields: Rc::from(out),
                        });
                    }
                    if let Some(enm) = (decls.enums)(&scope.module, None, head) {
                        if enm.type_params.len() != args.len() {
                            return Err(format!(
                                "enum `{head}` expects {} type argument{}, found {}",
                                enm.type_params.len(),
                                if enm.type_params.len() == 1 { "" } else { "s" },
                                args.len()
                            ));
                        }
                        let nested_env = schema_param_env(&enm.type_params, args, env, scope);
                        let enum_scope = SchemaScope {
                            module: enm.module.clone(),
                            src: enm.src,
                        };
                        seen.push(instance_key.clone());
                        let mut out = Vec::with_capacity(enm.variants.len());
                        for (vname, vidx, payloads) in &enm.variants {
                            let mut pschemas = Vec::with_capacity(payloads.len());
                            for p in payloads {
                                pschemas.push(schema_of_with_env(
                                    p,
                                    decls,
                                    &enum_scope,
                                    &nested_env,
                                    seen,
                                    resolving_params,
                                )?);
                            }
                            out.push((Rc::from(vname.as_str()), *vidx, Rc::from(pschemas)));
                        }
                        seen.pop();
                        return Ok(Schema::Enum {
                            enum_id: Rc::from(enm.name),
                            declaration_identity: enm.declaration_identity.map(Rc::from),
                            variants: Rc::from(out),
                        });
                    }
                    if let Some(newtype) = (decls.newtypes)(&scope.module, None, head) {
                        if newtype.type_params.len() != args.len() {
                            return Err(format!(
                                "newtype `{head}` expects {} type argument{}, found {}",
                                newtype.type_params.len(),
                                if newtype.type_params.len() == 1 {
                                    ""
                                } else {
                                    "s"
                                },
                                args.len()
                            ));
                        }
                        let nested_env = schema_param_env(&newtype.type_params, args, env, scope);
                        let newtype_scope = SchemaScope {
                            module: newtype.module.clone(),
                            src: newtype.src,
                        };
                        seen.push(instance_key);
                        let base_schema = schema_of_with_env(
                            newtype.base,
                            decls,
                            &newtype_scope,
                            &nested_env,
                            seen,
                            resolving_params,
                        )?;
                        seen.pop();
                        return Ok(Schema::Newtype {
                            newtype_id: Rc::from(newtype.name),
                            declaration_identity: newtype.declaration_identity.map(Rc::from),
                            base: Rc::new(base_schema),
                        });
                    }
                    Err(format!("`{head}` is not JSON-decodable"))
                }
            }
        }
        TypeKind::Record(fields) => {
            let mut out = Vec::with_capacity(fields.len());
            for field in fields {
                let fname = scope
                    .src
                    .get(field.name.span.lo as usize..field.name.span.hi as usize)
                    .ok_or_else(|| "unresolved record field name".to_string())?;
                out.push((
                    Rc::from(fname),
                    schema_of_with_env(&field.ty, decls, scope, env, seen, resolving_params)?,
                    None,
                ));
            }
            Ok(Schema::StructRecord {
                fields: Rc::from(out),
            })
        }
        TypeKind::Function { .. } => Err("a function is not JSON-decodable".to_string()),
        TypeKind::Union(_) => Err("a union has no single JSON shape".to_string()),
        TypeKind::Qualified { ns, name, args } => {
            let namespace = scope
                .src
                .get(ns.span.lo as usize..ns.span.hi as usize)
                .ok_or_else(|| "unresolved namespace name".to_string())?;
            let head = scope
                .src
                .get(name.span.lo as usize..name.span.hi as usize)
                .ok_or_else(|| "unresolved type name".to_string())?;
            let instance_key = schema_type_key(ty, scope, env, resolving_params)?;
            if seen.iter().any(|seen_key| seen_key == &instance_key) {
                return Err(format!(
                    "JSON_LIMIT: a recursive type `{namespace}.{head}` has no finite JSON decode schema"
                ));
            }
            if seen.len() >= STRUCT_DEPTH {
                return Err(
                    "JSON_LIMIT: the type does not resolve to a finite JSON decode schema"
                        .to_string(),
                );
            }
            if let Some(alias) = (decls.aliases)(&scope.module, Some(namespace), head) {
                if alias.type_params.len() != args.len() {
                    return Err(format!(
                        "type alias `{namespace}.{head}` expects {} type argument{}, found {}",
                        alias.type_params.len(),
                        if alias.type_params.len() == 1 {
                            ""
                        } else {
                            "s"
                        },
                        args.len()
                    ));
                }
                let nested_env = schema_param_env(&alias.type_params, args, env, scope);
                let alias_scope = SchemaScope {
                    module: alias.module,
                    src: alias.src,
                };
                seen.push(instance_key);
                let out = schema_of_with_env(
                    alias.body,
                    decls,
                    &alias_scope,
                    &nested_env,
                    seen,
                    resolving_params,
                );
                seen.pop();
                return out;
            }
            if let Some(record) = (decls.records)(&scope.module, Some(namespace), head) {
                if record.type_params.len() != args.len() {
                    return Err(format!(
                        "record `{namespace}.{head}` expects {} type argument{}, found {}",
                        record.type_params.len(),
                        if record.type_params.len() == 1 {
                            ""
                        } else {
                            "s"
                        },
                        args.len()
                    ));
                }
                let nested_env = schema_param_env(&record.type_params, args, env, scope);
                let record_scope = SchemaScope {
                    module: record.module.clone(),
                    src: record.src,
                };
                seen.push(instance_key);
                let mut out = Vec::with_capacity(record.fields.len());
                for (field_name, field_type, field_default) in &record.fields {
                    let schema = schema_of_with_env(
                        field_type,
                        decls,
                        &record_scope,
                        &nested_env,
                        seen,
                        resolving_params,
                    )?;
                    let folded =
                        field_default.and_then(|value| const_fold_default(value, record.src));
                    out.push((Rc::from(field_name.as_str()), schema, folded));
                }
                seen.pop();
                return Ok(Schema::Record {
                    record_id: Rc::from(record.name),
                    declaration_identity: record.declaration_identity.map(Rc::from),
                    fields: Rc::from(out),
                });
            }
            if let Some(enumeration) = (decls.enums)(&scope.module, Some(namespace), head) {
                if enumeration.type_params.len() != args.len() {
                    return Err(format!(
                        "enum `{namespace}.{head}` expects {} type argument{}, found {}",
                        enumeration.type_params.len(),
                        if enumeration.type_params.len() == 1 {
                            ""
                        } else {
                            "s"
                        },
                        args.len()
                    ));
                }
                let nested_env = schema_param_env(&enumeration.type_params, args, env, scope);
                let enum_scope = SchemaScope {
                    module: enumeration.module.clone(),
                    src: enumeration.src,
                };
                seen.push(instance_key);
                let mut variants = Vec::with_capacity(enumeration.variants.len());
                for (variant_name, variant_index, payload_types) in &enumeration.variants {
                    let mut payloads = Vec::with_capacity(payload_types.len());
                    for payload_type in payload_types {
                        payloads.push(schema_of_with_env(
                            payload_type,
                            decls,
                            &enum_scope,
                            &nested_env,
                            seen,
                            resolving_params,
                        )?);
                    }
                    variants.push((
                        Rc::from(variant_name.as_str()),
                        *variant_index,
                        Rc::from(payloads),
                    ));
                }
                seen.pop();
                return Ok(Schema::Enum {
                    enum_id: Rc::from(enumeration.name),
                    declaration_identity: enumeration.declaration_identity.map(Rc::from),
                    variants: Rc::from(variants),
                });
            }
            if let Some(newtype) = (decls.newtypes)(&scope.module, Some(namespace), head) {
                if newtype.type_params.len() != args.len() {
                    return Err(format!(
                        "newtype `{namespace}.{head}` expects {} type argument{}, found {}",
                        newtype.type_params.len(),
                        if newtype.type_params.len() == 1 {
                            ""
                        } else {
                            "s"
                        },
                        args.len()
                    ));
                }
                let nested_env = schema_param_env(&newtype.type_params, args, env, scope);
                let newtype_scope = SchemaScope {
                    module: newtype.module.clone(),
                    src: newtype.src,
                };
                seen.push(instance_key);
                let base = schema_of_with_env(
                    newtype.base,
                    decls,
                    &newtype_scope,
                    &nested_env,
                    seen,
                    resolving_params,
                )?;
                seen.pop();
                return Ok(Schema::Newtype {
                    newtype_id: Rc::from(newtype.name),
                    declaration_identity: newtype.declaration_identity.map(Rc::from),
                    base: Rc::new(base),
                });
            }
            Err(format!("`{namespace}.{head}` is not JSON-decodable"))
        }
    }
}

/// Canonicalize a type AST under the current nominal substitution environment
/// for recursion detection. Tracking only a declaration head would incorrectly
/// reject finite nesting such as `Box<Box<int>>`; tracking the instantiated
/// shape still rejects `Node` and `Nest<int> -> Nest<int>` cycles.
pub(in crate::value) fn schema_type_key<'a>(
    ty: &'a ast::Type,
    scope: &SchemaScope<'a>,
    env: &SchemaEnv<'a>,
    resolving_params: &mut Vec<SchemaResolution>,
) -> Result<String, String> {
    match &ty.kind {
        TypeKind::Named { name, args } => {
            let head = scope
                .src
                .get(name.span.lo as usize..name.span.hi as usize)
                .ok_or_else(|| "unresolved type name".to_string())?;
            if let Some(replacement) = env.get(head).cloned() {
                if !args.is_empty() {
                    return Err(format!(
                        "type parameter `{head}` cannot take type arguments"
                    ));
                }
                let resolution = (
                    head.to_string(),
                    replacement.ty.span.lo,
                    replacement.ty.span.hi,
                    Rc::as_ptr(&replacement.env) as usize,
                );
                if resolving_params.iter().any(|entry| entry == &resolution) {
                    return Err(format!(
                        "JSON_LIMIT: type parameter `{head}` does not resolve to a finite schema"
                    ));
                }
                resolving_params.push(resolution);
                let key = schema_type_key(
                    replacement.ty,
                    &replacement.scope,
                    replacement.env.as_ref(),
                    resolving_params,
                );
                resolving_params.pop();
                return key;
            }
            let arg_keys = args
                .iter()
                .map(|arg| schema_type_key(arg, scope, env, resolving_params))
                .collect::<Result<Vec<_>, _>>()?;
            if arg_keys.is_empty() {
                Ok(format!("{}::{head}", scope.module))
            } else {
                Ok(format!("{}::{head}<{}>", scope.module, arg_keys.join(",")))
            }
        }
        TypeKind::Record(fields) => {
            let mut keys = Vec::with_capacity(fields.len());
            for field in fields {
                let name = scope
                    .src
                    .get(field.name.span.lo as usize..field.name.span.hi as usize)
                    .ok_or_else(|| "unresolved record field name".to_string())?;
                keys.push(format!(
                    "{name}:{}",
                    schema_type_key(&field.ty, scope, env, resolving_params)?
                ));
            }
            Ok(format!("{{{}}}", keys.join(",")))
        }
        TypeKind::Union(members) => Ok(members
            .iter()
            .map(|member| schema_type_key(member, scope, env, resolving_params))
            .collect::<Result<Vec<_>, _>>()?
            .join("|")),
        TypeKind::Qualified { ns, name, args } => {
            let namespace = scope
                .src
                .get(ns.span.lo as usize..ns.span.hi as usize)
                .ok_or_else(|| "unresolved namespace name".to_string())?;
            let head = scope
                .src
                .get(name.span.lo as usize..name.span.hi as usize)
                .ok_or_else(|| "unresolved type name".to_string())?;
            let arg_keys = args
                .iter()
                .map(|arg| schema_type_key(arg, scope, env, resolving_params))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(format!(
                "{}::{namespace}.{head}<{}>",
                scope.module,
                arg_keys.join(",")
            ))
        }
        TypeKind::Literal | TypeKind::Function { .. } | TypeKind::Unit => scope
            .src
            .get(ty.span.lo as usize..ty.span.hi as usize)
            .map(str::to_string)
            .ok_or_else(|| "unresolved type syntax".to_string()),
    }
}

/// Fold a record-field default into the const subset the JSON decoder may use
/// when the key is absent. Non-const defaults leave the field required.
pub fn const_fold_default(expr: &Expr, src: &str) -> Option<Value> {
    match &expr.kind {
        ExprKind::Int => src
            .get(expr.span.lo as usize..expr.span.hi as usize)
            .and_then(|t| t.replace('_', "").parse::<i64>().ok())
            .map(Value::Int),
        ExprKind::Float => src
            .get(expr.span.lo as usize..expr.span.hi as usize)
            .and_then(|t| t.replace('_', "").parse::<f64>().ok())
            .map(Value::Float),
        ExprKind::Bool(b) => Some(Value::Bool(*b)),
        ExprKind::Null => Some(Value::Null),
        ExprKind::Unit => Some(Value::Unit),
        ExprKind::String(lit) if lit.tag.is_none() => {
            let mut buf = String::new();
            for part in &lit.parts {
                match part {
                    StringPart::Text(span) => {
                        decode_escapes(
                            src.get(span.lo as usize..span.hi as usize)?,
                            &mut buf,
                            *span,
                        )
                        .ok()?;
                    }
                    StringPart::Interpolation(_) => return None,
                }
            }
            Some(Value::str(buf))
        }
        ExprKind::Paren(inner) => const_fold_default(inner, src),
        ExprKind::Unary { op, operand } => {
            let v = const_fold_default(operand, src)?;
            unary_value(*op, v, expr.span).ok()
        }
        ExprKind::Binary { op, lhs, rhs }
            if !matches!(
                op,
                ast::BinaryOp::And | ast::BinaryOp::Or | ast::BinaryOp::Coalesce
            ) =>
        {
            let l = const_fold_default(lhs, src)?;
            let r = const_fold_default(rhs, src)?;
            binary_value(*op, l, r, expr.span).ok()
        }
        _ => None,
    }
}

pub fn builtin_json_parse_as(arg: Value, schema: &Schema, span: Span) -> Result<Value, RtError> {
    let text = match &arg {
        Value::Str(s) => s.clone(),
        other => {
            return Err(fault(
                codes::GUARD_TYPE,
                format!(
                    "`JSON.parseAs` takes a string; got `{}` (§22)",
                    other.kind()
                ),
                span,
            ));
        }
    };
    Ok(match json_parse(&text) {
        Err(e) => Value::Err(Rc::new(Value::str(format!(
            "$: invalid JSON at line {}, column {}: {}",
            e.line, e.column, e.message
        )))),
        Ok(tree) => json_decode_result(&tree, schema),
    })
}

pub fn builtin_json_decode(arg: Value, schema: &Schema, span: Span) -> Result<Value, RtError> {
    match arg {
        Value::Json(node) => Ok(json_decode_result(&node, schema)),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`JSON.decode` takes a JSONValue; got `{}` (§22)",
                other.kind()
            ),
            span,
        )),
    }
}

pub(in crate::value) fn json_decode_result(tree: &JsonValue, schema: &Schema) -> Value {
    match decode_json(tree, schema, "$", 0) {
        Ok(v) => Value::Ok(Rc::new(v)),
        Err(msg) => Value::Err(Rc::new(Value::str(msg))),
    }
}

/// Walk a parsed JSON tree against a lowered Topaz type schema.
pub fn decode_json(
    json: &JsonValue,
    schema: &Schema,
    path: &str,
    depth: u32,
) -> Result<Value, String> {
    if depth > JSON_MAX_DEPTH {
        return Err(format!(
            "JSON_LIMIT: {path}: structure exceeds the JSON.decode depth limit"
        ));
    }
    match schema {
        Schema::Json => Ok(Value::Json(Rc::new(json.clone()))),
        Schema::Int => match json {
            JsonValue::Number(n) => match n.int {
                Some(i) => Ok(Value::Int(i)),
                None => Err(format!(
                    "{path}: expected an integer, found a non-integer number"
                )),
            },
            other => Err(format!(
                "{path}: expected int, found {}",
                json_kind_name(other)
            )),
        },
        Schema::Str => match json {
            JsonValue::String(s) => Ok(Value::Str(s.clone())),
            other => Err(format!(
                "{path}: expected string, found {}",
                json_kind_name(other)
            )),
        },
        Schema::Bool => match json {
            JsonValue::Bool(b) => Ok(Value::Bool(*b)),
            other => Err(format!(
                "{path}: expected bool, found {}",
                json_kind_name(other)
            )),
        },
        Schema::Unit => match json {
            JsonValue::Null => Ok(Value::Unit),
            other => Err(format!(
                "{path}: expected null, found {}",
                json_kind_name(other)
            )),
        },
        Schema::Null => match json {
            JsonValue::Null => Ok(Value::Null),
            other => Err(format!(
                "{path}: expected null, found {}",
                json_kind_name(other)
            )),
        },
        Schema::Option(inner) => match json {
            JsonValue::Null => Ok(Value::None),
            _ => Ok(Value::Some(Rc::new(decode_json(json, inner, path, depth)?))),
        },
        Schema::Array(elem) => match json {
            JsonValue::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for (i, item) in items.iter().enumerate() {
                    out.push(decode_json(item, elem, &format!("{path}[{i}]"), depth + 1)?);
                }
                Ok(Value::array(out))
            }
            other => Err(format!(
                "{path}: expected array, found {}",
                json_kind_name(other)
            )),
        },
        Schema::Map(value_schema) => match json {
            JsonValue::Object(entries) => {
                let mut map = OrderedMap::new();
                for (k, v) in entries.iter() {
                    let decoded = decode_json(v, value_schema, &format!("{path}.{k}"), depth + 1)?;
                    let canon = canonical_key(&Value::Str(k.clone()))
                        .map_err(|_| format!("{path}.{k}: invalid map key"))?;
                    map.try_insert(canon, decoded);
                }
                Ok(Value::Map(Rc::new(RefCell::new(map))))
            }
            other => Err(format!(
                "{path}: expected object, found {}",
                json_kind_name(other)
            )),
        },
        Schema::Newtype {
            newtype_id,
            declaration_identity,
            base,
        } => {
            let inner = decode_json(json, base, path, depth)?;
            Ok(match declaration_identity {
                Some(identity) => Value::newtype_with_identities(
                    newtype_id.as_ref(),
                    identity.as_ref(),
                    None::<&str>,
                    inner,
                ),
                None => Value::newtype(newtype_id.as_ref(), inner),
            })
        }
        Schema::StructRecord { fields } => match json {
            JsonValue::Object(entries) => {
                let mut out = BTreeMap::new();
                for (fname, fschema, _) in fields.iter() {
                    let val = match entries.get(fname) {
                        Some(child) => {
                            decode_json(child, fschema, &format!("{path}.{fname}"), depth + 1)?
                        }
                        None => return Err(format!("{path}: missing required field `{fname}`")),
                    };
                    out.insert(fname.to_string(), val);
                }
                Ok(Value::Record(Rc::new(out)))
            }
            other => Err(format!(
                "{path}: expected object, found {}",
                json_kind_name(other)
            )),
        },
        Schema::Record {
            record_id,
            declaration_identity,
            fields,
        } => match json {
            JsonValue::Object(entries) => {
                let mut out = Vec::with_capacity(fields.len());
                for (fname, fschema, fdefault) in fields.iter() {
                    let val = match entries.get(fname) {
                        Some(child) => {
                            decode_json(child, fschema, &format!("{path}.{fname}"), depth + 1)?
                        }
                        None => match fdefault {
                            Some(d) => d.clone(),
                            None => {
                                return Err(format!("{path}: missing required field `{fname}`"));
                            }
                        },
                    };
                    out.push((fname.clone(), val));
                }
                Ok(match declaration_identity {
                    Some(identity) => Value::nominal_record_with_identities(
                        record_id.as_ref(),
                        identity.as_ref(),
                        None::<&str>,
                        out,
                    ),
                    None => Value::nominal_record(record_id.as_ref(), out),
                })
            }
            other => Err(format!(
                "{path}: expected object, found {}",
                json_kind_name(other)
            )),
        },
        Schema::Enum {
            enum_id,
            declaration_identity,
            variants,
        } => match json {
            JsonValue::Object(entries) => {
                let tag = match entries.get("tag") {
                    Some(JsonValue::String(s)) => s.clone(),
                    Some(other) => {
                        return Err(format!(
                            "{path}.tag: expected string, found {}",
                            json_kind_name(other)
                        ));
                    }
                    None => return Err(format!("{path}: missing enum `tag` field")),
                };
                let (variant_name, variant_index, payload_schemas) = variants
                    .iter()
                    .find(|(name, _, _)| name.as_ref() == tag.as_ref())
                    .ok_or_else(|| format!("{path}: unknown variant tag `{tag}`"))?;
                let values = match entries.get("values") {
                    Some(JsonValue::Array(items)) => items.clone(),
                    Some(other) => {
                        return Err(format!(
                            "{path}.values: expected array, found {}",
                            json_kind_name(other)
                        ));
                    }
                    None => Rc::from([] as [JsonValue; 0]),
                };
                if values.len() != payload_schemas.len() {
                    return Err(format!(
                        "{path}: variant `{tag}` expects {} value(s), found {}",
                        payload_schemas.len(),
                        values.len()
                    ));
                }
                let mut payloads = Vec::with_capacity(values.len());
                for (i, (item, pschema)) in values.iter().zip(payload_schemas.iter()).enumerate() {
                    payloads.push(decode_json(
                        item,
                        pschema,
                        &format!("{path}.values[{i}]"),
                        depth + 1,
                    )?);
                }
                Ok(Value::Enum {
                    enum_id: enum_id.clone(),
                    declaration_identity: declaration_identity.clone(),
                    method_identity: None,
                    variant: variant_name.clone(),
                    variant_index: *variant_index,
                    payloads: Rc::from(payloads),
                })
            }
            other => Err(format!(
                "{path}: expected an enum object, found {}",
                json_kind_name(other)
            )),
        },
    }
}
