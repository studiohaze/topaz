use crate::*;

/// The innermost binding of `name` in scope (a child scope shadows an enclosing
/// one, so scan most-recent-first), or `None` if it is not a local.
pub(crate) fn lookup_bind(locals: &[(String, Bind)], name: &str) -> Option<Bind> {
    locals
        .iter()
        .rev()
        .find(|(n, _)| n == name)
        .map(|(_, b)| *b)
}

pub(crate) fn read_local(name: &str, bind: Bind) -> String {
    match bind {
        // A recursion `ImmCell` reads through `cell_get` exactly like a `Cell`
        // (the borrow is dropped before any recursive call).
        Bind::Cell | Bind::ImmCell => format!("cell_get(&{})", mangle(name)),
        // §17 a `Namespace` binding (an imported module's record) reads like an
        // `Imm` — it is an ordinary value clone; only `type_test` treats it specially.
        Bind::Imm | Bind::Mut | Bind::Namespace => format!("{}.clone()", mangle(name)),
        // §7 a top-level forward-reference cell is read ONLY through the `Ident`
        // arm, which has the reading identifier's span for the fallible
        // `top_cell_get(.., span)?`. It never reaches here: a function name is not
        // a mutation root (3516) nor a pipe placeholder (5026) — the checker
        // rejects assigning/placeholdering a function — so this is unreachable.
        Bind::TopFnCell | Bind::TopValueCell | Bind::TopMutValueCell => {
            unreachable!("top-level option-cell read via the Ident arm")
        }
    }
}

pub(crate) fn emit_let_statement(
    emission: LetStatementEmission<'_, '_, '_>,
) -> Result<(), EmitError> {
    let LetStatementEmission {
        stmt,
        mutable,
        pattern,
        value,
        cells,
        captured,
        self_runtime_default_cells,
        src,
        aliases,
        locals,
        base,
        in_loop,
        lines,
    } = emission;
    // §6 a typed `let x: T = v` RUNTIME-conformance-checks `v`
    // against `T` (the interpreter's `KLetPattern` → `match_pattern`
    // → `type_matches`; a non-conforming value faults "let pattern
    // did not match"). The value is wrapped in the same `type_test`
    // guard the typed match pattern uses (scalar / union / structural
    // `Option`/`Result`/`Array`/`Set` / record), then bound immutably.
    // Only an IMMUTABLE `let` carries a typed PATTERN: the parser binds
    // `let mut` as a plain `Binding` with the type in a separate
    // (runtime-ignored) `ty` field, so a `mut` typed pattern cannot
    // currently occur — the `mutable` guard is defensive (a typed
    // `let mut` would need the cell interaction). A `Map` with an undecidable
    // inner type, or an undecidable alias, is still refused.
    if let PatternKind::Typed { name, ty } = &pattern.kind {
        if mutable {
            return Err(EmitError::unsupported("typed mutable let").at(stmt.span));
        }
        let mut __tc = 0u32;
        let test = type_test(ty, src, "&__v", &mut __tc, aliases, locals, &mut Vec::new())
            .ok_or_else(|| EmitError::unsupported("typed let type").at(stmt.span))?;
        let bname = text(src, name.span);
        let is_top_value = locals[base..]
            .iter()
            .any(|(n, b)| n == bname && matches!(b, Bind::TopValueCell));
        if !is_top_value && locals[base..].iter().any(|(n, _)| n == bname) {
            return Err(EmitError::unsupported("same-scope redeclaration").at(stmt.span));
        }
        if captured.contains(&bname) {
            return Err(
                EmitError::unsupported("declaration shadows a captured binding").at(stmt.span),
            );
        }
        let value_rs = emit_expr(value, src, aliases, locals, in_loop)?;
        let checked = format!(
            "{{ let __v = {value_rs}; if {test} {{ __v }} else {{ return Err(fault(codes::GUARD_TYPE, {msg:?}, {span})); }} }}",
            msg = "`let` pattern did not match the value (§4)",
            span = emit_span(stmt.span),
        );
        if is_top_value {
            lines.push_str(&format!(
                "    top_cell_set(&{}, {checked});\n",
                mangle(bname)
            ));
        } else {
            lines.push_str(&format!("    let {} = {checked};\n", mangle(bname)));
            locals.push((bname.to_string(), Bind::Imm));
        }
    } else if matches!(
        &pattern.kind,
        PatternKind::List(_) | PatternKind::Record(_) | PatternKind::Or(_)
    ) {
        // §4 a DESTRUCTURING `let [a, b] = v` / `let { x, y } = r` /
        // exhaustive same-bindings `let Ok(x) | Err(x) = value`.
        let line = emit_destructure_let(DestructureLetEmission {
            pattern,
            value,
            span: stmt.span,
            mutable,
            src,
            aliases,
            locals,
            base,
            captured,
            in_loop,
        })?;
        lines.push_str(&line);
    } else {
        let value_rs = emit_expr(value, src, aliases, locals, in_loop)?;
        match binding_name(pattern, src)? {
            Some(name) => {
                let top_bind = locals[base..]
                    .iter()
                    .rev()
                    .find(|(n, _)| n == name)
                    .map(|(_, bind)| *bind)
                    .filter(|bind| matches!(bind, Bind::TopValueCell | Bind::TopMutValueCell));
                // §4 same-scope redeclaration is a static error
                // (the interpreter faults GUARD_REDECLARE in the
                // unchecked path); refuse rather than Rust-shadow.
                // Only THIS scope's own bindings (`base..`) count
                // — shadowing an enclosing binding is legal.
                if top_bind.is_none() && locals[base..].iter().any(|(n, _)| n == name) {
                    return Err(EmitError::unsupported("same-scope redeclaration").at(stmt.span));
                }
                // …but shadowing an enclosing binding that an earlier
                // closure CAPTURED is refused (see `captured`).
                if captured.contains(&name) {
                    return Err(
                        EmitError::unsupported("declaration shadows a captured binding")
                            .at(stmt.span),
                    );
                }
                // §5 a `let mut` that a closure CAPTURES becomes a
                // shared `Rc<RefCell<Value>>` cell (so a later mutation
                // is visible inside the closure, matching the
                // interpreter's whole-env capture); an un-captured
                // `let mut` stays a plain Rust `let mut`. An immutable
                // `let` is unchanged.
                let bind = if let Some(bind) = top_bind {
                    bind
                } else if mutable && cells.contains(&name) {
                    Bind::Cell
                } else if mutable {
                    Bind::Mut
                } else {
                    Bind::Imm
                };
                if matches!(bind, Bind::TopValueCell | Bind::TopMutValueCell) {
                    lines.push_str(&format!(
                        "    top_cell_set(&{}, {value_rs});\n",
                        mangle(name)
                    ));
                } else if bind == Bind::Cell {
                    lines.push_str(&format!(
                        "    let {} = cell_new({value_rs});\n",
                        mangle(name)
                    ));
                } else {
                    let kw = if mutable { "let mut" } else { "let" };
                    let annotation = if expr_has_bare_return(value) {
                        ": Value"
                    } else {
                        ""
                    };
                    lines.push_str(&format!(
                        "    {kw} {}{annotation} = {value_rs};\n",
                        mangle(name)
                    ));
                }
                if top_bind.is_none() {
                    locals.push((name.to_string(), bind));
                }
                if let Some(cell) = self_runtime_default_cells.get(name) {
                    lines.push_str(&format!(
                        "    top_cell_set(&{cell}, {}.clone());\n",
                        mangle(name)
                    ));
                }
            }
            // `let _ = e`: evaluate for effects, discard.
            None => lines.push_str(&format!("    let _ = {value_rs};\n")),
        }
    }
    Ok(())
}

pub(crate) fn emit_using_statement(
    emission: UsingStatementEmission<'_, '_, '_>,
) -> Result<(), EmitError> {
    let UsingStatementEmission {
        stmt,
        name,
        value,
        body,
        src,
        aliases,
        locals,
        in_loop,
        lines,
    } = emission;
    let id = {
        let mut f = aliases.flow.borrow_mut();
        f.next_id += 1;
        f.next_id
    };
    let stack = format!("__defers_using{id}");
    let ret = format!("__using_ret{id}");
    let uname = text(src, name.span);
    let mname = mangle(uname);
    let span = emit_span(stmt.span);
    let value_rs = emit_expr(value, src, aliases, locals, in_loop)?;

    lines.push_str("    {\n");
    lines.push_str(&format!("    let {stack} = defer_stack();\n"));
    lines.push_str(&format!("    let {mname} = {value_rs};\n"));
    lines.push_str(&format!(
        "    if {mname}.kind() != \"File\" {{ return Err(fault(codes::GUARD_TYPE, format!(\"`using` expects a `File`, found `{{}}`\", {mname}.kind()), {span})); }}\n"
    ));

    aliases.flow.borrow_mut().stacks.push(stack.clone());
    let close_body = format!(
        "call_resource_method(&*cx.host(), {mname}.clone(), \"close\", vec![], {span}, {span})?"
    );
    let close = emit_closure_value(ClosureEmission {
        param_names: &[],
        captures: &[uname],
        defaults: &[],
        variadic: None,
        variadic_guard: None,
        param_guards: "",
        body: &close_body,
        return_guard: None,
        has_defers: false,
    });
    lines.push_str(&format!("    defer_push(&{stack}, {close});\n"));

    let mut body_locals = locals.clone();
    body_locals.push((uname.to_string(), Bind::Imm));
    let body_rs = emit_block(body, src, aliases, &body_locals, in_loop);
    aliases.flow.borrow_mut().stacks.pop();
    let body_rs = body_rs?;
    lines.push_str(&format!("    let {ret} = {body_rs};\n"));
    lines.push_str(&format!("    run_defers(&{stack}, &cx).await;\n"));
    lines.push_str(&format!("    let _ = {ret};\n"));
    lines.push_str("    }\n");
    Ok(())
}

pub(crate) fn emit_defer_statement(
    emission: DeferStatementEmission<'_, '_, '_>,
) -> Result<(), EmitError> {
    let DeferStatementEmission {
        stmt,
        action,
        src,
        aliases,
        locals,
        lines,
    } = emission;
    // §14 push onto the INNERMOST active defer stack — the function body's
    // `__defers` or the enclosing block/loop/match's `__defersN` (set up at
    // the top of this `emit_stmt_seq`). No active stack ⇒ a `defer` with no
    // draining scope at all (module top, not inside any block) ⇒ refuse.
    let target = aliases.flow.borrow().stacks.last().cloned();
    let Some(target) = target else {
        return Err(EmitError::unsupported("defer outside a function body").at(stmt.span));
    };
    // §14 the interpreter runs a deferred action as a CONTAINED sub-run
    // (no function boundary), so an ESCAPING `return` or `?` inside it
    // faults "return outside a function" and routes to `defer_error`. A
    // native thunk closure would instead ABSORB the `return` (becoming a
    // silent `Ok`) or MIS-ROUTE the `?`-propagated `Err` — a divergence.
    // Refuse such an unsupported action; `expr_has_bare_return` flags
    // both an escaping `return` and a `?` (it does not descend into a
    // nested lambda, which is its own boundary).
    if expr_has_bare_return(action) {
        return Err(EmitError::unsupported("defer with an escaping return or `?`").at(stmt.span));
    }
    let captures = lambda_captures(action, &[], locals, src)?;
    let mut action_locals: Vec<(String, Bind)> = Vec::new();
    push_capture_locals(&captures, locals, &mut action_locals)?;
    // A defer action body is capture-pruned like a lambda — refuse a
    // qualified type in it (conservative; it always runs in a fn body).
    let action_aliases = aliases.with_body(&[], true);
    // §14 the deferred action is its own closure — reset the flow so a block
    // defer inside it cannot inherit/drain the enclosing scope's stacks.
    let action_rs = with_reset_flow(&action_aliases, |a| {
        emit_expr(action, src, a, &action_locals, false)
    })?;
    let action_closure = emit_closure_value(ClosureEmission {
        param_names: &[],
        captures: &captures,
        defaults: &[],
        variadic: None,
        variadic_guard: None,
        param_guards: "",
        body: &action_rs,
        return_guard: None,
        has_defers: false,
    });
    lines.push_str(&format!("    defer_push(&{target}, {action_closure});\n"));
    Ok(())
}

pub(crate) fn emit_return_statement(
    emission: ReturnStatementEmission<'_, '_, '_>,
) -> Result<(), EmitError> {
    let ReturnStatementEmission {
        value,
        src,
        aliases,
        locals,
        in_loop,
        lines,
    } = emission;
    // Evaluate the return value before draining crossed block-defer stacks.
    // A faulting value therefore leaves those stacks undrained, matching the
    // interpreter's evaluate-then-unwind order.
    let value = match value {
        Some(value) => emit_expr(value, src, aliases, locals, in_loop)?,
        None => "Value::Unit".to_string(),
    };
    let drain = {
        let flow = aliases.flow.borrow();
        flow.drain_from(flow.fn_base)
    };
    if drain.is_empty() {
        lines.push_str(&format!("    return Ok({value});\n"));
    } else {
        lines.push_str(&format!(
            "    let __ret_v = {value}; {drain}return Ok(__ret_v);\n"
        ));
    }
    Ok(())
}

pub(crate) fn seed_nested_function_cells(
    seeding: NestedFunctionCellSeeding<'_>,
) -> Result<(), EmitError> {
    let NestedFunctionCellSeeding {
        stmts,
        src,
        locals,
        base,
        lines,
    } = seeding;
    for stmt in stmts {
        let StmtKind::Function(declaration) = &stmt.kind else {
            continue;
        };
        let name = text(src, declaration.name.span);
        if locals[base..]
            .iter()
            .any(|(local, binding)| local == name && matches!(binding, Bind::TopFnCell))
        {
            return Err(
                EmitError::unsupported("same-scope redeclaration").at(declaration.name.span)
            );
        }
        if locals[base..].iter().any(|(local, _)| local == name) {
            continue;
        }
        let outer_seed = lookup_bind(&locals[..base], name).map(|binding| match binding {
            Bind::TopFnCell | Bind::TopValueCell | Bind::TopMutValueCell => format!(
                "top_cell_get(&{}, {:?}, {})?",
                mangle(name),
                name,
                emit_span(declaration.name.span)
            ),
            other => read_local(name, other),
        });
        if let Some(outer_seed) = outer_seed {
            let seed_name = format!("__top_fn_seed_{}", declaration.name.span.lo);
            lines.push_str(&format!(
                "    let {} = {{ let {seed_name} = top_cell(); top_cell_set(&{seed_name}, {outer_seed}); {seed_name} }};\n",
                mangle(name)
            ));
        } else {
            lines.push_str(&format!("    let {} = top_cell();\n", mangle(name)));
        }
        locals.push((name.to_string(), Bind::TopFnCell));
    }
    Ok(())
}

pub(crate) fn seed_recursion_cluster(
    seeding: RecursionClusterSeeding<'_>,
) -> Result<(), EmitError> {
    let RecursionClusterSeeding {
        stmt,
        names,
        locals,
        base,
        captured,
        lines,
    } = seeding;
    let Some(names) = names else {
        return Ok(());
    };
    for name in names {
        if locals[base..]
            .iter()
            .any(|(local, binding)| local == name && matches!(binding, Bind::TopFnCell))
        {
            continue;
        }
        if locals[base..].iter().any(|(local, _)| local == name) {
            return Err(EmitError::unsupported("same-scope redeclaration").at(stmt.span));
        }
        if captured.contains(&name.as_str()) {
            return Err(
                EmitError::unsupported("declaration shadows a captured binding").at(stmt.span),
            );
        }
        lines.push_str(&format!(
            "    let {} = cell_new(Value::Unit);\n",
            mangle(name)
        ));
        locals.push((name.clone(), Bind::ImmCell));
    }
    Ok(())
}

pub(crate) fn stmt_registers_defer(stmt: &Stmt) -> bool {
    matches!(&stmt.kind, StmtKind::Defer(_))
}

pub(crate) fn classify_function_binding(
    name: &str,
    emission: &FunctionStatementEmission<'_, '_, '_>,
) -> Result<FunctionBinding, EmitError> {
    let is_recursion_cell = emission.rec_celled_idx.contains(&emission.stmt_idx);
    let is_top_cell = emission.locals[emission.base..]
        .iter()
        .any(|(local, binding)| local == name && matches!(binding, Bind::TopFnCell));
    if is_top_cell {
        return Ok(FunctionBinding::TopCell);
    }
    if is_recursion_cell {
        return Ok(FunctionBinding::RecursionCell);
    }
    if emission.locals[emission.base..]
        .iter()
        .any(|(local, _)| local == name)
    {
        return Err(EmitError::unsupported("same-scope redeclaration").at(emission.stmt.span));
    }
    if emission.captured.contains(&name) {
        return Err(
            EmitError::unsupported("declaration shadows a captured binding").at(emission.stmt.span),
        );
    }
    Ok(FunctionBinding::Local)
}

pub(crate) fn prepare_function_parameters<'a>(
    decl: &'a FunctionDecl,
    src: &'a LoweredText,
    aliases: &Aliases<'_, '_>,
    locals: &[(String, Bind)],
) -> Result<FunctionParameters<'a>, EmitError> {
    let variadic = decl
        .params
        .last()
        .filter(|parameter| parameter.variadic)
        .map(|parameter| text(src, parameter.name.span));
    let fixed_count = decl.params.len() - variadic.is_some() as usize;
    let mut names = Vec::with_capacity(fixed_count);
    let mut defaults = Vec::with_capacity(fixed_count);
    for parameter in &decl.params[..fixed_count] {
        names.push(text(src, parameter.name.span));
        defaults.push(
            parameter
                .default
                .as_ref()
                .map(|default| {
                    emit_function_default_entry(
                        default,
                        src,
                        aliases,
                        locals,
                        "function default shape",
                    )
                })
                .transpose()?,
        );
    }
    let mut parameter_locals = names
        .iter()
        .map(|name| ((*name).to_string(), Bind::Imm))
        .collect::<Vec<_>>();
    if let Some(variadic) = variadic {
        parameter_locals.push((variadic.to_string(), Bind::Imm));
    }
    Ok(FunctionParameters {
        variadic,
        fixed_count,
        names,
        defaults,
        locals: parameter_locals,
    })
}

pub(crate) fn collect_function_captures<'a>(
    decl: &'a FunctionDecl,
    src: &'a LoweredText,
    enclosing: &[(String, Bind)],
    parameter_locals: &[(String, Bind)],
) -> Result<Vec<&'a str>, EmitError> {
    let mut captures = closure_captures_block(&decl.body, parameter_locals, enclosing, src)?;
    // Defaults execute from the defining environment when the caller omits a
    // slot. Their free variables belong to the function value just as body free
    // variables do; a module-top Rust factory must therefore receive them as
    // explicit parameters rather than refer to initializer locals it cannot see.
    for capture in function_defaults_captures(decl, enclosing, src)? {
        if !captures.contains(&capture) {
            captures.push(capture);
        }
    }
    for (index, statement) in decl.body.stmts.iter().enumerate() {
        let nested = match &statement.kind {
            StmtKind::Function(nested) => Some(nested),
            StmtKind::Export(inner) => match &inner.kind {
                StmtKind::Function(nested) => Some(nested),
                _ => None,
            },
            _ => None,
        };
        let Some(nested) = nested else { continue };
        let name = text(src, nested.name.span);
        let read_before_declaration = decl.body.stmts[..index]
            .iter()
            .any(|statement| stmt_references_name(statement, src, name));
        if read_before_declaration && has_local(enclosing, name) && !captures.contains(&name) {
            captures.push(name);
        }
    }
    Ok(captures)
}

pub(crate) fn emit_function_boundary_guards(
    decl: &FunctionDecl,
    parameters: &FunctionParameters<'_>,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
) -> FunctionBoundaryGuards {
    let mut parameter_guards = String::new();
    for (parameter, name) in decl.params[..parameters.fixed_count]
        .iter()
        .zip(&parameters.names)
    {
        if !boundary_guardable(&parameter.ty, src, &decl.type_params) {
            continue;
        }
        let mut counter = 0u32;
        let access = format!("&{}", mangle(name));
        if let Some(test) = type_test(
            &parameter.ty,
            src,
            &access,
            &mut counter,
            aliases,
            &[],
            &mut Vec::new(),
        ) {
            parameter_guards.push_str(&format!(
                "if !{test} {{ return Err(fault(codes::GUARD_TYPE, {message:?}, {span})); }} ",
                message = "argument does not match parameter type (§6)",
                span = emit_span(parameter.ty.span),
            ));
        }
    }
    let result = decl.return_type.as_ref().and_then(|result| {
        if !boundary_guardable(result, src, &decl.type_params) {
            return None;
        }
        let mut counter = 0u32;
        type_test(
            result,
            src,
            "&__ret",
            &mut counter,
            aliases,
            &[],
            &mut Vec::new(),
        )
        .map(|test| {
            format!(
                "if !{test} {{ return Err(fault(codes::GUARD_TYPE, {message:?}, {span})); }} ",
                message = "return value does not match the declared type (§6)",
                span = emit_span(result.span),
            )
        })
    });
    let variadic_element = decl
        .params
        .last()
        .filter(|parameter| {
            parameter.variadic && boundary_guardable(&parameter.ty, src, &decl.type_params)
        })
        .and_then(|parameter| {
            let mut counter = 0u32;
            type_test(
                &parameter.ty,
                src,
                "__e",
                &mut counter,
                aliases,
                &[],
                &mut Vec::new(),
            )
            .map(|test| {
                format!(
                    "if !{test} {{ return Err(fault(codes::GUARD_TYPE, {message:?}, {span})); }} ",
                    message = "argument does not match parameter type (§6)",
                    span = emit_span(parameter.ty.span),
                )
            })
        });
    FunctionBoundaryGuards {
        parameters: parameter_guards,
        result,
        variadic_element,
    }
}

/// Lower a block expression to a Rust block expression yielding its
/// `Value` (CDR-003 §5/§1a). The block is its OWN lexical scope: it
/// lowers on a COPY of the visible bindings, so a `let` inside cannot
/// leak out and a child binding may shadow an enclosing one — and the
/// emitted mangled locals then shadow correctly under Rust's own block
/// scoping (a shadowed name mangles identically, which is exactly what
/// Rust shadowing wants).
pub(crate) fn emit_function_statement(
    decl: &FunctionDecl,
    emission: FunctionStatementEmission<'_, '_, '_>,
) -> Result<(), EmitError> {
    let fname = text(emission.src, decl.name.span);
    let binding = classify_function_binding(fname, &emission)?;
    let FunctionStatementEmission {
        stmt,
        stmt_idx: _,
        src,
        aliases,
        locals,
        base: _,
        at_module_top,
        rec_celled_idx: _,
        captured: _,
        lines,
    } = emission;
    // §5 a trailing `...rest` parameter is VARIADIC: like the
    // interpreter (`params.last().filter(|p| p.variadic)`), only the
    // LAST parameter's variadic flag counts, and the FIXED parameters
    // are everything before it. The variadic is bound by a collect
    // inside the closure (not a fixed slot), so a default on it is
    // irrelevant (it always collects the positional surplus).
    let parameters = prepare_function_parameters(decl, src, aliases, locals)?;
    let captures = collect_function_captures(decl, src, locals, &parameters.locals)?;
    // A body-local function declaration is positional: until its
    // declaration executes, a same-named enclosing binding remains
    // visible. The ordinary free-variable walk quite correctly treats
    // that name as local to the body, so it does not capture the outer
    // value on its own. Preserve that outer binding explicitly; the
    // body-scope seeding pass below copies it into the fresh positional
    // cell, which is then overwritten at the declaration statement.
    // The body runs in the call env that holds the params
    // (`ClosureBody::Block` → `KBlock`, no extra scope), so a
    // body-top-level declaration colliding with a PARAM is a
    // same-scope redeclaration the interpreter FAULTS on — not a
    // shadow. Lower with the captures ENCLOSING (a body `let` may
    // shadow them) but the params in THIS scope (`base` set
    // BEFORE them), so `binding_name`'s redeclaration check sees
    // a param collision. (A lambda body cannot hit this: it is an
    // EXPRESSION — even `{ … }` is a block expression with its own
    // child env — so it shadows instead.)
    let mut scope = Vec::with_capacity(captures.len() + parameters.names.len());
    // The captures lower as ENCLOSING locals (a body `let` may
    // shadow them). Classify each by the enclosing binding — a
    // captured `Cell` carries its cell-ness into the body (reads go
    // through `cell_get`); a captured plain `Mut` is refused.
    // `push_capture_locals` has no AST node param, so locate its
    // refusal at THIS function-decl statement (the closure-as-expression
    // callers are covered by `emit_expr`'s `.at`; first-wins keeps a
    // tighter span if one is ever set deeper).
    push_capture_locals(&captures, locals, &mut scope).map_err(|e| e.at(stmt.span))?;
    let body_base = scope.len();
    for n in &parameters.names {
        scope.push((n.to_string(), Bind::Imm));
    }
    if let Some(v) = parameters.variadic {
        scope.push((v.to_string(), Bind::Imm));
    }
    // §3/§7 the function's generic type-params are in scope for its
    // BODY's typed bindings (a bare in-scope param erases in `type_test`;
    // mirrors the interpreter swapping `self.type_params` to the callee's).
    // REPLACES the enclosing set — a nested fn / lambda resets to its own.
    // §17 a qualified type in the body is shadow-decidable only when the
    // function is declared at the FLAT module top (its only non-captured
    // enclosing is module-top, handled by namespace filtering). Declared
    // inside a block / loop / match / another body, the closure env chains
    // to those scopes' locals that the capture-pruned emit locals miss —
    // so refuse (`in_nested`). Carries forward an already-nested context.
    let body_aliases = aliases.with_body(&decl.type_params, aliases.in_nested || !at_module_top);
    let (body_lines, body_tail) = with_reset_flow(&body_aliases, |aliases| {
        emit_stmt_seq(StatementSequenceEmission {
            stmts: &decl.body.stmts,
            tail: decl.body.tail.as_deref(),
            src,
            aliases,
            locals: &mut scope,
            base: body_base,
            in_loop: false,
            // §14 this is the one function-body scope whose `defer`s the
            // closure wrapper drains.
            defer_scope: true,
            // A function body is NOT the flat module top.
            at_module_top: false,
        })
    })?;
    let guards = emit_function_boundary_guards(decl, &parameters, src, aliases);
    // The fixed-param guards (`guard_lines`) are NOT prepended to the
    // body — they are threaded to `emit_closure_value` as `param_guards`
    // so they run in the bind prelude, BEFORE the variadic element guard
    // (the interpreter's order). The body is just the statements + tail.
    let body_rs = if body_lines.trim().is_empty() {
        body_tail
    } else {
        format!("{{ {body_lines}{body_tail} }}")
    };
    // §14 this function body owns a defer stack iff it has a top-level
    // `defer` (a nested-block defer was already refused while lowering
    // the body); the wrapper then drains it on the non-fault exits.
    let has_defers = decl.body.stmts.iter().any(stmt_registers_defer);
    let boxed_closure = emit_closure_value(ClosureEmission {
        param_names: &parameters.names,
        captures: &captures,
        defaults: &parameters.defaults,
        variadic: parameters.variadic,
        variadic_guard: guards.variadic_element.as_deref(),
        param_guards: &guards.parameters,
        body: &body_rs,
        return_guard: guards.result.as_deref(),
        has_defers,
    });
    let closure = if at_module_top {
        if let Some(native_closure) = aliases
            .type_ctx
            .hybrid
            .as_ref()
            .and_then(|plan| {
                plan.closure(aliases.identity, text(src, decl.name.span), decl.name.span)
            })
            .cloned()
        {
            native_closure
        } else {
            let (factory, call) =
                emit_top_level_closure_factory(decl.name.span, &captures, locals, &boxed_closure)?;
            aliases
                .type_ctx
                .closure_factories
                .borrow_mut()
                .push_str(&factory);
            call
        }
    } else {
        boxed_closure
    };
    if matches!(binding, FunctionBinding::TopCell) {
        // §7 fill the module-wide `TopFnCell` at this declaration's
        // position (POSITIONAL binding): every read via `top_cell_get`
        // now resolves; a read BEFORE here faulted `GUARD_UNBOUND`.
        lines.push_str(&format!(
            "    top_cell_set(&{}, {closure});\n",
            mangle(fname)
        ));
    } else if matches!(binding, FunctionBinding::RecursionCell) {
        // The cell was seeded `Value::Unit` at the cluster start and
        // the binding (`ImmCell`) is already in `locals`; fill it with
        // the closure so every reference through `cell_get` (own body,
        // siblings, later statements) now resolves to the function.
        lines.push_str(&format!("    cell_set(&{}, {closure});\n", mangle(fname)));
    } else {
        lines.push_str(&format!("    let {} = {closure};\n", mangle(fname)));
        locals.push((fname.to_string(), Bind::Imm));
    }
    Ok(())
}

/// A §7 function or method default that is a pure, non-faulting scalar const
/// expression. The interpreter evaluates defaults when a call needs them; the boxed
/// emitter stores closure defaults at closure creation. To keep those equivalent, this
/// helper first runs the same const evaluator used by `const`, then accepts only scalar
/// values it can render as an inert `Value` literal.
pub(crate) fn emit_function_default_value(
    expr: &Expr,
    src: &LoweredText,
    what: &'static str,
) -> Result<String, EmitError> {
    let consts = ConstValues::new();
    if !const_initializer_ok(expr, src, &consts) {
        return Err(EmitError::unsupported(what).at(expr.span));
    }
    let value = const_eval_emit(expr, src, &consts)
        .map_err(|_| EmitError::unsupported(what).at(expr.span))?;
    match value {
        Value::Int(_)
        | Value::Float(_)
        | Value::Bool(_)
        | Value::Null
        | Value::Unit
        | Value::Str(_) => Ok(render_value_rust(&value)),
        _ => Err(EmitError::unsupported(what).at(expr.span)),
    }
}

pub(crate) fn emit_function_default_entry(
    expr: &Expr,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    locals: &[(String, Bind)],
    what: &'static str,
) -> Result<String, EmitError> {
    if let Ok(value) = emit_function_default_value(expr, src, what) {
        return Ok(format!("EmittedDefault::Value({value})"));
    }
    if !function_default_const_shape(expr) || !function_default_has_runtime_read(expr) {
        return Err(EmitError::unsupported(what).at(expr.span));
    }
    let captures = function_default_captures(expr, locals, src)?;
    let mut scope = Vec::with_capacity(captures.len());
    push_capture_locals(&captures, locals, &mut scope).map_err(|e| e.at(expr.span))?;
    let expr_rs = emit_expr(expr, src, aliases, &scope, false)?;
    let snapshot: String = captures
        .iter()
        .map(|c| format!("let __defcap{m} = {m}.clone(); ", m = mangle(c)))
        .collect();
    let percall: String = captures
        .iter()
        .map(|c| format!("let {m} = __defcap{m}.clone(); ", m = mangle(c)))
        .collect();
    Ok(format!(
        "{{ {snapshot}EmittedDefault::Thunk(Rc::new(move |cx: RtCx| -> CallFuture {{ {percall}Box::pin(async move {{ let _ = &cx; Ok({expr_rs}) }}) }})) }}"
    ))
}

pub(crate) fn function_default_const_shape(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Int
        | ExprKind::Float
        | ExprKind::Bool(_)
        | ExprKind::Null
        | ExprKind::Unit
        | ExprKind::Ident => true,
        ExprKind::String(lit) => {
            lit.tag.is_none()
                && lit
                    .parts
                    .iter()
                    .all(|part| matches!(part, StringPart::Text(_)))
        }
        ExprKind::Paren(inner) => function_default_const_shape(inner),
        ExprKind::Unary { operand, .. } => function_default_const_shape(operand),
        ExprKind::Binary { op, lhs, rhs }
            if !matches!(op, BinaryOp::And | BinaryOp::Or | BinaryOp::Coalesce) =>
        {
            function_default_const_shape(lhs) && function_default_const_shape(rhs)
        }
        _ => false,
    }
}

pub(crate) fn function_default_has_runtime_read(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Ident => true,
        ExprKind::Paren(inner) => function_default_has_runtime_read(inner),
        ExprKind::Unary { operand, .. } => function_default_has_runtime_read(operand),
        ExprKind::Binary { lhs, rhs, .. } => {
            function_default_has_runtime_read(lhs) || function_default_has_runtime_read(rhs)
        }
        _ => false,
    }
}
