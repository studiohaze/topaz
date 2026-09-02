use crate::*;

pub(super) fn emit_type_spec_for_typed_let(
    ty: &Type,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let mut expanding = Vec::new();
    emit_type_spec_with_bindings(ty, ctx, &BTreeMap::new(), &mut expanding, true)
}

pub(super) fn emit_type_spec_for_typed_pattern(
    ty: &Type,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    let mut expanding = Vec::new();
    emit_type_spec_with_bindings(ty, ctx, &BTreeMap::new(), &mut expanding, false)
}

pub(super) type TypeSpecBindings = BTreeMap<String, String>;

pub(super) const MAX_TYPE_SPEC_EXPANSION_DEPTH: usize = 64;

pub(super) fn emit_type_spec_with_bindings(
    ty: &Type,
    ctx: &Ctx<'_>,
    bindings: &TypeSpecBindings,
    expanding: &mut Vec<String>,
    allow_function_type: bool,
) -> Result<String, PyEmitError> {
    match &ty.kind {
        TypeKind::Named { name, args } => {
            let source_name = ctx.text(name.span);
            if args.is_empty()
                && let Some(bound) = bindings.get(source_name)
            {
                return Ok(bound.clone());
            }
            if let Some(alias) = ctx.type_alias(source_name)
                && args.len() == alias.params
            {
                let checked_bindings = args
                    .iter()
                    .map(|arg| {
                        emit_type_spec_with_bindings(
                            arg,
                            ctx,
                            bindings,
                            expanding,
                            allow_function_type,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                return emit_type_spec_for_checked_type(
                    &alias.body,
                    ty.span,
                    ctx,
                    allow_function_type,
                    &checked_bindings,
                );
            }
            if ctx.has_type_alias(source_name) {
                return Err(PyEmitError::unsupported("typed pattern type").at(ty.span));
            }
            match (source_name, args.as_slice()) {
                ("int", []) => Ok(py_string("int")),
                ("float", []) => Ok(py_string("float")),
                ("string", []) => Ok(py_string("string")),
                ("bool", []) => Ok(py_string("bool")),
                ("JSONValue", []) => Ok(py_string("JSONValue")),
                ("Bytes", []) => Ok(py_string("Bytes")),
                ("ByteBuffer", []) => Ok(py_string("ByteBuffer")),
                ("Option", [inner]) => Ok(format!(
                    "({}, {})",
                    py_string("option"),
                    emit_type_spec_with_bindings(
                        inner,
                        ctx,
                        bindings,
                        expanding,
                        allow_function_type,
                    )?
                )),
                ("Result", [ok, err]) => Ok(format!(
                    "({}, {}, {})",
                    py_string("result"),
                    emit_type_spec_with_bindings(
                        ok,
                        ctx,
                        bindings,
                        expanding,
                        allow_function_type,
                    )?,
                    emit_type_spec_with_bindings(
                        err,
                        ctx,
                        bindings,
                        expanding,
                        allow_function_type,
                    )?
                )),
                ("Array", [elem]) => Ok(format!(
                    "({}, {})",
                    py_string("array"),
                    emit_type_spec_with_bindings(
                        elem,
                        ctx,
                        bindings,
                        expanding,
                        allow_function_type
                    )?
                )),
                ("Set", [elem]) => Ok(format!(
                    "({}, {})",
                    py_string("set"),
                    emit_type_spec_with_bindings(elem, ctx, bindings, expanding, false)?
                )),
                ("Map", [key, value]) => Ok(format!(
                    "({}, {}, {})",
                    py_string("map"),
                    emit_type_spec_with_bindings(key, ctx, bindings, expanding, false)?,
                    emit_type_spec_with_bindings(
                        value,
                        ctx,
                        bindings,
                        expanding,
                        allow_function_type
                    )?
                )),
                (record_name, _) if ctx.records.contains_key(record_name) => {
                    let record = ctx.records.get(record_name).expect("checked record");
                    emit_nominal_record_type_spec(record, args, ty.span, ctx, bindings, expanding)
                }
                (newtype_name, _) if ctx.newtypes.contains_key(newtype_name) => {
                    let newtype = ctx.newtypes.get(newtype_name).expect("checked newtype");
                    emit_newtype_type_spec(newtype, args, ty.span, ctx, bindings, expanding)
                }
                (enum_name, _) if ctx.enums.contains_key(enum_name) => {
                    let enum_def = ctx.enums.get(enum_name).expect("checked enum");
                    emit_enum_type_spec(enum_def, args, ty.span, ctx, bindings, expanding)
                }
                _ => Err(PyEmitError::unsupported("typed pattern type").at(ty.span)),
            }
        }
        TypeKind::Union(members) => {
            let specs = members
                .iter()
                .map(|member| emit_type_spec_with_bindings(member, ctx, bindings, expanding, false))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(format!("({}, {})", py_string("union"), py_tuple(specs)))
        }
        TypeKind::Record(fields) => {
            let specs = fields
                .iter()
                .map(|field| {
                    let source_name = ctx.text(field.name.span);
                    Ok(format!(
                        "({}, {}, {})",
                        py_string(source_name),
                        py_string(&mangle(source_name)),
                        emit_type_spec_with_bindings(
                            &field.ty,
                            ctx,
                            bindings,
                            expanding,
                            allow_function_type,
                        )?
                    ))
                })
                .collect::<Result<Vec<_>, PyEmitError>>()?;
            Ok(format!("({}, {})", py_string("record"), py_tuple(specs)))
        }
        TypeKind::Function { params, .. } => {
            if !allow_function_type {
                return Err(PyEmitError::unsupported("typed pattern type").at(ty.span));
            }
            let type_variadic = params.last().is_some_and(|param| param.variadic);
            let fixed_count = params.len() - type_variadic as usize;
            let py_variadic = if type_variadic { "True" } else { "False" };
            Ok(format!(
                "({}, {fixed_count}, {py_variadic})",
                py_string("function")
            ))
        }
        TypeKind::Qualified { ns, name, args } => {
            let namespace = ctx.text(ns.span);
            let member = ctx.text(name.span);
            match ctx.namespace_export(namespace, member) {
                Some(ModuleRuntimeExport::Record { record, .. }) => {
                    emit_nominal_record_type_spec(record, args, ty.span, ctx, bindings, expanding)
                }
                Some(ModuleRuntimeExport::Enum { enum_def, .. }) => {
                    emit_enum_type_spec(enum_def, args, ty.span, ctx, bindings, expanding)
                }
                Some(ModuleRuntimeExport::Newtype { newtype, .. }) => {
                    emit_newtype_type_spec(newtype, args, ty.span, ctx, bindings, expanding)
                }
                _ => Err(PyEmitError::unsupported("typed pattern type").at(ty.span)),
            }
        }
        TypeKind::Literal => Ok(format!(
            "({}, {})",
            py_string("literal"),
            py_string(ctx.text(ty.span))
        )),
        TypeKind::Unit => Ok(py_string("unit")),
    }
}

pub(super) fn emit_type_spec_for_checked_type(
    ty: &CheckType,
    span: Span,
    ctx: &Ctx<'_>,
    allow_function_type: bool,
    bindings: &[String],
) -> Result<String, PyEmitError> {
    match ty {
        CheckType::Prim(CheckPrim::Int) => Ok(py_string("int")),
        CheckType::Prim(CheckPrim::Float) => Ok(py_string("float")),
        CheckType::Prim(CheckPrim::String) => Ok(py_string("string")),
        CheckType::Prim(CheckPrim::Bool) => Ok(py_string("bool")),
        CheckType::Prim(CheckPrim::Unit) => Ok(py_string("unit")),
        CheckType::Var(index) => bindings
            .get(*index as usize)
            .cloned()
            .ok_or_else(|| PyEmitError::unsupported("typed pattern type").at(span)),
        CheckType::JsonValue => Ok(py_string("JSONValue")),
        CheckType::Bytes => Ok(py_string("Bytes")),
        CheckType::ByteBuffer => Ok(py_string("ByteBuffer")),
        CheckType::Union(members) => {
            let specs = members
                .iter()
                .map(|member| emit_type_spec_for_checked_type(member, span, ctx, false, bindings))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(format!("({}, {})", py_string("union"), py_tuple(specs)))
        }
        CheckType::Record(fields) => {
            let specs = fields
                .iter()
                .map(|(field_name, field_ty)| {
                    Ok(format!(
                        "({}, {}, {})",
                        py_string(field_name),
                        py_string(&mangle(field_name)),
                        emit_type_spec_for_checked_type(
                            field_ty,
                            span,
                            ctx,
                            allow_function_type,
                            bindings,
                        )?
                    ))
                })
                .collect::<Result<Vec<_>, PyEmitError>>()?;
            Ok(format!("({}, {})", py_string("record"), py_tuple(specs)))
        }
        CheckType::Func {
            params, variadic, ..
        } => {
            if !allow_function_type {
                return Err(PyEmitError::unsupported("typed pattern type").at(span));
            }
            let fixed_count = params.len();
            let py_variadic = if variadic.is_some() { "True" } else { "False" };
            Ok(format!(
                "({}, {fixed_count}, {py_variadic})",
                py_string("function")
            ))
        }
        CheckType::Ctor(CheckCtor::Option, args) if args.len() == 1 => Ok(format!(
            "({}, {})",
            py_string("option"),
            emit_type_spec_for_checked_type(&args[0], span, ctx, allow_function_type, bindings,)?
        )),
        CheckType::Ctor(CheckCtor::Result, args) if args.len() == 2 => Ok(format!(
            "({}, {}, {})",
            py_string("result"),
            emit_type_spec_for_checked_type(&args[0], span, ctx, allow_function_type, bindings,)?,
            emit_type_spec_for_checked_type(&args[1], span, ctx, allow_function_type, bindings,)?
        )),
        CheckType::Ctor(CheckCtor::Array, args) if args.len() == 1 => Ok(format!(
            "({}, {})",
            py_string("array"),
            emit_type_spec_for_checked_type(&args[0], span, ctx, allow_function_type, bindings,)?
        )),
        CheckType::Ctor(CheckCtor::Set, args) if args.len() == 1 => Ok(format!(
            "({}, {})",
            py_string("set"),
            emit_type_spec_for_checked_type(&args[0], span, ctx, false, bindings)?
        )),
        CheckType::Ctor(CheckCtor::Map, args) if args.len() == 2 => Ok(format!(
            "({}, {}, {})",
            py_string("map"),
            emit_type_spec_for_checked_type(&args[0], span, ctx, false, bindings)?,
            emit_type_spec_for_checked_type(&args[1], span, ctx, allow_function_type, bindings)?
        )),
        CheckType::NominalRecord { base, args } if args.is_empty() => {
            if let Some(record) = ctx.records.get(base) {
                Ok(format!(
                    "({}, {})",
                    py_string("nominal_record"),
                    py_string(nominal_declaration_identity(
                        &record.source_name,
                        record.declaration_identity.as_deref(),
                    ))
                ))
            } else {
                Err(PyEmitError::unsupported("typed pattern type").at(span))
            }
        }
        CheckType::Enum { base, args } if args.is_empty() => {
            if let Some(enum_def) = ctx.enums.get(base) {
                Ok(format!(
                    "({}, {})",
                    py_string("enum"),
                    py_string(nominal_declaration_identity(
                        &enum_def.source_name,
                        enum_def.declaration_identity.as_deref(),
                    ))
                ))
            } else {
                Err(PyEmitError::unsupported("typed pattern type").at(span))
            }
        }
        CheckType::Newtype { base, args } if args.is_empty() => {
            if let Some(newtype) = ctx.newtypes.get(base) {
                Ok(format!(
                    "({}, {})",
                    py_string("newtype"),
                    py_string(nominal_declaration_identity(
                        &newtype.source_name,
                        newtype.declaration_identity.as_deref(),
                    ))
                ))
            } else {
                Err(PyEmitError::unsupported("typed pattern type").at(span))
            }
        }
        _ => Err(PyEmitError::unsupported("typed pattern type").at(span)),
    }
}

pub(super) fn emit_nominal_record_type_spec(
    record: &NominalRecordDef<'_>,
    args: &[Rc<Type>],
    span: Span,
    ctx: &Ctx<'_>,
    bindings: &TypeSpecBindings,
    expanding: &mut Vec<String>,
) -> Result<String, PyEmitError> {
    if args.is_empty() && record.type_params.is_empty() {
        return Ok(format!(
            "({}, {})",
            py_string("nominal_record"),
            py_string(nominal_declaration_identity(
                &record.source_name,
                record.declaration_identity.as_deref(),
            ))
        ));
    }
    if args.len() != record.type_params.len() {
        return Err(PyEmitError::unsupported("typed pattern type").at(span));
    }
    let arg_specs = args
        .iter()
        .map(|arg| emit_type_spec_with_bindings(arg, ctx, bindings, expanding, false))
        .collect::<Result<Vec<_>, PyEmitError>>()?;
    let expansion_key = format!("{}<{}>", record.py_class_name, arg_specs.join("|"));
    if expanding.iter().any(|existing| existing == &expansion_key) {
        return Err(PyEmitError::unsupported("typed pattern type").at(span));
    }
    if expanding.len() >= MAX_TYPE_SPEC_EXPANSION_DEPTH {
        return Err(PyEmitError::unsupported("typed pattern type").at(span));
    }
    let mut nested = bindings.clone();
    for (param, spec) in record.type_params.iter().zip(arg_specs.iter()) {
        nested.insert(param.clone(), spec.clone());
    }
    expanding.push(expansion_key);
    let field_specs = record
        .fields
        .iter()
        .map(|field| {
            Ok(format!(
                "({}, {}, {})",
                py_string(&field.source_name),
                py_string(&mangle(&field.source_name)),
                emit_type_spec_with_bindings(field.ty, ctx, &nested, expanding, false)?
            ))
        })
        .collect::<Result<Vec<_>, PyEmitError>>();
    expanding.pop();
    let field_specs = field_specs?;
    Ok(format!(
        "({}, {}, {})",
        py_string("nominal_record"),
        py_string(nominal_declaration_identity(
            &record.source_name,
            record.declaration_identity.as_deref(),
        )),
        py_tuple(field_specs)
    ))
}

pub(super) fn emit_enum_type_spec(
    enum_def: &EnumDef,
    args: &[Rc<Type>],
    span: Span,
    ctx: &Ctx<'_>,
    bindings: &TypeSpecBindings,
    expanding: &mut Vec<String>,
) -> Result<String, PyEmitError> {
    if args.is_empty() && enum_def.type_params.is_empty() {
        return Ok(format!(
            "({}, {})",
            py_string("enum"),
            py_string(nominal_declaration_identity(
                &enum_def.source_name,
                enum_def.declaration_identity.as_deref(),
            ))
        ));
    }
    if enum_def.type_params.is_empty() || args.len() != enum_def.type_params.len() {
        return Err(PyEmitError::unsupported("typed pattern type").at(span));
    }
    let arg_specs = args
        .iter()
        .map(|arg| emit_type_spec_with_bindings(arg, ctx, bindings, expanding, false))
        .collect::<Result<Vec<_>, PyEmitError>>()?;
    let expansion_key = format!("enum:{}<{}>", enum_def.source_name, arg_specs.join("|"));
    if expanding.iter().any(|existing| existing == &expansion_key) {
        return Ok(format!(
            "({}, {})",
            py_string("type_ref"),
            py_string(&expansion_key)
        ));
    }
    if expanding.len() >= MAX_TYPE_SPEC_EXPANSION_DEPTH {
        return Err(PyEmitError::unsupported("typed pattern type").at(span));
    }
    let mut nested = bindings.clone();
    for (param, spec) in enum_def.type_params.iter().zip(arg_specs.iter()) {
        nested.insert(param.clone(), spec.clone());
    }
    expanding.push(expansion_key.clone());
    let variant_specs = enum_def
        .variants
        .iter()
        .map(|(variant_name, variant)| {
            let payload_specs = variant
                .payload
                .iter()
                .map(|payload| {
                    emit_type_spec_with_bindings(payload, ctx, &nested, expanding, false)
                })
                .collect::<Result<Vec<_>, PyEmitError>>()?;
            Ok(format!(
                "({}, {})",
                py_string(variant_name),
                py_tuple(payload_specs)
            ))
        })
        .collect::<Result<Vec<_>, PyEmitError>>();
    expanding.pop();
    let variant_specs = variant_specs?;
    Ok(format!(
        "({}, {}, {}, {})",
        py_string("enum"),
        py_string(nominal_declaration_identity(
            &enum_def.source_name,
            enum_def.declaration_identity.as_deref(),
        )),
        py_string(&expansion_key),
        py_tuple(variant_specs)
    ))
}

pub(super) fn emit_newtype_type_spec(
    newtype: &NewtypeDef,
    args: &[Rc<Type>],
    span: Span,
    ctx: &Ctx<'_>,
    bindings: &TypeSpecBindings,
    expanding: &mut Vec<String>,
) -> Result<String, PyEmitError> {
    if args.is_empty() && newtype.type_params.is_empty() {
        return Ok(format!(
            "({}, {})",
            py_string("newtype"),
            py_string(nominal_declaration_identity(
                &newtype.source_name,
                newtype.declaration_identity.as_deref(),
            ))
        ));
    }
    if newtype.type_params.is_empty() || args.len() != newtype.type_params.len() {
        return Err(PyEmitError::unsupported("typed pattern type").at(span));
    }
    let arg_specs = args
        .iter()
        .map(|arg| emit_type_spec_with_bindings(arg, ctx, bindings, expanding, false))
        .collect::<Result<Vec<_>, PyEmitError>>()?;
    let expansion_key = format!("newtype:{}<{}>", newtype.source_name, arg_specs.join("|"));
    if expanding.iter().any(|existing| existing == &expansion_key) {
        return Ok(format!(
            "({}, {})",
            py_string("type_ref"),
            py_string(&expansion_key)
        ));
    }
    if expanding.len() >= MAX_TYPE_SPEC_EXPANSION_DEPTH {
        return Err(PyEmitError::unsupported("typed pattern type").at(span));
    }
    let mut nested = bindings.clone();
    for (param, spec) in newtype.type_params.iter().zip(arg_specs.iter()) {
        nested.insert(param.clone(), spec.clone());
    }
    expanding.push(expansion_key.clone());
    let base_spec = emit_type_spec_with_bindings(&newtype.base, ctx, &nested, expanding, false);
    expanding.pop();
    Ok(format!(
        "({}, {}, {}, {})",
        py_string("newtype"),
        py_string(nominal_declaration_identity(
            &newtype.source_name,
            newtype.declaration_identity.as_deref(),
        )),
        py_string(&expansion_key),
        base_spec?
    ))
}
