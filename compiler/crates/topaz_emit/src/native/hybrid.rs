use super::*;

pub(super) fn top_level_function(statement: &Stmt) -> Option<&FunctionDecl> {
    match &statement.kind {
        StmtKind::Function(declaration) => Some(declaration),
        StmtKind::Export(inner) => match &inner.kind {
            StmtKind::Function(declaration) => Some(declaration),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn stable_decline(error: &EmitError) -> (&'static str, Option<&'static str>) {
    match error.kind {
        crate::EmitErrorKind::NativeDeclined("an extern unit") => ("extern_unit", None),
        crate::EmitErrorKind::NativeDeclined(detail) => ("unsupported_native_shape", Some(detail)),
        crate::EmitErrorKind::Unsupported(detail) => ("unsupported_native_shape", Some(detail)),
        crate::EmitErrorKind::MalformedLiteral(detail) => ("malformed_literal", Some(detail)),
        crate::EmitErrorKind::NoEntry => ("missing_entry", None),
    }
}

pub(super) struct HybridCandidate<'a> {
    pub(super) module: &'a topaz_hir::LoweredModule,
    pub(super) declaration: &'a FunctionDecl,
    pub(super) statement_span: Span,
}

pub(super) type HybridCandidates<'a> = BTreeMap<&'a str, Vec<HybridCandidate<'a>>>;

#[derive(Clone, Copy)]
pub(super) enum HybridDisposition {
    Selected,
    Declined {
        reason: &'static str,
        detail: Option<&'static str>,
    },
}

#[derive(Default)]
pub(super) struct HybridDecisionIndex {
    pub(super) by_module: BTreeMap<String, BTreeMap<(u32, u32), HybridDisposition>>,
}

impl HybridDecisionIndex {
    pub(super) fn get(
        &self,
        module: &str,
        span_lo: u32,
        span_hi: u32,
    ) -> Option<HybridDisposition> {
        self.by_module
            .get(module)?
            .get(&(span_lo, span_hi))
            .copied()
    }

    pub(super) fn insert(
        &mut self,
        candidate: &HybridCandidate<'_>,
        disposition: HybridDisposition,
    ) {
        let module = candidate.module.identity.as_str();
        let span = (candidate.statement_span.lo, candidate.statement_span.hi);
        if let Some(functions) = self.by_module.get_mut(module) {
            functions.insert(span, disposition);
            return;
        }
        let mut functions = BTreeMap::new();
        functions.insert(span, disposition);
        self.by_module.insert(module.to_string(), functions);
    }
}

#[derive(Default)]
pub(super) struct HybridBuild {
    pub(super) plan: crate::HybridPlan,
    pub(super) decisions: HybridDecisionIndex,
}

pub(super) fn build_hybrid_plan<'a>(input: &'a NativeInput<'a>) -> HybridBuild {
    let candidates = hybrid_candidates_by_module(input.unit);
    let mut build = HybridBuild::default();
    let Some(typed_hir) = input.unit.typed.as_ref() else {
        decline_all_hybrid(
            &candidates,
            &mut build,
            "missing_typed_hir",
            Some("the checked typed HIR is unavailable"),
        );
        return build;
    };
    if typed_hir.contains_concurrent {
        decline_all_hybrid(
            &candidates,
            &mut build,
            "concurrent_unit",
            Some("the unit contains concurrent execution"),
        );
        return build;
    }
    if input.unit.modules.iter().any(|module| module.is_extern) {
        decline_all_hybrid(
            &candidates,
            &mut build,
            "extern_unit",
            Some("the unit contains an extern module"),
        );
        return build;
    }

    let hir_locals = TypedLocalIndex::from_typed_hir(typed_hir);

    for module in &input.unit.modules {
        let Some(module_candidates) = candidates.get(module.identity.as_str()) else {
            continue;
        };
        let src = &module.text;
        let (byte_record_params, byte_projections) =
            byte_facts_for_module(typed_hir, &module.identity);
        let mut active = HashMap::new();
        for candidate in module_candidates {
            match hybrid_native_fn_sig(candidate.declaration, src, &module.identity, typed_hir) {
                Ok(signature) => {
                    active.insert(
                        text(src, candidate.declaration.name.span).to_string(),
                        Rc::new(signature),
                    );
                }
                Err((reason, detail)) => {
                    build
                        .decisions
                        .insert(candidate, HybridDisposition::Declined { reason, detail });
                }
            }
        }

        let stable_functions = loop {
            let mut removed = Vec::new();
            let mut lowered = Vec::new();
            for candidate in module_candidates {
                let name = text(src, candidate.declaration.name.span);
                if !active.contains_key(name) {
                    continue;
                }
                let mut ctx = hybrid_ctx(
                    src,
                    &hir_locals,
                    &byte_record_params,
                    &byte_projections,
                    &active,
                );
                match emit_fn(candidate.declaration, &mut ctx) {
                    Ok(()) => lowered.push((candidate, ctx.fn_defs)),
                    Err(error) => {
                        let (_, detail) = stable_decline(&error);
                        build.decisions.insert(
                            candidate,
                            HybridDisposition::Declined {
                                reason: "unsupported_native_body",
                                detail,
                            },
                        );
                        removed.push(name.to_string());
                    }
                }
            }
            if removed.is_empty() {
                break lowered;
            }
            for name in removed {
                active.remove(&name);
            }
        };

        for (candidate, helper) in stable_functions {
            let name = text(src, candidate.declaration.name.span);
            let Some(signature) = active.get(name) else {
                continue;
            };
            build.plan.helpers.push_str(&helper);
            build.plan.insert_closure(
                module.identity.clone(),
                name.to_string(),
                candidate.declaration.name.span,
                hybrid_closure(candidate.declaration, signature, src),
            );
            build
                .decisions
                .insert(candidate, HybridDisposition::Selected);
        }
    }
    build
}

pub(super) fn hybrid_candidates_by_module(unit: &LoweredUnit) -> HybridCandidates<'_> {
    let mut by_module = BTreeMap::new();
    for module in &unit.modules {
        let mut candidates = module
            .program
            .items
            .iter()
            .filter_map(|statement| {
                top_level_function(statement).map(|declaration| HybridCandidate {
                    module,
                    declaration,
                    statement_span: statement.span,
                })
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            continue;
        }
        candidates
            .sort_by_key(|candidate| (candidate.statement_span.lo, candidate.statement_span.hi));
        by_module.insert(module.identity.as_str(), candidates);
    }
    by_module
}

pub(super) fn decline_all_hybrid(
    candidates: &HybridCandidates<'_>,
    build: &mut HybridBuild,
    reason: &'static str,
    detail: Option<&'static str>,
) {
    for candidate in candidates.values().flatten() {
        build
            .decisions
            .insert(candidate, HybridDisposition::Declined { reason, detail });
    }
}

pub(super) fn hybrid_native_fn_sig<'a>(
    declaration: &'a FunctionDecl,
    src: &'a LoweredText,
    module: &str,
    typed_hir: &TypedUnit,
) -> Result<NativeFn<'a>, (&'static str, Option<&'static str>)> {
    if !declaration.type_params.is_empty() {
        return Err(("generic_function", None));
    }
    if declaration.params.iter().any(|param| param.variadic) {
        return Err(("variadic_parameter", None));
    }
    if declaration
        .params
        .iter()
        .any(|param| param.default.is_some())
    {
        return Err(("default_parameter", None));
    }
    let mut params = Vec::with_capacity(declaration.params.len());
    let mut names = Vec::with_capacity(declaration.params.len());
    for param in &declaration.params {
        let name = text(src, param.name.span);
        let repr = scalar_of_type(&param.ty, src)
            .map(NativeParam::Scalar)
            .or_else(|| byte_handle_type(&param.ty, src).map(NativeParam::ByteHandle))
            .or_else(|| {
                typed_hir
                    .byte_record_params
                    .iter()
                    .find(|fact| {
                        fact.module == module
                            && fact.function_span == declaration.name.span
                            && fact.name == name
                            && fact.span == param.name.span
                    })
                    .map(|fact| NativeParam::ByteRecord(fact.declaration_identity.clone()))
            })
            .ok_or((
                "non_scalar_signature",
                Some("unsupported byte/record parameter"),
            ))?;
        names.push(name);
        params.push(repr);
    }
    let ret = declaration
        .return_type
        .as_ref()
        .and_then(|ty| scalar_of_type(ty, src))
        .ok_or(("non_scalar_signature", None))?;
    let mut signature = NativeFn {
        names: names.into(),
        defaults: vec![None; params.len()].into(),
        params,
        ret,
        rust_name: String::new(),
    };
    let is_bounded_scalar = |ty: NativeTy| {
        matches!(
            ty,
            NativeTy::I64 | NativeTy::F64 | NativeTy::Bool | NativeTy::Unit
        )
    };
    if !is_bounded_scalar(signature.ret)
        || signature.params.iter().any(|param| {
            !matches!(
                param,
                NativeParam::Scalar(ty) if is_bounded_scalar(*ty)
            ) && !matches!(
                param,
                NativeParam::ByteHandle(MonoTy::BytesHandle | MonoTy::ByteBufferHandle)
                    | NativeParam::ByteRecord(_)
            )
        })
    {
        return Err(("non_scalar_signature", None));
    }
    signature.rust_name = format!(
        "__topaz_hybrid_{}_{}_{}_{}_{}",
        mangle(module),
        mangle(text(src, declaration.name.span)),
        declaration.name.span.file.0,
        declaration.name.span.lo,
        declaration.name.span.hi
    );
    Ok(signature)
}

pub(super) fn hybrid_ctx<'a>(
    src: &'a LoweredText,
    hir_locals: &'a TypedLocalIndex,
    byte_record_params: &'a [ByteRecordParam],
    byte_projections: &'a [ByteProjectionProof],
    functions: &'a NativeFunctionIndex<'a>,
) -> Ctx<'a> {
    Ctx {
        src,
        hir_locals,
        current_function: None,
        byte_record_params,
        byte_projections,
        fns: Cow::Borrowed(functions),
        generic_fns: HashMap::new(),
        generic_specs: GenericFunctionIndex::default(),
        fn_defs: String::new(),
        elide_checkpoints: true,
        math_namespaces: Vec::new(),
        hybrid: true,
    }
}

pub(super) fn byte_facts_for_module(
    typed_hir: &TypedUnit,
    module: &str,
) -> (Vec<ByteRecordParam>, Vec<ByteProjectionProof>) {
    let params = typed_hir
        .byte_record_params
        .iter()
        .filter(|fact| fact.module == module)
        .map(|fact| {
            (
                fact.function_span,
                fact.name.clone(),
                fact.span,
                fact.declaration_identity.clone(),
            )
        })
        .collect();
    let projections = typed_hir
        .byte_projections
        .iter()
        .filter(|fact| fact.module == module)
        .map(|fact| ByteProjectionProof {
            function_span: fact.function_span,
            receiver_name: fact.receiver_name.clone(),
            receiver_span: fact.receiver_span,
            field: fact.field.clone(),
            expression_span: fact.expression_span,
            local_name: fact.local_name.clone(),
            local_span: fact.local_span,
            mono: fact.mono,
        })
        .collect();
    (params, projections)
}

pub(super) fn hybrid_closure(
    declaration: &FunctionDecl,
    signature: &NativeFn<'_>,
    src: &LoweredText,
) -> String {
    let params = declaration
        .params
        .iter()
        .map(|param| text(src, param.name.span))
        .collect::<Vec<_>>();
    let mut binds = String::new();
    let mut args = Vec::new();
    for ((param, name), repr) in declaration
        .params
        .iter()
        .zip(params.iter())
        .zip(signature.params.iter())
    {
        let local = mangle(name);
        binds.push_str(&format!(
            "let {local} = __args.next().expect(\"arity checked at the call site\"); "
        ));
        let value = format!("__native_{local}");
        let pattern = match repr {
            NativeParam::Scalar(NativeTy::I64) => "Value::Int(__value) => __value".to_string(),
            NativeParam::Scalar(NativeTy::F64) => "Value::Float(__value) => __value".to_string(),
            NativeParam::Scalar(NativeTy::Bool) => "Value::Bool(__value) => __value".to_string(),
            NativeParam::Scalar(NativeTy::Unit) => "Value::Unit => ()".to_string(),
            NativeParam::Scalar(NativeTy::Str) => {
                unreachable!("string parameters are not hybrid")
            }
            NativeParam::ByteHandle(MonoTy::BytesHandle) => {
                "__value @ Value::Bytes(_) => __value".to_string()
            }
            NativeParam::ByteHandle(MonoTy::ByteBufferHandle) => {
                "__value @ Value::ByteBuffer(_) => __value".to_string()
            }
            NativeParam::ByteHandle(_) => {
                unreachable!("only exact byte handles enter hybrid signatures")
            }
            NativeParam::ByteRecord(declaration_identity) => {
                format!(
                    "__value @ Value::NominalRecord {{ .. }} if __value.is_nominal_record_declaration({declaration_identity:?}) => __value"
                )
            }
            NativeParam::ScalarArray(_) => {
                unreachable!("scalar arrays are not hybrid parameters")
            }
        };
        binds.push_str(&format!(
            "let {value} = match {local} {{ {pattern}, _ => return Err(fault(codes::GUARD_TYPE, {:?}, {})) }}; ",
            "argument does not match parameter type (§6)",
            emit_span(param.ty.span),
        ));
        args.push(value);
    }
    let param_list = params
        .iter()
        .map(|name| format!("{name:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let call_args = if args.is_empty() {
        String::new()
    } else {
        format!(", {}", args.join(", "))
    };
    let boxed_result = signature.ret.box_expr("__native_result");
    format!(
        "Value::Closure(Rc::new(EmittedClosure {{ call: |cx: RtCx, args: Vec<Value>| -> CallFuture {{ Box::pin(async move {{ let mut __args = args.into_iter(); {binds}let __native_result = Box::pin({helper}(cx.clone(), {span}, true{call_args})).await?; Ok({boxed_result}) }}) }}, params: &[{param_list}], defaults: Vec::new(), variadic: false }}))",
        helper = signature.rust_name,
        span = emit_span(declaration.name.span),
    )
}
