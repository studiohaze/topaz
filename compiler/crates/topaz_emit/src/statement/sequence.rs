use crate::*;

/// Split a statement list into its non-tail statements and the optional
/// tail expression: the final statement is the program/block value iff
/// it is a bare expression statement (CDR-003 §5/§1a).
pub(crate) fn split_tail(items: &[Stmt]) -> (&[Stmt], Option<&Expr>) {
    if let Some((last, rest)) = items.split_last()
        && let StmtKind::Expr(expr) = &last.kind
    {
        return (rest, Some(expr));
    }
    (items, None)
}

/// Recursion-cell planning for a statement sequence (CDR-006 §7). A top-level
/// `function f` lowers to a closure binding, so `f` is not in scope while its
/// own body is emitted — a SELF reference, or a FORWARD reference to a
/// later-declared sibling, is otherwise a free identifier. The fix mirrors the
/// interpreter's closure↔env `Rc` cycle: such a function is seeded as a
/// `cell_new(Value::Unit)` cell BEFORE the bodies, then `cell_set` to its
/// closure, so the body reaches it by name through `cell_get`.
///
/// This historical cluster analysis remains as a compatibility backstop. The
/// current positional pass pre-seeds every function in module and non-module
/// statement scopes as a `Bind::TopFnCell`, including references that cross a
/// non-function statement. Consequently the cluster seed below normally sees
/// that same-scope top cell and emits no legacy `ImmCell`; declaration-time
/// `top_cell_set` owns the observable timing and missing-aware TPZ5002 fault.
///
/// Returns `(clusters, celled)`: `clusters` maps each cluster's FIRST-statement
/// index to its celled function names (in declaration order, to seed before the
/// bodies); `celled` is the set of STATEMENT INDICES of celled function
/// declarations (NOT names — a name keyed globally would misclassify a
/// same-name redeclaration in a DIFFERENT cluster as a recursion-cell fill,
/// skipping the redeclaration fault). The `Function` arm dispatches on its own
/// statement index.
pub(crate) fn recursion_cells(
    stmts: &[Stmt],
    src: &LoweredText,
) -> (Vec<(usize, Vec<String>)>, Vec<usize>) {
    let mut clusters: Vec<(usize, Vec<String>)> = Vec::new();
    let mut celled_indices: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < stmts.len() {
        if !matches!(stmts[i].kind, StmtKind::Function(_)) {
            i += 1;
            continue;
        }
        // Gather the consecutive-function cluster starting at `i`.
        let start = i;
        let mut names: Vec<&str> = Vec::new();
        let mut bodies: Vec<(&Block, Vec<&str>)> = Vec::new();
        while i < stmts.len() {
            if let StmtKind::Function(decl) = &stmts[i].kind {
                names.push(text(src, decl.name.span));
                let params: Vec<&str> =
                    decl.params.iter().map(|p| text(src, p.name.span)).collect();
                bodies.push((&decl.body, params));
                i += 1;
            } else {
                break;
            }
        }
        // A duplicate name in one cluster is a same-scope redeclaration the
        // normal `Function` arm must still reject — skip celling so it does.
        let mut deduped = names.clone();
        deduped.sort_unstable();
        deduped.dedup();
        if deduped.len() != names.len() {
            continue;
        }
        // `f` needs a cell iff referenced (in body) by a function at or before
        // its own position — self (same position) or forward (earlier position
        // references a later `f`). A backward reference already resolves via the
        // earlier declaration's normal binding. Imprecise shadowing only ever
        // OVER- or UNDER-counts into an over-refusal, never a miscompile.
        let mut celled: Vec<&str> = Vec::new();
        for (pos, (body, params)) in bodies.iter().enumerate() {
            let mut referenced: Vec<&str> = Vec::new();
            let mut bound: Vec<&str> = Vec::new();
            collect_block_idents(body, src, &mut referenced, &mut bound);
            for r in referenced {
                if bound.contains(&r) || params.contains(&r) {
                    continue;
                }
                if let Some(idx) = names.iter().position(|n| *n == r)
                    && idx >= pos
                    && !celled.contains(&r)
                {
                    celled.push(r);
                }
            }
        }
        if !celled.is_empty() {
            let ordered: Vec<String> = names
                .iter()
                .filter(|n| celled.contains(*n))
                .map(|n| n.to_string())
                .collect();
            // A celled member at cluster position `pos` is statement `start + pos`.
            for (pos, name) in names.iter().enumerate() {
                if celled.contains(name) {
                    celled_indices.push(start + pos);
                }
            }
            clusters.push((start, ordered));
        }
    }
    (clusters, celled_indices)
}

impl StatementDeferFlow {
    pub(crate) fn enter(
        stmts: &[Stmt],
        defer_scope: bool,
        aliases: &Aliases<'_, '_>,
        lines: &mut String,
    ) -> Self {
        let block_has_defer = stmts.iter().any(stmt_registers_defer);
        let saved = if defer_scope {
            let mut flow = aliases.flow.borrow_mut();
            let saved = (
                std::mem::take(&mut flow.stacks),
                std::mem::take(&mut flow.loop_markers),
                flow.fn_base,
            );
            if block_has_defer {
                flow.stacks.push("__defers".to_string());
            }
            flow.fn_base = flow.stacks.len();
            Some(saved)
        } else {
            None
        };
        let block_stack = if !defer_scope && block_has_defer {
            let name = {
                let mut flow = aliases.flow.borrow_mut();
                flow.next_id += 1;
                format!("__defers_b{}", flow.next_id)
            };
            lines.push_str(&format!("    let {name} = defer_stack();\n"));
            aliases.flow.borrow_mut().stacks.push(name.clone());
            Some(name)
        } else {
            None
        };
        Self {
            defer_scope,
            block_has_defer,
            saved,
            block_stack,
        }
    }

    pub(crate) fn finish(
        self,
        aliases: &Aliases<'_, '_>,
        lines: &mut String,
        result: String,
        terminal_transfer: bool,
    ) -> String {
        let result = match (&self.block_stack, terminal_transfer) {
            (_, true) => result,
            (Some(stack), false) => {
                let value = stack.replace("__defers", "__block_ret");
                lines.push_str(&format!("    let {value} = {result};\n"));
                lines.push_str(&format!("    run_defers(&{stack}, &cx).await;\n"));
                value
            }
            (None, false) => result,
        };
        let mut flow = aliases.flow.borrow_mut();
        if self.block_stack.is_some() || (self.defer_scope && self.block_has_defer) {
            flow.stacks.pop();
        }
        if let Some((stacks, markers, function_base)) = self.saved {
            flow.stacks = stacks;
            flow.loop_markers = markers;
            flow.fn_base = function_base;
        }
        result
    }
}

pub(crate) fn emit_expression_statement(
    expr: &Expr,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    locals: &[(String, Bind)],
    in_loop: bool,
) -> Result<String, EmitError> {
    if let ExprKind::For {
        pattern,
        iter,
        body,
    } = &expr.kind
    {
        return emit_for(ForEmission {
            pattern,
            iter,
            body,
            span: expr.span,
            src,
            aliases,
            locals,
            collect: false,
        });
    }
    Ok(format!(
        "let _ = {};",
        emit_expr(expr, src, aliases, locals, in_loop)?
    ))
}

pub(crate) fn emit_const_statement(
    emission: ConstStatementEmission<'_, '_, '_>,
) -> Result<(), EmitError> {
    let ConstStatementEmission {
        stmt,
        name,
        value,
        src,
        aliases,
        locals,
        base,
        captured,
        in_loop,
        lines,
    } = emission;
    let name = text(src, name.span);
    if locals[base..].iter().any(|(local, _)| local == name) {
        return Err(EmitError::unsupported("same-scope redeclaration").at(stmt.span));
    }
    if captured.contains(&name) {
        return Err(EmitError::unsupported("declaration shadows a captured binding").at(stmt.span));
    }
    let value = emit_expr(value, src, aliases, locals, in_loop)?;
    lines.push_str(&format!("    let {} = {value};\n", mangle(name)));
    locals.push((name.to_string(), Bind::Imm));
    Ok(())
}

pub(crate) fn emit_stmt_seq(
    emission: StatementSequenceEmission<'_, '_, '_>,
) -> Result<(String, String), EmitError> {
    let StatementSequenceEmission {
        stmts,
        tail,
        src,
        aliases,
        locals,
        base,
        in_loop,
        defer_scope,
        at_module_top,
    } = emission;
    // `base` is the index in `locals` where THIS scope's own bindings
    // begin (everything before it is enclosing). Usually `locals.len()`
    // at entry, but a `for` body PRE-pushes its loop variable and passes
    // a `base` BEFORE it, so the loop variable shares this scope (a body
    // `let` of the same name is a same-scope redeclaration, not a shadow
    // — the interpreter runs the body in the very env that binds the
    // loop variable).
    let mut lines = String::new();
    let self_runtime_default_cells: HashMap<String, String> = if at_module_top {
        aliases
            .type_ctx
            .module(aliases.identity)
            .map(|module| {
                module
                    .record_defaults
                    .self_runtime_refs
                    .values()
                    .flat_map(|refs| {
                        refs.iter()
                            .map(|(local, cell)| (local.clone(), cell.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    } else {
        HashMap::new()
    };
    let defer_flow = StatementDeferFlow::enter(stmts, defer_scope, aliases, &mut lines);
    if !at_module_top {
        seed_nested_function_cells(NestedFunctionCellSeeding {
            stmts,
            src,
            locals,
            base,
            lines: &mut lines,
        })?;
    }
    // §5 escape analysis: which of THIS scope's `let mut` bindings a closure
    // captures — those become rebinding CELLS (`Rc<RefCell<Value>>`); the rest
    // stay plain `let mut`. Computed once up front (a closure capturing `x`
    // comes after `let mut x`, so the per-statement loop below cannot decide it
    // locally).
    let cells = scope_cell_set(stmts, tail, src, locals);
    // §7 recursion: the legacy consecutive-cluster analysis remains for module-top
    // lowering; nested scopes were pre-seeded above with missing-aware TopFnCells.
    // `rec_celled_idx` are the STATEMENT INDICES of those functions (index-keyed,
    // not name-keyed, so a same-name redeclaration in another cluster still faults).
    let (rec_clusters, rec_celled_idx) = recursion_cells(stmts, src);
    // Enclosing bindings that a closure declared EARLIER in this scope has
    // captured (snapshotted). A later declaration that shadows one of these
    // is refused: the interpreter's whole-env capture would observe the new
    // binding, but the emitted snapshot froze the old value (CDR-006 §4).
    let mut captured: Vec<&str> = Vec::new();
    for (stmt_idx, stmt) in stmts.iter().enumerate() {
        let recursion_names = rec_clusters
            .iter()
            .find(|(index, _)| *index == stmt_idx)
            .map(|(_, names)| names.as_slice());
        seed_recursion_cluster(RecursionClusterSeeding {
            stmt,
            names: recursion_names,
            locals,
            base,
            captured: &captured,
            lines: &mut lines,
        })?;
        match &stmt.kind {
            // A non-tail expression statement is evaluated for its
            // effects and faults (e.g. `1 / 0` must still fault), then
            // discarded — exactly as the interpreter sequences a block.
            // A `for` in STATEMENT position is special: it iterates for
            // effects (value discarded) and MAY `break`/`continue` (§5),
            // unlike a value-collecting `for` in expression position.
            StmtKind::Expr(expr) => {
                let statement = emit_expression_statement(expr, src, aliases, locals, in_loop)?;
                lines.push_str(&format!("    {statement}\n"));
            }
            // §4 `let` binding. The value lowers BEFORE the name is in
            // scope (a self-referential RHS is an unbound free name).
            // `mut` maps to a Rust `let mut`; rebinding is a plain
            // assignment. A binding uses a cell exactly when capture analysis
            // marks it as shared with a closure; other mutable bindings lower
            // to plain Rust `let mut` values.
            StmtKind::Let {
                mutable,
                pattern,
                value,
                ..
            } => emit_let_statement(LetStatementEmission {
                stmt,
                mutable: *mutable,
                pattern,
                value,
                cells: &cells,
                captured: &captured,
                self_runtime_default_cells: &self_runtime_default_cells,
                src,
                aliases,
                locals,
                base,
                in_loop,
                lines: &mut lines,
            })?,
            // v5.4 `using name = expr { body }`: bind a File resource for the body
            // only, push a close thunk onto a dedicated stack, then drain that stack
            // when the body exits normally. Early `return`/`?`/loop-control inside
            // the body sees the stack in `FlowCtx` and drains it before crossing.
            StmtKind::Using { name, value, body } => {
                emit_using_statement(UsingStatementEmission {
                    stmt,
                    name,
                    value,
                    body,
                    src,
                    aliases,
                    locals,
                    in_loop,
                    lines: &mut lines,
                })?;
            }
            // §5 a rebinding `x = e` of a mutable local, or §9 an index-assign
            // `xs[i] = v` into a mutable Array cell. A member or nested target
            // follows the checked path-update rules; an immutable or free root,
            // or an optional in the path, is a static error.
            StmtKind::Assign { target, op, value } => {
                let emission = AssignmentEmission {
                    op: *op,
                    value,
                    span: stmt.span,
                    src,
                    aliases,
                    locals,
                    in_loop,
                };
                lines.push_str(&emit_assignment_statement(target, &emission)?);
            }
            // §5 `while cond { body }` — a STATEMENT (its value is Unit).
            // Lowers to a Rust `while`: the condition is re-tested each
            // iteration through the SHARED `condition_bool` guard (a
            // non-`bool` faults identically, at the WHOLE `while`
            // statement's span — exactly the span the interpreter's
            // KWhile threads), and the body is a fresh scope per
            // iteration whose value is discarded. `break`/`continue`
            // inside lower to Rust's, which target the nearest loop.
            StmtKind::While { cond, body } => {
                let statement = emit_while_statement(stmt, cond, body, src, aliases, locals)?;
                lines.push_str(&format!("    {statement}\n"));
            }
            StmtKind::Break { label, value } => emit_loop_control(LoopControlEmission {
                stmt,
                kind: LoopControlKind::Break(value.as_ref()),
                label: label.as_ref(),
                src,
                aliases,
                locals,
                in_loop,
                lines: &mut lines,
            })?,
            StmtKind::Continue { label } => emit_loop_control(LoopControlEmission {
                stmt,
                kind: LoopControlKind::Continue,
                label: label.as_ref(),
                src,
                aliases,
                locals,
                in_loop,
                lines: &mut lines,
            })?,
            // §4/§7 a top-level/block `function f(params) { body }` lowers
            // to a closure binding (over the SAME `EmittedClosure` ABI as a
            // lambda), then `f(args)` is an indirect call. Concrete,
            // boundary-guardable fixed/variadic param and return types emit §6
            // guards at the closure boundary; TYPE PARAMS `function f<T>(…)` are
            // runtime-erased, so unguardable generic boundaries are skipped.
            // Scalar/unit/plain-string defaults and a trailing variadic parameter
            // are handled below; other defaults remain refused. Self-/mutual
            // recursion and cross-statement forward references are supported through
            // the scope's positional function cells.
            StmtKind::Function(decl) => emit_function_statement(
                decl,
                FunctionStatementEmission {
                    stmt,
                    stmt_idx,
                    src,
                    aliases,
                    locals,
                    base,
                    at_module_top,
                    rec_celled_idx: &rec_celled_idx,
                    captured: &captured,
                    lines: &mut lines,
                },
            )?,
            StmtKind::Return(value) => emit_return_statement(ReturnStatementEmission {
                value: value.as_ref(),
                src,
                aliases,
                locals,
                in_loop,
                lines: &mut lines,
            })?,
            // §6 a `type` alias declaration is a RUNTIME no-op (the interpreter's
            // statement executor returns `Ok(())`; the alias is registered in a
            // separate pre-pass for §6 conformance, not executed). It binds no
            // value and carries no sub-expression, so it emits nothing. (A typed
            // binding `let x: Alias` USING a top-level monomorphic alias resolves:
            // `type_test` expands the alias to its body, exactly as the
            // interpreter's `type_matches`; a generic / poisoned / self-recursive
            // alias is left to the interpreter (TPZ6001). Emitting nothing for the
            // alias DECLARATION itself cannot diverge regardless.)
            // A type/enum/record/newtype DECLARATION emits nothing (a declaration is
            // a runtime no-op — its construction/match emits separately). The decl
            // itself cannot diverge.
            // §4 (v5.4) an `impl` block emits nothing inline — its methods are
            // lowered + registered ONCE at entry start (`emit_entry_body_seeded_inner`
            // builds the method registry). A method call's dispatch reads that
            // registry, so the decl itself is a runtime no-op here.
            // §4 (v5.4) a `protocol` declaration emits nothing — it declares method
            // SIGNATURES (empty bodies). A `Protocol.method(x)` call's dispatch is
            // emitted at the call site; manual-impl method bodies are registered at
            // entry start (the method registry). The decl itself is a runtime no-op.
            StmtKind::Record(decl) if at_module_top => {
                lines.push_str(&emit_record_default_thunk_initializers(
                    decl, src, aliases, locals,
                )?);
            }
            StmtKind::TypeAlias(_)
            | StmtKind::Enum(_)
            | StmtKind::Record(_)
            | StmtKind::Newtype(_)
            | StmtKind::Impl(_)
            | StmtKind::Protocol(_) => {}
            // §17/§6 a DEFENSIVE arm: the entry pipeline now normalizes EVERY top-level
            // `Export(inner)` to its inner stmt (`emit_entry_body_seeded_inner`) and a
            // non-entry module unwraps its exports in `emit_items`, so no `Export` reaches
            // a normally-parsed statement sequence anymore. Should one arrive (a
            // defensively-constructed AST), an exported `type` alias still erases at
            // runtime — no value, no sub-expression — so it emits nothing, exactly like a
            // bare `type` alias. (A non-type `Export` would fall through to the unsupported
            // fallback, but the normalization means it cannot.)
            StmtKind::Export(inner)
                if matches!(
                    &inner.kind,
                    StmtKind::TypeAlias(_)
                        | StmtKind::Enum(_)
                        | StmtKind::Record(_)
                        | StmtKind::Newtype(_)
                ) => {}
            // §17 a prologue `import m` declaration emits NOTHING in the body: the
            // multi-module path (`emit_items`) has already bound `m` to a record of
            // the imported module's exports as a prelude, so `m.foo` is an ordinary
            // member access. (A single-module unit never contains an import — the
            // resolver only admits one there — so this arm is multi-module only.)
            StmtKind::Import(_) => {}
            // §4 a BLOCK-LOCAL `const` binding. The interpreter's statement
            // executor evaluates a block const's initializer NORMALLY (a
            // `Frame::Eval` + `KLet`, like a `let`; the const-expression
            // restriction is checker-era), so it lowers to an immutable `let` IN
            // PLACE — a const value is never a mutable cell and never assignable.
            // (A TOP-LEVEL const is bound at load time by the const pass, so
            // `emit_entry_body` HOISTS it; only block-local consts reach here.)
            StmtKind::Const { name, value, .. } => {
                emit_const_statement(ConstStatementEmission {
                    stmt,
                    name,
                    value,
                    src,
                    aliases,
                    locals,
                    base,
                    captured: &captured,
                    in_loop,
                    lines: &mut lines,
                })?;
            }
            // §14 `defer action`. Lower the action as a zero-arg closure (a `() => action`
            // lambda) so the shared capture machinery (`lambda_captures` + snapshot)
            // carries its free locals, then PUSH it onto the INNERMOST active defer stack
            // AT THIS POSITION — an early exit before here leaves it unregistered, exactly
            // the interpreter's per-scope `defers` push. The stack drains LIFO at its
            // scope's non-fault exit: the function body's `__defers` via the closure
            // wrapper, a nested block's `__defersN` at the block exit / on a crossing
            // early exit. Only an action with an escaping `return`/`?` is refused (below).
            StmtKind::Defer(action) => {
                emit_defer_statement(DeferStatementEmission {
                    stmt,
                    action,
                    src,
                    aliases,
                    locals,
                    lines: &mut lines,
                })?;
            }
            _ => return Err(EmitError::unsupported("statement kind").at(stmt.span)),
        }
        // Record the closures THIS statement creates that snapshot an
        // enclosing binding, so a LATER declaration in this scope shadowing
        // one is refused — the interpreter's whole-env capture would observe
        // the new binding, but the emitter froze the old value (CDR-006 §4).
        stmt_lambda_captures(stmt, src, locals, &mut captured)?;
    }
    // A block whose final statement transfers control has no normal value path.
    // Emitting the usual `Value::Unit` after that statement produces dead Rust
    // (and, for a deferred block, a second unreachable normal-exit drain).  Keep
    // the block genuinely diverging so Rust can coerce it at its use site.
    let terminal_transfer = tail.is_none()
        && stmts.last().is_some_and(|stmt| {
            matches!(
                &stmt.kind,
                StmtKind::Return(_) | StmtKind::Break { .. } | StmtKind::Continue { .. }
            )
        });
    let result = match tail {
        Some(expr) => emit_expr(expr, src, aliases, locals, in_loop)?,
        None if terminal_transfer => String::new(),
        None => "Value::Unit".to_string(),
    };
    let result = defer_flow.finish(aliases, &mut lines, result, terminal_transfer);
    Ok((lines, result))
}
