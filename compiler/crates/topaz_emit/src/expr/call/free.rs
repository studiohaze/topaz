use super::*;

pub(crate) fn free_builtin_kind(aliases: &Aliases<'_, '_>, name: &str) -> Option<&'static str> {
    match name {
        "print" => Some("Print"),
        "toInt" => Some("ToInt"),
        "toIntRadix" => Some("ToIntRadix"),
        "fromCodePoint" => Some("FromCodePoint"),
        "toFloat" => Some("ToFloat"),
        "input" => Some("Input"),
        "assert" => Some("TestAssert"),
        "map" => Some("MapFn"),
        "filter" => Some("FilterFn"),
        "reduce" => Some("ReduceFn"),
        "open" => Some("Open"),
        _ => lispex_intrinsic_kind(aliases, name),
    }
}

pub(crate) fn emit_builtin_value_call(
    kind: &str,
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
    span: Span,
) -> Result<String, EmitError> {
    emit_call_value_with_args(
        &format!("Value::Builtin {{ kind: Builtin::{kind}, recv: None }}"),
        &[],
        args,
        ctx,
        span,
    )
}

pub(crate) fn emit_newtype_constructor_call(
    definition: &NewtypeDef<'_>,
    name: &str,
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
    span: Span,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    match args {
        [CallArg::Positional(argument)] => {
            let argument = emit_expr(argument, src, aliases, locals, in_loop)?;
            Ok(match &definition.declaration_identity {
                Some(identity) => {
                    let method_identity = definition
                        .method_identity
                        .as_ref()
                        .map_or("None::<&str>".to_string(), |method| {
                            format!("Some({method:?})")
                        });
                    format!(
                        "Value::newtype_with_identities({:?}, {identity:?}, {method_identity}, {argument})",
                        definition.id
                    )
                }
                None => match &definition.method_identity {
                    Some(method_identity) => format!(
                        "Value::newtype_with_method_identity({:?}, Some({method_identity:?}), {argument})",
                        definition.id
                    ),
                    None => format!("Value::newtype({:?}, {argument})", definition.id),
                },
            })
        }
        [_] => {
            let message = format!("newtype `{name}` takes a positional argument");
            Ok(format!(
                "{{ let __v: Value = return Err(fault(codes::GUARD_TYPE, {message:?}, {})); __v }}",
                emit_span(span)
            ))
        }
        _ => {
            let message = format!("newtype `{name}` constructor takes exactly one argument");
            Ok(format!(
                "{{ let __v: Value = return Err(fault(codes::GUARD_ARITY, {message:?}, {})); __v }}",
                emit_span(span)
            ))
        }
    }
}

pub(crate) fn emit_unshadowed_ident_call(
    expr: &Expr,
    callee: &Expr,
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let name = text(src, callee.span);
    let span = emit_span(expr.span);
    // §3 (v5.4) NEWTYPE construction `UserId(5)`: a declared newtype
    // NOT shadowed by a local, ONE positional arg → wrap it via the
    // shared `Value::newtype` leaf (the SAME the interpreter's
    // `KNewtypeCtor` calls), so the constructed value is byte-identical
    // run≡build. A WRONG arg shape under `--unchecked` (the checker
    // rejects it on the checked path) must FAULT identically to the
    // interpreter's `schedule_call`, NOT decline with TPZ6001: a wrong
    // arity is GUARD_ARITY before any arg is evaluated; a single
    // non-positional arg is GUARD_TYPE. The fault is a TYPED
    // `Value`-position expression (`let __v: Value = return Err(…)`) so
    // it compiles in any context (a bare `!` block leaves inference
    // unconstrained → rustc E0282). Mirrors the enum N-payload path.
    if let Some(definition) = aliases.newtypes.get(name) {
        return emit_newtype_constructor_call(definition, name, args, ctx, expr.span);
    }
    // §5/§22 a FREE builtin called with NAMED args (`print(value: x)`,
    // `toInt(text: s)`, `map(xs: a, f: g)`) — the positional fast
    // paths below take positional-only, so route a named call through the
    // generic `call_value_named` over the builtin VALUE. The runtime binds
    // names to the builtin's signature slots then dispatches, with the §5
    // no-parameter / given-twice / missing faults — exactly the
    // interpreter's `apply_call` builtin named path. (Positional-after-named
    // and spread+named keep refusing, as elsewhere.)
    let builtin_kind = free_builtin_kind(aliases, name);
    if let Some(kind) = builtin_kind
        && args
            .iter()
            .any(|argument| matches!(argument, CallArg::Named { .. } | CallArg::Spread(_)))
    {
        return emit_builtin_value_call(kind, args, ctx, expr.span);
    }
    if let Some(kind) = lispex_intrinsic_kind(aliases, name) {
        return emit_builtin_value_call(kind, args, ctx, expr.span);
    }
    match name {
        "assert" => return emit_builtin_value_call("TestAssert", args, ctx, expr.span),
        // §22 `input()` — zero-arg host pull; the SHARED leaf
        // `builtin_input` both engines call. A non-zero-arg call is
        // checker-rejected (arity); in `--unchecked` it must still match
        // the interpreter's RUNTIME arity fault, so route the (positional)
        // args through `call_value` over the builtin VALUE — which
        // arity-faults — rather than an emit over-refusal.
        "input" => {
            if args.is_empty() {
                return Ok("builtin_input(&*cx.host())".to_string());
            }
            return emit_builtin_value_call("Input", args, ctx, expr.span);
        }
        "print" | "toInt" => {
            let [CallArg::Positional(arg)] = args else {
                return Err(EmitError::unsupported("builtin call shape"));
            };
            let arg_rs = emit_expr(arg, src, aliases, locals, in_loop)?;
            return Ok(if name == "print" {
                format!("builtin_print(&*cx.host(), {arg_rs}, {span})?")
            } else {
                format!("builtin_to_int({arg_rs}, {span})?")
            });
        }
        // toIntRadix(text, radix) — positional (named routes via the gate)
        "toIntRadix" => {
            if let [CallArg::Positional(text), CallArg::Positional(radix)] = args {
                let t = emit_expr(text, src, aliases, locals, in_loop)?;
                let r = emit_expr(radix, src, aliases, locals, in_loop)?;
                return Ok(format!("builtin_to_int_radix({t}, {r}, {span})?"));
            }
            return emit_builtin_value_call("ToIntRadix", args, ctx, expr.span);
        }
        "fromCodePoint" => {
            if let [CallArg::Positional(arg)] = args {
                let arg_rs = emit_expr(arg, src, aliases, locals, in_loop)?;
                return Ok(format!("builtin_from_code_point({arg_rs}, {span})?"));
            }
            return emit_builtin_value_call("FromCodePoint", args, ctx, expr.span);
        }
        "toFloat" => {
            if let [CallArg::Positional(arg)] = args {
                let arg_rs = emit_expr(arg, src, aliases, locals, in_loop)?;
                return Ok(format!("builtin_to_float({arg_rs}, {span})?"));
            }
            return emit_builtin_value_call("ToFloat", args, ctx, expr.span);
        }
        // §22 HOF: `map(items, f)` / `filter(items, f)` —
        // materialize the iterable then call `f` per element
        // through `call_value` (so the per-element closure's
        // faults match `apply_call`); `filter` keeps an
        // element via the SHARED `filter_keep` guard.
        "map" | "filter" => {
            let [CallArg::Positional(items), CallArg::Positional(f)] = args else {
                return Err(EmitError::unsupported("builtin call shape"));
            };
            let items_rs = emit_expr(items, src, aliases, locals, in_loop)?;
            let f_rs = emit_expr(f, src, aliases, locals, in_loop)?;
            return Ok(emit_hof(name, &items_rs, &f_rs, &span));
        }
        // §22 `reduce(items, initial, f)` — fold `f` over the
        // elements from `initial` (args evaluate left to
        // right: iterable, initial, function).
        "reduce" => {
            let [
                CallArg::Positional(items),
                CallArg::Positional(initial),
                CallArg::Positional(f),
            ] = args
            else {
                return Err(EmitError::unsupported("builtin call shape"));
            };
            let items_rs = emit_expr(items, src, aliases, locals, in_loop)?;
            let initial_rs = emit_expr(initial, src, aliases, locals, in_loop)?;
            let f_rs = emit_expr(f, src, aliases, locals, in_loop)?;
            return Ok(emit_reduce(&items_rs, &initial_rs, &f_rs, &span));
        }
        // §22.1 prelude constructors Some/Ok/Err — a free
        // (non-shadowed) Ident callee with ONE `value` arg wraps it,
        // exactly as the interpreter's `KCtor`
        // (`Value::Some/Ok/Err(Rc::new(arg))`). The name maps
        // directly to the `Value` variant. A wrong arity/name is
        // refused (the interpreter faults GUARD_ARITY/ARG; refusing
        // declines the program rather than mis-emitting). A
        // SHADOWED name is an ordinary call (handled below).
        "Some" | "Ok" | "Err" => {
            let arg = match args {
                [CallArg::Positional(arg)] => arg,
                [CallArg::Named { name, value }] if text(src, name.span) == "value" => value,
                _ => return Err(EmitError::unsupported("constructor call shape")),
            };
            let arg_rs = emit_expr(arg, src, aliases, locals, in_loop)?;
            return Ok(format!("Value::{name}(Rc::new({arg_rs}))"));
        }
        _ => {}
    }

    let callee_rs = emit_expr(callee, src, aliases, locals, in_loop)?;
    emit_call_value_with_args(&callee_rs, &[], args, ctx, expr.span)
}
