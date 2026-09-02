use crate::*;

pub(crate) fn enum_recursion_marker(src: &LoweredText, enum_name: &str, helper: &str) -> String {
    format!(
        "enum-rec\u{0}{}\u{0}{enum_name}\u{0}{helper}",
        src.identity()
    )
}

pub(crate) fn enum_recursion_helper_for<'a>(
    ty: &Type,
    params: &[Ident],
    shared: TypeTestShared<'_, '_, '_>,
    seen: &'a [String],
) -> Option<&'a str> {
    let TypeKind::Named { name, args } = &ty.kind else {
        return None;
    };
    if !type_args_are_identity_params(args, params, shared.src) {
        return None;
    }
    let enum_name = text(shared.src, name.span);
    let src_key = shared.src.identity();
    for marker in seen.iter().rev() {
        let mut parts = marker.split('\u{0}');
        let (Some(kind), Some(src), Some(name), Some(helper), None) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            continue;
        };
        if kind == "enum-rec" && src == src_key && name == enum_name {
            return Some(helper);
        }
    }
    None
}

pub(crate) fn type_args_are_identity_params(
    args: &[Type],
    params: &[Ident],
    src: &LoweredText,
) -> bool {
    args.len() == params.len()
        && args.iter().zip(params.iter()).all(|(arg, param)| {
            let TypeKind::Named { name, args: nested } = &arg.kind else {
                return false;
            };
            nested.is_empty() && text(src, name.span) == text(src, param.span)
        })
}

pub(crate) fn type_mentions_params(ty: &Type, params: &[Ident], src: &LoweredText) -> bool {
    match &ty.kind {
        TypeKind::Named { name, args } => {
            params
                .iter()
                .any(|p| text(src, p.span) == text(src, name.span))
                || args
                    .iter()
                    .any(|arg| type_mentions_params(arg, params, src))
        }
        TypeKind::Qualified { args, .. } => args
            .iter()
            .any(|arg| type_mentions_params(arg, params, src)),
        TypeKind::Record(fields) => fields
            .iter()
            .any(|field| type_mentions_params(&field.ty, params, src)),
        TypeKind::Function {
            params: fn_params,
            ret,
        } => {
            fn_params
                .iter()
                .any(|param| type_mentions_params(&param.ty, params, src))
                || type_mentions_params(ret, params, src)
        }
        TypeKind::Union(members) => members
            .iter()
            .any(|member| type_mentions_params(member, params, src)),
        TypeKind::Literal | TypeKind::Unit => false,
    }
}

pub(crate) fn nominal_decl_type_test(
    ty: &Type,
    params: &[Ident],
    args: &[Type],
    access: &str,
    counter: &mut u32,
    seen: &mut Vec<String>,
    shared: TypeTestShared<'_, '_, '_>,
) -> Option<String> {
    if let Some(helper) = enum_recursion_helper_for(ty, params, shared, seen) {
        return Some(format!("{helper}({access})"));
    }
    if let TypeKind::Named {
        name,
        args: named_args,
    } = &ty.kind
    {
        let n = text(shared.src, name.span);
        if named_args.is_empty()
            && let Some(index) = params.iter().position(|p| text(shared.src, p.span) == n)
        {
            return type_test(
                &args[index],
                shared.arg_src,
                access,
                counter,
                shared.arg_aliases,
                shared.use_locals,
                seen,
            );
        }
    }
    if shared.src.identity() != shared.arg_src.identity()
        && type_mentions_params(ty, params, shared.src)
    {
        return match &ty.kind {
            TypeKind::Named {
                name,
                args: named_args,
            } => {
                let n = text(shared.src, name.span);
                match (n, named_args.as_slice()) {
                    ("Option", [inner]) => {
                        let k = *counter;
                        *counter += 1;
                        let t = nominal_decl_type_test(
                            inner,
                            params,
                            args,
                            &format!("__v{k}"),
                            counter,
                            seen,
                            shared,
                        )?;
                        Some(format!(
                            "match {access} {{ Value::None => true, Value::Some(__tt{k}) => {{ let __v{k}: &Value = __tt{k}; {t} }}, _ => false }}"
                        ))
                    }
                    ("Result", [ok, err]) => {
                        let k = *counter;
                        *counter += 1;
                        let to = nominal_decl_type_test(
                            ok,
                            params,
                            args,
                            &format!("__v{k}"),
                            counter,
                            seen,
                            shared,
                        )?;
                        let te = nominal_decl_type_test(
                            err,
                            params,
                            args,
                            &format!("__v{k}"),
                            counter,
                            seen,
                            shared,
                        )?;
                        Some(format!(
                            "match {access} {{ Value::Ok(__tt{k}) => {{ let __v{k}: &Value = __tt{k}; {to} }}, Value::Err(__tt{k}) => {{ let __v{k}: &Value = __tt{k}; {te} }}, _ => false }}"
                        ))
                    }
                    ("Array", [elem]) => {
                        let k = *counter;
                        *counter += 1;
                        let t = nominal_decl_type_test(
                            elem,
                            params,
                            args,
                            &format!("__v{k}"),
                            counter,
                            seen,
                            shared,
                        )?;
                        Some(format!(
                            "match {access} {{ Value::Array(__tt{k}) => __tt{k}.borrow().iter().all(|__v{k}| {t}), _ => false }}"
                        ))
                    }
                    ("Set", [elem]) => {
                        let k = *counter;
                        *counter += 1;
                        let t = nominal_decl_type_test(
                            elem,
                            params,
                            args,
                            &format!("__v{k}"),
                            counter,
                            seen,
                            shared,
                        )?;
                        Some(format!(
                            "match {access} {{ Value::Set(__tt{k}) => __tt{k}.borrow().items().iter().all(|__v{k}| {t}), _ => false }}"
                        ))
                    }
                    ("Map", [key, val]) => {
                        let k = *counter;
                        *counter += 1;
                        let tk = nominal_decl_type_test(
                            key,
                            params,
                            args,
                            &format!("__mk{k}"),
                            counter,
                            seen,
                            shared,
                        )?;
                        let tv = nominal_decl_type_test(
                            val,
                            params,
                            args,
                            &format!("__mv{k}"),
                            counter,
                            seen,
                            shared,
                        )?;
                        Some(format!(
                            "match {access} {{ Value::Map(__tt{k}) => __tt{k}.borrow().pairs().into_iter().all(|(__mk{k}, __mv{k})| {{ let __mk{k}: &Value = &__mk{k}; let __mv{k}: &Value = &__mv{k}; {tk} && {tv} }}), _ => false }}"
                        ))
                    }
                    _ => None,
                }
            }
            TypeKind::Record(fields) => {
                let k = *counter;
                *counter += 1;
                let n = fields.len();
                let mut checks = String::new();
                for field in fields {
                    let fname = text(shared.src, field.name.span);
                    let fk = *counter;
                    *counter += 1;
                    let t = nominal_decl_type_test(
                        &field.ty,
                        params,
                        args,
                        &format!("__v{fk}"),
                        counter,
                        seen,
                        shared,
                    )?;
                    checks.push_str(&format!(
                        " && (match __m{k}.get({fname:?}) {{ Some(__v{fk}) => {t}, None => false }})"
                    ));
                }
                Some(format!(
                    "match {access} {{ Value::Record(__m{k}) => __m{k}.len() == {n}{checks}, _ => false }}"
                ))
            }
            TypeKind::Union(members) => {
                let k = *counter;
                *counter += 1;
                let u = format!("__u{k}");
                let mut tests = Vec::with_capacity(members.len());
                for member in members {
                    tests.push(nominal_decl_type_test(
                        member, params, args, &u, counter, seen, shared,
                    )?);
                }
                Some(format!(
                    "{{ let {u}: &Value = {access}; {} }}",
                    tests
                        .iter()
                        .map(|test| format!("({test})"))
                        .collect::<Vec<_>>()
                        .join(" || ")
                ))
            }
            TypeKind::Function {
                params: fn_params, ..
            } => {
                let type_variadic = fn_params.last().is_some_and(|p| p.variadic);
                let n_fixed = fn_params.len() - type_variadic as usize;
                Some(format!(
                    "callable_shape_matches({access}, {n_fixed}, {type_variadic})"
                ))
            }
            TypeKind::Literal => Some(literal_type_test(
                text(shared.src, ty.span),
                access,
                counter,
            )),
            TypeKind::Qualified { .. } | TypeKind::Unit => None,
        };
    }
    let substituted = substitute_alias_type_args(ty, params, args, shared.src)?;
    type_test(
        &substituted,
        shared.src,
        access,
        counter,
        shared.aliases,
        shared.use_locals,
        seen,
    )
}

pub(crate) fn nominal_record_type_test(
    decl: &RecordDecl,
    id: &str,
    args: &[Type],
    access: &str,
    counter: &mut u32,
    seen: &mut Vec<String>,
    shared: TypeTestShared<'_, '_, '_>,
) -> Option<String> {
    if decl.type_params.len() != args.len() {
        return None;
    }
    let k = *counter;
    *counter += 1;
    let rec = format!("__nr{k}");
    let mut checks = Vec::new();
    for field in &decl.fields {
        let fname = text(shared.src, field.name.span);
        let ftest = nominal_decl_type_test(
            &field.ty,
            &decl.type_params,
            args,
            "__nf",
            counter,
            seen,
            shared,
        )?;
        checks.push(format!(
            "{rec}_fields.iter().find(|(n, _)| n.as_ref() == {fname:?}).map(|(_, __nf)| {ftest}).unwrap_or(false)"
        ));
    }
    let body = if checks.is_empty() {
        "true".to_string()
    } else {
        checks.join(" && ")
    };
    Some(format!(
        "({{ let {rec}: &Value = {access}; match {rec} {{ Value::NominalRecord {{ record_id, declaration_identity, fields: {rec}_fields, .. }} if nominal_declaration_identity(record_id.as_ref(), declaration_identity.as_deref()) == {id:?} => {{ {body} }}, _ => false }} }})"
    ))
}

pub(crate) fn nominal_enum_type_test(
    decl: &EnumDecl,
    id: &str,
    args: &[Type],
    access: &str,
    counter: &mut u32,
    seen: &mut Vec<String>,
    shared: TypeTestShared<'_, '_, '_>,
) -> Option<String> {
    if decl.type_params.len() != args.len() {
        return None;
    }
    let k = *counter;
    *counter += 1;
    let en = format!("__ne{k}");
    let helper = format!("__tpz_enum_type_{k}");
    let enum_name = text(shared.src, decl.name.span);
    seen.push(enum_recursion_marker(shared.src, enum_name, &helper));
    let mut arms = Vec::new();
    let arms_result = (|| {
        for variant in &decl.variants {
            let vname = text(shared.src, variant.name.span);
            let payload_tys = variant.payload.as_deref().unwrap_or(&[]);
            let mut checks = vec![format!("payloads.len() == {}", payload_tys.len())];
            for (i, pty) in payload_tys.iter().enumerate() {
                let pname = format!("__np{k}_{i}");
                let ptest = nominal_decl_type_test(
                    pty,
                    &decl.type_params,
                    args,
                    &pname,
                    counter,
                    seen,
                    shared,
                )?;
                checks.push(format!(
                    "{{ let {pname}: &Value = &payloads[{i}]; {ptest} }}"
                ));
            }
            arms.push(format!("{vname:?} if {} => true", checks.join(" && ")));
        }
        Some(())
    })();
    seen.pop();
    arms_result?;
    Some(format!(
        "({{ fn {helper}({en}: &Value) -> bool {{ match {en} {{ Value::Enum {{ enum_id, declaration_identity, variant, payloads, .. }} if nominal_declaration_identity(enum_id.as_ref(), declaration_identity.as_deref()) == {id:?} => match variant.as_ref() {{ {}, _ => false }}, _ => false }} }} {helper}({access}) }})",
        arms.join(", ")
    ))
}

pub(crate) fn nominal_newtype_type_test(
    decl: &NewtypeDecl,
    id: &str,
    args: &[Type],
    access: &str,
    counter: &mut u32,
    seen: &mut Vec<String>,
    shared: TypeTestShared<'_, '_, '_>,
) -> Option<String> {
    if decl.type_params.len() != args.len() {
        return None;
    }
    let k = *counter;
    *counter += 1;
    let nt = format!("__nn{k}");
    let inner = format!("__ni{k}");
    let test = nominal_decl_type_test(
        &decl.base,
        &decl.type_params,
        args,
        &inner,
        counter,
        seen,
        shared,
    )?;
    Some(format!(
        "({{ let {nt}: &Value = {access}; match {nt} {{ Value::Newtype {{ newtype_id, declaration_identity, inner, .. }} if nominal_declaration_identity(newtype_id.as_ref(), declaration_identity.as_deref()) == {id:?} => {{ let {inner}: &Value = inner; {test} }}, _ => false }} }})"
    ))
}

pub(crate) fn literal_type_test(literal_text: &str, access: &str, counter: &mut u32) -> String {
    let k = *counter;
    *counter += 1;
    let lit = format!("__lit{k}");
    format!(
        "({{ let {lit}: &str = {literal_text:?}; match {access} {{ Value::Null => {lit} == \"null\", Value::Bool(__b) => {lit} == if *__b {{ \"true\" }} else {{ \"false\" }}, Value::Int(__v) => {lit}.parse::<i64>() == Ok(*__v), Value::Float(__v) => {lit}.parse::<f64>() == Ok(*__v), Value::Str(__s) => {lit}.len() >= 2 && &{lit}[1..{lit}.len() - 1] == __s.as_ref(), _ => false }} }})"
    )
}

impl TypeTestEnv<'_, '_, '_> {
    pub(crate) fn test(
        self,
        ty: &Type,
        access: &str,
        counter: &mut u32,
        seen: &mut Vec<String>,
    ) -> Option<String> {
        type_test(
            ty,
            self.src,
            access,
            counter,
            self.aliases,
            self.use_locals,
            seen,
        )
    }
}

pub(crate) fn union_type_test(
    members: &[Type],
    access: &str,
    counter: &mut u32,
    seen: &mut Vec<String>,
    env: TypeTestEnv<'_, '_, '_>,
) -> Option<String> {
    let k = *counter;
    *counter += 1;
    let value = format!("__u{k}");
    let tests = members
        .iter()
        .map(|member| env.test(member, &value, counter, seen))
        .collect::<Option<Vec<_>>>()?;
    Some(format!(
        "{{ let {value}: &Value = {access}; {} }}",
        tests
            .iter()
            .map(|test| format!("({test})"))
            .collect::<Vec<_>>()
            .join(" || ")
    ))
}

pub(crate) fn record_type_test(
    fields: &[FieldType],
    access: &str,
    counter: &mut u32,
    seen: &mut Vec<String>,
    env: TypeTestEnv<'_, '_, '_>,
) -> Option<String> {
    let k = *counter;
    *counter += 1;
    let mut checks = String::new();
    for field in fields {
        let name = text(env.src, field.name.span);
        let field_k = *counter;
        *counter += 1;
        let test = env.test(&field.ty, &format!("__v{field_k}"), counter, seen)?;
        checks.push_str(&format!(
            " && (match __m{k}.get({name:?}) {{ Some(__v{field_k}) => {test}, None => false }})"
        ));
    }
    Some(format!(
        "match {access} {{ Value::Record(__m{k}) => __m{k}.len() == {}{checks}, _ => false }}",
        fields.len()
    ))
}

pub(crate) fn option_type_test(
    inner: &Type,
    access: &str,
    counter: &mut u32,
    seen: &mut Vec<String>,
    env: TypeTestEnv<'_, '_, '_>,
) -> Option<String> {
    let k = *counter;
    *counter += 1;
    let test = env.test(inner, &format!("__v{k}"), counter, seen)?;
    Some(format!(
        "match {access} {{ Value::None => true, Value::Some(__tt{k}) => {{ let __v{k}: &Value = __tt{k}; {test} }}, _ => false }}"
    ))
}

pub(crate) fn result_type_test(
    ok: &Type,
    err: &Type,
    access: &str,
    counter: &mut u32,
    seen: &mut Vec<String>,
    env: TypeTestEnv<'_, '_, '_>,
) -> Option<String> {
    let k = *counter;
    *counter += 1;
    let ok_test = env.test(ok, &format!("__v{k}"), counter, seen)?;
    let err_test = env.test(err, &format!("__v{k}"), counter, seen)?;
    Some(format!(
        "match {access} {{ Value::Ok(__tt{k}) => {{ let __v{k}: &Value = __tt{k}; {ok_test} }}, Value::Err(__tt{k}) => {{ let __v{k}: &Value = __tt{k}; {err_test} }}, _ => false }}"
    ))
}

pub(crate) fn collection_type_test(
    kind: CollectionTypeTest,
    element: &Type,
    access: &str,
    counter: &mut u32,
    seen: &mut Vec<String>,
    env: TypeTestEnv<'_, '_, '_>,
) -> Option<String> {
    let k = *counter;
    *counter += 1;
    let test = env.test(element, &format!("__v{k}"), counter, seen)?;
    Some(match kind {
        CollectionTypeTest::Array => format!(
            "match {access} {{ Value::Array(__tt{k}) => __tt{k}.borrow().iter().all(|__v{k}| {test}), _ => false }}"
        ),
        CollectionTypeTest::Set => format!(
            "match {access} {{ Value::Set(__tt{k}) => __tt{k}.borrow().items().iter().all(|__v{k}| {test}), _ => false }}"
        ),
    })
}

pub(crate) fn map_type_test(
    key: &Type,
    value: &Type,
    access: &str,
    counter: &mut u32,
    seen: &mut Vec<String>,
    env: TypeTestEnv<'_, '_, '_>,
) -> Option<String> {
    let k = *counter;
    *counter += 1;
    let key_test = env.test(key, &format!("__mk{k}"), counter, seen)?;
    let value_test = env.test(value, &format!("__mv{k}"), counter, seen)?;
    Some(format!(
        "match {access} {{ Value::Map(__tt{k}) => __tt{k}.borrow().pairs().into_iter().all(|(__mk{k}, __mv{k})| {{ let __mk{k}: &Value = &__mk{k}; let __mv{k}: &Value = &__mv{k}; ({key_test}) && ({value_test}) }}), _ => false }}"
    ))
}

pub(crate) fn bare_named_type_test(
    name: &str,
    access: &str,
    counter: &mut u32,
    seen: &mut Vec<String>,
    env: TypeTestEnv<'_, '_, '_>,
) -> Option<String> {
    Some(match name {
        "int" => format!("matches!({access}, Value::Int(_))"),
        "float" => format!("matches!({access}, Value::Float(_))"),
        "string" => format!("matches!({access}, Value::Str(_))"),
        "bool" => format!("matches!({access}, Value::Bool(_))"),
        "JSONValue" => format!("matches!({access}, Value::Json(_))"),
        "Bytes" => format!("matches!({access}, Value::Bytes(_))"),
        "ByteBuffer" => format!("matches!({access}, Value::ByteBuffer(_))"),
        "Path" => format!("matches!({access}, Value::Path(_))"),
        "Regex" => format!("matches!({access}, Value::Regex(_))"),
        "Match" => format!("matches!({access}, Value::RegexMatch(_))"),
        "TOMLValue" => format!("matches!({access}, Value::Toml(_))"),
        "URL" => format!("matches!({access}, Value::Url(_))"),
        "Date" => format!("matches!({access}, Value::Date(_))"),
        "BigInt" => format!("matches!({access}, Value::BigInt(_))"),
        "Decimal" => format!("matches!({access}, Value::Decimal(_))"),
        "RoundingMode" => format!(
            "matches!({access}, Value::Enum {{ enum_id, .. }} if enum_id.as_ref() == \"RoundingMode\")"
        ),
        _ if let Some(def) = env.aliases.enums.get(name) => {
            if !env.aliases.schema_enums.get(name)?.type_params.is_empty() {
                return None;
            }
            let identity =
                nominal_declaration_identity(def.id, def.declaration_identity.as_deref());
            format!(
                "matches!({access}, Value::Enum {{ enum_id, declaration_identity, .. }} if nominal_declaration_identity(enum_id.as_ref(), declaration_identity.as_deref()) == {identity:?})"
            )
        }
        _ if let Some(def) = env.aliases.records.get(name) => {
            if !env.aliases.schema_records.get(name)?.type_params.is_empty() {
                return None;
            }
            let identity =
                nominal_declaration_identity(def.id, def.declaration_identity.as_deref());
            format!("({access}).is_nominal_record_declaration({identity:?})")
        }
        _ if let Some(def) = env.aliases.newtypes.get(name) => {
            if !env
                .aliases
                .schema_newtypes
                .get(name)?
                .type_params
                .is_empty()
            {
                return None;
            }
            let identity =
                nominal_declaration_identity(def.id, def.declaration_identity.as_deref());
            format!("({access}).is_newtype_declaration({identity:?})")
        }
        _ => {
            if env.aliases.poison.contains(name) || seen.iter().any(|seen| seen == name) {
                return None;
            }
            if let Some(body) = env.aliases.table.get(name).copied() {
                seen.push(name.to_string());
                let test = env.test(body, access, counter, seen);
                seen.pop();
                return test;
            }
            if env
                .aliases
                .type_params
                .iter()
                .any(|param| text(env.src, param.span) == name)
            {
                return Some("true".to_string());
            }
            return None;
        }
    })
}

pub(crate) fn parameterized_named_type_test(
    name: &str,
    args: &[Type],
    access: &str,
    counter: &mut u32,
    seen: &mut Vec<String>,
    env: TypeTestEnv<'_, '_, '_>,
) -> Option<String> {
    match (name, args) {
        ("Option", [inner]) => return option_type_test(inner, access, counter, seen, env),
        ("Result", [ok, err]) => {
            return result_type_test(ok, err, access, counter, seen, env);
        }
        ("Array", [element]) => {
            return collection_type_test(
                CollectionTypeTest::Array,
                element,
                access,
                counter,
                seen,
                env,
            );
        }
        ("Set", [element]) => {
            return collection_type_test(
                CollectionTypeTest::Set,
                element,
                access,
                counter,
                seen,
                env,
            );
        }
        ("Map", [key, value]) => {
            return map_type_test(key, value, access, counter, seen, env);
        }
        _ => {}
    }

    if let Some(decl) = env.aliases.schema_records.get(name) {
        let record_definition = env.aliases.records.get(name)?;
        let record_id = nominal_declaration_identity(
            record_definition.id,
            record_definition.declaration_identity.as_deref(),
        );
        if let Some(target_id) = env.aliases.imported_schema_record_modules.get(name) {
            let target = env.aliases.type_ctx.module(target_id)?;
            let target_aliases = env.aliases.with_def_module(target);
            return nominal_record_type_test(
                decl,
                record_id,
                args,
                access,
                counter,
                seen,
                TypeTestShared {
                    src: target.emission.src,
                    aliases: &target_aliases,
                    arg_src: env.src,
                    arg_aliases: env.aliases,
                    use_locals: env.use_locals,
                },
            );
        }
        return nominal_record_type_test(
            decl,
            record_id,
            args,
            access,
            counter,
            seen,
            TypeTestShared {
                src: env.src,
                aliases: env.aliases,
                arg_src: env.src,
                arg_aliases: env.aliases,
                use_locals: env.use_locals,
            },
        );
    }

    if let Some(decl) = env.aliases.schema_enums.get(name) {
        let enum_id = env.aliases.enums.get(name).map_or(name, |definition| {
            nominal_declaration_identity(definition.id, definition.declaration_identity.as_deref())
        });
        if let Some(target_id) = env.aliases.imported_schema_enum_modules.get(name) {
            let target = env.aliases.type_ctx.module(target_id)?;
            let target_aliases = env.aliases.with_def_module(target);
            return nominal_enum_type_test(
                decl,
                enum_id,
                args,
                access,
                counter,
                seen,
                TypeTestShared {
                    src: target.emission.src,
                    aliases: &target_aliases,
                    arg_src: env.src,
                    arg_aliases: env.aliases,
                    use_locals: env.use_locals,
                },
            );
        }
        return nominal_enum_type_test(
            decl,
            enum_id,
            args,
            access,
            counter,
            seen,
            TypeTestShared {
                src: env.src,
                aliases: env.aliases,
                arg_src: env.src,
                arg_aliases: env.aliases,
                use_locals: env.use_locals,
            },
        );
    }

    if let Some(decl) = env.aliases.schema_newtypes.get(name) {
        let newtype_id = env.aliases.newtypes.get(name).map_or(name, |definition| {
            nominal_declaration_identity(definition.id, definition.declaration_identity.as_deref())
        });
        if let Some(target_id) = env.aliases.imported_schema_newtype_modules.get(name) {
            let target = env.aliases.type_ctx.module(target_id)?;
            let target_aliases = env.aliases.with_def_module(target);
            return nominal_newtype_type_test(
                decl,
                newtype_id,
                args,
                access,
                counter,
                seen,
                TypeTestShared {
                    src: target.emission.src,
                    aliases: &target_aliases,
                    arg_src: env.src,
                    arg_aliases: env.aliases,
                    use_locals: env.use_locals,
                },
            );
        }
        return nominal_newtype_type_test(
            decl,
            newtype_id,
            args,
            access,
            counter,
            seen,
            TypeTestShared {
                src: env.src,
                aliases: env.aliases,
                arg_src: env.src,
                arg_aliases: env.aliases,
                use_locals: env.use_locals,
            },
        );
    }

    if env.aliases.poison.contains(name) || seen.iter().any(|seen| seen == name) {
        return None;
    }
    let (params, body) = env.aliases.generic_table.get(name).copied()?;
    let expanded = substitute_alias_type_args(body, params, args, env.src)?;
    seen.push(name.to_string());
    let test = env.test(&expanded, access, counter, seen);
    seen.pop();
    test
}

pub(crate) fn qualified_type_test(
    namespace: &Ident,
    name: &Ident,
    args: &[Type],
    access: &str,
    counter: &mut u32,
    seen: &mut Vec<String>,
    env: TypeTestEnv<'_, '_, '_>,
) -> Option<String> {
    if env.aliases.in_nested {
        return None;
    }
    let namespace = text(env.src, namespace.span);
    let member = text(env.src, name.span);
    if !matches!(
        lookup_bind(env.use_locals, namespace),
        None | Some(Bind::Namespace)
    ) {
        return None;
    }

    let use_module = env.aliases.type_ctx.module(env.aliases.identity)?;
    let target_id = use_module.type_imports.namespaces.get(namespace)?;
    let target = env.aliases.type_ctx.module(target_id)?;
    let local_aliases = &target.local_aliases;
    let exported_types = &target.exported_type_surface;
    let target_aliases = env.aliases.with_def_module(target);
    let shared = TypeTestShared {
        src: target.emission.src,
        aliases: &target_aliases,
        arg_src: env.src,
        arg_aliases: env.aliases,
        use_locals: env.use_locals,
    };

    if let Some(definition) = exported_types.enum_defs.get(member) {
        let identity =
            nominal_declaration_identity(definition.id, definition.declaration_identity.as_deref());
        let declaration = target.local_types.schema_enums.get(member)?;
        if args.is_empty() && declaration.type_params.is_empty() {
            return Some(format!(
                "matches!({access}, Value::Enum {{ enum_id, declaration_identity, .. }} if nominal_declaration_identity(enum_id.as_ref(), declaration_identity.as_deref()) == {identity:?})"
            ));
        }
        return nominal_enum_type_test(declaration, identity, args, access, counter, seen, shared);
    }
    if let Some(definition) = exported_types.record_defs.get(member) {
        let identity =
            nominal_declaration_identity(definition.id, definition.declaration_identity.as_deref());
        let declaration = target.local_types.schema_records.get(member)?;
        if args.is_empty() && declaration.type_params.is_empty() {
            return Some(format!(
                "({access}).is_nominal_record_declaration({identity:?})"
            ));
        }
        return nominal_record_type_test(
            declaration,
            identity,
            args,
            access,
            counter,
            seen,
            shared,
        );
    }
    if let Some(definition) = exported_types.newtype_defs.get(member) {
        let identity =
            nominal_declaration_identity(definition.id, definition.declaration_identity.as_deref());
        let declaration = target.local_types.schema_newtypes.get(member)?;
        if args.is_empty() && declaration.type_params.is_empty() {
            return Some(format!("({access}).is_newtype_declaration({identity:?})"));
        }
        return nominal_newtype_type_test(
            declaration,
            identity,
            args,
            access,
            counter,
            seen,
            shared,
        );
    }

    if !exported_types.names.contains(member) || local_aliases.poison.contains(member) {
        return None;
    }
    let (params, body) = if args.is_empty() {
        (&[][..], *local_aliases.table.get(member)?)
    } else {
        local_aliases.generic_table.get(member).copied()?
    };
    let recursion_key = format!("{target_id}\u{0}{member}");
    if seen.contains(&recursion_key) || body_reaches_qualified(body, target, &mut Vec::new()) {
        return None;
    }

    seen.push(recursion_key);
    let test = if args.is_empty() {
        type_test(
            body,
            target.emission.src,
            access,
            counter,
            &target_aliases,
            env.use_locals,
            seen,
        )
    } else {
        nominal_decl_type_test(body, params, args, access, counter, seen, shared)
    };
    seen.pop();
    test
}

pub(crate) fn type_test(
    ty: &Type,
    src: &LoweredText,
    access: &str,
    counter: &mut u32,
    aliases: &Aliases,
    // §17 the USE-SITE emit locals (params + same-scope lets + captures), for the
    // QUALIFIED shadow check: a namespace head bound here to a non-namespace local
    // refuses. Threaded UNCHANGED through every recursion (alias expansion does not
    // change the consuming use-site env). `Qualified` is the only consumer.
    use_locals: &[(String, Bind)],
    seen: &mut Vec<String>,
) -> Option<String> {
    let env = TypeTestEnv {
        src,
        aliases,
        use_locals,
    };
    Some(match &ty.kind {
        TypeKind::Unit => format!("matches!({access}, Value::Unit)"),
        TypeKind::Literal => literal_type_test(text(src, ty.span), access, counter),
        TypeKind::Named { name, args } if args.is_empty() => {
            bare_named_type_test(text(src, name.span), access, counter, seen, env)?
        }
        // §6 a UNION matches iff ANY member matches → a `||` disjunction, exactly
        // the interpreter's `type_matches` Union arm. Every member must itself be
        // emitter-decidable; an undecidable member refuses the whole union.
        // `access` is bound ONCE to `__u{k}` so the disjunction evaluates it a
        // single time (the call sites only ever pass a side-effect-free place
        // expression, but binding keeps `type_test` correct for any `access`).
        TypeKind::Union(members) => union_type_test(members, access, counter, seen, env)?,
        // §6/§8 the structural containers — the interpreter's `type_matches`
        // Named arms for `Option`/`Result`/`Array`/`Set`. The payload / each
        // element is recursively checked against the inner `__v{K}` (an `&Value`
        // re-borrowed from the matched `__tt{K}`). Non-builtin `Named<T...>` falls
        // through to generic-alias instantiation below.
        TypeKind::Named { name, args } => {
            parameterized_named_type_test(text(src, name.span), args, access, counter, seen, env)?
        }
        // §8 a RECORD type `{ f0: T0, … }` — the interpreter's `type_matches`
        // Record arm: an EXACT field set (`len == N`) with every field present
        // and recursively conforming. A field with an undecidable type `?`-aborts
        // and refuses the whole record.
        TypeKind::Record(fields) => record_type_test(fields, access, counter, seen, env)?,
        // §17 a QUALIFIED type `m.Id` — the interpreter resolves `m` through the
        // LIVE value env to a `Value::Namespace`, then the member against the
        // EXPORTING module's exported alias table (machine.rs `TypeKind::Qualified`
        // + `expand_alias_match`: the body matches under the DEFINING source). The
        // bounded slice mirrors that for the cases the use-site emit locals can
        // decide soundly, and refuses (→ TPZ6001) the rest:
        TypeKind::Qualified { ns, name, args } => {
            qualified_type_test(ns, name, args, access, counter, seen, env)?
        }
        // §3 a FUNCTION type `(P…) -> R` — SHAPE-only conformance, mirroring the
        // interpreter's `type_matches` Function arm: parameter/return types are not
        // runtime-inspectable, only the callable's arity range is (`callable_shape_matches`
        // shares `Builtin::arity_range` with the interpreter). A variadic function type
        // accepts only a variadic-capable callable. The inner P/R types are not
        // recursed (erased), so this never refuses on an "undecidable" param/return.
        TypeKind::Function { params, .. } => {
            let type_variadic = params.last().is_some_and(|p| p.variadic);
            let n_fixed = params.len() - type_variadic as usize;
            format!("callable_shape_matches({access}, {n_fixed}, {type_variadic})")
        }
    })
}

/// §17 transitive: does `ty`, expanded through `module`'s own alias table, reach
/// ANY qualified type `m.T`? The bounded qualified slice refuses such bodies (a
/// chained `Id = A; A = n.T` resolves the inner qualified in a SECOND module's
/// use-site env, which this slice does not thread). `seen` bounds recursive aliases.
pub(crate) fn body_reaches_qualified(
    ty: &Type,
    module: &ModuleTypeCtx<'_>,
    seen: &mut Vec<String>,
) -> bool {
    match &ty.kind {
        TypeKind::Qualified { .. } => true,
        TypeKind::Named { name, args } => {
            if args.iter().any(|a| body_reaches_qualified(a, module, seen)) {
                return true;
            }
            let n = text(module.emission.src, name.span);
            if seen.iter().any(|s| s == n) {
                return false;
            }
            match module.local_aliases.table.get(n) {
                Some(body) => {
                    seen.push(n.to_string());
                    let r = body_reaches_qualified(body, module, seen);
                    seen.pop();
                    r
                }
                None => match module.local_aliases.generic_table.get(n).copied() {
                    Some((params, body)) => {
                        let Some(expanded) =
                            substitute_alias_type_args(body, params, args, module.emission.src)
                        else {
                            return false;
                        };
                        seen.push(n.to_string());
                        let r = body_reaches_qualified(&expanded, module, seen);
                        seen.pop();
                        r
                    }
                    None => false,
                },
            }
        }
        TypeKind::Record(fields) => fields
            .iter()
            .any(|f| body_reaches_qualified(&f.ty, module, seen)),
        TypeKind::Union(members) => members
            .iter()
            .any(|m| body_reaches_qualified(m, module, seen)),
        TypeKind::Function { params, ret } => {
            params
                .iter()
                .any(|p| body_reaches_qualified(&p.ty, module, seen))
                || body_reaches_qualified(ret, module, seen)
        }
        TypeKind::Literal | TypeKind::Unit => false,
    }
}
