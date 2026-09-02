use crate::*;

pub(super) type JsonSchemaBindings = BTreeMap<String, String>;

pub(super) struct NamedJsonSchemaEmission<'a, 'ctx> {
    pub(super) head: &'a str,
    pub(super) namespace: Option<&'a str>,
    pub(super) args: &'a [Rc<Type>],
    pub(super) span: Span,
    pub(super) ctx: &'a Ctx<'ctx>,
    pub(super) scope_module: &'a str,
    pub(super) bindings: &'a JsonSchemaBindings,
}

pub(super) fn emit_json_schema(ty: &Type, ctx: &Ctx<'_>) -> Result<String, PyEmitError> {
    emit_json_schema_with_bindings(
        ty,
        ctx,
        ctx.module_identity,
        &BTreeMap::new(),
        &mut Vec::new(),
    )
}

pub(super) fn emit_json_schema_with_bindings(
    ty: &Type,
    ctx: &Ctx<'_>,
    scope_module: &str,
    bindings: &JsonSchemaBindings,
    seen: &mut Vec<String>,
) -> Result<String, PyEmitError> {
    match &ty.kind {
        TypeKind::Unit => Ok(py_string("unit")),
        TypeKind::Literal => match ctx.text(ty.span) {
            "null" => Ok(py_string("null")),
            "true" | "false" => Ok(py_string("bool")),
            text if text.starts_with('"') => Ok(py_string("string")),
            text if !text.contains(['.', 'e', 'E']) => Ok(py_string("int")),
            _ => Err(PyEmitError::unsupported("typed JSON type arguments").at(ty.span)),
        },
        TypeKind::Named { name, args } => emit_json_schema_named(
            NamedJsonSchemaEmission {
                head: ctx.text(name.span),
                namespace: None,
                args,
                span: ty.span,
                ctx,
                scope_module,
                bindings,
            },
            seen,
        ),
        TypeKind::Qualified { ns, name, args } => emit_json_schema_named(
            NamedJsonSchemaEmission {
                head: ctx.text(name.span),
                namespace: Some(ctx.text(ns.span)),
                args,
                span: ty.span,
                ctx,
                scope_module,
                bindings,
            },
            seen,
        ),
        TypeKind::Record(fields) => {
            let fields = fields
                .iter()
                .map(|field| {
                    let source_name = ctx.text(field.name.span);
                    Ok(format!(
                        "({}, {}, {})",
                        py_string(source_name),
                        py_string(&mangle(source_name)),
                        emit_json_schema_with_bindings(
                            &field.ty,
                            ctx,
                            scope_module,
                            bindings,
                            seen,
                        )?
                    ))
                })
                .collect::<Result<Vec<_>, PyEmitError>>()?;
            Ok(format!("({}, {})", py_string("struct"), py_tuple(fields)))
        }
        TypeKind::Function { .. } | TypeKind::Union(_) => {
            Err(PyEmitError::unsupported("typed JSON type arguments").at(ty.span))
        }
    }
}

pub(super) fn emit_json_schema_named(
    input: NamedJsonSchemaEmission<'_, '_>,
    seen: &mut Vec<String>,
) -> Result<String, PyEmitError> {
    let NamedJsonSchemaEmission {
        head,
        namespace,
        args,
        span,
        ctx,
        scope_module,
        bindings,
    } = input;
    if namespace.is_none()
        && args.is_empty()
        && let Some(schema) = bindings.get(head)
    {
        return Ok(schema.clone());
    }
    if namespace.is_none() {
        match (head, args) {
            ("int", []) => return Ok(py_string("int")),
            ("string", []) => return Ok(py_string("string")),
            ("bool", []) => return Ok(py_string("bool")),
            ("JSONValue", []) => return Ok(py_string("json")),
            ("Option", [inner]) => {
                return Ok(format!(
                    "({}, {})",
                    py_string("option"),
                    emit_json_schema_with_bindings(inner, ctx, scope_module, bindings, seen)?
                ));
            }
            ("Array", [inner]) => {
                return Ok(format!(
                    "({}, {})",
                    py_string("array"),
                    emit_json_schema_with_bindings(inner, ctx, scope_module, bindings, seen)?
                ));
            }
            ("Map", [key, value]) => {
                let key = emit_json_schema_with_bindings(key, ctx, scope_module, bindings, seen)?;
                if key != py_string("string") {
                    return Err(PyEmitError::unsupported("typed JSON type arguments").at(span));
                }
                return Ok(format!(
                    "({}, {})",
                    py_string("map"),
                    emit_json_schema_with_bindings(value, ctx, scope_module, bindings, seen)?
                ));
            }
            _ => {}
        }
    }

    let arg_schemas = args
        .iter()
        .map(|arg| emit_json_schema_with_bindings(arg, ctx, scope_module, bindings, seen))
        .collect::<Result<Vec<_>, _>>()?;
    let Some((target_module, target_name)) =
        resolve_json_schema_decl(ctx, scope_module, namespace, head)
    else {
        return Err(PyEmitError::unsupported("typed JSON type arguments").at(span));
    };
    let instance_key = format!("{target_module}::{target_name}<{}>", arg_schemas.join("|"));
    if seen.contains(&instance_key) || seen.len() >= MAX_TYPE_SPEC_EXPANSION_DEPTH {
        return Err(PyEmitError::unsupported("typed JSON type arguments").at(span));
    }
    let Some(module) = ctx.schema_modules.get(&target_module) else {
        return Err(PyEmitError::unsupported("typed JSON type arguments").at(span));
    };

    if let Some(alias) = module.aliases.get(&target_name) {
        if alias.type_params.len() != arg_schemas.len() {
            return Err(PyEmitError::unsupported("typed JSON type arguments").at(span));
        }
        let nested = json_schema_nested_bindings(bindings, &alias.type_params, &arg_schemas);
        seen.push(instance_key);
        let schema =
            emit_json_schema_with_bindings(&alias.body, ctx, &target_module, &nested, seen);
        seen.pop();
        return schema;
    }
    if let Some(record) = module.records.get(&target_name) {
        if record.type_params.len() != arg_schemas.len() {
            return Err(PyEmitError::unsupported("typed JSON type arguments").at(span));
        }
        let nested = json_schema_nested_bindings(bindings, &record.type_params, &arg_schemas);
        seen.push(instance_key);
        let fields = record
            .fields
            .iter()
            .map(|field| {
                let schema =
                    emit_json_schema_with_bindings(field.ty, ctx, &target_module, &nested, seen)?;
                let mut parts = vec![
                    py_string(&field.source_name),
                    py_string(&mangle(&field.source_name)),
                    schema,
                ];
                if let Some(default) = field
                    .default
                    .as_ref()
                    .and_then(|default| default.const_py.clone())
                {
                    parts.push(default);
                }
                Ok(py_tuple(parts))
            })
            .collect::<Result<Vec<_>, PyEmitError>>();
        seen.pop();
        let fields = py_tuple(fields?);
        return Ok(record.declaration_identity.as_ref().map_or_else(
            || {
                format!(
                    "({}, {}, {fields})",
                    py_string("record"),
                    py_string(&record.source_name)
                )
            },
            |identity| {
                format!(
                    "({}, {}, {}, {fields})",
                    py_string("record"),
                    py_string(&record.source_name),
                    py_string(identity)
                )
            },
        ));
    }
    if let Some(enum_def) = module.enums.get(&target_name) {
        if enum_def.type_params.len() != arg_schemas.len() {
            return Err(PyEmitError::unsupported("typed JSON type arguments").at(span));
        }
        let nested = json_schema_nested_bindings(bindings, &enum_def.type_params, &arg_schemas);
        seen.push(instance_key);
        let variants = enum_def
            .variants
            .iter()
            .map(|(name, variant)| {
                let payloads = variant
                    .payload
                    .iter()
                    .map(|payload| {
                        emit_json_schema_with_bindings(payload, ctx, &target_module, &nested, seen)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(format!(
                    "({}, {}, {})",
                    py_string(name),
                    variant.variant_index,
                    py_tuple(payloads)
                ))
            })
            .collect::<Result<Vec<_>, PyEmitError>>();
        seen.pop();
        let variants = py_tuple(variants?);
        return Ok(enum_def.declaration_identity.as_ref().map_or_else(
            || {
                format!(
                    "({}, {}, {variants})",
                    py_string("enum"),
                    py_string(&enum_def.source_name)
                )
            },
            |identity| {
                format!(
                    "({}, {}, {}, {variants})",
                    py_string("enum"),
                    py_string(&enum_def.source_name),
                    py_string(identity)
                )
            },
        ));
    }
    if let Some(newtype) = module.newtypes.get(&target_name) {
        if newtype.type_params.len() != arg_schemas.len() {
            return Err(PyEmitError::unsupported("typed JSON type arguments").at(span));
        }
        let nested = json_schema_nested_bindings(bindings, &newtype.type_params, &arg_schemas);
        seen.push(instance_key);
        let base =
            emit_json_schema_with_bindings(&newtype.base, ctx, &target_module, &nested, seen);
        seen.pop();
        let base = base?;
        return Ok(newtype.declaration_identity.as_ref().map_or_else(
            || {
                format!(
                    "({}, {}, {base})",
                    py_string("newtype"),
                    py_string(&newtype.source_name)
                )
            },
            |identity| {
                format!(
                    "({}, {}, {}, {base})",
                    py_string("newtype"),
                    py_string(&newtype.source_name),
                    py_string(identity)
                )
            },
        ));
    }
    Err(PyEmitError::unsupported("typed JSON type arguments").at(span))
}

pub(super) fn resolve_json_schema_decl(
    ctx: &Ctx<'_>,
    scope_module: &str,
    namespace: Option<&str>,
    head: &str,
) -> Option<(String, String)> {
    let scope = ctx.schema_modules.get(scope_module)?;
    if let Some(namespace) = namespace {
        return scope
            .imports
            .namespaces
            .get(namespace)
            .cloned()
            .map(|target| (target, head.to_string()));
    }
    scope
        .imports
        .selected
        .get(head)
        .cloned()
        .or_else(|| Some((scope_module.to_string(), head.to_string())))
}

pub(super) fn json_schema_nested_bindings(
    outer: &JsonSchemaBindings,
    params: &[String],
    args: &[String],
) -> JsonSchemaBindings {
    let mut nested = outer.clone();
    for (param, arg) in params.iter().zip(args) {
        nested.insert(param.clone(), arg.clone());
    }
    nested
}

pub(super) fn emit_or_pattern_condition(
    value_py: &str,
    alts: &[Pattern],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<(String, Vec<PatternBinding>), PyEmitError> {
    let mut lowered = Vec::with_capacity(alts.len());
    for alt in alts {
        lowered.push(emit_pattern_condition(value_py, alt, ctx)?);
    }
    let canonical_order: Vec<String> = lowered
        .first()
        .map(|(_, bindings)| {
            bindings
                .iter()
                .map(|binding| binding.name.clone())
                .collect()
        })
        .unwrap_or_default();
    let mut canonical_sorted = canonical_order.clone();
    canonical_sorted.sort();
    if canonical_sorted.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(PyEmitError::unsupported("match pattern").at(span));
    }
    for (_, bindings) in &lowered {
        let mut names: Vec<String> = bindings
            .iter()
            .map(|binding| binding.name.clone())
            .collect();
        names.sort();
        if names.windows(2).any(|pair| pair[0] == pair[1]) || names != canonical_sorted {
            return Err(PyEmitError::unsupported("match pattern").at(span));
        }
    }
    let condition = if lowered.is_empty() {
        "False".to_string()
    } else {
        format!(
            "({})",
            lowered
                .iter()
                .map(|(condition, _)| condition.as_str())
                .collect::<Vec<_>>()
                .join(" or ")
        )
    };
    let mut bindings = Vec::new();
    for name in canonical_order {
        let mut expr = format!("tpz_impossible_match({value_py}, {})", py_span(span));
        for (condition, alt_bindings) in lowered.iter().rev() {
            let Some(binding) = alt_bindings.iter().find(|binding| binding.name == name) else {
                return Err(PyEmitError::unsupported("match pattern").at(span));
            };
            expr = format!("({} if {condition} else {expr})", binding.value_py);
        }
        bindings.push(PatternBinding::always(name, expr));
    }
    Ok((condition, bindings))
}

pub(super) fn emit_nominal_pattern_condition(
    tmp: &str,
    name: &Ident,
    fields: &[RecordPatternField],
    ctx: &Ctx<'_>,
) -> Result<(String, Vec<PatternBinding>), PyEmitError> {
    let record_name = ctx.text(name.span);
    let Some(record) = ctx.records.get(record_name) else {
        return Err(PyEmitError::unsupported("match pattern").at(name.span));
    };
    let mut bindings = Vec::with_capacity(fields.len());
    let mut conditions = Vec::new();
    for field in fields {
        let source_name = ctx.text(field.name.span);
        if !record
            .fields
            .iter()
            .any(|decl| decl.source_name == source_name)
        {
            return Err(PyEmitError::unsupported("match pattern").at(field.span));
        }
        let binding = match field.pattern.as_ref() {
            None => source_name.to_string(),
            Some(pattern) => {
                let (subcondition, subbindings) = emit_pattern_condition(
                    &format!("{tmp}.{}", mangle(source_name)),
                    pattern,
                    ctx,
                )?;
                bindings.extend(subbindings);
                conditions.push(subcondition);
                continue;
            }
        };
        bindings.push(PatternBinding::always(
            binding,
            format!("{tmp}.{}", mangle(source_name)),
        ));
    }
    let identity =
        nominal_declaration_identity(&record.source_name, record.declaration_identity.as_deref());
    let nominal_test = format!("tpz_is_nominal_record({tmp}, {})", py_string(identity));
    Ok((
        if conditions.is_empty() {
            nominal_test
        } else {
            format!("({nominal_test} and {})", conditions.join(" and "))
        },
        bindings,
    ))
}

pub(super) fn emit_record_pattern_condition(
    value_py: &str,
    fields: &[RecordPatternField],
    ctx: &Ctx<'_>,
) -> Result<(String, Vec<PatternBinding>), PyEmitError> {
    let mut conditions = vec![
        format!("isinstance(getattr({value_py}, \"__topaz_record_fields__\", None), tuple)"),
        format!("not isinstance(getattr({value_py}, \"__topaz_record_id__\", None), str)"),
    ];
    let mut bindings = Vec::new();
    for field in fields {
        let source_name = ctx.text(field.name.span);
        let py_field = mangle(source_name);
        conditions.push(format!(
            "any(_py == {} and _source == {} for _py, _source in {value_py}.__topaz_record_fields__)",
            py_string(&py_field),
            py_string(source_name)
        ));
        let access = format!(
            "tpz_record_field({value_py}, {}, {}, {})",
            py_string(&py_field),
            py_string(source_name),
            py_span(field.span)
        );
        match field.pattern.as_ref() {
            None => bindings.push(PatternBinding::always(source_name, access)),
            Some(pattern) => {
                let (subcondition, subbindings) = emit_pattern_condition(&access, pattern, ctx)?;
                conditions.push(subcondition);
                bindings.extend(subbindings);
            }
        }
    }
    Ok((format!("({})", conditions.join(" and ")), bindings))
}

pub(super) fn emit_list_pattern_condition(
    value_py: &str,
    elements: &[ListPatternElem],
    ctx: &Ctx<'_>,
) -> Result<(String, Vec<PatternBinding>), PyEmitError> {
    let mut conditions = vec![format!("isinstance({value_py}, list)")];
    let mut bindings = Vec::new();
    let rest_at = elements
        .iter()
        .position(|element| matches!(element, ListPatternElem::Rest(_)));
    match rest_at {
        None => {
            conditions.push(format!("len({value_py}) == {}", elements.len()));
            for (idx, element) in elements.iter().enumerate() {
                let ListPatternElem::Pattern(pattern) = element else {
                    unreachable!("rest_at is none")
                };
                let access = format!("{value_py}[{idx}]");
                let (subcondition, subbindings) = emit_pattern_condition(&access, pattern, ctx)?;
                conditions.push(subcondition);
                bindings.extend(subbindings);
            }
        }
        Some(pos) => {
            let after = elements.len() - pos - 1;
            conditions.push(format!("len({value_py}) >= {}", pos + after));
            for (idx, element) in elements[..pos].iter().enumerate() {
                let ListPatternElem::Pattern(pattern) = element else {
                    unreachable!("prefix cannot contain rest")
                };
                let access = format!("{value_py}[{idx}]");
                let (subcondition, subbindings) = emit_pattern_condition(&access, pattern, ctx)?;
                conditions.push(subcondition);
                bindings.extend(subbindings);
            }
            for (offset, element) in elements[pos + 1..].iter().enumerate() {
                let ListPatternElem::Pattern(pattern) = element else {
                    unreachable!("suffix cannot contain rest")
                };
                let access = format!("{value_py}[len({value_py}) - {after} + {offset}]");
                let (subcondition, subbindings) = emit_pattern_condition(&access, pattern, ctx)?;
                conditions.push(subcondition);
                bindings.extend(subbindings);
            }
            if let ListPatternElem::Rest(Some(pattern)) = &elements[pos] {
                let access = if after == 0 {
                    format!("{value_py}[{pos}:]")
                } else {
                    format!("{value_py}[{pos}:len({value_py}) - {after}]")
                };
                let (subcondition, subbindings) = emit_pattern_condition(&access, pattern, ctx)?;
                conditions.push(subcondition);
                bindings.extend(subbindings);
            }
        }
    }
    Ok((format!("({})", conditions.join(" and ")), bindings))
}

pub(super) fn emit_case_arm_body_as_stmt(
    body: &CaseArmBody,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    match body {
        CaseArmBody::Expr(expr) => match &expr.kind {
            ExprKind::Block(block) => emit_block_as_stmt(block, ctx, indent, out),
            _ => emit_expr_stmt(expr, ctx, indent, out),
        },
        CaseArmBody::Return {
            value: Some(value), ..
        } => emit_return_value(value, ctx, indent, out),
        CaseArmBody::Return { value: None, .. } => {
            writeln!(out, "{}raise TpzReturn(TPZ_UNIT)", " ".repeat(indent))
                .expect("write to string");
            Ok(())
        }
    }
}

pub(super) fn emit_block_as_stmt(
    block: &Block,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    if block.stmts.is_empty() && block.tail.is_none() {
        writeln!(out, "{}pass", " ".repeat(indent)).expect("write to string");
        return Ok(());
    }
    if block_has_direct_defer(block) {
        emit_defer_scoped_block_as_stmt(block, ctx, indent, out)
    } else {
        emit_block_as_stmt_contents(block, ctx, indent, out)
    }
}

pub(super) fn emit_defer_scoped_block_as_stmt(
    block: &Block,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    let pad = " ".repeat(indent);
    let mark = ctx.fresh_temp("defer_mark");
    writeln!(out, "{pad}{mark} = len(__tpz_defers)").expect("write to string");
    writeln!(out, "{pad}try:").expect("write to string");
    emit_block_as_stmt_contents(block, ctx, indent + 4, out)?;
    writeln!(out, "{pad}    __tpz_run_defers_to({mark})").expect("write to string");
    writeln!(
        out,
        "{pad}except (TpzReturn, TpzLoopBreak, TpzLoopContinue):"
    )
    .expect("write to string");
    writeln!(out, "{pad}    __tpz_run_defers_to({mark})").expect("write to string");
    writeln!(out, "{pad}    raise").expect("write to string");
    writeln!(out, "{pad}except TpzFault:").expect("write to string");
    writeln!(out, "{pad}    raise").expect("write to string");
    Ok(())
}

pub(super) fn emit_block_as_stmt_contents(
    block: &Block,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<(), PyEmitError> {
    ctx.push_scope();
    let snapshot = match pre_register_nested_functions(block, ctx, indent, out) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            ctx.pop_scope();
            return Err(error);
        }
    };
    let result = (|| -> Result<(), PyEmitError> {
        for stmt in &block.stmts {
            emit_stmt(stmt, ctx, indent, out)?;
        }
        if let Some(tail) = block.tail.as_deref() {
            emit_expr_stmt(tail, ctx, indent, out)?;
        }
        Ok(())
    })();
    snapshot.restore(ctx);
    ctx.pop_scope();
    result
}

pub(super) fn block_has_direct_defer(block: &Block) -> bool {
    block
        .stmts
        .iter()
        .any(|stmt| matches!(stmt.kind, StmtKind::Defer(_)))
}
