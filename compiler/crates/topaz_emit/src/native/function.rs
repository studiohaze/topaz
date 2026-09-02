use super::*;

// ----------------------------------------------------------------------------
// Top-level program → `entry` body + native function definitions.
// ----------------------------------------------------------------------------

/// Lower the entry program: collect top-level scalar function SIGNATURES first
/// (so a body may call a function declared later — the same forward visibility
/// the interpreter gives top-level functions), then lower each function body and
/// the top-level statement sequence. The entry's value is its tail expression,
/// boxed at the boundary (`Unit` when there is none).
pub(super) fn emit_entry<'a>(program: &'a Program, ctx: &mut Ctx<'a>) -> Result<String, EmitError> {
    for stmt in &program.items {
        if let StmtKind::Import(imp) = &stmt.kind
            && let Some(alias) = std_math_namespace_alias(imp, ctx.src)
            && !ctx.math_namespaces.iter().any(|known| known == &alias)
        {
            ctx.math_namespaces.push(alias);
        }
    }

    // Pass 1: collect every top-level `function` signature as a `NativeFn`.
    // A non-scalar param/return, a type parameter, a default, a variadic, or a
    // missing annotation refuses the whole program (native handles only fully
    // concrete-scalar same-module functions this slice).
    let mut generic_order = Vec::new();
    for stmt in &program.items {
        if let StmtKind::Function(decl) = &stmt.kind {
            let name = text(ctx.src, decl.name.span);
            if ctx.fns.contains_key(name) || ctx.generic_fns.contains_key(name) {
                // A duplicate top-level function ??refuse (the boxed backend has
                // the redeclaration diagnostic; native just declines to fallback).
                return Err(decline("a redeclared function").at(decl.name.span));
            }
            if !decl.type_params.is_empty() {
                let specs =
                    native_generic_specs(decl, ctx.src).map_err(|e| e.at(decl.name.span))?;
                ctx.generic_fns.insert(name.to_string(), decl);
                for (type_args, sig) in specs {
                    ctx.generic_specs.insert(name, type_args, sig);
                }
                generic_order.push(decl);
                continue;
            }
            let nf = native_fn_sig(decl, ctx.src).map_err(|e| e.at(decl.name.span))?;
            if ctx
                .fns
                .to_mut()
                .insert(name.to_string(), Rc::new(nf))
                .is_some()
            {
                // A duplicate top-level function — refuse (the boxed backend has
                // the redeclaration diagnostic; native just declines to fallback).
                return Err(decline("a redeclared function").at(decl.name.span));
            }
        }
    }

    // Pass 2: lower each top-level function body.
    for stmt in &program.items {
        if let StmtKind::Function(decl) = &stmt.kind
            && decl.type_params.is_empty()
        {
            emit_fn(decl, ctx)?;
        }
    }
    for decl in generic_order {
        emit_generic_fn_specs(decl, ctx)?;
    }

    // Pass 3: lower the top-level statement sequence (skipping the function
    // declarations — already emitted as Rust `fn`s above).
    let (lines, tail) = split_top(&program.items);
    let mut scope: Vec<NativeLocal> = Vec::new();
    let mut out = String::new();
    for stmt in &lines {
        emit_stmt(stmt, ctx, &mut scope, &mut out, false)?;
    }
    // The program value: the tail expression boxed, or `Value::Unit`.
    let result = match tail {
        Some(tail) => emit_entry_result(tail, ctx, &scope)?,
        None => "Value::Unit".to_string(),
    };
    out.push_str(&format!("    Ok({result})\n"));
    Ok(out)
}

/// Lower the entry tail to a runtime `Value`.
///
/// The native island still keeps locals/functions scalar-only; this is only the
/// final program boundary. First try the ordinary native scalar path, then allow
/// small boxed values whose construction is local and shared-leaf based.
pub(super) fn emit_entry_result(
    expr: &Expr,
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
) -> Result<String, EmitError> {
    match emit_expr(expr, ctx, scope) {
        Ok(low) => Ok(low.ty.box_expr(&low.rs)),
        Err(e) if e.is_native_decline() => emit_boxed_boundary_expr(expr, ctx, scope),
        Err(e) => Err(e),
    }
}

/// Build a top-level `function`'s native signature, or refuse. Every parameter
/// must be either a CONCRETE annotated scalar or a read-only `Array<scalar>`
/// boundary; the return type must be a CONCRETE annotated scalar. A type
/// parameter, a variadic, or an inferred (un-annotated) param/return refuses.
/// A defaulted parameter is still a normal parameter when the call supplies it;
/// omitted-default calls are native only when the default is the same pure
/// literal/unary shape the boxed emitter can pre-evaluate without observing an
/// environment.
pub(super) fn native_fn_sig<'a>(
    decl: &'a FunctionDecl,
    src: &'a LoweredText,
) -> Result<NativeFn<'a>, EmitError> {
    if !decl.type_params.is_empty() {
        return Err(decline("a generic function"));
    }
    let rust_name = mangle(text(src, decl.name.span));
    let mut names = Vec::with_capacity(decl.params.len());
    let mut defaults = Vec::with_capacity(decl.params.len());
    let mut params = Vec::with_capacity(decl.params.len());
    for param in &decl.params {
        if param.variadic {
            return Err(decline("a variadic parameter"));
        }
        let param_repr = scalar_of_type(&param.ty, src)
            .map(NativeParam::Scalar)
            .or_else(|| scalar_array_type(&param.ty, src).map(NativeParam::ScalarArray))
            .or_else(|| byte_handle_type(&param.ty, src).map(NativeParam::ByteHandle))
            .ok_or_else(|| decline("a non-native parameter"))?;
        names.push(text(src, param.name.span));
        defaults.push(param.default.as_ref());
        params.push(param_repr);
    }
    let ret = match &decl.return_type {
        Some(ty) => scalar_of_type(ty, src).ok_or_else(|| decline("a non-scalar return type"))?,
        // No annotated return type: native needs a concrete scalar return to
        // pick the Rust signature; refuse rather than infer (boxed handles it).
        None => return Err(decline("a function without a return-type annotation")),
    };
    Ok(NativeFn {
        names: names.into(),
        defaults: defaults.into(),
        params,
        ret,
        rust_name,
    })
}

pub(super) fn byte_handle_type(
    ty: &topaz_hir::emission::Type,
    src: &LoweredText,
) -> Option<MonoTy> {
    let TypeKind::Named { name, args } = &ty.kind else {
        return None;
    };
    if !args.is_empty() {
        return None;
    }
    match text(src, name.span) {
        "Bytes" => Some(MonoTy::BytesHandle),
        "ByteBuffer" => Some(MonoTy::ByteBufferHandle),
        _ => None,
    }
}

/// Build the pre-generated scalar specializations for a narrow generic function
/// template. This first monomorphization slice is intentionally conservative:
/// one type parameter, no bounds/defaults/variadics, a declared scalar-or-`T`
/// return type, and no statement body. The checker has already proven the
/// generic body valid; the specialization only picks concrete scalar storage.
pub(super) fn native_generic_specs<'a>(
    decl: &'a FunctionDecl,
    src: &'a LoweredText,
) -> Result<Vec<(Vec<NativeTy>, NativeFn<'a>)>, EmitError> {
    if decl.type_params.len() != 1 {
        return Err(decline("a multi-parameter generic function"));
    }
    if decl
        .type_param_bounds
        .iter()
        .any(|bounds| !bounds.is_empty())
    {
        return Err(decline("a bounded generic function"));
    }
    if !decl.body.stmts.is_empty() {
        return Err(decline("a generic function with statements"));
    }
    let params = generic_type_param_names(decl, src);
    let name = text(src, decl.name.span);
    let names: Rc<[&str]> = decl
        .params
        .iter()
        .map(|param| text(src, param.name.span))
        .collect::<Vec<_>>()
        .into();
    let defaults: Rc<[Option<&Expr>]> = vec![None; decl.params.len()].into();
    let mut specs = Vec::new();
    for ty in GENERIC_NATIVE_TYPES {
        let type_args = vec![ty];
        let mut native_params = Vec::with_capacity(decl.params.len());
        let mut valid = true;
        for param in &decl.params {
            if param.default.is_some() {
                return Err(decline("a default parameter"));
            }
            if param.variadic {
                return Err(decline("a variadic parameter"));
            }
            let Some(repr) = generic_param_repr(&param.ty, &params, &type_args, src) else {
                valid = false;
                break;
            };
            native_params.push(repr);
        }
        if !valid {
            continue;
        }
        let ret = match &decl.return_type {
            Some(ret) => match generic_return_ty(ret, &params, &type_args, src) {
                Some(ret) => ret,
                None => continue,
            },
            None => return Err(decline("a function without a return-type annotation")),
        };
        let rust_name = native_generic_rust_name(name, &type_args);
        specs.push((
            type_args,
            NativeFn {
                names: Rc::clone(&names),
                defaults: Rc::clone(&defaults),
                params: native_params,
                ret,
                rust_name,
            },
        ));
    }
    if specs.is_empty() {
        return Err(decline("a non-native generic function"));
    }
    Ok(specs)
}

pub(super) fn emit_generic_fn_specs(
    decl: &FunctionDecl,
    ctx: &mut Ctx<'_>,
) -> Result<(), EmitError> {
    let name = text(ctx.src, decl.name.span);
    let params = generic_type_param_names(decl, ctx.src);
    for ty in GENERIC_NATIVE_TYPES {
        let type_args = [ty];
        let Some(sig) = ctx.generic_specs.get(name, &type_args).cloned() else {
            continue;
        };
        emit_generic_fn_spec(decl, &params, &sig, ctx)?;
    }
    Ok(())
}

pub(super) fn emit_generic_fn_spec(
    decl: &FunctionDecl,
    type_params: &[&str],
    sig: &NativeFn<'_>,
    ctx: &mut Ctx<'_>,
) -> Result<(), EmitError> {
    let mut scope: Vec<NativeLocal> = Vec::new();
    for (param, repr) in decl.params.iter().zip(sig.params.iter()) {
        let pname = text(ctx.src, param.name.span).to_string();
        let expected_hir = if generic_type_depends_on_param(&param.ty, type_params, ctx.src) {
            MonoTy::Boxed
        } else {
            repr.mono()
        };
        ctx.confirm_local(&pname, param.name.span, expected_hir)
            .map_err(|e| e.at(param.name.span))?;
        scope.push(NativeLocal {
            name: pname,
            kind: repr.local_kind(),
            mutable: false,
        });
    }

    let tail = decl
        .body
        .tail
        .as_deref()
        .ok_or_else(|| decline("a generic function without a tail value").at(decl.name.span))?;
    let ret = emit_expr(tail, ctx, &scope)?;
    if ret.ty != sig.ret {
        return Err(
            decline("a generic function tail whose type is not the return type").at(tail.span),
        );
    }

    let mut params_rs = String::from("cx: RtCx, __call_span: Span");
    for (p, repr) in decl.params.iter().zip(sig.params.iter()) {
        params_rs.push_str(&format!(
            ", {}: {}",
            mangle(text(ctx.src, p.name.span)),
            repr.rust()
        ));
    }
    ctx.fn_defs.push_str(&format!(
        "/// Native scalar generic specialization (v5.4 monomorphized emit).\n\
         async fn {fname}({params_rs}) -> Result<{ret_ty}, RtError> {{\n\
         \x20   let _guard = __native_enter_call(&cx, __call_span)?;\n\
         \x20   let _ = &cx;\n\
         \x20   Ok({ret})\n\
         }}\n\n",
        fname = sig.rust_name,
        ret_ty = sig.ret.rust(),
        ret = ret.rs,
    ));
    Ok(())
}

pub(super) fn generic_type_param_names<'a>(
    decl: &FunctionDecl,
    src: &'a LoweredText,
) -> Vec<&'a str> {
    decl.type_params
        .iter()
        .map(|param| text(src, param.span))
        .collect()
}

pub(super) fn native_generic_rust_name(name: &str, type_args: &[NativeTy]) -> String {
    let mut rust_name = format!("{}__tpz_mono_", mangle(name));
    for (index, ty) in type_args.iter().enumerate() {
        if index > 0 {
            rust_name.push('_');
        }
        rust_name.push_str(ty.tag());
    }
    rust_name
}

pub(super) fn generic_param_repr(
    ty: &topaz_hir::emission::Type,
    type_params: &[&str],
    type_args: &[NativeTy],
    src: &LoweredText,
) -> Option<NativeParam> {
    if let Some(idx) = generic_type_param_index(ty, type_params, src) {
        return Some(NativeParam::Scalar(type_args[idx]));
    }
    if let Some(elem) = generic_array_elem(ty, type_params, type_args, src) {
        return Some(NativeParam::ScalarArray(elem));
    }
    scalar_of_type(ty, src)
        .map(NativeParam::Scalar)
        .or_else(|| scalar_array_type(ty, src).map(NativeParam::ScalarArray))
}

pub(super) fn generic_return_ty(
    ty: &topaz_hir::emission::Type,
    type_params: &[&str],
    type_args: &[NativeTy],
    src: &LoweredText,
) -> Option<NativeTy> {
    if let Some(idx) = generic_type_param_index(ty, type_params, src) {
        return Some(type_args[idx]);
    }
    scalar_of_type(ty, src)
}

pub(super) fn generic_type_param_index(
    ty: &topaz_hir::emission::Type,
    type_params: &[&str],
    src: &LoweredText,
) -> Option<usize> {
    let TypeKind::Named { name, args } = &ty.kind else {
        return None;
    };
    if !args.is_empty() {
        return None;
    }
    let n = text(src, name.span);
    type_params.iter().position(|param| *param == n)
}

pub(super) fn generic_array_elem(
    ty: &topaz_hir::emission::Type,
    type_params: &[&str],
    type_args: &[NativeTy],
    src: &LoweredText,
) -> Option<NativeTy> {
    let idx = generic_array_param_index(ty, type_params, src)?;
    match type_args[idx] {
        elem @ (NativeTy::I64 | NativeTy::F64 | NativeTy::Bool | NativeTy::Str) => Some(elem),
        NativeTy::Unit => None,
    }
}

pub(super) fn generic_array_param_index(
    ty: &topaz_hir::emission::Type,
    type_params: &[&str],
    src: &LoweredText,
) -> Option<usize> {
    let TypeKind::Named { name, args } = &ty.kind else {
        return None;
    };
    if text(src, name.span) != "Array" || args.len() != 1 {
        return None;
    }
    generic_type_param_index(&args[0], type_params, src)
}

pub(super) fn generic_type_depends_on_param(
    ty: &topaz_hir::emission::Type,
    type_params: &[&str],
    src: &LoweredText,
) -> bool {
    generic_type_param_index(ty, type_params, src).is_some()
        || generic_array_param_index(ty, type_params, src).is_some()
}

/// Lower one top-level scalar function to a native `async fn` over bare scalars,
/// appending it to `ctx.fn_defs`. The body is the function's `Block`; its tail
/// value is the (already-scalar) return value.
///
/// ASYNC-NATIVE-FNS slice: the fn ABI is `async fn f(cx: RtCx, __call_span: Span,
/// args...) -> Result<scalar, RtError>` whose FIRST statement threads the SHARED
/// recursion guard (`__native_enter_call(&cx, __call_span)?`), so a deep call
/// chain faults `GUARD_RECURSION` at the SAME depth/code/message/span as
/// interp+boxed. The body may now host native LOOPS (the checkpoint is elided
/// when the unit has no `concurrent`, kept otherwise — both valid in an async fn)
/// and native CALLS (`Box::pin(g(cx.clone(), <span>, args)).await?`). Boxed
/// futures provide the indirection recursive async functions require, so
/// self/mutual recursion and long acyclic chains stay in the native parity set.
pub(super) fn emit_fn(decl: &FunctionDecl, ctx: &mut Ctx<'_>) -> Result<(), EmitError> {
    let name = text(ctx.src, decl.name.span);
    let sig = ctx.fns[name].clone();
    ctx.current_function = Some(decl.name.span);

    let mut scope: Vec<NativeLocal> = Vec::new();
    for (param, repr) in decl.params.iter().zip(sig.params.iter()) {
        let pname = text(ctx.src, param.name.span).to_string();
        ctx.confirm_local(&pname, param.name.span, repr.mono())
            .map_err(|e| e.at(param.name.span))?;
        if matches!(repr, NativeParam::ByteRecord(_)) {
            ctx.confirm_byte_record_param(&pname, param.name.span)
                .map_err(|e| e.at(param.name.span))?;
        }
        scope.push(NativeLocal {
            name: pname,
            kind: repr.local_kind(),
            mutable: false,
        });
    }

    let mut body = String::new();
    let (stmts, tail) = (&decl.body.stmts, decl.body.tail.as_deref());
    for stmt in stmts {
        // A `return` inside a function body is supported only as the tail form
        // here (refuse early returns this slice — they need flow lowering).
        if matches!(stmt.kind, StmtKind::Return(_)) {
            return Err(decline("an early return").at(stmt.span));
        }
        emit_stmt(stmt, ctx, &mut scope, &mut body, false)?;
    }
    let ret_rs = match tail {
        Some(tail) => {
            let low = emit_expr(tail, ctx, &scope)?;
            if low.ty != sig.ret {
                return Err(
                    decline("a function tail whose type is not the return type").at(tail.span)
                );
            }
            low.rs
        }
        None if sig.ret == NativeTy::Unit => "()".to_string(),
        None => return Err(decline("a non-unit function with no tail value").at(decl.name.span)),
    };

    // The scalar params follow the fixed `cx` + `__call_span` ABI prefix.
    let mut params_rs = String::from("cx: RtCx, __call_span: Span");
    if ctx.hybrid {
        params_rs.push_str(", __hybrid_outer_guard: bool");
    }
    for (p, repr) in decl.params.iter().zip(sig.params.iter()) {
        params_rs.push_str(&format!(
            ", {}: {}",
            mangle(text(ctx.src, p.name.span)),
            repr.rust()
        ));
    }
    // The guard is the FIRST statement: it checks the cap at the CALL-EXPRESSION
    // span (passed in) then enters one call level, restored on every exit by the
    // RAII guard — byte-identical to the boxed `call_value` path. `let _ = &cx;`
    // keeps `cx` live for a leaf fn that makes no further calls (no unused warn).
    let guard = if ctx.hybrid {
        "    let _guard = if __hybrid_outer_guard { None } else { Some(__native_enter_call(&cx, __call_span)?) };\n"
    } else {
        "    let _guard = __native_enter_call(&cx, __call_span)?;\n"
    };
    ctx.fn_defs.push_str(&format!(
        "/// Native scalar function (v5.4 monomorphized emit).\n\
         async fn {fname}({params_rs}) -> Result<{ret}, RtError> {{\n\
         {guard}\
         \x20   let _ = &cx;\n\
         {body}    Ok({ret_rs})\n\
         }}\n\n",
        fname = sig.rust_name,
        ret = sig.ret.rust(),
    ));
    ctx.current_function = None;
    Ok(())
}
