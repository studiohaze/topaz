use super::*;

impl<'a> Machine<'a> {
    pub(super) fn step_access_and_call_frame(&mut self, frame: Frame) -> Result<(), RtError> {
        match frame {
            Frame::KMember { field, span, root } => {
                let object = self.values.pop().expect("member object");
                let value =
                    self.member_access(object, self.text(field.span), span, root.as_deref())?;
                self.values.push(value);
                Ok(())
            }
            Frame::KOptional { field, span, root } => {
                let object = self.values.pop().expect("optional object");
                // §12: one layer, container preserved.
                match object {
                    Value::None => self.values.push(Value::None),
                    Value::Null => self.values.push(Value::Null),
                    Value::Some(inner) => {
                        let v = self.member_access(
                            (*inner).clone(),
                            self.text(field.span),
                            span,
                            root.as_deref(),
                        )?;
                        self.values.push(wrap_optional(v));
                    }
                    other => {
                        let v = self.member_access(
                            other,
                            self.text(field.span),
                            span,
                            root.as_deref(),
                        )?;
                        self.values.push(v);
                    }
                }
                Ok(())
            }
            Frame::KIndexObj { index, span } => {
                self.frames.push(Frame::KIndexApply { span });
                self.frames.push(Frame::Eval(index));
                Ok(())
            }
            Frame::KIndexApply { span } => {
                let index = self.values.pop().expect("index");
                let object = self.values.pop().expect("indexed object");
                // §1 index through the shared leaf so the bounds / type faults
                // are byte-identical to the emitter.
                let v = index_value(object, index, span)?;
                self.values.push(v);
                Ok(())
            }
            Frame::KCallArgs {
                args,
                idx,
                mut acc,
                mut named,
                mut spread,
                mut seen_spread,
                span,
            } => {
                if idx > 0 {
                    let value = self.values.pop().expect("call arg");
                    match &args[idx - 1] {
                        CallArg::Positional(_) => {
                            if !named.is_empty() {
                                return Err(fault(
                                    codes::GUARD_ARITY,
                                    "positional arguments may not follow named arguments (§5)",
                                    span,
                                ));
                            }
                            if seen_spread {
                                // §5: after a spread, every further
                                // value belongs to the variadic tail
                                // region — even after `...[]`.
                                spread.push(value);
                            } else {
                                acc.push(value);
                            }
                        }
                        CallArg::Spread(e) => {
                            // §5: spread arguments fill only the
                            // variadic tail region — they ride
                            // separately so the callee can enforce
                            // that — and named arguments must come
                            // after them.
                            if !named.is_empty() {
                                return Err(fault(
                                    codes::GUARD_ARITY,
                                    "named arguments must follow spread arguments (§5)",
                                    span,
                                ));
                            }
                            // §5 spread-of-non-array faults through the shared leaf
                            // (the SAME one the emitter's `Array.of`/`Set.of` spread
                            // lowering calls), so the fault cannot drift.
                            call_spread_extend(&mut spread, value, e.span)?;
                            seen_spread = true;
                        }
                        CallArg::Named { name, .. } => {
                            // §5: named arguments follow ALL
                            // positional and spread arguments.
                            named.push((self.text(name.span).into(), value));
                        }
                    }
                }
                if idx < args.len() {
                    let expr = match &args[idx] {
                        CallArg::Positional(e) => e,
                        CallArg::Spread(e) => e,
                        CallArg::Named { value, .. } => value,
                    };
                    self.frames.push(Frame::KCallArgs {
                        args: args.clone(),
                        idx: idx + 1,
                        acc,
                        named,
                        spread,
                        seen_spread,
                        span,
                    });
                    self.eval_expr(expr)?;
                } else {
                    let argc = acc.len();
                    self.values.extend(acc);
                    self.apply_call(argc, named, spread, seen_spread, span)?;
                }
                Ok(())
            }
            Frame::KPositionalArgs { args, idx } => {
                if idx >= args.len() {
                    return Ok(());
                }
                let CallArg::Positional(expr) = &args[idx] else {
                    unreachable!("special call arguments were validated as positional");
                };
                self.frames.push(Frame::KPositionalArgs {
                    args: args.clone(),
                    idx: idx + 1,
                });
                self.eval_expr(expr)
            }
            Frame::KCtor { name, span } => {
                let arg = self.values.pop().expect("ctor arg");
                let v = match name.as_ref() {
                    "Some" => Value::Some(Rc::new(arg)),
                    "Ok" => Value::Ok(Rc::new(arg)),
                    "Err" => Value::Err(Rc::new(arg)),
                    _ => {
                        return Err(fault(codes::GUARD_TYPE, "unknown constructor", span));
                    }
                };
                self.values.push(v);
                Ok(())
            }
            Frame::KEnumCtor {
                enum_id,
                declaration_identity,
                method_identity,
                variant,
                variant_index,
                arity,
            } => {
                // The N args evaluated left-to-right, so they sit on the value
                // stack with the LAST on top — pop into reverse, then un-reverse.
                let mut payloads: Vec<Value> = Vec::with_capacity(arity);
                for _ in 0..arity {
                    payloads.push(self.values.pop().expect("enum payload arg"));
                }
                payloads.reverse();
                self.values.push(Value::Enum {
                    enum_id,
                    declaration_identity,
                    method_identity,
                    variant,
                    variant_index,
                    payloads: Rc::from(payloads),
                });
                Ok(())
            }
            Frame::KNewtypeCtor {
                newtype_id,
                declaration_identity,
                method_identity,
            } => {
                let inner = self.values.pop().expect("newtype ctor arg");
                self.values.push(match declaration_identity {
                    Some(identity) => Value::newtype_with_identities(
                        newtype_id.as_ref(),
                        identity.as_ref(),
                        method_identity.as_deref(),
                        inner,
                    ),
                    None => Value::newtype_with_method_identity(
                        newtype_id.as_ref(),
                        method_identity.as_deref(),
                        inner,
                    ),
                });
                Ok(())
            }
            Frame::KMethodCall {
                field,
                args,
                span,
                member_span,
                root,
            } => {
                let recv = self.values.pop().expect("method receiver");
                let member = self.text(field.span);
                // STATIC dispatch: pick the method by the receiver's runtime nominal
                // id. The checker rejects a field/method name collision (so a method
                // name is never also a field), so the nominal-id lookup is
                // unambiguous. A non-nominal receiver (id = None) or an absent
                // `(id, m)` falls back to ordinary member access — which resolves a
                // builtin method or faults BYTE-IDENTICALLY to the emitter (run≡build,
                // incl. `--unchecked`).
                let method = recv
                    .method_dispatch_id()
                    .and_then(|id| self.method_defs.get(&(id.to_string(), member.to_string())))
                    .cloned();
                match method {
                    Some(closure) => {
                        // Schedule a closure call with the receiver prepended as the
                        // FIRST argument (`self`), then the explicit args L→R. The
                        // closure value is the callee (popped by `apply_call`).
                        self.values.push(closure);
                        self.frames.push(Frame::KCallArgs {
                            args,
                            idx: 0,
                            named: Vec::new(),
                            spread: Vec::new(),
                            seen_spread: false,
                            acc: vec![recv],
                            span,
                        });
                        Ok(())
                    }
                    None => {
                        // No method — resolve the member normally so the absent-member
                        // fault (and any bound-builtin-method, incl. a mutator needing
                        // `let mut`) matches the emitter. The fault uses the MEMBER span
                        // (`recv.m`), matching the emitter's `check_member_method` span.
                        let bound =
                            self.member_access(recv, member, member_span, root.as_deref())?;
                        self.values.push(bound);
                        self.frames.push(Frame::KCallArgs {
                            args,
                            idx: 0,
                            named: Vec::new(),
                            spread: Vec::new(),
                            seen_spread: false,
                            acc: Vec::new(),
                            span,
                        });
                        Ok(())
                    }
                }
            }
            Frame::KProtocolCall {
                protocol,
                method,
                arity,
                span,
            } => {
                // The `arity` args evaluated left-to-right sit on the value stack with
                // the LAST on top — pop into reverse, un-reverse so `args[0]` is the
                // conforming value (the dispatch receiver).
                let mut call_args: Vec<Value> = Vec::with_capacity(arity);
                for _ in 0..arity {
                    call_args.push(self.values.pop().expect("protocol arg"));
                }
                call_args.reverse();
                // STATIC dispatch on arg0's runtime nominal id: a MANUAL impl method
                // `("{protocol}<{id}>", method)` wins; else the DERIVED value leaf.
                let manual = call_args
                    .first()
                    .and_then(|v| v.nominal_id())
                    .and_then(|id| {
                        let source_key = self.src.as_ptr() as usize;
                        let module = self
                            .source_module_index
                            .get(&source_key)
                            .and_then(|module| self.module_scopes.get(module))
                            .map(|scope| scope.types.declaration_identity.as_ref())
                            .unwrap_or("");
                        self.method_defs.get(&(
                            protocol_method_identity(module, &protocol, id),
                            method.to_string(),
                        ))
                    })
                    .cloned();
                match manual {
                    Some(closure) => {
                        // Call the user method closure with all args (the conforming
                        // value is arg0 — an ORDINARY parameter, no `self`).
                        self.values.push(closure);
                        let argc = call_args.len();
                        self.values.extend(call_args);
                        self.apply_call(argc, Vec::new(), Vec::new(), false, span)?;
                        Ok(())
                    }
                    None => {
                        // DERIVED conformance: the shared builtin leaf (Show→render,
                        // Eq→values_equal, Order→values_compare) — byte-identical to
                        // the emitter's `builtin_protocol_dispatch` call.
                        let v = builtin_protocol_dispatch(&protocol, &method, call_args, span)?;
                        self.values.push(v);
                        Ok(())
                    }
                }
            }
            Frame::KOptionalCall {
                field,
                args,
                span,
                root,
                lead,
            } => {
                let recv = self.values.pop().expect("optional-call receiver");
                let schedule_call = |machine: &mut Self, method: Value, wrap: bool| {
                    if wrap {
                        machine.frames.push(Frame::KWrapSome);
                    }
                    machine.frames.push(Frame::KCallArgs {
                        args,
                        idx: 0,
                        named: Vec::new(),
                        spread: Vec::new(),
                        seen_spread: false,
                        acc: lead.into_iter().collect(),
                        span,
                    });
                    machine.values.push(method);
                };
                match recv {
                    // §12: None/null short-circuits — the call and
                    // its arguments are not evaluated.
                    Value::None => self.values.push(Value::None),
                    Value::Null => self.values.push(Value::Null),
                    // `Option<T>` preserves the container: the result
                    // wraps back into `Some`. member_access also
                    // enforces the §9 mutator-root here.
                    Value::Some(inner) => {
                        let method = self.member_access(
                            (*inner).clone(),
                            self.text(field.span),
                            span,
                            root.as_deref(),
                        )?;
                        schedule_call(self, method, true);
                    }
                    // `T | null` non-null: call directly; the result
                    // is `U | null`, no Some-wrapping.
                    other => {
                        let method = self.member_access(
                            other,
                            self.text(field.span),
                            span,
                            root.as_deref(),
                        )?;
                        schedule_call(self, method, false);
                    }
                }
                Ok(())
            }
            Frame::KWrapSome => {
                let v = self.values.pop().expect("optional-call result");
                self.values.push(wrap_optional(v));
                Ok(())
            }

            _ => unreachable!("frame family changed after classification"),
        }
    }

    pub(super) fn step_higher_order_frame(&mut self, frame: Frame) -> Result<(), RtError> {
        match frame {
            Frame::KCallbackHof { pending, span } => {
                let result = self.values.pop().expect("callback HOF result");
                let execution = pending.resume(result, span)?;
                self.continue_callback_hof(execution, span)
            }
            Frame::KCallbackKey {
                pending,
                destination,
                span,
            } => {
                let key = self.values.pop().expect("callback key result");
                self.continue_callback_key_collection(pending.resume(key), destination, span)
            }
            Frame::KCallbackRetain {
                cell,
                pending,
                span,
            } => {
                let predicate = self.values.pop().expect("retain predicate result");
                self.continue_callback_retain(pending.resume(predicate, span)?, cell, span)
            }
            Frame::KCallbackMapHof { pending, span } => {
                let result = self.values.pop().expect("map callback result");
                self.continue_callback_map_hof(pending.resume(result, span)?, span)
            }
            Frame::KCallbackMapUpdate { pending, span } => {
                let result = self.values.pop().expect("map update callback result");
                self.values.push(pending.resume(result, span)?);
                Ok(())
            }
            Frame::KCallbackOkOrElse { pending } => {
                let result = self.values.pop().expect("okOrElse callback result");
                self.values.push(pending.resume(result));
                Ok(())
            }
            Frame::KCallbackReceiverMap { pending } => {
                let result = self.values.pop().expect("receiver map callback result");
                self.values.push(pending.resume(result));
                Ok(())
            }
            _ => unreachable!("frame family changed after classification"),
        }
    }

    pub(super) fn continue_callback_hof(
        &mut self,
        execution: CallbackHofExecution,
        span: Span,
    ) -> Result<(), RtError> {
        match execution.next() {
            CallbackHofStep::Complete(value) => {
                self.values.push(value);
                Ok(())
            }
            CallbackHofStep::Call {
                pending,
                callee,
                args,
            } => {
                let arity = args.len();
                self.frames.push(Frame::KCallbackHof { pending, span });
                self.values.push(callee);
                self.values.extend(args);
                self.apply_call(arity, Vec::new(), Vec::new(), false, span)
            }
        }
    }

    pub(super) fn continue_callback_key_collection(
        &mut self,
        collection: CallbackKeyCollection,
        destination: CallbackKeyDestination,
        span: Span,
    ) -> Result<(), RtError> {
        match collection.next() {
            CallbackKeyStep::Complete { items, keys } => {
                let sorted = sorted_by_keys(&items, &keys, span)?;
                match destination {
                    CallbackKeyDestination::ReturnArray => self.values.push(Value::array(sorted)),
                    CallbackKeyDestination::WriteArray(cell) => {
                        *cell.borrow_mut() = sorted;
                        self.values.push(Value::Unit);
                    }
                }
                Ok(())
            }
            CallbackKeyStep::Call(pending) => {
                let (callee, item) = pending.invocation();
                self.frames.push(Frame::KCallbackKey {
                    pending,
                    destination,
                    span,
                });
                self.values.push(callee);
                self.values.push(item);
                self.apply_call(1, Vec::new(), Vec::new(), false, span)
            }
        }
    }

    pub(super) fn continue_callback_retain(
        &mut self,
        execution: CallbackRetainExecution,
        cell: Rc<RefCell<Vec<Value>>>,
        span: Span,
    ) -> Result<(), RtError> {
        match execution.next() {
            CallbackRetainStep::Complete(kept) => {
                *cell.borrow_mut() = kept;
                self.values.push(Value::Unit);
                Ok(())
            }
            CallbackRetainStep::Call(pending) => {
                let (callee, item) = pending.invocation();
                self.frames.push(Frame::KCallbackRetain {
                    cell,
                    pending,
                    span,
                });
                self.values.push(callee);
                self.values.push(item);
                self.apply_call(1, Vec::new(), Vec::new(), false, span)
            }
        }
    }

    pub(super) fn continue_callback_map_hof(
        &mut self,
        execution: CallbackMapHofExecution,
        span: Span,
    ) -> Result<(), RtError> {
        match execution.next() {
            CallbackMapHofStep::Complete(value) => {
                self.values.push(value);
                Ok(())
            }
            CallbackMapHofStep::Call {
                pending,
                callee,
                args,
            } => {
                let arity = args.len();
                self.frames.push(Frame::KCallbackMapHof { pending, span });
                self.values.push(callee);
                self.values.extend(args);
                self.apply_call(arity, Vec::new(), Vec::new(), false, span)
            }
        }
    }

    pub(super) fn continue_callback_map_update(
        &mut self,
        step: CallbackMapUpdateStep,
        span: Span,
    ) -> Result<(), RtError> {
        match step {
            CallbackMapUpdateStep::Complete(value) => {
                self.values.push(value);
                Ok(())
            }
            CallbackMapUpdateStep::Call {
                pending,
                callee,
                existing,
            } => {
                self.frames
                    .push(Frame::KCallbackMapUpdate { pending, span });
                self.values.push(callee);
                self.values.push(existing);
                self.apply_call(1, Vec::new(), Vec::new(), false, span)
            }
        }
    }

    pub(super) fn continue_callback_receiver_map(
        &mut self,
        step: CallbackReceiverMapStep,
        method: &'static str,
        span: Span,
    ) -> Result<(), RtError> {
        match step {
            CallbackReceiverMapStep::Complete(value) => {
                self.values.push(value);
                Ok(())
            }
            CallbackReceiverMapStep::Call {
                pending,
                callee,
                input,
            } => {
                self.frames.push(Frame::KCallbackReceiverMap { pending });
                self.values.push(callee);
                self.values.push(input);
                self.apply_call(1, Vec::new(), Vec::new(), false, span)
            }
            CallbackReceiverMapStep::Delegate { receiver, .. }
            | CallbackReceiverMapStep::Unsupported { receiver } => {
                Err(no_member_fault(&receiver, method, span))
            }
        }
    }

    pub(super) fn step_call_boundary_frame(&mut self, frame: Frame) -> Result<(), RtError> {
        match frame {
            Frame::CallBoundary {
                saved,
                saved_src,
                saved_type_params,
                return_guard,
                ..
            } => {
                // A tail value and every unwound return share the same defer,
                // guard, and caller-state exit. Clone only the stack handle so the
                // mutable boundary transition does not borrow `self.values`.
                let return_value = self.values.last().cloned();
                self.exit_call_boundary(
                    saved,
                    saved_src,
                    saved_type_params,
                    return_guard,
                    return_value.as_ref(),
                )
            }
            Frame::KReturn { span } => {
                let value = self.values.pop().expect("return value");
                self.start_unwind(UnwindAction::Return { value, span });
                Ok(())
            }
            // The break value evaluated; unwind to the target loop.
            Frame::KBreak { span, label } => {
                let value = self.values.pop().expect("break value");
                self.start_unwind(UnwindAction::Break { span, label, value });
                Ok(())
            }
            _ => unreachable!("frame family changed after classification"),
        }
    }

    /// §22 builtin dispatch (CDR-003 §1: effects via the host).
    pub(super) fn call_builtin(
        &mut self,
        kind: Builtin,
        recv: Option<Rc<Value>>,
        args: Vec<Value>,
        span: Span,
    ) -> Result<(), RtError> {
        let mut args = args;
        if let Some(outcome) = call_pure_builtin(kind, &mut args, span) {
            self.values.push(outcome?);
            return Ok(());
        }
        if let Some(outcome) = call_host_builtin(self.host, kind, &mut args, span) {
            let value = outcome?;
            self.values.push(
                if self.language_version >= LangVersion::V5_20
                    && kind.lispex_application_operation().is_some()
                {
                    project_lispex_application_host_value(value)
                } else {
                    value
                },
            );
            return Ok(());
        }
        if let Some(kind) = CallbackHofKind::from_builtin(kind) {
            let args = match recv.as_ref() {
                Some(receiver) => {
                    let mut receiver_args = Vec::with_capacity(args.len() + 1);
                    receiver_args.push((**receiver).clone());
                    receiver_args.extend(args);
                    receiver_args
                }
                None => args,
            };
            return self.continue_callback_hof(prepare_callback_hof(kind, args, span)?, span);
        }
        // Every synchronous receiver leaf and host-backed resource call consumes
        // the same receiver catalog that created its bound Builtin. Callback
        // routes remain below because they schedule engine continuations.
        if let Some(recv) = recv.as_ref()
            && let Some(receiver) = receiver_builtin_by_kind(recv, kind)
        {
            match receiver.route {
                ReceiverBuiltinRoute::Method => {
                    self.values.push(call_method(
                        (**recv).clone(),
                        receiver.name,
                        args,
                        span,
                        span,
                    )?);
                    return Ok(());
                }
                ReceiverBuiltinRoute::Resource => {
                    self.values.push(call_resource_method(
                        self.host,
                        (**recv).clone(),
                        receiver.name,
                        args,
                        span,
                        span,
                    )?);
                    return Ok(());
                }
                ReceiverBuiltinRoute::Callback => {}
            }
        }
        let out: Value = match kind {
            // §6 (v5.4) `m.mapValues(f)` / `m.filter(f)` consume the shared map
            // callback state over an insertion-order pair snapshot.
            Builtin::MapMapValues | Builtin::MapFilter => {
                let Some(recv) = recv else { unreachable!() };
                let [f] = exact_args(args, span)?;
                let Value::Map(map) = &*recv else {
                    unreachable!()
                };
                let pairs = map.borrow().pairs();
                let kind = if matches!(kind, Builtin::MapFilter) {
                    CallbackMapHofKind::Filter
                } else {
                    CallbackMapHofKind::MapValues
                };
                return self
                    .continue_callback_map_hof(prepare_callback_map_hof(kind, pairs, f), span);
            }
            // §6 (v5.4) `m.update(k, initial, f)` uses the shared callback transition.
            // `member_access` already proved the `mut` root.
            Builtin::MapUpdate => {
                let Some(recv) = recv else { unreachable!() };
                let [key, initial, f] = exact_args(args, span)?;
                let Value::Map(map) = &*recv else {
                    unreachable!()
                };
                return self.continue_callback_map_update(
                    prepare_callback_map_update(map.clone(), key, initial, f, span)?,
                    span,
                );
            }
            // §6 (v5.4) `xs.sortBy(f)` collects keys through the shared callback-key
            // state. The receiver cell is written only after every callback and the
            // stable sort succeed, so callback/comparison faults leave it untouched.
            Builtin::ArrSortBy => {
                let Some(recv) = recv else { unreachable!() };
                let [f] = exact_args(args, span)?;
                let Value::Array(cell) = &*recv else {
                    unreachable!()
                };
                let items = cell.borrow().clone();
                return self.continue_callback_key_collection(
                    prepare_callback_key_collection(items, f),
                    CallbackKeyDestination::WriteArray(cell.clone()),
                    span,
                );
            }
            // §6 (v5.4) `xs.retain(f)` drives predicates through the shared retain
            // state and writes the receiver only after every predicate succeeds.
            Builtin::ArrRetain => {
                let Some(recv) = recv else { unreachable!() };
                let [f] = exact_args(args, span)?;
                let Value::Array(cell) = &*recv else {
                    unreachable!()
                };
                let items = cell.borrow().clone();
                return self.continue_callback_retain(
                    prepare_callback_retain(items, f),
                    cell.clone(),
                    span,
                );
            }
            // §22 (v5.4) `xs.sortedBy(f)` uses the same key-collection state and
            // returns the stable sorted copy instead of writing into the receiver.
            Builtin::ArrSortedBy => {
                let Some(recv) = recv else { unreachable!() };
                let [f] = exact_args(args, span)?;
                let items = iterable_items((*recv).clone(), span)?;
                return self.continue_callback_key_collection(
                    prepare_callback_key_collection(items, f),
                    CallbackKeyDestination::ReturnArray,
                    span,
                );
            }
            Builtin::OkOr => {
                let Some(recv) = recv else { unreachable!() };
                call_method((*recv).clone(), "okOr", args, span, span)?
            }
            // §22.2 `opt.okOrElse(f)` uses the shared lazy Option-to-Result
            // transition; this engine only schedules and resumes its callback.
            Builtin::OkOrElse => {
                let [f] = exact_args(args, span)?;
                let Some(recv) = recv else { unreachable!() };
                match prepare_callback_ok_or_else((*recv).clone(), f) {
                    CallbackOkOrElseStep::Complete(value) => self.values.push(value),
                    CallbackOkOrElseStep::Call { pending, callee } => {
                        self.frames.push(Frame::KCallbackOkOrElse { pending });
                        self.values.push(callee);
                        self.apply_call(0, Vec::new(), Vec::new(), false, span)?;
                    }
                    CallbackOkOrElseStep::Unsupported { receiver } => {
                        return Err(no_member_fault(&receiver, "okOrElse", span));
                    }
                }
                return Ok(());
            }
            // §22 `Option.map` and `Result.map` share their lazy callback,
            // passthrough, and result-wrap transition.
            Builtin::OptionMap | Builtin::ResultMap => {
                let [f] = exact_args(args, span)?;
                let Some(recv) = recv else { unreachable!() };
                return self.continue_callback_receiver_map(
                    prepare_callback_receiver_map((*recv).clone(), f),
                    "map",
                    span,
                );
            }
            // §22 `Option.flatMap` and `Result.flatMap` share the same callback
            // and passthrough transition as `map`, with identity completion.
            Builtin::OptionFlatMap | Builtin::ResultFlatMap => {
                let [f] = exact_args(args, span)?;
                let Some(recv) = recv else { unreachable!() };
                return self.continue_callback_receiver_map(
                    prepare_callback_receiver_flat_map((*recv).clone(), f),
                    "flatMap",
                    span,
                );
            }
            _ => unreachable!("namespace builtin reached receiver dispatch"),
        };
        self.values.push(out);
        Ok(())
    }

    /// §15: all arms start; round-robin with a fixed quantum; the
    /// Schedules a call. `lead`, when present, is a §11 piped value
    /// inserted as the call's FIRST positional argument (the
    /// first-argument-insertion pipe stage). Handles the §12
    /// optional-call callee and the §22.1 prelude constructors.
    pub(super) fn schedule_call(
        &mut self,
        callee: &Expr,
        args: &[CallArg],
        span: Span,
        lead: Option<Value>,
    ) -> Result<(), RtError> {
        // §12 optional call: short-circuit None/null, else call.
        if let ExprKind::OptionalAccess { object, field } = &callee.kind {
            let root = self.mutator_root_of(object, field);
            self.frames.push(Frame::KOptionalCall {
                field: *field,
                args: args.into(),
                span,
                root: root.map(Rc::from),
                lead,
            });
            self.frames.push(Frame::Eval(object.clone()));
            return Ok(());
        }
        // §22.1 prelude constructors by name. The piped value, if
        // any, is the constructor's single argument.
        if let ExprKind::Ident = &callee.kind {
            let name = self.text(callee.span).to_string();
            if matches!(name.as_str(), "Some" | "Ok" | "Err") && lookup(&self.env, &name).is_none()
            {
                let total = args.len() + usize::from(lead.is_some());
                if total != 1 {
                    return Err(fault(
                        codes::GUARD_ARITY,
                        format!("`{name}` takes exactly one argument"),
                        span,
                    ));
                }
                self.frames.push(Frame::KCtor {
                    name: name.as_str().into(),
                    span,
                });
                if let Some(v) = lead {
                    self.values.push(v);
                } else {
                    let arg = match &args[0] {
                        CallArg::Positional(e) => e,
                        CallArg::Named {
                            name: arg_name,
                            value,
                        } if self.text(arg_name.span) == "value" => value,
                        _ => {
                            return Err(fault(
                                codes::GUARD_TYPE,
                                format!("`{name}` takes a positional argument"),
                                span,
                            ));
                        }
                    };
                    self.eval_expr(arg)?;
                }
                return Ok(());
            }
            // §3 (v5.4) NEWTYPE construction `UserId(5)`: the callee is a declared
            // newtype NOT shadowed by a binding. Evaluate the single arg and wrap via
            // `KNewtypeCtor`. A leading pipe value (`5 |> UserId`) is the arg. The
            // checker validated arity/type; a wrong arg count faults below, matching
            // the emitter byte-identically under `--unchecked`.
            if let Some(definition) = self.newtype_definition_in(&self.src, &name).cloned()
                && lookup(&self.env, &name).is_none()
            {
                let total = args.len() + usize::from(lead.is_some());
                if total != 1 {
                    return Err(fault(
                        codes::GUARD_ARITY,
                        format!("newtype `{name}` constructor takes exactly one argument"),
                        span,
                    ));
                }
                let identities = self.nominal_identity_projection(definition.method_identity);
                self.frames.push(Frame::KNewtypeCtor {
                    newtype_id: definition.runtime_id,
                    declaration_identity: identities.declaration,
                    method_identity: identities.method,
                });
                if let Some(v) = lead {
                    self.values.push(v);
                } else {
                    let arg = match &args[0] {
                        CallArg::Positional(e) => e,
                        _ => {
                            return Err(fault(
                                codes::GUARD_TYPE,
                                format!("newtype `{name}` takes a positional argument"),
                                span,
                            ));
                        }
                    };
                    self.eval_expr(arg)?;
                }
                return Ok(());
            }
        }
        // §3 (v5.3/v5.4) N-payload enum construction `Bin(a, b, c)`: the callee is
        // `Member(Ident(enum), variant)` where `enum` is a declared enum NOT
        // shadowed by a binding and `variant` is a payloadful variant. Evaluate the
        // N payload args (a leading pipe value is the first) and wrap via
        // `KEnumCtor`. The checker validated arity/type; an arity mismatch here
        // (e.g. `--unchecked`) faults below, matching the emitter byte-identically.
        if let ExprKind::Member { object, field } = &callee.kind
            && let ExprKind::Ident = &object.kind
        {
            let head = self.text(object.span);
            if lookup(&self.env, head).is_none()
                && let Some(definition) = self.enum_definition_in(&self.src, head)
                && let Some(&(arity, variant_index)) =
                    definition.variants.get(self.text(field.span))
            {
                let variant = self.text(field.span);
                // Only a PAYLOADFUL variant is constructed by a call; a CALLED
                // payload-less variant (`Color.Red(1)`) is rejected by the checker
                // (arity) and faults here, matching the emitter under `--unchecked`.
                if arity >= 1 {
                    let total = args.len() + usize::from(lead.is_some());
                    if total != arity {
                        return Err(fault(
                            codes::GUARD_ARITY,
                            format!(
                                "enum variant `{head}.{variant}` takes {arity} payload{}",
                                if arity == 1 { "" } else { "s" }
                            ),
                            span,
                        ));
                    }
                    let enum_id = definition.runtime_id.clone();
                    let identities =
                        self.nominal_identity_projection(definition.method_identity.clone());
                    let variant: Rc<str> = Rc::from(variant);
                    self.frames.push(Frame::KEnumCtor {
                        enum_id,
                        declaration_identity: identities.declaration,
                        method_identity: identities.method,
                        variant,
                        variant_index,
                        arity,
                    });
                    for a in args {
                        if !matches!(a, CallArg::Positional(_)) {
                            return Err(fault(
                                codes::GUARD_TYPE,
                                "an enum payload must be a positional argument",
                                span,
                            ));
                        }
                    }
                    if !args.is_empty() {
                        self.frames.push(Frame::KPositionalArgs {
                            args: args.into(),
                            idx: 0,
                        });
                    }
                    if let Some(v) = lead {
                        // The pipe value is arg0 — it sits BELOW the others on the
                        // value stack, so push it now (it is already evaluated).
                        self.values.push(v);
                    }
                    return Ok(());
                }
            }
        }
        // §4 (v5.4) a PROTOCOL static dispatch `Show.show(x)` / `Order.compare(a, b)`:
        // the callee is `Member(Ident(protocol), method)` where `protocol` is a
        // declared protocol NOT shadowed by a binding. Evaluate the positional args
        // (a leading pipe value is arg0) and dispatch via `KProtocolCall` — manual
        // impl method first, else the derived value leaf. The static heads/ctors above
        // already returned, so this never shadows them.
        if let ExprKind::Member { object, field } = &callee.kind
            && let ExprKind::Ident = &object.kind
        {
            let head = self.text(object.span);
            if lookup(&self.env, head).is_none() && self.protocol_defs.contains(head) {
                let protocol: Rc<str> = Rc::from(head);
                let method: Rc<str> = Rc::from(self.text(field.span));
                for a in args {
                    if !matches!(a, CallArg::Positional(_)) {
                        return Err(fault(
                            codes::GUARD_TYPE,
                            "a protocol call takes positional arguments only",
                            span,
                        ));
                    }
                }
                let arity = args.len() + usize::from(lead.is_some());
                self.frames.push(Frame::KProtocolCall {
                    protocol,
                    method,
                    arity,
                    span,
                });
                if !args.is_empty() {
                    self.frames.push(Frame::KPositionalArgs {
                        args: args.into(),
                        idx: 0,
                    });
                }
                if let Some(v) = lead {
                    self.values.push(v);
                }
                return Ok(());
            }
        }
        // §4 (v5.4) a user METHOD call `recv.m(args)`: when `m` names a method
        // registered for SOME nominal type, defer the dispatch to `KMethodCall` —
        // it evaluates the receiver, reads its runtime nominal id, and (when the
        // receiver is a matching nominal value with that method, field NOT shadowing)
        // calls the method closure with `recv` prepended. A non-nominal receiver or
        // an absent method falls back to ordinary member access (so a builtin method
        // / record-field-shadow / a fault all behave exactly as before). The static
        // heads + ctors above already returned, so this never shadows them. Only the
        // un-piped form is intercepted; a piped `recv.m` falls to the generic path.
        if lead.is_none()
            && let ExprKind::Member { object, field } = &callee.kind
        {
            let member = self.text(field.span);
            if self.method_defs.keys().any(|(_, m)| m == member) {
                let root = self.mutator_root_of(object, field).map(Rc::from);
                self.frames.push(Frame::KMethodCall {
                    field: *field,
                    args: args.into(),
                    span,
                    member_span: callee.span,
                    root,
                });
                self.frames.push(Frame::Eval(object.clone()));
                return Ok(());
            }
        }
        self.frames.push(Frame::KCallArgs {
            args: args.into(),
            idx: 0,
            named: Vec::new(),
            spread: Vec::new(),
            seen_spread: false,
            acc: lead.into_iter().collect(),
            span,
        });
        self.eval_expr(callee)
    }

    pub(super) fn prepare_closure_call(
        &mut self,
        data: &ClosureData,
        args: Vec<Value>,
        named: Vec<(Rc<str>, Value)>,
        spread: Vec<Value>,
        seen_spread: bool,
        span: Span,
    ) -> Result<PreparedClosureCall, RtError> {
        // §7/§5: resolve the argument shape against declaration-owned source
        // before changing the machine's live source or environment. A rejected
        // call therefore has no interpreter state to unwind.
        let (slots, variadic): (Vec<ClosureCallSlot<'_>>, Option<(String, &Type)>) =
            match &data.params {
                ClosureParams::Declared(params) => {
                    // Keep the variadic parameter's element type alongside its
                    // name — each surplus argument is guarded against it below
                    // (`...xs: int` binds `xs: Array<int>`).
                    let variadic =
                        params
                            .last()
                            .filter(|parameter| parameter.variadic)
                            .map(|parameter| {
                                (
                                    text_in(&data.src, parameter.name.span).to_string(),
                                    &parameter.ty,
                                )
                            });
                    let fixed = params.len() - variadic.is_some() as usize;
                    (
                        params[..fixed]
                            .iter()
                            .map(|parameter| ClosureCallSlot {
                                name: text_in(&data.src, parameter.name.span).to_string(),
                                default: parameter.default.as_ref(),
                                ty: Some(&parameter.ty),
                            })
                            .collect(),
                        variadic,
                    )
                }
                ClosureParams::Lambda(params) => (
                    params
                        .iter()
                        .map(|parameter| ClosureCallSlot {
                            name: text_in(&data.src, parameter.name.span).to_string(),
                            default: None,
                            ty: None,
                        })
                        .collect(),
                    None,
                ),
            };
        let mut filled: Vec<Option<Value>> = (0..slots.len()).map(|_| None).collect();
        let mut rest = Vec::new();
        for (index, value) in args.into_iter().enumerate() {
            if index < slots.len() {
                filled[index] = Some(value);
            } else if variadic.is_some() {
                rest.push(value);
            } else {
                return Err(fault(
                    codes::GUARD_ARITY,
                    format!("expected {} argument(s), found more", slots.len()),
                    span,
                ));
            }
        }
        if seen_spread {
            // §5: spread fills only the variadic tail, never fixed parameter
            // slots, and cannot skip an unsatisfied required fixed parameter.
            if variadic.is_none() {
                return Err(fault(
                    codes::GUARD_ARITY,
                    "spread arguments require a variadic parameter (§5)",
                    span,
                ));
            }
            let positional_filled = filled.iter().filter(|slot| slot.is_some()).count();
            if slots[positional_filled..]
                .iter()
                .any(|slot| slot.default.is_none())
            {
                return Err(fault(
                    codes::GUARD_ARITY,
                    "a spread argument cannot skip an unsatisfied fixed parameter (§5)",
                    span,
                ));
            }
            rest.extend(spread);
        }
        let filled = bind_named_arg_slots(
            filled,
            slots.len(),
            |index| slots.get(index).map(|slot| slot.name.as_str()),
            named,
            span,
        )?;

        // Defaults and parameter guards inspect the callee's source and defining
        // environment. This is the only fallible phase that changes machine state;
        // one exit restores both values, while success commits the call environment.
        let saved_env = self.env.clone();
        let saved_src = std::mem::replace(&mut self.src, data.src.clone());
        self.env = data.env.clone();
        let call_env = child_env(&data.env);
        let prepared = (|| -> Result<Option<(Type, Rc<str>)>, RtError> {
            for (slot, value) in slots.iter().zip(filled) {
                let value = match (value, slot.default) {
                    (Some(value), _) => value,
                    (None, Some(default)) => self.const_eval(default)?,
                    (None, None) => {
                        return Err(fault(
                            codes::GUARD_ARITY,
                            format!("missing argument for parameter `{}` (§5)", slot.name),
                            span,
                        ));
                    }
                };
                // Guard values crossing concrete parameter boundaries (§6).
                if let Some(parameter_type) = slot.ty
                    && boundary_guardable(parameter_type, &data.src, &data.type_params)
                    && !self.value_matches_type(
                        parameter_type,
                        &data.src,
                        &value,
                        parameter_type.span,
                    )?
                {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        "argument does not match parameter type (§6)",
                        parameter_type.span,
                    ));
                }
                call_env.borrow_mut().vars.insert(
                    slot.name.clone(),
                    BindingCell {
                        value,
                        mutable: false,
                    },
                );
            }

            // Variadic guards observe earlier fixed bindings through the call
            // environment, preserving the existing alias-resolution boundary.
            self.env = call_env.clone();
            if let Some((name, variadic_type)) = variadic {
                if boundary_guardable(variadic_type, &data.src, &data.type_params) {
                    for value in &rest {
                        if !self.value_matches_type(
                            variadic_type,
                            &data.src,
                            value,
                            variadic_type.span,
                        )? {
                            return Err(fault(
                                codes::GUARD_TYPE,
                                "argument does not match parameter type (§6)",
                                variadic_type.span,
                            ));
                        }
                    }
                }
                call_env.borrow_mut().vars.insert(
                    name,
                    BindingCell {
                        value: Value::array(rest),
                        mutable: false,
                    },
                );
            }

            Ok(match &data.return_type {
                Some(return_type)
                    if boundary_guardable(return_type, &data.src, &data.type_params) =>
                {
                    Some((return_type.clone(), data.src.clone()))
                }
                _ => None,
            })
        })();

        match prepared {
            Ok(return_guard) => Ok(PreparedClosureCall {
                saved_env,
                saved_src,
                return_guard,
            }),
            Err(error) => {
                self.env = saved_env;
                self.src = saved_src;
                Err(error)
            }
        }
    }

    pub(super) fn apply_call(
        &mut self,
        argc: usize,
        named: Vec<(Rc<str>, Value)>,
        spread: Vec<Value>,
        seen_spread: bool,
        span: Span,
    ) -> Result<(), RtError> {
        // Stack: callee, arg0..argN-1 (callee under the args).
        let args_start = self.values.len() - argc;
        let callee = self.values.remove(args_start - 1);
        let args: Vec<Value> = self.values.split_off(args_start - 1);
        match callee {
            Value::Builtin { kind, recv } => {
                let mut args = args;
                if !named.is_empty() {
                    args = bind_builtin_named_args(kind, recv.is_some(), args, named, span)?;
                }
                if seen_spread {
                    // §5: spread fills only a variadic tail.
                    if !matches!(kind, Builtin::ArrayOf | Builtin::SetOf) {
                        return Err(fault(
                            codes::GUARD_ARITY,
                            "spread arguments require a variadic parameter (§5)",
                            span,
                        ));
                    }
                    args.extend(spread);
                }
                self.call_builtin(kind, recv, args, span)
            }
            Value::Closure(call) => {
                if let Some(extern_fn) = call.as_any().downcast_ref::<ExternFunction>() {
                    if self.call_depth >= CALL_DEPTH_LIMIT {
                        return Err(recursion_fault(span));
                    }
                    let arity = extern_fn.arity();
                    if args.len() > arity {
                        return Err(fault(
                            codes::GUARD_ARITY,
                            format!("expected {arity} argument(s), found more"),
                            span,
                        ));
                    }
                    if seen_spread {
                        return Err(fault(
                            codes::GUARD_ARITY,
                            "spread arguments require a variadic parameter (§5)",
                            span,
                        ));
                    }
                    let slots = bind_named_arg_slots(
                        args.into_iter().map(Some).collect(),
                        arity,
                        |index| extern_fn.param_name(index),
                        named,
                        span,
                    )?;
                    let mut args = Vec::with_capacity(arity);
                    for (i, slot) in slots.into_iter().enumerate() {
                        match slot {
                            Some(value) => args.push(value),
                            None => {
                                let name = extern_fn.param_name(i).unwrap_or_default();
                                return Err(fault(
                                    codes::GUARD_ARITY,
                                    format!("missing argument for parameter `{name}` (§5)"),
                                    span,
                                ));
                            }
                        }
                    }
                    self.call_depth += 1;
                    let out = extern_fn.call_host(self.host, args);
                    self.call_depth = self.call_depth.saturating_sub(1);
                    self.values.push(out?);
                    return Ok(());
                }
                // Recover the interpreter's concrete closure (CDR-006
                // §3 downcast bridge): every callable the interpreter
                // builds is an AST-backed `ClosureData`. A miss is an
                // internal invariant violation, reported as a fault
                // (never a panic).
                let data = crate::value::as_closure(&call).ok_or_else(|| {
                    fault(
                        codes::GUARD_UNIMPLEMENTED,
                        "internal: a non-interpreter callable reached the frame machine",
                        span,
                    )
                })?;
                // §4 the SHARED recursion guard — fault `GUARD_RECURSION` once this
                // nested call would exceed `CALL_DEPTH_LIMIT`, the SAME cap the emitted
                // `call_value` enforces, so the interpreter stops here instead of
                // recursing far past the native stack the emitted binary overflows.
                // Checked BEFORE any state swap (a clean fault, nothing to restore); the
                // matching `+= 1` rides with the `CallBoundary` push (so every early
                // fault BELOW does not leak the count) and the pop decrements it.
                if self.call_depth >= CALL_DEPTH_LIMIT {
                    return Err(recursion_fault(span));
                }
                let vstack = self.values.len();
                let prepared =
                    self.prepare_closure_call(data, args, named, spread, seen_spread, span)?;
                // §3/§7 swap in the callee's generic type-param scope LAST — after
                // every fallible call-setup step (arity, defaults, §6 param/variadic
                // boundary guards, which only ever inspect CONCRETE, non-type-param
                // types). No fallible step runs between here and the body, so the
                // matching `CallBoundary` pop is the single, guaranteed restore — no
                // pre-boundary error path can leak the callee's scope to the caller.
                let saved_type_params =
                    std::mem::replace(&mut self.type_params, data.type_params.clone());
                // §4 enter one call level — paired with the `CallBoundary` pop's
                // decrement (both unwind and normal-return pops), and with the early
                // depth check above. No fallible step runs between here and the body.
                self.call_depth += 1;
                self.frames.push(Frame::CallBoundary {
                    saved: prepared.saved_env,
                    vstack,
                    saved_src: prepared.saved_src,
                    saved_type_params,
                    return_guard: prepared.return_guard,
                });
                match &data.body {
                    ClosureBody::Block(block) => {
                        self.collect_block_aliases(block);
                        self.frames.push(Frame::KBlock {
                            block: block.clone(),
                            idx: 0,
                        });
                    }
                    ClosureBody::Expr(e) => self.frames.push(Frame::Eval(e.clone())),
                }
                Ok(())
            }
            Value::Composed(pair) => {
                // (f >> g)(args) == g(f(args)) (SS11).
                self.frames.push(Frame::KComposeAfter {
                    g: pair.1.clone(),
                    span,
                });
                self.values.push(pair.0.clone());
                let argc = args.len();
                self.values.extend(args);
                self.apply_call(argc, named, spread, seen_spread, span)
            }
            other => Err(fault(
                codes::GUARD_NOT_CALLABLE,
                format!("`{}` is not callable", other.kind()),
                span,
            )),
        }
    }
}

/// The arity range of a callable value: (min, max), `max = None` for
/// variadic callables; `None` overall when the value is not callable.
pub(super) fn callable_arity(value: &Value) -> Option<(usize, Option<usize>)> {
    match value {
        Value::Closure(call) => {
            if let Some(data) = crate::value::as_closure(call) {
                match &data.params {
                    ClosureParams::Declared(params) => {
                        let variadic = params.last().is_some_and(|p| p.variadic);
                        let fixed = params.len() - variadic as usize;
                        let required = params[..fixed]
                            .iter()
                            .filter(|p| p.default.is_none())
                            .count();
                        Some((required, if variadic { None } else { Some(fixed) }))
                    }
                    ClosureParams::Lambda(params) => Some((params.len(), Some(params.len()))),
                }
            } else {
                let fixed = call.arity();
                let required = (0..fixed)
                    .filter(|&index| !call.has_param_default(index))
                    .count();
                Some((
                    required,
                    if call.is_variadic() {
                        None
                    } else {
                        Some(fixed)
                    },
                ))
            }
        }
        // §22 the builtin arity table is the SHARED `Builtin::arity_range` (the
        // emitted runtime's `callable_shape_matches` uses the same source).
        Value::Builtin { kind, .. } => Some(kind.arity_range()),
        Value::Composed(pair) => callable_arity(&pair.0),
        _ => None,
    }
}
