use super::*;

impl<'a> Machine<'a> {
    pub(super) fn step_pipe_and_decode_frame(&mut self, frame: Frame) -> Result<(), RtError> {
        match frame {
            Frame::KPipe { rhs, span, root } => match rhs.as_ref() {
                PipeRhs::Field(field) => {
                    let object = self.values.pop().expect("pipe lhs");
                    let v =
                        self.member_access(object, self.text(field.span), span, root.as_deref())?;
                    self.values.push(v);
                    Ok(())
                }
                PipeRhs::Expr(f) => {
                    let lhs = self.values.pop().expect("pipe lhs");
                    if let ExprKind::Call {
                        callee,
                        args,
                        type_args,
                    } = &f.kind
                    {
                        if let Some((schema, parse_text, param)) =
                            self.json_typed_decode_member(callee, type_args)?
                        {
                            let member = if parse_text { "parseAs" } else { "decode" };
                            self.frames.push(Frame::KJsonDecode {
                                schema: Rc::new(schema),
                                span: f.span,
                                parse_text,
                            });
                            match args.as_slice() {
                                [] => {
                                    self.values.push(lhs);
                                    return Ok(());
                                }
                                _ if topaz_syntax::ast::call_args_contain_placeholder(args) => {
                                    let saved = self.env.clone();
                                    self.env = child_env(&saved);
                                    self.bind("_".to_string(), lhs, false);
                                    self.frames.push(Frame::PopScope(saved));
                                    let arg =
                                        self.json_typed_decode_arg(f.span, member, args, param)?;
                                    return self.eval_expr(arg);
                                }
                                _ => {
                                    let arg =
                                        self.json_typed_decode_arg(f.span, member, args, param)?;
                                    return self.eval_expr(arg);
                                }
                            }
                        }
                        if topaz_syntax::ast::contains_placeholder(callee) {
                            // §11: `_` is valid only in the stage
                            // call's argument list, never as the
                            // callee.
                            return Err(fault(
                                codes::GUARD_TYPE,
                                "a placeholder `_` is valid only in a pipeline stage's argument list (§11)",
                                callee.span,
                            ));
                        }
                        if topaz_syntax::ast::call_args_contain_placeholder(args) {
                            // §11: bind `_` to the piped value in a
                            // child scope for the stage's evaluation.
                            // Closures created in the stage CAPTURE
                            // this scope (so an escaping lambda keeps
                            // `_`), and scope exit / unwind / arm
                            // isolation clean it up automatically.
                            let saved = self.env.clone();
                            self.env = child_env(&saved);
                            self.bind("_".to_string(), lhs, false);
                            self.frames.push(Frame::PopScope(saved));
                            self.schedule_call(callee, args, f.span, None)
                        } else {
                            // §11: first-argument insertion — the
                            // piped value is the call's first
                            // positional.
                            self.schedule_call(callee, args, f.span, Some(lhs))
                        }
                    } else {
                        // §11: unary application of a callable stage.
                        self.frames
                            .push(Frame::KCallApplyWithArg { arg: lhs, span });
                        self.frames.push(Frame::Eval(f.clone()));
                        Ok(())
                    }
                }
            },
            Frame::KCallApplyWithArg { arg, span } => {
                self.values.push(arg);
                self.apply_call(1, Vec::new(), Vec::new(), false, span)
            }
            Frame::KJsonDecode {
                schema,
                span,
                parse_text,
            } => {
                let arg = self.values.pop().expect("JSON typed decode arg");
                let v = if parse_text {
                    builtin_json_parse_as(arg, &schema, span)?
                } else {
                    builtin_json_decode(arg, &schema, span)?
                };
                self.values.push(v);
                Ok(())
            }
            _ => unreachable!("frame family changed after classification"),
        }
    }

    pub(super) fn resolve_json_schema_decl<'b>(
        &'b self,
        current_module: &str,
        namespace: Option<&str>,
        head: &str,
    ) -> Option<(String, Rc<str>, String, &'b str, &'b SchemaDeclTables)> {
        let current_scope = &self.module_scopes.get(current_module)?.types;
        let (target_module, target_name) = match namespace {
            Some(namespace) => {
                let target = current_scope.schema_imports.namespaces.get(namespace)?;
                (target.to_string(), head.to_string())
            }
            None => current_scope
                .schema_imports
                .selected
                .get(head)
                .map(|(target, imported)| (target.to_string(), imported.clone()))
                .unwrap_or_else(|| (current_module.to_string(), head.to_string())),
        };
        let target_scope = &self.module_scopes.get(target_module.as_str())?.types;
        let src = &target_scope.src;
        Some((
            target_module,
            target_scope.declaration_identity.clone(),
            target_name,
            src.as_ref(),
            &target_scope.schema_decls,
        ))
    }

    pub(super) fn build_json_schema(&self, ty: &Type) -> Result<Schema, String> {
        let src = self.src.as_ref();
        let module = self
            .source_module_index
            .get(&(self.src.as_ptr() as usize))
            .map(|module| module.to_string())
            .ok_or_else(|| "typed JSON declaration scope is unavailable".to_string())?;
        let aliases = |current_module: &str, namespace: Option<&str>, head: &str| {
            let (target_module, _, target_name, target_src, decls) =
                self.resolve_json_schema_decl(current_module, namespace, head)?;
            let decl = decls.aliases.get(&target_name)?;
            Some(SchemaAliasDecl {
                module: target_module,
                src: target_src,
                type_params: decl
                    .type_params
                    .iter()
                    .map(|param| {
                        target_src[param.span.lo as usize..param.span.hi as usize].to_string()
                    })
                    .collect(),
                body: &decl.ty,
            })
        };
        let records = |current_module: &str, namespace: Option<&str>, head: &str| {
            let (target_module, target_identity, target_name, target_src, decls) =
                self.resolve_json_schema_decl(current_module, namespace, head)?;
            let decl = decls.records.get(&target_name)?;
            let name =
                target_src[decl.name.span.lo as usize..decl.name.span.hi as usize].to_string();
            let declaration_identity = (self.language_version >= LangVersion::V5_20)
                .then(|| receiver_method_identity(&target_identity, &name));
            Some(SchemaRecordDecl {
                module: target_module,
                src: target_src,
                name,
                declaration_identity,
                type_params: decl
                    .type_params
                    .iter()
                    .map(|param| {
                        target_src[param.span.lo as usize..param.span.hi as usize].to_string()
                    })
                    .collect(),
                fields: decl
                    .fields
                    .iter()
                    .map(|field| {
                        let name = target_src
                            [field.name.span.lo as usize..field.name.span.hi as usize]
                            .to_string();
                        (name, &field.ty, field.default.as_deref())
                    })
                    .collect(),
            })
        };
        let enums = |current_module: &str, namespace: Option<&str>, head: &str| {
            let (target_module, target_identity, target_name, target_src, decls) =
                self.resolve_json_schema_decl(current_module, namespace, head)?;
            let decl = decls.enums.get(&target_name)?;
            let name =
                target_src[decl.name.span.lo as usize..decl.name.span.hi as usize].to_string();
            let declaration_identity = (self.language_version >= LangVersion::V5_20)
                .then(|| receiver_method_identity(&target_identity, &name));
            Some(SchemaEnumDecl {
                module: target_module,
                src: target_src,
                name,
                declaration_identity,
                type_params: decl
                    .type_params
                    .iter()
                    .map(|param| {
                        target_src[param.span.lo as usize..param.span.hi as usize].to_string()
                    })
                    .collect(),
                variants: decl
                    .variants
                    .iter()
                    .enumerate()
                    .map(|(index, variant)| {
                        let name = target_src
                            [variant.name.span.lo as usize..variant.name.span.hi as usize]
                            .to_string();
                        let payloads = variant
                            .payload
                            .as_ref()
                            .map_or_else(Vec::new, |types| types.iter().collect());
                        (name, index as u32, payloads)
                    })
                    .collect(),
            })
        };
        let newtypes = |current_module: &str, namespace: Option<&str>, head: &str| {
            let (target_module, target_identity, target_name, target_src, decls) =
                self.resolve_json_schema_decl(current_module, namespace, head)?;
            let decl = decls.newtypes.get(&target_name)?;
            let name =
                target_src[decl.name.span.lo as usize..decl.name.span.hi as usize].to_string();
            let declaration_identity = (self.language_version >= LangVersion::V5_20)
                .then(|| receiver_method_identity(&target_identity, &name));
            Some(SchemaNewtypeDecl {
                module: target_module,
                src: target_src,
                name,
                declaration_identity,
                type_params: decl
                    .type_params
                    .iter()
                    .map(|param| {
                        target_src[param.span.lo as usize..param.span.hi as usize].to_string()
                    })
                    .collect(),
                base: &decl.base,
            })
        };
        let schema_decls = SchemaDecls {
            src,
            module,
            aliases: &aliases,
            records: &records,
            enums: &enums,
            newtypes: &newtypes,
        };
        schema_of(ty, &schema_decls, &mut Vec::new())
    }

    pub(super) fn json_typed_decode_member(
        &self,
        callee: &Expr,
        type_args: &[Type],
    ) -> Result<Option<(Schema, bool, &'static str)>, RtError> {
        let ExprKind::Member { object, field } = &callee.kind else {
            return Ok(None);
        };
        let ExprKind::Ident = &object.kind else {
            return Ok(None);
        };
        let head = self.text(object.span);
        let member = self.text(field.span);
        if head != "JSON" || lookup(&self.env, head).is_some() {
            return Ok(None);
        }
        let (parse_text, param) = match member {
            "parseAs" => (true, "text"),
            "decode" => (false, "value"),
            _ => return Ok(None),
        };
        let ty = type_args.first().ok_or_else(|| {
            fault(
                codes::GUARD_TYPE,
                format!("`JSON.{member}` requires an explicit type argument (§22)"),
                callee.span,
            )
        })?;
        let schema = self.build_json_schema(ty).map_err(|e| {
            fault(
                codes::GUARD_TYPE,
                format!("`JSON.{member}` type argument is not JSON-decodable: {e}"),
                callee.span,
            )
        })?;
        Ok(Some((schema, parse_text, param)))
    }

    pub(super) fn json_typed_decode_arg<'b>(
        &self,
        span: Span,
        member: &str,
        args: &'b [CallArg],
        param: &str,
    ) -> Result<&'b Expr, RtError> {
        let expr = match args {
            [CallArg::Positional(e)] => e,
            [CallArg::Named { name, value }] if self.text(name.span) == param => value,
            _ => {
                return Err(fault(
                    codes::GUARD_ARITY,
                    format!("`JSON.{member}` takes one `{param}` argument (§22)"),
                    span,
                ));
            }
        };
        Ok(expr)
    }
}

/// §2/§13a: a fault inside a `ConstExpression` is a static error —
/// reported as a dynamic guard here, never as a runtime fault.
pub(super) fn const_guarded(result: Result<Value, RtError>, span: Span) -> Result<Value, RtError> {
    result.map_err(|e| {
        if e.is_fault() {
            fault(
                codes::GUARD_TYPE,
                format!("constant expression error: {}", e.message),
                span,
            )
        } else {
            e
        }
    })
}

/// Type-parameter bindings of a generic alias expansion: each
/// parameter maps to its argument TOGETHER with the source it was
/// written in and the bindings it was written under, closure-style,
/// so nested and cross-module generic aliases substitute correctly.
/// name → (bound type, the source its spans index, the bindings in
/// force where it was supplied). Reference-counted so nested
/// expansions share rather than copy.
type TypeBindingMap = BTreeMap<String, (Rc<Type>, Rc<str>, TypeBindings)>;
#[derive(Clone, Default)]
pub(super) struct TypeBindings(Rc<TypeBindingMap>);

pub(super) struct TypeMatchState {
    expanding: Vec<(usize, String)>,
    span: Span,
}

impl TypeMatchState {
    pub(super) fn new(span: Span) -> Self {
        Self {
            expanding: Vec::new(),
            span,
        }
    }
}

pub(super) struct AliasMatchDefinition<'a> {
    name: &'a str,
    body_src: &'a Rc<str>,
    params: &'a [Ident],
    body: &'a Type,
}

pub(super) fn text_in(src: &str, span: Span) -> &str {
    &src[span.lo as usize..span.hi as usize]
}

impl<'a> Machine<'a> {
    pub(super) fn value_matches_type(
        &self,
        ty: &Type,
        src: &Rc<str>,
        value: &Value,
        span: Span,
    ) -> Result<bool, RtError> {
        let bindings = TypeBindings::default();
        let mut state = TypeMatchState::new(span);
        self.type_matches(ty, src, value, &bindings, &mut state)
    }

    pub(super) fn nominal_type_bindings_in(
        &self,
        params: &[Ident],
        param_src: &Rc<str>,
        args: &[Rc<Type>],
        arg_src: &Rc<str>,
        env: &TypeBindings,
        span: Span,
    ) -> Result<TypeBindings, RtError> {
        if params.len() != args.len() {
            return Err(fault(
                codes::GUARD_UNIMPLEMENTED,
                format!(
                    "generic nominal type takes {} type argument(s), found {}",
                    params.len(),
                    args.len()
                ),
                span,
            ));
        }
        let mut inner: BTreeMap<String, (Rc<Type>, Rc<str>, TypeBindings)> = BTreeMap::new();
        for (param, arg) in params.iter().zip(args.iter()) {
            inner.insert(
                text_in(param_src, param.span).to_string(),
                (arg.clone(), arg_src.clone(), env.clone()),
            );
        }
        Ok(TypeBindings(Rc::new(inner)))
    }

    pub(super) fn nominal_enum_definition_type_matches(
        &self,
        definition: &EnumRuntimeDef,
        args: &[Rc<Type>],
        arg_src: &Rc<str>,
        value: &Value,
        env: &TypeBindings,
        state: &mut TypeMatchState,
    ) -> Result<bool, RtError> {
        let runtime_id = self.nominal_definition_identity(
            definition.method_identity.as_ref(),
            &definition.runtime_id,
        );
        let Value::Enum {
            variant, payloads, ..
        } = value
        else {
            return Ok(false);
        };
        if value.nominal_declaration_id() != Some(runtime_id) {
            return Ok(false);
        }
        let bindings = self.nominal_type_bindings_in(
            &definition.decl.type_params,
            &definition.decl_src,
            args,
            arg_src,
            env,
            state.span,
        )?;
        let Some(vdecl) = definition
            .decl
            .variants
            .iter()
            .find(|v| text_in(&definition.decl_src, v.name.span) == variant.as_ref())
        else {
            return Ok(false);
        };
        let tys: &[Type] = vdecl.payload.as_deref().unwrap_or(&[]);
        if tys.len() != payloads.len() {
            return Ok(false);
        }
        for (ty, payload) in tys.iter().zip(payloads.iter()) {
            if !self.type_matches(ty, &definition.decl_src, payload, &bindings, state)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(super) fn nominal_enum_type_matches(
        &self,
        name: &str,
        args: &[Rc<Type>],
        arg_src: &Rc<str>,
        value: &Value,
        env: &TypeBindings,
        state: &mut TypeMatchState,
    ) -> Result<bool, RtError> {
        let Some(definition) = self.enum_definition_in(arg_src, name) else {
            return Ok(false);
        };
        self.nominal_enum_definition_type_matches(definition, args, arg_src, value, env, state)
    }

    pub(super) fn nominal_record_definition_type_matches(
        &self,
        definition: &RecordRuntimeDef,
        args: &[Rc<Type>],
        arg_src: &Rc<str>,
        value: &Value,
        env: &TypeBindings,
        state: &mut TypeMatchState,
    ) -> Result<bool, RtError> {
        let runtime_id = self.nominal_definition_identity(
            definition.method_identity.as_ref(),
            &definition.runtime_id,
        );
        let Value::NominalRecord { fields, .. } = value else {
            return Ok(false);
        };
        if value.nominal_declaration_id() != Some(runtime_id) {
            return Ok(false);
        }
        let bindings = self.nominal_type_bindings_in(
            &definition.decl.type_params,
            &definition.decl_src,
            args,
            arg_src,
            env,
            state.span,
        )?;
        for f in &definition.decl.fields {
            let fname = text_in(&definition.decl_src, f.name.span);
            let Some((_, value)) = fields.iter().find(|(n, _)| n.as_ref() == fname) else {
                return Ok(false);
            };
            if !self.type_matches(&f.ty, &definition.decl_src, value, &bindings, state)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(super) fn nominal_record_type_matches(
        &self,
        name: &str,
        args: &[Rc<Type>],
        arg_src: &Rc<str>,
        value: &Value,
        env: &TypeBindings,
        state: &mut TypeMatchState,
    ) -> Result<bool, RtError> {
        let Some(definition) = self.record_definition_in(arg_src, name) else {
            return Ok(false);
        };
        self.nominal_record_definition_type_matches(definition, args, arg_src, value, env, state)
    }

    pub(super) fn nominal_newtype_definition_type_matches(
        &self,
        definition: &NewtypeRuntimeDef,
        args: &[Rc<Type>],
        arg_src: &Rc<str>,
        value: &Value,
        env: &TypeBindings,
        state: &mut TypeMatchState,
    ) -> Result<bool, RtError> {
        let runtime_id = self.nominal_definition_identity(
            definition.method_identity.as_ref(),
            &definition.runtime_id,
        );
        let Value::Newtype { inner, .. } = value else {
            return Ok(false);
        };
        if value.nominal_declaration_id() != Some(runtime_id) {
            return Ok(false);
        }
        let bindings = self.nominal_type_bindings_in(
            &definition.decl.type_params,
            &definition.decl_src,
            args,
            arg_src,
            env,
            state.span,
        )?;
        self.type_matches(
            &definition.decl.base,
            &definition.decl_src,
            inner,
            &bindings,
            state,
        )
    }

    pub(super) fn nominal_newtype_type_matches(
        &self,
        name: &str,
        args: &[Rc<Type>],
        arg_src: &Rc<str>,
        value: &Value,
        env: &TypeBindings,
        state: &mut TypeMatchState,
    ) -> Result<bool, RtError> {
        let Some(definition) = self.newtype_definition_in(arg_src, name) else {
            return Ok(false);
        };
        self.nominal_newtype_definition_type_matches(definition, args, arg_src, value, env, state)
    }

    /// §6 runtime type conformance: primitives, unit,
    /// literals, unions, records (exact fields), standard
    /// containers (element-checked), function shapes (arity-range),
    /// lexically scoped type aliases incl. generic ones, and
    /// qualified types through namespace bindings. `src` is the
    /// source the TYPE's spans index (its defining module). Alias
    /// cycles are guards.
    pub(super) fn type_matches(
        &self,
        ty: &Type,
        src: &Rc<str>,
        value: &Value,
        env: &TypeBindings,
        state: &mut TypeMatchState,
    ) -> Result<bool, RtError> {
        let span = state.span;
        Ok(match &ty.kind {
            TypeKind::Union(members) => {
                for m in members {
                    if self.type_matches(m, src, value, env, state)? {
                        return Ok(true);
                    }
                }
                false
            }
            TypeKind::Unit => matches!(value, Value::Unit),
            TypeKind::Record(fields) => match value {
                Value::Record(map) => {
                    // §8: exact field sets.
                    if map.len() != fields.len() {
                        return Ok(false);
                    }
                    for field in fields {
                        let name = text_in(src, field.name.span);
                        let Some(v) = map.get(name).cloned() else {
                            return Ok(false);
                        };
                        if !self.type_matches(&field.ty, src, &v, env, state)? {
                            return Ok(false);
                        }
                    }
                    true
                }
                _ => false,
            },
            TypeKind::Function { params, .. } => {
                // Shape-level conformance: parameter/return types
                // are not dynamically inspectable; the arity range
                // is. A variadic function type accepts only
                // variadic-capable callables.
                let type_variadic = params.last().is_some_and(|p| p.variadic);
                let n_fixed = params.len() - type_variadic as usize;
                match callable_arity(value) {
                    None => false, // not callable
                    Some((min, max)) => {
                        if type_variadic {
                            max.is_none() && min <= n_fixed
                        } else {
                            min <= n_fixed && max.is_none_or(|m| n_fixed <= m)
                        }
                    }
                }
            }
            TypeKind::Named { name, args } => {
                let n = text_in(src, name.span);
                if args.is_empty() {
                    match n {
                        "int" => return Ok(matches!(value, Value::Int(_))),
                        "float" => return Ok(matches!(value, Value::Float(_))),
                        "string" => return Ok(matches!(value, Value::Str(_))),
                        "bool" => return Ok(matches!(value, Value::Bool(_))),
                        "JSONValue" => return Ok(matches!(value, Value::Json(_))),
                        "Bytes" => return Ok(matches!(value, Value::Bytes(_))),
                        "ByteBuffer" => return Ok(matches!(value, Value::ByteBuffer(_))),
                        "Path" => return Ok(matches!(value, Value::Path(_))),
                        "Regex" => return Ok(matches!(value, Value::Regex(_))),
                        "Match" => return Ok(matches!(value, Value::RegexMatch(_))),
                        "TOMLValue" => return Ok(matches!(value, Value::Toml(_))),
                        "URL" => return Ok(matches!(value, Value::Url(_))),
                        "Date" => return Ok(matches!(value, Value::Date(_))),
                        "BigInt" => return Ok(matches!(value, Value::BigInt(_))),
                        "Decimal" => return Ok(matches!(value, Value::Decimal(_))),
                        "RoundingMode" => {
                            return Ok(matches!(
                                value,
                                Value::Enum { enum_id, .. } if enum_id.as_ref() == "RoundingMode"
                            ));
                        }
                        _ => {}
                    }
                    // §3 (v5.3): a user-enum type pattern matches NOMINALLY — the
                    // value is an enum of this exact enum id (a same-named variant
                    // of a different enum does NOT match).
                    if let Some(definition) = self.enum_definition_in(src, n)
                        && definition.decl.type_params.is_empty()
                    {
                        let expected = self.nominal_definition_identity(
                            definition.method_identity.as_ref(),
                            &definition.runtime_id,
                        );
                        return Ok(matches!(value, Value::Enum { .. })
                            && value.nominal_declaration_id() == Some(expected));
                    }
                    // §3 (v5.4): a user nominal-record type pattern matches NOMINALLY
                    // — the value is a record of this exact record id (a same-shaped
                    // record of a different id, or a structural record, does NOT).
                    if let Some(definition) = self.record_definition_in(src, n)
                        && definition.decl.type_params.is_empty()
                    {
                        let expected = self.nominal_definition_identity(
                            definition.method_identity.as_ref(),
                            &definition.runtime_id,
                        );
                        return Ok(matches!(value, Value::NominalRecord { .. })
                            && value.nominal_declaration_id() == Some(expected));
                    }
                    // §3 (v5.4): a user newtype type pattern matches NOMINALLY — the
                    // value is a newtype of this exact id (NOT its base, NOT another
                    // newtype over the same base) — so `is UserId` rejects a raw int.
                    if let Some(definition) = self.newtype_definition_in(src, n)
                        && definition.decl.type_params.is_empty()
                    {
                        let expected = self.nominal_definition_identity(
                            definition.method_identity.as_ref(),
                            &definition.runtime_id,
                        );
                        return Ok(matches!(value, Value::Newtype { .. })
                            && value.nominal_declaration_id() == Some(expected));
                    }
                    if let Some((bound, bound_src, bound_env)) = env.0.get(n).cloned() {
                        return self.type_matches(&bound, &bound_src, value, &bound_env, state);
                    }
                }
                match (n, args.as_slice()) {
                    ("Option", [inner]) => match value {
                        Value::None => true,
                        Value::Some(v) => self.type_matches(inner, src, v, env, state)?,
                        _ => false,
                    },
                    ("Result", [ok, err]) => match value {
                        Value::Ok(v) => self.type_matches(ok, src, v, env, state)?,
                        Value::Err(v) => self.type_matches(err, src, v, env, state)?,
                        _ => false,
                    },
                    ("Array", [elem]) => match value {
                        Value::Array(items) => {
                            for item in items.borrow().iter() {
                                if !self.type_matches(elem, src, item, env, state)? {
                                    return Ok(false);
                                }
                            }
                            true
                        }
                        _ => false,
                    },
                    ("Set", [elem]) => match value {
                        Value::Set(items) => {
                            for item in items.borrow().items() {
                                if !self.type_matches(elem, src, &item, env, state)? {
                                    return Ok(false);
                                }
                            }
                            true
                        }
                        _ => false,
                    },
                    ("Map", [k, v]) => match value {
                        Value::Map(entries) => {
                            for (key, val) in entries.borrow().pairs() {
                                if !self.type_matches(k, src, &key, env, state)?
                                    || !self.type_matches(v, src, &val, env, state)?
                                {
                                    return Ok(false);
                                }
                            }
                            true
                        }
                        _ => false,
                    },
                    _ if self.enum_definition_in(src, n).is_some() => {
                        self.nominal_enum_type_matches(n, args, src, value, env, state)?
                    }
                    _ if self.record_definition_in(src, n).is_some() => {
                        self.nominal_record_type_matches(n, args, src, value, env, state)?
                    }
                    _ if self.newtype_definition_in(src, n).is_some() => {
                        self.nominal_newtype_type_matches(n, args, src, value, env, state)?
                    }
                    _ => {
                        // A lexically scoped or module-level alias.
                        let Some((params, body)) = self.lookup_alias_in(src, n) else {
                            // §3/§7 PURE-ONLY type-param erasure (A20): a bare type
                            // pattern over an in-scope generic type-param that is
                            // NEITHER a builtin (the int/float/string/bool arm above
                            // did not match) NOR a visible alias (this lookup just
                            // missed) carries no runtime type — erase (always-match),
                            // aligning the runtime with the checker. A param that
                            // SHADOWS a same-named builtin/alias is intentionally NOT
                            // erased (those resolve above); that shadow ordering is
                            // owner-gated unresolved semantics.
                            if args.is_empty()
                                && self
                                    .type_params
                                    .iter()
                                    .any(|p| text_in(&self.src, p.span) == n)
                            {
                                return Ok(true);
                            }
                            return Err(fault(
                                codes::GUARD_UNIMPLEMENTED,
                                format!("`{n}` is not a runtime type"),
                                span,
                            ));
                        };
                        self.expand_alias_match(
                            AliasMatchDefinition {
                                name: n,
                                body_src: src,
                                params: &params,
                                body: &body,
                            },
                            src,
                            args,
                            value,
                            env,
                            state,
                        )?
                    }
                }
            }
            TypeKind::Literal => {
                let text = text_in(src, ty.span);
                match value {
                    Value::Null => text == "null",
                    Value::Bool(b) => text == if *b { "true" } else { "false" },
                    Value::Int(v) => text.parse::<i64>() == Ok(*v),
                    // Finite float literal patterns use IEEE equality.
                    Value::Float(v) => text.parse::<f64>() == Ok(*v),
                    Value::Str(s) => text.len() >= 2 && &text[1..text.len() - 1] == s.as_ref(),
                    _ => false,
                }
            }
            TypeKind::Qualified { ns, name, args } => {
                // §17: the namespace binding leads to the exporting
                // module's alias table; the body's spans index THAT
                // module's source.
                let ns_text = text_in(src, ns.span);
                let n = text_in(src, name.span);
                let Some(Value::Namespace(identity)) = lookup(&self.env, ns_text) else {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!("`{ns_text}` is not a namespace binding (§17)"),
                        span,
                    ));
                };
                let Some(module_scope) = self.module_scopes.get(identity.as_ref()) else {
                    return Err(fault(
                        codes::GUARD_UNIMPLEMENTED,
                        format!("module `{identity}` is not part of this run"),
                        span,
                    ));
                };
                let fsrc = &module_scope.types.src;
                let nominals = &module_scope.types.nominals;
                let exported = module_scope.runtime.exports.contains(n);
                if args.is_empty() && exported {
                    if let Some(definition) = nominals.enum_defs.get(n)
                        && definition.decl.type_params.is_empty()
                    {
                        let expected = self.nominal_definition_identity(
                            definition.method_identity.as_ref(),
                            &definition.runtime_id,
                        );
                        return Ok(matches!(value, Value::Enum { .. })
                            && value.nominal_declaration_id() == Some(expected));
                    }
                    if let Some(definition) = nominals.record_defs.get(n)
                        && definition.decl.type_params.is_empty()
                    {
                        let expected = self.nominal_definition_identity(
                            definition.method_identity.as_ref(),
                            &definition.runtime_id,
                        );
                        return Ok(matches!(value, Value::NominalRecord { .. })
                            && value.nominal_declaration_id() == Some(expected));
                    }
                    if let Some(definition) = nominals.newtype_defs.get(n)
                        && definition.decl.type_params.is_empty()
                    {
                        let expected = self.nominal_definition_identity(
                            definition.method_identity.as_ref(),
                            &definition.runtime_id,
                        );
                        return Ok(matches!(value, Value::Newtype { .. })
                            && value.nominal_declaration_id() == Some(expected));
                    }
                }
                if exported {
                    if let Some(definition) = nominals.enum_defs.get(n) {
                        return self.nominal_enum_definition_type_matches(
                            definition, args, src, value, env, state,
                        );
                    }
                    if let Some(definition) = nominals.record_defs.get(n) {
                        return self.nominal_record_definition_type_matches(
                            definition, args, src, value, env, state,
                        );
                    }
                    if let Some(definition) = nominals.newtype_defs.get(n) {
                        return self.nominal_newtype_definition_type_matches(
                            definition, args, src, value, env, state,
                        );
                    }
                }
                let found = module_scope.types.aliases.get(n);
                // §17: only EXPORTED aliases cross the boundary.
                let Some((params, body, true)) = found else {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!("`{n}` is not an exported type of `{ns_text}` (§17)"),
                        span,
                    ));
                };
                self.expand_alias_match(
                    AliasMatchDefinition {
                        name: n,
                        body_src: fsrc,
                        params,
                        body,
                    },
                    src,
                    args,
                    value,
                    env,
                    state,
                )?
            }
        })
    }

    /// Expands an alias (local or qualified) against a value: the
    /// body matches under the DEFINING source; the arguments keep
    /// the USE-site source and bindings, closure-style.
    pub(super) fn expand_alias_match(
        &self,
        definition: AliasMatchDefinition<'_>,
        arg_src: &Rc<str>,
        args: &[Rc<Type>],
        value: &Value,
        env: &TypeBindings,
        state: &mut TypeMatchState,
    ) -> Result<bool, RtError> {
        let AliasMatchDefinition {
            name,
            body_src,
            params,
            body,
        } = definition;
        let key = (body_src.as_ptr() as usize, name.to_string());
        if state.expanding.contains(&key) {
            return Err(fault(
                codes::GUARD_TYPE,
                format!("type alias `{name}` is recursive (§3)"),
                state.span,
            ));
        }
        if params.len() != args.len() {
            return Err(fault(
                codes::GUARD_TYPE,
                format!(
                    "`{name}` takes {} type argument(s), found {}",
                    params.len(),
                    args.len()
                ),
                state.span,
            ));
        }
        let mut inner: BTreeMap<String, (Rc<Type>, Rc<str>, TypeBindings)> = BTreeMap::new();
        for (param, arg) in params.iter().zip(args.iter()) {
            inner.insert(
                text_in(body_src, param.span).to_string(),
                (arg.clone(), arg_src.clone(), env.clone()),
            );
        }
        let inner = TypeBindings(Rc::new(inner));
        state.expanding.push(key);
        let matched = self.type_matches(body, body_src, value, &inner, state);
        state.expanding.pop();
        matched
    }

    /// Resolves a type-alias name lexically against a given module
    /// source: scope chain first (the chain belongs to the running
    /// module), then the module table for that source.
    pub(super) fn lookup_alias_in(
        &self,
        src: &Rc<str>,
        name: &str,
    ) -> Option<(Rc<[Ident]>, Rc<Type>)> {
        let mut env = Some(self.env.clone());
        while let Some(scope) = env {
            let borrowed = scope.borrow();
            if let Some(found) = borrowed.aliases.get(name) {
                return Some(found.clone());
            }
            env = borrowed.parent.clone();
        }
        self.source_module_index
            .get(&(src.as_ptr() as usize))
            .and_then(|module| self.module_scopes.get(module))
            .and_then(|scope| scope.types.aliases.get(name))
            .map(|(params, body, _)| (params.clone(), body.clone()))
    }
}
