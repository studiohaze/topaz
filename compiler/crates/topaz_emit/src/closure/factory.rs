use crate::*;

/// §4 (v5.4) lower ONE receiver method to a `Value::Closure` expression — the
/// method's `self` + remaining parameters, captures (the top-level functions /
/// methods its body references, as `Rc` clones from `locals`), boundary guards, and
/// body. Mirrors the `StmtKind::Function` closure build, so a method runs through the
/// SAME `call_value` machinery as a free function (run≡build). The receiver fills
/// `self` (the FIRST parameter); a wrong receiver/arity faults at the shared
/// boundary, byte-identically to the interpreter.
pub(crate) fn emit_method_closure(
    decl: &FunctionDecl,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    locals: &[(String, Bind)],
) -> Result<String, EmitError> {
    let variadic_name = decl
        .params
        .last()
        .filter(|p| p.variadic)
        .map(|p| text(src, p.name.span));
    let fixed_count = decl.params.len() - variadic_name.is_some() as usize;
    let mut param_names: Vec<&str> = Vec::with_capacity(fixed_count);
    let mut defaults: Vec<Option<String>> = Vec::with_capacity(fixed_count);
    for p in &decl.params[..fixed_count] {
        param_names.push(text(src, p.name.span));
        match &p.default {
            None => defaults.push(None),
            Some(d) => {
                defaults.push(Some(emit_function_default_entry(
                    d,
                    src,
                    aliases,
                    locals,
                    "method default shape",
                )?));
            }
        }
    }
    let mut param_locals: Vec<(String, Bind)> = param_names
        .iter()
        .map(|n| (n.to_string(), Bind::Imm))
        .collect();
    if let Some(v) = variadic_name {
        param_locals.push((v.to_string(), Bind::Imm));
    }
    let captures = closure_captures_block(&decl.body, &param_locals, locals, src)?;
    let mut scope: Vec<(String, Bind)> = Vec::with_capacity(captures.len() + param_names.len());
    push_capture_locals(&captures, locals, &mut scope).map_err(|e| e.at(decl.name.span))?;
    let body_base = scope.len();
    for n in &param_names {
        scope.push((n.to_string(), Bind::Imm));
    }
    if let Some(v) = variadic_name {
        scope.push((v.to_string(), Bind::Imm));
    }
    // A method body is its own (non-flat-module-top) body; carry the nested flag so a
    // qualified type inside it refuses, exactly like a nested function.
    let body_aliases = aliases.with_body(&decl.type_params, true);
    let (body_lines, body_tail) = emit_stmt_seq(StatementSequenceEmission {
        stmts: &decl.body.stmts,
        tail: decl.body.tail.as_deref(),
        src,
        aliases: &body_aliases,
        locals: &mut scope,
        base: body_base,
        in_loop: false,
        defer_scope: true,
        at_module_top: false,
    })?;
    // §6 boundary guards for the non-self fixed params (self's placeholder type
    // is never guarded — it is the receiver, supplied by dispatch). Skip slot 0.
    let mut guard_lines = String::new();
    for (p, pname) in decl.params[..fixed_count].iter().zip(&param_names).skip(1) {
        if !boundary_guardable(&p.ty, src, &decl.type_params) {
            continue;
        }
        let mut __tc = 0u32;
        let access = format!("&{}", mangle(pname));
        if let Some(test) = type_test(
            &p.ty,
            src,
            &access,
            &mut __tc,
            aliases,
            &[],
            &mut Vec::new(),
        ) {
            guard_lines.push_str(&format!(
                "if !{test} {{ return Err(fault(codes::GUARD_TYPE, {msg:?}, {span})); }} ",
                msg = "argument does not match parameter type (§6)",
                span = emit_span(p.ty.span),
            ));
        }
    }
    let body_rs = if body_lines.trim().is_empty() {
        body_tail
    } else {
        format!("{{ {body_lines}{body_tail} }}")
    };
    let return_guard = decl.return_type.as_ref().and_then(|rt| {
        if !boundary_guardable(rt, src, &decl.type_params) {
            return None;
        }
        let mut __tc = 0u32;
        type_test(rt, src, "&__ret", &mut __tc, aliases, &[], &mut Vec::new()).map(|test| {
            format!(
                "if !{test} {{ return Err(fault(codes::GUARD_TYPE, {msg:?}, {span})); }} ",
                msg = "return value does not match the declared type (§6)",
                span = emit_span(rt.span),
            )
        })
    });
    let variadic_guard = decl
        .params
        .last()
        .filter(|p| p.variadic && boundary_guardable(&p.ty, src, &decl.type_params))
        .and_then(|p| {
            let mut __tc = 0u32;
            type_test(&p.ty, src, "__e", &mut __tc, aliases, &[], &mut Vec::new()).map(|test| {
                format!(
                    "if !{test} {{ return Err(fault(codes::GUARD_TYPE, {msg:?}, {span})); }} ",
                    msg = "argument does not match parameter type (§6)",
                    span = emit_span(p.ty.span),
                )
            })
        });
    let has_defers = decl.body.stmts.iter().any(stmt_registers_defer);
    Ok(emit_closure_value(ClosureEmission {
        param_names: &param_names,
        captures: &captures,
        defaults: &defaults,
        variadic: variadic_name,
        variadic_guard: variadic_guard.as_deref(),
        param_guards: &guard_lines,
        body: &body_rs,
        return_guard: return_guard.as_deref(),
        has_defers,
    }))
}

pub(crate) fn emit_closure_value(emission: ClosureEmission<'_>) -> String {
    let ClosureEmission {
        param_names,
        captures,
        defaults,
        variadic,
        variadic_guard,
        param_guards,
        body: body_rs,
        return_guard,
        has_defers,
    } = emission;
    let mut binds: String = param_names
        .iter()
        .map(|n| {
            format!(
                "let {} = __args.next().expect(\"arity checked at the call site\"); ",
                mangle(n)
            )
        })
        .collect();
    // §6 fixed-parameter guards run here — right after the fixed bindings
    // and BEFORE the variadic element guard below — so the boundary fault ORDER
    // matches the interpreter (`apply_call` guards each fixed param, then the
    // variadic tail). Emitting them into the body prelude instead would let a bad
    // variadic argument fault ahead of a bad fixed argument, diverging the fault
    // span run vs build. A `return Err(..)` here leaves the outer body before the
    // inner body/return-guard block runs.
    binds.push_str(param_guards);
    // §5 a trailing `...rest` parameter binds the surplus positional arguments
    // (everything past the fixed params, which `call_value` left in `__args`)
    // as an array — mirroring the interpreter collecting them into `rest`.
    if let Some(v) = variadic {
        match variadic_guard {
            // §6 guards each surplus argument against the element type before
            // binding the rest array — the emit twin of the interpreter's per-
            // element `rest` guard. A `return Err(..)` here leaves the outer body
            // (a param-class fault, BEFORE the body/return guard run).
            Some(guard) => binds.push_str(&format!(
                "let __rest = __args.collect::<Vec<_>>(); for __e in &__rest {{ {guard} }} let {} = Value::array(__rest); ",
                mangle(v)
            )),
            None => binds.push_str(&format!(
                "let {} = Value::array(__args.collect::<Vec<_>>()); ",
                mangle(v)
            )),
        }
    }
    let param_list = param_names
        .iter()
        .map(|n| format!("{n:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    // §7 per-parameter defaults (parallel to `params`). All-`None` (every lambda
    // and a `function` without defaults) emits an empty `Vec` — `has_param_default`
    // then returns false for every slot. Literal defaults are inert values; defaults
    // that read the defining environment are call-time thunks.
    let defaults_rs = if defaults.iter().all(Option::is_none) {
        "Vec::new()".to_string()
    } else {
        let entries: Vec<String> = defaults
            .iter()
            .map(|d| match d {
                Some(e) => format!("Some({e})"),
                None => "None".to_string(),
            })
            .collect();
        format!("vec![{}]", entries.join(", "))
    };
    let hidden_captures = hidden_rust_module_captures(body_rs);
    let percall: String = captures
        .iter()
        .map(|c| format!("let {m} = __cap{m}.clone(); ", m = mangle(c)))
        .chain(
            hidden_captures
                .iter()
                .map(|name| format!("let {name} = __cap_{name}.clone(); ")),
        )
        .collect();
    // §6: with a guardable declared return type, funnel every return path
    // (the tail, an explicit `return Ok(..)`, a `?`-propagated `return Ok(__early)`,
    // a case-arm `return Ok(..)`) into one `__ret` by running the body inside an
    // inner `async move` block — that block is the "nearest async move block" each
    // emitted `return Ok(..)` targets — then guard `__ret` once. `.await?`
    // re-propagates a real fault (a failed param guard, a body fault) UNguarded.
    // This is the emit twin of the interpreter's single-CallBoundary guard. A
    // lambda / a non-guardable return type passes `None` and keeps the bare tail.
    // §14 with `defer`s, the body runs in the SAME inner-async funnel, but the
    // result is matched rather than `?`-propagated: a Rust `Ok` (a Topaz
    // `return`/`?`/normal exit) drains `__defers` LIFO THEN applies the return
    // guard; a Rust `Err` (an ORDINARY runtime fault) propagates WITHOUT draining
    // (the interpreter does not drain on an ordinary fault). `__defers`/`cx` are
    // CLONED into the inner `async move` (which moves the param binds + captures) so
    // the OUTER still holds them to drain afterwards. The pushed defer closures own
    // snapshots of their captured locals, so they outlive the inner block.
    let body = match (has_defers, return_guard) {
        (false, Some(guard)) => format!(
            "let __ret: Value = async move {{ Ok::<Value, RtError>({body_rs}) }}.await?; {guard}Ok(__ret)"
        ),
        (false, None) => format!("Ok({body_rs})"),
        (true, Some(guard)) => format!(
            "let __defers = defer_stack(); let __defers_b = __defers.clone(); let __cx_d = cx.clone(); \
             let __inner = async move {{ let __defers = __defers_b; let cx = __cx_d; Ok::<Value, RtError>({body_rs}) }}.await; \
             let __ret: Value = match __inner {{ Ok(__v) => {{ run_defers(&__defers, &cx).await; __v }} Err(__e) => return Err(__e) }}; {guard}Ok(__ret)"
        ),
        (true, None) => format!(
            "let __defers = defer_stack(); let __defers_b = __defers.clone(); let __cx_d = cx.clone(); \
             let __inner = async move {{ let __defers = __defers_b; let cx = __cx_d; Ok::<Value, RtError>({body_rs}) }}.await; \
             match __inner {{ Ok(__v) => {{ run_defers(&__defers, &cx).await; Ok(__v) }} Err(__e) => Err(__e) }}"
        ),
    };
    let closure = format!(
        "EmittedClosure {{ call: {}|cx: RtCx, args: Vec<Value>| -> CallFuture {{ {percall}Box::pin(async move {{ let _ = &cx; let mut __args = args.into_iter(); {binds}{body} }}) }}, params: &[{param_list}], defaults: {defaults_rs}, variadic: {variadic} }}",
        if captures.is_empty() && hidden_captures.is_empty() {
            ""
        } else {
            "move "
        },
        variadic = variadic.is_some(),
    );
    if captures.is_empty() && hidden_captures.is_empty() {
        format!("Value::Closure(Rc::new({closure}))")
    } else {
        let snapshot: String = captures
            .iter()
            .map(|c| format!("let __cap{m} = {m}.clone(); ", m = mangle(c)))
            .chain(
                hidden_captures
                    .iter()
                    .map(|name| format!("let __cap_{name} = {name}.clone(); ")),
            )
            .collect();
        format!("{{ {snapshot}Value::Closure(Rc::new({closure})) }}")
    }
}

/// Move a flat module-top Topaz function's closure construction into its own
/// Rust item. Large Topaz modules can contain hundreds of declarations; leaving
/// every constructor expression in the module initializer makes rustc and Wasm
/// combine all of their capture snapshots and temporaries into one enormous
/// function. A local Rust item cannot capture its enclosing scope, so every
/// Topaz capture is an explicit owned parameter and initialization order remains
/// at the original `top_cell_set` call site.
pub(crate) fn emit_top_level_closure_factory(
    span: Span,
    captures: &[&str],
    locals: &[(String, Bind)],
    closure: &str,
) -> Result<(String, String), EmitError> {
    let name = format!(
        "__topaz_make_function_{}_{}_{}",
        span.file.0, span.lo, span.hi
    );
    let mut parameters = Vec::new();
    let mut arguments = Vec::new();
    for capture in captures {
        let bind = lookup_bind(locals, capture).unwrap_or(Bind::Imm);
        let ty = match bind {
            Bind::Imm | Bind::Namespace => "Value",
            Bind::Cell | Bind::ImmCell => "Rc<std::cell::RefCell<Value>>",
            Bind::TopFnCell | Bind::TopValueCell | Bind::TopMutValueCell => "TopCell",
            Bind::Mut => {
                return Err(EmitError::unsupported(
                    "closure capture of a mutable binding",
                ));
            }
        };
        let rust_name = mangle(capture);
        parameters.push(format!("{rust_name}: {ty}"));
        arguments.push(format!("{rust_name}.clone()"));
    }
    for hidden in hidden_rust_module_captures(closure) {
        if parameters
            .iter()
            .any(|parameter| parameter.starts_with(&format!("{hidden}:")))
        {
            continue;
        }
        let ty = if hidden.starts_with("__mod_") {
            "Value"
        } else {
            "TopCell"
        };
        parameters.push(format!("{hidden}: {ty}"));
        arguments.push(format!("{hidden}.clone()"));
    }
    Ok((
        format!(
            "fn {name}({}) -> Value {{ {closure} }}\n",
            parameters.join(", ")
        ),
        format!("{name}({})", arguments.join(", ")),
    ))
}

pub(crate) fn hidden_rust_module_captures(body_rs: &str) -> Vec<String> {
    let mut captures = Vec::new();
    let bytes = body_rs.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i = (i + 2).min(bytes.len());
                    } else if bytes[i] == b'"' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
            b'\'' => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i = (i + 2).min(bytes.len());
                    } else if bytes[i] == b'\'' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
            _ if (body_rs[i..].starts_with("__mod_")
                || body_rs[i..].starts_with("__topaz_self_default_")
                || body_rs[i..].starts_with("__topaz_record_default_"))
                && i.checked_sub(1)
                    .is_none_or(|prev| !is_rust_ident_byte(bytes[prev])) =>
            {
                let name_len = bytes[i..]
                    .iter()
                    .take_while(|byte| is_rust_ident_byte(**byte))
                    .count();
                let name = &body_rs[i..i + name_len];
                if !captures.iter().any(|captured| captured == name) {
                    captures.push(name.to_string());
                }
                i += name_len;
            }
            _ => {
                i += 1;
            }
        }
    }
    captures
}

pub(crate) fn is_rust_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}
