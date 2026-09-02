use super::*;

impl<'a> Machine<'a> {
    /// Drive the machine until the frame stack drains. The pending
    /// value stack's top (or Unit) is the result.
    pub(super) fn run_to_completion(&mut self) -> RunResult {
        while let Some(frame) = self.frames.pop() {
            self.step(frame)?;
        }
        Ok(self.values.pop().unwrap_or(Value::Unit))
    }

    /// Schedule an unwind; the machine processes it frame by frame
    /// so §14 deferred actions can run mid-unwind.
    pub(super) fn start_unwind(&mut self, action: UnwindAction) {
        self.frames.push(Frame::KUnwind(action));
    }

    /// Enter one iteration of a `loop` expression's body. Pushes the
    /// `LoopExprBody` boundary (so a `break`/`continue` targeting this loop
    /// unwinds here), opens a fresh per-iteration child scope (drained on exit),
    /// then runs the body statements with its tail value DISCARDED — the body's
    /// own value never escapes; only a `break <value>` yields the loop's result.
    /// Mirrors `KWhile`'s body launch. Used both on first entry (from `eval_expr`)
    /// and on each re-entry (normal completion or `continue`).
    pub(super) fn enter_loop_expr_body(
        &mut self,
        body: Rc<Block>,
        label: Option<String>,
        span: Span,
    ) {
        self.frames.push(Frame::LoopExprBody {
            body: body.clone(),
            label,
            span,
            vstack: self.values.len(),
        });
        let saved = self.env.clone();
        self.env = child_env(&saved);
        self.frames.push(Frame::PopScope(saved));
        // The body is a STATEMENT context here (its value is discarded); only a
        // `break <value>` contributes the loop's result.
        self.frames.push(Frame::KDiscard);
        self.frames.push(Frame::KBlock {
            block: body,
            idx: 0,
        });
    }

    /// Drain the current scope's deferred actions LIFO, each in a
    /// contained sub-run (§13/§14): a fault, guard, or escaping
    /// control flow from a deferred action goes to
    /// `Host::defer_error` and never replaces the in-flight result.
    pub(super) fn drain_defers(&mut self) {
        // The scope being exited; each deferred action drains from it
        // and runs against it. A faulted sub-run can strand a child
        // scope (a block, or a §11 placeholder stage that did not
        // reach its `PopScope`), so the draining scope is restored
        // after every sub-run.
        let scope = self.env.clone();
        loop {
            let next = scope.borrow_mut().defers.pop();
            let Some(action) = next else { break };
            let expr = match action {
                DeferredAction::Expr(expr) => expr,
                DeferredAction::CloseResource { value, span } => {
                    if let Err(e) =
                        call_resource_method(self.host, value, "close", Vec::new(), span, span)
                    {
                        self.host.defer_error(&format!("{}: {}", e.code, e.message));
                    }
                    continue;
                }
            };
            // §4 a deferred sub-run is SELF-CONTAINED. One context swap parks every
            // ambient execution field and installs independent stacks while preserving
            // inherited lexical and call authority.
            let mut sub_execution = self.contained_execution(vec![Frame::Eval(expr)]);
            self.swap_execution(&mut sub_execution);
            let mut failed: Option<RtError> = None;
            while let Some(frame) = self.frames.pop() {
                if let Err(e) = self.step(frame) {
                    failed = Some(e);
                    break;
                }
            }
            let produced = self.values.pop();
            self.swap_execution(&mut sub_execution);
            match (failed, produced) {
                (Some(e), _) => self.host.defer_error(&format!("{}: {}", e.code, e.message)),
                (None, Some(Value::Err(e))) => self.host.defer_error(&render(&e)),
                _ => {}
            }
        }
    }

    pub(super) fn step_unwind(&mut self, action: UnwindAction) -> Result<(), RtError> {
        while let Some(frame) = self.frames.pop() {
            match frame {
                Frame::PopScope(saved) => {
                    // Run this scope's defers before it closes.
                    self.drain_defers();
                    self.env = saved;
                }
                Frame::CallBoundary {
                    saved,
                    vstack,
                    saved_src,
                    saved_type_params,
                    return_guard,
                } => {
                    let return_value = match &action {
                        UnwindAction::Return { value, .. } => Some(value),
                        UnwindAction::Break { .. } | UnwindAction::Continue { .. } => None,
                    };
                    self.exit_call_boundary(
                        saved,
                        saved_src,
                        saved_type_params,
                        return_guard,
                        return_value,
                    )?;
                    match &action {
                        UnwindAction::Return { value, .. } => {
                            self.values.truncate(vstack);
                            self.values.push(value.clone());
                            return Ok(());
                        }
                        UnwindAction::Break { span, .. } | UnwindAction::Continue { span, .. } => {
                            return Err(fault(
                                codes::GUARD_TYPE,
                                "`break`/`continue` cannot cross a function boundary",
                                *span,
                            ));
                        }
                    }
                }
                // A `while` loop boundary — UNLABELED. It catches only an
                // unlabeled `break`/`continue`; a LABELED one targets a named
                // enclosing `loop`, so it must PASS THROUGH here (the labeled
                // loop's `LoopExprBody` frame is further out). A value-break's
                // value is discarded by a `while` (it has no value).
                Frame::LoopBody {
                    cond,
                    body,
                    span,
                    vstack,
                } => match &action {
                    UnwindAction::Return { .. } => continue,
                    UnwindAction::Break { label: Some(_), .. }
                    | UnwindAction::Continue { label: Some(_), .. } => continue,
                    UnwindAction::Break { .. } => {
                        self.values.truncate(vstack);
                        return Ok(());
                    }
                    UnwindAction::Continue { .. } => {
                        self.values.truncate(vstack);
                        self.frames.push(Frame::KWhile {
                            cond: cond.clone(),
                            body,
                            span,
                        });
                        self.frames.push(Frame::Eval(cond));
                        return Ok(());
                    }
                },
                // A `for` loop boundary — UNLABELED, same pass-through rule as
                // `while` for a labeled break/continue.
                Frame::ForBody {
                    pattern,
                    body,
                    items,
                    next,
                    acc,
                    span,
                    vstack,
                    is_stmt,
                } => match &action {
                    UnwindAction::Return { .. } => continue,
                    UnwindAction::Break { label: Some(_), .. }
                    | UnwindAction::Continue { label: Some(_), .. } => continue,
                    UnwindAction::Break { span: bspan, .. }
                    | UnwindAction::Continue { span: bspan, .. }
                        if !is_stmt =>
                    {
                        // §5: break/continue may not target a
                        // value-collecting `for` (checker-era static
                        // error; dynamic guard here).
                        return Err(fault(
                            codes::GUARD_TYPE,
                            "`break`/`continue` cannot target a value-collecting `for` (§5)",
                            *bspan,
                        ));
                    }
                    UnwindAction::Break { .. } => {
                        self.values.truncate(vstack);
                        self.values.push(Value::array(acc));
                        return Ok(());
                    }
                    UnwindAction::Continue { .. } => {
                        self.values.truncate(vstack);
                        self.frames.push(Frame::KForNext {
                            pattern,
                            body,
                            items,
                            next,
                            acc,
                            span,
                            is_stmt,
                        });
                        return Ok(());
                    }
                },
                // A `loop` expression boundary may be labeled. It
                // catches a `break`/`continue` whose label is `None` (nearest) or
                // matches this loop's own label. A labeled control statement whose
                // label does NOT match passes through to an outer loop.
                Frame::LoopExprBody {
                    body,
                    label,
                    span,
                    vstack,
                } => match &action {
                    UnwindAction::Return { .. } => continue,
                    UnwindAction::Break {
                        label: blabel,
                        value,
                        ..
                    } => {
                        if blabel.is_some() && *blabel != label {
                            continue; // targets an outer labeled loop
                        }
                        // The loop expression's value IS the break value.
                        self.values.truncate(vstack);
                        self.values.push(value.clone());
                        return Ok(());
                    }
                    UnwindAction::Continue { label: clabel, .. } => {
                        if clabel.is_some() && *clabel != label {
                            continue; // targets an outer labeled loop
                        }
                        // Re-enter the loop body (next iteration).
                        self.values.truncate(vstack);
                        self.enter_loop_expr_body(body, label, span);
                        return Ok(());
                    }
                },
                // §6.4 a comprehension being unwound past (a `?` propagating an `Err`,
                // or a `return`, inside the body): DROP its in-progress accumulator so
                // it does not leak into an enclosing comprehension, then keep unwinding.
                // (A `break`/`continue` cannot target a comprehension — its body is an
                // expression — so it always passes through, dropping the accumulator.)
                Frame::KCompFinish { .. } => {
                    self.comp_accs.pop();
                    continue;
                }
                Frame::KRecordDefaultExit => {
                    self.record_default_depth = self.record_default_depth.saturating_sub(1);
                    continue;
                }
                Frame::RestoreSource(saved_src) => {
                    self.src = saved_src;
                    continue;
                }
                Frame::RestoreEnv(saved_env) => {
                    self.env = saved_env;
                    continue;
                }
                _ => {}
            }
        }
        match action {
            UnwindAction::Return { span, .. } => Err(fault(
                codes::GUARD_TYPE,
                "`return` outside a function",
                span,
            )),
            UnwindAction::Break { span, label, .. } => Err(fault(
                codes::GUARD_TYPE,
                match label {
                    Some(l) => format!("no loop labeled `'{l}` in scope"),
                    None => "`break` outside a loop".to_string(),
                },
                span,
            )),
            UnwindAction::Continue { span, label } => Err(fault(
                codes::GUARD_TYPE,
                match label {
                    Some(l) => format!("no loop labeled `'{l}` in scope"),
                    None => "`continue` outside a loop".to_string(),
                },
                span,
            )),
        }
    }

    pub(super) fn exit_call_boundary(
        &mut self,
        saved_env: EnvRef,
        saved_src: Rc<str>,
        saved_type_params: Rc<[Ident]>,
        return_guard: Option<(Type, Rc<str>)>,
        return_value: Option<&Value>,
    ) -> Result<(), RtError> {
        // The function-body scope exits here: defers run inside the callee and at
        // its call depth, matching the emitted `CallDepthGuard` lifetime.
        self.drain_defers();
        self.call_depth = self.call_depth.saturating_sub(1);
        self.type_params = saved_type_params;

        // §6: tail completion, explicit `return`, `?` propagation, and
        // case-arm return all cross one guard. Control-flow faults pass `None`.
        let guarded = match (return_guard.as_ref(), return_value) {
            (Some((return_type, callee_src)), Some(value)) => self
                .value_matches_type(return_type, callee_src, value, return_type.span)
                .and_then(|matches| {
                    if matches {
                        Ok(())
                    } else {
                        Err(fault(
                            codes::GUARD_TYPE,
                            "return value does not match the declared type (§6)",
                            return_type.span,
                        ))
                    }
                }),
            _ => Ok(()),
        };

        // Every boundary exit, including a return-guard failure or illegal
        // break/continue crossing, restores the caller through this one exit.
        self.env = saved_env;
        self.src = saved_src;
        guarded
    }

    pub(super) fn step(&mut self, frame: Frame) -> Result<(), RtError> {
        match frame.family() {
            FrameFamily::Lifecycle => self.step_lifecycle_frame(frame),
            FrameFamily::Value => self.step_value_frame(frame),
            FrameFamily::Aggregate => self.step_aggregate_frame(frame),
            FrameFamily::AccessAndCall => self.step_access_and_call_frame(frame),
            FrameFamily::HigherOrder => self.step_higher_order_frame(frame),
            FrameFamily::CallBoundary => self.step_call_boundary_frame(frame),
            FrameFamily::PatternControl => self.step_pattern_control_frame(frame),
            FrameFamily::PipeAndDecode => self.step_pipe_and_decode_frame(frame),
        }
    }

    pub(super) fn step_lifecycle_frame(&mut self, frame: Frame) -> Result<(), RtError> {
        match frame {
            Frame::ExecBlockStatement { block, idx } => self.exec_stmt(&block.stmts[idx]),
            Frame::Eval(expr) => self.eval_expr(&expr),
            Frame::RestoreSource(saved_src) => {
                self.src = saved_src;
                Ok(())
            }
            Frame::RestoreEnv(saved_env) => {
                self.env = saved_env;
                Ok(())
            }
            Frame::PopScope(saved) => {
                self.drain_defers();
                self.env = saved;
                Ok(())
            }
            Frame::KUnwind(action) => self.step_unwind(action),
            Frame::KConcurrent(state) => self.step_concurrent(state),

            Frame::KDiscard => {
                self.values.pop();
                Ok(())
            }
            Frame::KBlock { block, idx } => {
                if idx < block.stmts.len() {
                    self.frames.push(Frame::KBlock {
                        block: block.clone(),
                        idx: idx + 1,
                    });
                    self.frames.push(Frame::ExecBlockStatement { block, idx });
                } else if let Some(tail) = &block.tail {
                    self.frames.push(Frame::Eval(tail.clone()));
                } else {
                    self.values.push(Value::Unit);
                }
                Ok(())
            }
            Frame::KLet { name, mutable } => {
                let value = self.values.pop().expect("let value");
                let span = name.span;
                self.bind_checked(self.text(span).to_string(), value, mutable, span)
            }
            Frame::KLetPattern {
                pattern,
                mutable,
                span,
            } => {
                let value = self.values.pop().expect("let value");
                match self.match_pattern(&pattern, &value, span)? {
                    Some(binds) => {
                        for (name, v) in binds {
                            self.bind_checked(name, v, mutable, span)?;
                        }
                        Ok(())
                    }
                    // Irrefutability is checker-era (§4); a refuted
                    // `let` is a dynamic guard at runtime.
                    None => Err(fault(
                        codes::GUARD_TYPE,
                        "`let` pattern did not match the value (§4)",
                        span,
                    )),
                }
            }
            Frame::KUsingBind {
                name,
                body,
                saved,
                span,
            } => {
                let value = self.values.pop().expect("using value");
                if !matches!(value, Value::Resource(_)) {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!("`using` expects a `File`, found `{}`", value.kind()),
                        span,
                    ));
                }
                self.env = child_env(&saved);
                self.bind_checked(
                    self.text(name.span).to_string(),
                    value.clone(),
                    false,
                    name.span,
                )?;
                self.env
                    .borrow_mut()
                    .defers
                    .push(DeferredAction::CloseResource { value, span });
                self.frames.push(Frame::KDiscard);
                self.frames.push(Frame::PopScope(saved));
                self.frames.push(Frame::KBlock {
                    block: body,
                    idx: 0,
                });
                Ok(())
            }
            Frame::KAssign {
                target,
                op,
                span,
                current,
            } => self.apply_assign(&target, op, span, current),
            Frame::KRecordPathAssign {
                root,
                fields,
                op,
                span,
                current,
            } => self.apply_record_path(&root, &fields, op, span, current),
            Frame::KCellPathAssign {
                fields,
                op,
                value,
                span,
            } => {
                let index = self.values.pop().expect("cell index");
                let base = self.values.pop().expect("cell base");
                // The target reference resolves here, before the RHS
                // evaluates (§2 read-operation-write; §13a bounds at
                // reference time).
                let current = match op {
                    AssignOp::Coalesce => {
                        let cur = self.read_cell_path(&base, &index, &fields, span)?;
                        if !matches!(cur, Value::Null | Value::None) {
                            return Ok(());
                        }
                        None
                    }
                    AssignOp::Assign => {
                        index_slot(&base, &index, span)?;
                        None
                    }
                    _ => Some(self.read_cell_path(&base, &index, &fields, span)?),
                };
                self.frames.push(Frame::KCellWrite {
                    base,
                    index,
                    fields,
                    op,
                    current,
                    span,
                });
                self.frames.push(Frame::Eval(value));
                Ok(())
            }
            Frame::KCellWrite {
                base,
                index,
                fields,
                op,
                current,
                span,
            } => {
                let mut value = self.values.pop().expect("assign value");
                if let Some(old) = current {
                    value = apply_op(&old, value, op, span)?;
                }
                self.apply_cell_path(base, index, &fields, value, span)
            }
            _ => unreachable!("frame family changed after classification"),
        }
    }

    /// deadline is checked at every round boundary; a fault observed
    /// while the deadline has not passed faults the whole
    /// expression; expiry runs `else` and abandons the rest.
    pub(super) fn start_concurrent(
        &mut self,
        timeout: Option<&Expr>,
        arms: &[ConcurrentArm],
        else_block: Option<Rc<Block>>,
        span: Span,
    ) -> Result<(), RtError> {
        let deadline = match timeout {
            None => None,
            Some(expr) => match &expr.kind {
                ExprKind::Duration(_) => {
                    let text = self.text(expr.span);
                    let ms = parse_duration_milliseconds(text).ok_or_else(|| {
                        fault(
                            codes::GUARD_TYPE,
                            "`concurrent` timeout duration must fit in u64 milliseconds (§15)",
                            expr.span,
                        )
                    })?;
                    Some(self.host.now_millis().saturating_add(ms))
                }
                _ => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        "`concurrent` timeout takes a duration literal (§15)",
                        expr.span,
                    ));
                }
            },
        };
        let arms = arms
            .iter()
            .map(|arm| ArmRun {
                name: self.text(arm.name.span).to_string(),
                execution: self.contained_execution(vec![Frame::Eval(arm.value.clone())]),
                done: None,
            })
            .collect();
        self.frames
            .push(Frame::KConcurrent(Box::new(ConcurrentState {
                arms,
                deadline,
                else_block,
            })));
        let _ = span;
        Ok(())
    }

    pub(super) fn step_concurrent(
        &mut self,
        mut state: Box<ConcurrentState>,
    ) -> Result<(), RtError> {
        // One quantum per pending arm, textual order; after EVERY
        // quantum check (a) arm faults — observable only while the
        // deadline has not passed — then (b) the deadline (expired →
        // `else`, remaining arms abandoned). CDR-003 §7.
        for i in 0..state.arms.len() {
            {
                let arm = &mut state.arms[i];
                if arm.done.is_some() {
                    continue;
                }
                // Every arm-local field crosses the suspension boundary together.
                self.swap_execution(&mut arm.execution);
            }
            let mut result: Result<(), RtError> = Ok(());
            for _ in 0..STEP_QUANTUM {
                let Some(frame) = self.frames.pop() else {
                    break;
                };
                result = self.step(frame);
                if result.is_err() {
                    break;
                }
            }
            let finished = result.is_ok() && self.frames.is_empty();
            let arm_value = if finished { self.values.pop() } else { None };
            {
                let arm = &mut state.arms[i];
                self.swap_execution(&mut arm.execution);
            }
            let expired = state
                .deadline
                .is_some_and(|deadline| self.host.now_millis() >= deadline);
            // (a) fault check: pre-deadline faults propagate; a
            // post-deadline fault belongs to abandoned work and is
            // unobserved (§15).
            if let Err(e) = result {
                if !expired {
                    return Err(e);
                }
            } else if finished {
                state.arms[i].done = Some(arm_value.unwrap_or(Value::Unit));
            }
            // (b) deadline check: expiry runs `else` and abandons
            // the remaining arms.
            if expired && state.arms.iter().any(|a| a.done.is_none()) {
                let else_block = state.else_block.expect("timeout form has else");
                self.push_block(else_block);
                return Ok(());
            }
        }
        if state.arms.iter().all(|a| a.done.is_some()) {
            // §15: the result record preserves arm names.
            let record: BTreeMap<String, Value> = state
                .arms
                .iter_mut()
                .map(|a| (a.name.clone(), a.done.take().expect("done")))
                .collect();
            self.values.push(Value::Record(Rc::new(record)));
            return Ok(());
        }
        self.frames.push(Frame::KConcurrent(state));
        Ok(())
    }

    pub(super) fn push_block(&mut self, block: Rc<Block>) {
        let saved = self.env.clone();
        self.env = child_env(&saved);
        self.collect_block_aliases(&block);
        self.frames.push(Frame::PopScope(saved));
        self.frames.push(Frame::KBlock { block, idx: 0 });
    }

    /// Pre-collects a block's `type` declarations into the current
    /// scope so §6 conformance resolves forward references, exactly
    /// like the checker's alias frames.
    pub(super) fn collect_block_aliases(&mut self, block: &Block) {
        for stmt in &block.stmts {
            if let StmtKind::TypeAlias(alias) = &stmt.kind {
                let name = self.text(alias.name.span);
                self.env.borrow_mut().aliases.insert(
                    name.to_string(),
                    (alias.type_params.as_slice().into(), alias.ty.clone()),
                );
            }
        }
    }

    pub(super) fn exec_stmt(&mut self, stmt: &Stmt) -> Result<(), RtError> {
        match &stmt.kind {
            StmtKind::Let {
                mutable,
                pattern,
                value,
                ..
            } => {
                self.frames.push(Frame::KLetPattern {
                    pattern: pattern.clone(),
                    mutable: *mutable,
                    span: stmt.span,
                });
                self.eval_expr(value)
            }
            StmtKind::Using { name, value, body } => {
                let saved = self.env.clone();
                self.frames.push(Frame::KUsingBind {
                    name: *name,
                    body: body.clone(),
                    saved,
                    span: stmt.span,
                });
                self.eval_expr(value)
            }
            StmtKind::Const { name, value, .. } => {
                // Top-level consts were bound by the load-time const
                // pass; block-local consts evaluate here (their
                // const-expression restriction is checker-era).
                if self.env.borrow().vars.contains_key(self.text(name.span)) {
                    return Ok(());
                }
                self.frames.push(Frame::KLet {
                    name: *name,
                    mutable: false,
                });
                self.eval_expr(value)
            }
            StmtKind::Function(decl) => {
                let function_name = self.text(decl.name.span).to_string();
                let closure = if self.current_module.is_extern {
                    let module = self
                        .current_module
                        .identity
                        .declaration_or_entry()
                        .to_string();
                    let params = decl
                        .params
                        .iter()
                        .map(|p| self.text(p.name.span).to_string())
                        .collect();
                    Value::Closure(Rc::new(ExternFunction::from_strings(
                        module,
                        function_name.clone(),
                        params,
                        decl.name.span,
                    )))
                } else {
                    Value::Closure(Rc::new(ClosureData {
                        name: Some(function_name.clone()),
                        params: ClosureParams::Declared(Rc::from(decl.params.as_slice())),
                        body: ClosureBody::Block(decl.body.clone()),
                        env: self.env.clone(),
                        src: self.src.clone(),
                        type_params: Rc::from(decl.type_params.as_slice()),
                        return_type: decl.return_type.clone(),
                    }))
                };
                self.bind_checked(function_name, closure, false, decl.name.span)
            }
            // Type/enum/record/newtype/impl/protocol decls are registered at program
            // load (not here); they are runtime no-ops in statement position.
            StmtKind::TypeAlias(_)
            | StmtKind::Enum(_)
            | StmtKind::Record(_)
            | StmtKind::Newtype(_)
            | StmtKind::Impl(_)
            | StmtKind::Protocol(_) => Ok(()),
            StmtKind::Assign { target, op, value } => {
                if !matches!(target.kind, ExprKind::Ident) {
                    // §4/§5 member and index assignment targets.
                    return self.schedule_path_assign(
                        target.clone(),
                        *op,
                        value.clone(),
                        stmt.span,
                    );
                }
                if *op == AssignOp::Coalesce {
                    // §12 ??=: the target is evaluated once; the RHS
                    // is evaluated only when the target is None/null,
                    // and no write happens otherwise.
                    let name = self.text(target.span);
                    let current = lookup(&self.env, name).ok_or_else(|| {
                        fault(
                            codes::GUARD_UNBOUND,
                            format!("`{name}` is not bound"),
                            stmt.span,
                        )
                    })?;
                    if !is_mutable(&self.env, name) {
                        return Err(fault(
                            codes::GUARD_IMMUTABLE,
                            format!("`{name}` is not `let mut` and cannot be assigned"),
                            stmt.span,
                        ));
                    }
                    if matches!(current, Value::Null | Value::None) {
                        self.frames.push(Frame::KAssign {
                            target: target.clone(),
                            op: AssignOp::Assign,
                            span: stmt.span,
                            current: None,
                        });
                        self.eval_expr(value)?;
                    }
                    return Ok(());
                }
                // §2: a compound op reads the target BEFORE the RHS
                // evaluates (read-operation-write, left to right).
                let current = if *op == AssignOp::Assign {
                    None
                } else {
                    // The read itself fails here, before the RHS.
                    let name = self.text(target.span);
                    Some(lookup(&self.env, name).ok_or_else(|| {
                        fault(
                            codes::GUARD_UNBOUND,
                            format!("`{name}` is not bound"),
                            stmt.span,
                        )
                    })?)
                };
                self.frames.push(Frame::KAssign {
                    target: target.clone(),
                    op: *op,
                    span: stmt.span,
                    current,
                });
                self.eval_expr(value)
            }
            StmtKind::Return(value) => {
                self.frames.push(Frame::KReturn { span: stmt.span });
                match value {
                    Some(e) => self.eval_expr(e)?,
                    None => self.values.push(Value::Unit),
                }
                Ok(())
            }
            StmtKind::While { cond, body } => {
                self.frames.push(Frame::KWhile {
                    cond: cond.clone(),
                    body: body.clone(),
                    span: stmt.span,
                });
                self.eval_expr(cond)
            }
            // `break (label)? (value)?`: evaluate the value (if any)
            // FIRST, then unwind to the target loop carrying it; a value-less
            // `break` carries `Value::Unit`. The label NAME resolves against the
            // source text now (it can never cross a function boundary).
            StmtKind::Break { label, value } => {
                let label = label.map(|l| self.text(l.span).to_string());
                match value {
                    Some(e) => {
                        self.frames.push(Frame::KBreak {
                            span: stmt.span,
                            label,
                        });
                        self.eval_expr(e)?;
                    }
                    None => {
                        self.start_unwind(UnwindAction::Break {
                            span: stmt.span,
                            label,
                            value: Value::Unit,
                        });
                    }
                }
                Ok(())
            }
            StmtKind::Continue { label } => {
                let label = label.map(|l| self.text(l.span).to_string());
                self.start_unwind(UnwindAction::Continue {
                    span: stmt.span,
                    label,
                });
                Ok(())
            }
            StmtKind::Expr(expr) => {
                self.frames.push(Frame::KDiscard);
                if let ExprKind::For {
                    pattern,
                    iter,
                    body,
                } = &expr.kind
                {
                    // §5: statement-position `for` may break/continue;
                    // the value-collecting expression form may not.
                    self.frames.push(Frame::KForStart {
                        pattern: pattern.clone(),
                        body: body.clone(),
                        span: expr.span,
                        is_stmt: true,
                    });
                    self.frames.push(Frame::Eval(iter.clone()));
                } else {
                    self.eval_expr(expr)?;
                }
                Ok(())
            }
            StmtKind::Defer(action) => {
                // §14: belongs to the innermost lexical scope.
                self.env
                    .borrow_mut()
                    .defers
                    .push(DeferredAction::Expr(action.clone()));
                Ok(())
            }
            StmtKind::Import(item) => self.exec_import(item, stmt.span),
            StmtKind::Export(inner) => {
                // §17: a zero-runtime wrapper; record the exported
                // names of the current module, then run the inner
                // declaration exactly as if unexported.
                if !self.current_module.identity.declaration.is_empty() {
                    let name = match &inner.kind {
                        StmtKind::Function(decl) => Some(self.text(decl.name.span).to_string()),
                        StmtKind::Const { name, .. } => Some(self.text(name.span).to_string()),
                        StmtKind::TypeAlias(alias) => Some(self.text(alias.name.span).to_string()),
                        StmtKind::Record(decl) => Some(self.text(decl.name.span).to_string()),
                        StmtKind::Enum(decl) => Some(self.text(decl.name.span).to_string()),
                        StmtKind::Newtype(decl) => Some(self.text(decl.name.span).to_string()),
                        StmtKind::Let { pattern, .. } => match &pattern.kind {
                            PatternKind::Binding(name) | PatternKind::Typed { name, .. } => {
                                Some(self.text(name.span).to_string())
                            }
                            _ => None, // resolver-rejected
                        },
                        _ => None,
                    };
                    if let Some(name) = name {
                        self.module_scopes
                            .get_mut(self.current_module.identity.declaration.as_ref())
                            .expect("module scope initialized")
                            .runtime
                            .exports
                            .insert(name);
                    }
                }
                self.exec_stmt(inner)
            }
        }
    }
}
