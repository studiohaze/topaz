use super::*;

impl<'a> ExprChecker<'a> {
    /// Resolve the enclosing-loop index a labeled/unlabeled loop-control
    /// statement targets. `label = None` → the innermost loop (the last frame);
    /// `label = Some('l)` → the NEAREST (innermost) frame labeled `'l`. Reports
    /// the right diagnostic and returns `None` when there is no such target:
    /// TPZ5720 for an unknown label, TYPE_MISMATCH ("outside a loop", matching the
    /// interpreter's runtime fault) for a bare control statement with no enclosing
    /// loop. The `loop_ctx` stack is reset at every function/lambda boundary, so a
    /// control statement inside a lambda cannot see an outer loop.
    pub(super) fn resolve_loop_target(
        &mut self,
        label: Option<&ast::Ident>,
        span: Span,
        kw: &str,
    ) -> Option<usize> {
        match label {
            None => {
                let Some((idx, frame)) = self.loop_ctx.iter().enumerate().next_back() else {
                    self.former
                        .error(codes::TYPE_MISMATCH, format!("`{kw}` outside a loop"), span);
                    return None;
                };
                if !frame.bare_target {
                    let target = frame.bare_error.unwrap_or("this expression");
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        format!("bare `{kw}` cannot target {target}"),
                        span,
                    );
                    None
                } else {
                    Some(idx)
                }
            }
            Some(l) => {
                let name = self.former.text(l.span);
                match self
                    .loop_ctx
                    .iter()
                    .rposition(|f| f.label.as_deref() == Some(name))
                {
                    Some(i) => Some(i),
                    None => {
                        self.former.error(
                            codes::NO_LOOP_LABEL,
                            format!("no loop labeled `'{name}` in scope"),
                            span,
                        );
                        None
                    }
                }
            }
        }
    }

    /// Check a `break (label)? (value)?`. Resolves the target loop, then
    /// contributes the value's type (Unit for a value-less `break`) to that loop's
    /// break-join. When the target loop has a contextual `expected` type, the value
    /// checks against it; otherwise the value must AGREE with the first break that
    /// reached the loop (TPZ5721 on a concrete mismatch), so the loop's value type
    /// is unambiguous.
    pub(super) fn check_break(
        &mut self,
        label: &Option<ast::Ident>,
        value: &'a Option<ast::Expr>,
        span: Span,
    ) {
        let target = self.resolve_loop_target(label.as_ref(), span, "break");
        // The target loop's value context (only a VALUE loop routes a contextual
        // type into its breaks; a `while`/`for` is valueless).
        let value_loop = target.map(|i| self.loop_ctx[i].value_loop).unwrap_or(false);
        let expected = target.and_then(|i| self.loop_ctx[i].expected.clone());
        // Always type the value expression (even when the target is a valueless
        // loop or unresolved) so its own errors still surface; only CONTRIBUTE it
        // to a resolved VALUE loop.
        let value_ty = match (value, expected) {
            (Some(v), Some(exp)) => self.check_expr(v, &exp),
            (Some(v), None) => self.infer(v),
            (None, _) => Type::Prim(Prim::Unit),
        };
        if let Some(i) = target
            && value_loop
        {
            // Concrete-mismatch agreement (only when there is no contextual type to
            // route the join): every break must AGREE with the FIRST break. Compare
            // WIDENED types — `break 100` and `break 0` are both `int` (singleton
            // literal types `100`/`0` are not subtypes of each other, but they widen
            // to the same `int`), exactly as the omitted-return join widens before
            // unifying. A genuine `int` vs `string` mismatch still fires.
            if self.loop_ctx[i].expected.is_none()
                && let Some(first) = self.loop_ctx[i].breaks.first().cloned()
            {
                let first_w = first.widen();
                let value_w = value_ty.clone().widen();
                if !first_w.has_unknown()
                    && !value_w.has_unknown()
                    && !is_subtype(&value_w, &first_w)
                    && !is_subtype(&first_w, &value_w)
                {
                    self.former.error(
                        codes::BREAK_VALUE_MISMATCH,
                        format!(
                            "`break` values targeting this loop disagree — earlier `break` is `{first_w}`, this is `{value_w}` (all break values must have the same type)"
                        ),
                        span,
                    );
                }
            }
            self.loop_ctx[i].breaks.push(value_ty);
        }
    }

    /// Check a `continue (label)?` — resolve the target loop (value-less).
    pub(super) fn check_continue(&mut self, label: &Option<ast::Ident>, span: Span) {
        let _ = self.resolve_loop_target(label.as_ref(), span, "continue");
    }

    /// §5 `for pattern in iter { body }`. Statement position is a real valueless
    /// loop-control target; expression position is value-collecting and therefore
    /// a barrier for bare `break`/`continue`, while labeled control may pass
    /// through to an enclosing labeled `loop`.
    pub(super) fn check_for(
        &mut self,
        pattern: &'a ast::Pattern,
        iter: &'a ast::Expr,
        body: &'a ast::Block,
        is_stmt: bool,
    ) -> Type {
        let iter_ty = self.infer(iter);
        // §10 iteration: Array, Set, and integer ranges (a map iterates via
        // `.keys`). A concrete non-iterable is a static error; opaque types stay
        // silent.
        let elem = match self.iter_elem(&iter_ty) {
            Some(elem) => elem,
            None => {
                // A rigid generic now projects `ElemOf<T>` (Some) above, so a
                // `None` is a concrete non-iterable (error) or a gradual receiver
                // (stay silent).
                if !iter_ty.has_unknown() {
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        format!("`{iter_ty}` is not iterable (§10)"),
                        iter.span,
                    );
                }
                Type::Unknown
            }
        };
        self.push_scope();
        // A `for` loop pattern is a BINDING context (not a match arm).
        self.bind_match_pattern_at(pattern, &elem, false);
        self.loop_ctx.push(LoopFrame {
            label: None,
            value_loop: false,
            bare_target: is_stmt,
            bare_error: (!is_stmt).then_some("a value-collecting `for` (§5)"),
            expected: None,
            breaks: Vec::new(),
        });
        let body_ty = self.check_block(body);
        self.loop_ctx.pop();
        self.pop_scope();
        if is_stmt {
            Type::Prim(Prim::Unit)
        } else {
            Type::Ctor(Ctor::Array, vec![body_ty.widen()])
        }
    }

    /// §4 (v5.4) type-checks an `impl Type { … }` block: each method body is checked
    /// like a free function, with `self` bound to the RECEIVER type (the nominal type
    /// named by the impl) and the remaining parameters bound by their formed types.
    /// Coherence/duplicate/collision/`self`-first diagnostics already fired at
    /// formation (`collect_methods`); this pass only checks bodies. A method on a
    /// receiver that is NOT an own-module nominal is skipped (its formation rejected).
    pub(super) fn check_impl(&mut self, decl: &'a ast::ImplDecl) {
        // §4.2 (v5.4) a MANUAL PROTOCOL impl `impl Show<User> { … }`: its methods are
        // FREE functions (the conforming value is an ordinary parameter, no `self`), so
        // each body checks exactly like a free function (`check_function`). The
        // coherence/orphan/conformance diagnostics already fired at formation.
        if decl.target.is_some() {
            for m in &decl.methods {
                self.check_function(&m.decl);
            }
            return;
        }
        let type_id = self.former.text(decl.name.span).to_string();
        if self.former.nominal_is_generic(&type_id) {
            // Formation already emitted the closed generic-impl diagnostic.
            return;
        }
        let Some(receiver) = self.receiver_type(&type_id) else {
            // Not a declared nominal — `collect_methods` already rejected the impl.
            return;
        };
        for m in &decl.methods {
            // Only the exact declaration admitted by formation owns a method body.
            // Name lookup is insufficient because a rejected duplicate has the same
            // receiver/name key as the earlier accepted declaration.
            if !self.former.receiver_method_was_accepted(m.decl.name.span) {
                continue;
            }
            let method_name = self.former.text(m.decl.name.span).to_string();
            if let Some(ret) = self.check_method(&receiver, &m.decl) {
                self.former.set_method_return(&type_id, &method_name, ret);
            }
        }
    }

    /// §4 (v5.4) the RECEIVER type for an `impl`'s nominal id — the nominal carrier
    /// matching whichever kind declared it (record/enum/newtype), else `None`.
    pub(super) fn receiver_type(&self, type_id: &str) -> Option<Type> {
        if self.former.is_record(type_id) {
            Some(Type::NominalRecord {
                base: type_id.to_string(),
                args: Vec::new(),
            })
        } else if self.former.is_enum(type_id) {
            Some(Type::Enum {
                base: type_id.to_string(),
                args: Vec::new(),
            })
        } else if self.former.is_newtype(type_id) {
            Some(Type::Newtype {
                base: type_id.to_string(),
                args: Vec::new(),
            })
        } else {
            None
        }
    }

    /// §4 (v5.4) checks ONE method body: bind `self` to `receiver`, the remaining
    /// parameters by their formed types, then check the body against the declared (or
    /// inferred) return type. A trimmed twin of `check_function` — no generics (a
    /// generic method is rejected at formation), `self` typed as the receiver.
    pub(super) fn check_method(
        &mut self,
        receiver: &Type,
        decl: &'a ast::FunctionDecl,
    ) -> Option<Type> {
        self.tyenv.push(self.tyenv());
        self.push_scope();
        // Pass 1: form non-self param types + check their defaults (a default may not
        // reference parameters — checked before any param binds).
        let mut formed_params: Vec<Type> = Vec::with_capacity(decl.params.len());
        for (i, param) in decl.params.iter().enumerate() {
            if i == 0 {
                // `self`: the receiver type (its placeholder annotation is ignored).
                formed_params.push(receiver.clone());
                continue;
            }
            if param.variadic && i + 1 != decl.params.len() {
                self.former.error(
                    codes::VARIADIC_POSITION,
                    "a variadic parameter must be final".to_string(),
                    param.span,
                );
            }
            let env = self.tyenv();
            let ty = self.former.form_for_body(&param.ty, &env);
            if let Some(default) = &param.default {
                self.check_expr(default, &ty);
                self.check_function_default_const_expr(default);
            }
            formed_params.push(ty);
        }
        let signature_params = decl
            .params
            .iter()
            .zip(&formed_params)
            .filter(|(parameter, _)| !parameter.variadic)
            .map(|(_, ty)| ty.clone())
            .collect::<Vec<_>>();
        let signature_variadic = decl
            .params
            .iter()
            .zip(&formed_params)
            .find(|(parameter, _)| parameter.variadic)
            .map(|(_, ty)| Box::new(ty.clone()));
        // Pass 2: bind the parameter names (`self` + the rest).
        for (param, ty) in decl.params.iter().zip(formed_params.iter().cloned()) {
            let name = self.former.text(param.name.span).to_string();
            let bound = if param.variadic {
                Type::Ctor(Ctor::Array, vec![ty])
            } else {
                ty
            };
            self.record_typed_local(&name, param.name.span, &bound);
            self.bind(name, bound);
        }
        let declared_ret = decl.return_type.as_ref().map(|ret| {
            let env = self.tyenv();
            self.former.form_for_body(ret, &env)
        });
        let body_check = self.check_callable_body(&decl.body, declared_ret.as_ref());
        self.pop_scope();
        self.tyenv.pop();
        if let Some(declared_ret) = declared_ret {
            self.record_typed_node(
                topaz_hir::TypedNodeKind::Declaration,
                decl.name.span,
                &Type::Func {
                    params: signature_params,
                    variadic: signature_variadic,
                    ret: Box::new(declared_ret),
                },
            );
            return None;
        }
        // A method with an omitted return type infers from its body. Receiver
        // methods are not pending-return declarations in this slice, so only the
        // shared body result join is consumed here.
        let (inferred, _) = self.infer_callable_return(&decl.body, body_check, decl.name.span);
        self.record_typed_node(
            topaz_hir::TypedNodeKind::Declaration,
            decl.name.span,
            &Type::Func {
                params: signature_params,
                variadic: signature_variadic,
                ret: Box::new(inferred.clone()),
            },
        );
        Some(inferred)
    }

    pub(super) fn check_function(&mut self, decl: &'a ast::FunctionDecl) {
        // Declaration constraints (SPEC §7): unique type parameters,
        // variadic-final. Parameter names bind to their formed types
        // inside the body.
        let mut env = self.tyenv();
        let mut seen: Vec<&'a str> = Vec::new();
        let mut skolem_map: Vec<(u32, u32)> = Vec::new();
        for (i, p) in decl.type_params.iter().enumerate() {
            let name = self.former.text(p.span);
            if seen.contains(&name) {
                self.former.error(
                    codes::MALFORMED_TYPE,
                    format!("type parameter `{name}` is declared twice"),
                    p.span,
                );
            } else {
                seen.push(name);
            }
            // Inside the BODY a type parameter is rigid (a skolem):
            // distinct from call-site inference variables, and
            // distinct per DECLARATION so nested generics that shadow
            // a name cannot collapse (identity carries an id).
            let _ = i;
            self.skolem_counter += 1;
            skolem_map.push((self.skolem_counter, i as u32));
            env.insert(
                name,
                Type::Skolem {
                    name: name.to_string(),
                    id: self.skolem_counter,
                    origin: format!("source:{}:{}:{}", p.span.file.0, p.span.lo, p.span.hi),
                },
            );
        }
        let mut bound_frame: HashMap<u32, HashSet<String>> = HashMap::new();
        for (i, bounds) in decl.type_param_bounds.iter().enumerate() {
            let Some((skolem_id, _)) = skolem_map.get(i).copied() else {
                continue;
            };
            let mut seen_protocols = HashSet::new();
            let mut protocols = HashSet::new();
            for bound in bounds {
                let protocol = self.former.text(bound.span);
                if !seen_protocols.insert(protocol.to_string()) {
                    self.former.error(
                        codes::DUPLICATE_BOUND,
                        format!(
                            "protocol bound `{protocol}` is repeated for the same type parameter"
                        ),
                        bound.span,
                    );
                    continue;
                }
                if self.former.protocol(protocol).is_none() {
                    self.former.error(
                        codes::MALFORMED_TYPE,
                        format!("unknown protocol bound `{protocol}`"),
                        bound.span,
                    );
                } else {
                    protocols.insert(protocol.to_string());
                }
            }
            if !protocols.is_empty() {
                bound_frame.insert(skolem_id, protocols);
            }
        }
        self.tyenv.push(env);
        self.skolem_bounds.push(bound_frame);
        self.push_scope();
        // Pass 1: form types and check defaults BEFORE any
        // parameter binds — §7 defaults are const expressions and
        // may not reference parameters.
        let mut formed_params: Vec<Type> = Vec::with_capacity(decl.params.len());
        for (i, param) in decl.params.iter().enumerate() {
            if param.variadic && i + 1 != decl.params.len() {
                self.former.error(
                    codes::VARIADIC_POSITION,
                    "a variadic parameter must be final".to_string(),
                    param.span,
                );
            }
            let env = self.tyenv();
            let ty = self.former.form_for_body(&param.ty, &env);
            if let Some(default) = &param.default {
                self.check_expr(default, &ty);
                self.check_function_default_const_expr(default);
            }
            formed_params.push(ty);
        }
        let signature_params = decl
            .params
            .iter()
            .zip(&formed_params)
            .filter(|(parameter, _)| !parameter.variadic)
            .map(|(_, ty)| ty.clone())
            .collect::<Vec<_>>();
        let signature_variadic = decl
            .params
            .iter()
            .zip(&formed_params)
            .find(|(parameter, _)| parameter.variadic)
            .map(|(_, ty)| Box::new(ty.clone()));
        // Pass 2: bind the names.
        for (param, ty) in decl.params.iter().zip(formed_params.iter().cloned()) {
            let name = self.former.text(param.name.span).to_string();
            let bound = if param.variadic {
                Type::Ctor(Ctor::Array, vec![ty])
            } else {
                ty
            };
            self.record_typed_local(&name, param.name.span, &bound);
            self.bind(name, bound);
        }
        let declared_ret = decl.return_type.as_ref().map(|ret| {
            let env = self.tyenv();
            self.former.form_for_body(ret, &env)
        });
        let mut final_ret = declared_ret.clone();
        let body_check = self.check_callable_body(&decl.body, declared_ret.as_ref());
        self.pop_scope();
        self.skolem_bounds.pop();
        self.tyenv.pop();
        if declared_ret.is_none() {
            // C-6: an omitted return type infers as the join of the
            // return statements and the body tail (`()` when the
            // body falls through with no tail).
            let hit_pending = body_check.hit_pending_return;
            let (inferred, tainted) =
                self.infer_callable_return(&decl.body, body_check, decl.name.span);
            // Body type parameters are rigid skolems; the published signature
            // speaks in scheme variables.
            let mut ret = skolems_to_vars(&inferred, &skolem_map);
            let name = self.former.text(decl.name.span).to_string();
            // A rigid projection (`FieldOf<T, x>` &c) has no nameable spelling, so
            // it must never escape into an inferred signature. If one survives into
            // the return, the body returned a destructured generic value whose
            // projected type the caller cannot name; demand an explicit annotation
            // and fall back to `Unknown` rather than publish the projection.
            if contains_projection(&ret, &self.projection_ids) {
                self.former.error(
                    codes::MALFORMED_TYPE,
                    format!(
                        "cannot infer a return type for `{name}`: it returns a destructured generic value with no nameable type — annotate the return type"
                    ),
                    decl.name.span,
                );
                ret = strip_projections(&ret, &self.projection_ids);
            }
            // CDR-004 §7: recursive and mutually recursive functions
            // require a declared return type — an inference tainted
            // by a call into a pending-return function is exactly
            // that shape.
            if tainted && hit_pending {
                self.former.error(
                    codes::MALFORMED_TYPE,
                    format!("`{name}` is (mutually) recursive: declare its return type (§7)"),
                    decl.name.span,
                );
            }
            self.patch_fn_ret(&name, ret.clone());
            self.clear_pending(&name);
            final_ret = Some(ret);
        }
        if let Some(ret) = final_ret {
            let origins = decl
                .type_params
                .iter()
                .map(|parameter| {
                    Some(Type::Skolem {
                        name: self.former.text(parameter.span).to_string(),
                        id: u32::MAX,
                        origin: format!(
                            "source:{}:{}:{}",
                            parameter.span.file.0, parameter.span.lo, parameter.span.hi
                        ),
                    })
                })
                .collect::<Vec<_>>();
            let signature = substitute(
                &Type::Func {
                    params: signature_params,
                    variadic: signature_variadic,
                    ret: Box::new(ret),
                },
                &origins,
            );
            self.record_typed_node(
                topaz_hir::TypedNodeKind::Declaration,
                decl.name.span,
                &signature,
            );
        }
    }

    /// Checks one callable body under a fresh return and loop-control boundary.
    /// Both free functions and receiver methods use this exact lifecycle so a
    /// nested callable cannot inherit loop targets or partial-return state from
    /// its enclosing expression.
    pub(super) fn check_callable_body(
        &mut self,
        body: &'a ast::Block,
        declared_ret: Option<&Type>,
    ) -> CallableBodyCheck {
        self.ret_ctx.push(declared_ret.cloned());
        self.ret_join.push(Vec::new());
        let saved_loop_ctx = std::mem::take(&mut self.loop_ctx);
        let saved_hit = std::mem::take(&mut self.hit_pending_ret);
        let saved_collect = self.collect_partial;
        if declared_ret.is_none() {
            // Omitted-return bodies may mutually complete (`Ok`/`Err` pairs)
            // through the join solver.
            self.collect_partial = true;
        }
        // A declared return is the body tail's contextual type (§22.1), so
        // literal unions and unsolved constructors check against it before they
        // widen. An omitted return retains the inferred tail for the final join.
        let tail = match (declared_ret, body.tail.is_some()) {
            (Some(ret), true) => {
                self.check_block_with(body, Some(ret));
                None
            }
            _ => {
                let ty = self.check_block(body);
                body.tail.as_ref().map(|_| ty)
            }
        };
        self.collect_partial = saved_collect;
        self.loop_ctx = saved_loop_ctx;
        let returns = self.ret_join.pop().expect("ret_join stack");
        let hit_pending_return = self.hit_pending_ret;
        self.hit_pending_ret = saved_hit;
        self.ret_ctx.pop();
        CallableBodyCheck {
            returns,
            tail,
            hit_pending_return,
        }
    }

    /// Finalizes an omitted callable result from explicit returns and the body
    /// tail. The boolean reports whether the join contained a true unknown; free
    /// functions combine it with pending-call observation for the recursive
    /// omitted-return diagnostic, while methods only publish the joined type.
    pub(super) fn infer_callable_return(
        &mut self,
        body: &'a ast::Block,
        checked: CallableBodyCheck,
        span: Span,
    ) -> (Type, bool) {
        let mut members = checked.returns;
        match checked.tail {
            Some(ty) => members.push(ty),
            None if block_diverges(body) => {}
            None => members.push(Type::Prim(Prim::Unit)),
        }
        let tainted = members.iter().any(contains_true_unknown);
        let result = if members.is_empty() {
            Type::Prim(Prim::Unit)
        } else if tainted {
            Type::Unknown
        } else {
            self.join_branches(members, None, false, span)
        };
        (result, tainted)
    }

    /// Rewrites a hoisted function binding's return type once the
    /// body join is known (omitted-return inference).
    pub(super) fn patch_fn_ret(&mut self, name: &str, new_ret: Type) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(Type::Func { ret, .. }) = scope.bindings.get_mut(name) {
                **ret = new_ret;
                return;
            }
        }
    }

    pub(super) fn check_block(&mut self, block: &'a ast::Block) -> Type {
        self.check_block_bare(block, false)
    }

    /// `bare` marks a block that IS the initializer of an unannotated
    /// binding: its tail value sits at the §22.1-reporting site.
    pub(super) fn check_block_bare(&mut self, block: &'a ast::Block, bare: bool) -> Type {
        self.push_scope();
        // Nested `type` declarations are lexically scoped (SPEC §5);
        // the block's aliases pre-collect so forward references
        // resolve within the frame, then validate here. Functions
        // hoist per scope.
        self.former.push_alias_frame();
        self.former.collect_frame(&block.stmts);
        let base = self.tyenv();
        self.former.validate_aliases_in(&base);
        self.hoist_functions(&block.stmts);
        for stmt in &block.stmts {
            self.check_stmt(stmt);
        }
        let ty = match &block.tail {
            Some(tail) => {
                self.at_bare_binding = bare;
                self.infer(tail)
            }
            None => Type::Prim(Prim::Unit),
        };
        self.former.pop_alias_frame();
        self.pop_scope();
        ty
    }

    pub(super) fn expect(&mut self, found: &Type, expected: &Type, span: Span) {
        if found.has_unknown() || expected.has_unknown() {
            return;
        }
        if !is_subtype(found, expected) {
            // A singleton-literal `found` against a plain-primitive expectation reads
            // better as its base type: `expected `int`, found `string`` rather than
            // `found `"default"`` (e.g. the right operand of `??`). When the
            // expectation is itself literal-bearing (a literal union), the specific
            // literal IS the informative part, so keep it.
            let shown = if matches!(expected, Type::Prim(_)) {
                found.clone().widen()
            } else {
                found.clone()
            };
            self.former.error(
                codes::TYPE_MISMATCH,
                format!("expected `{expected}`, found `{shown}`"),
                span,
            );
        }
    }

    pub(super) fn type_satisfies_protocol_bound(&self, ty: &Type, protocol: &str) -> GateCheck {
        if self.former.protocol(protocol).is_none() {
            return GateCheck::Defer;
        }
        match ty {
            Type::Unknown | Type::Var(_) => GateCheck::Defer,
            Type::Skolem { id, .. } => {
                if self.skolem_bounds.iter().rev().any(|frame| {
                    frame
                        .get(id)
                        .is_some_and(|protocols| protocols.contains(protocol))
                }) {
                    GateCheck::Accept
                } else {
                    GateCheck::Reject
                }
            }
            Type::NominalRecord { base, args }
            | Type::Enum { base, args }
            | Type::Newtype { base, args } => {
                let id = nominal_instance_id(base, args);
                if self.former.conforms(protocol, &id) {
                    GateCheck::Accept
                } else {
                    GateCheck::Reject
                }
            }
            Type::Foreign { name, args } if args.is_empty() => {
                if self.former.conforms(protocol, name) {
                    GateCheck::Accept
                } else {
                    GateCheck::Reject
                }
            }
            other if other.has_unknown() => GateCheck::Defer,
            _ => GateCheck::Reject,
        }
    }

    pub(super) fn check_protocol_bounds(
        &mut self,
        bounds: &[Vec<String>],
        subst: &[Option<Type>],
        span: Span,
    ) -> bool {
        let mut ok = true;
        for (i, protocols) in bounds.iter().enumerate() {
            let Some(Some(ty)) = subst.get(i) else {
                continue;
            };
            if type_has_var(ty) || contains_true_unknown(ty) {
                continue;
            }
            for protocol in protocols {
                match self.type_satisfies_protocol_bound(ty, protocol) {
                    GateCheck::Accept | GateCheck::Defer => {}
                    GateCheck::Reject => {
                        ok = false;
                        self.former.error(
                            codes::NO_CONFORMANCE,
                            format!("`{ty}` does not satisfy protocol bound `{protocol}`"),
                            span,
                        );
                    }
                }
            }
        }
        ok
    }

    pub(super) fn expect_bool(&mut self, found: &Type, span: Span) {
        self.expect(found, &Type::Prim(Prim::Bool), span);
    }

    /// Check-mode (CDR-004 §4): the expected type flows into the
    /// expression — lambda parameters, empty collections, and
    /// constructor type variables resolve from it (§22.1).
    /// When a record literal is checked against a UNION expected type, pick the
    /// single union member it narrows to: a record whose field names match the
    /// literal's, with any literal discriminant the literal supplies (e.g.
    /// `kind: "a"`) overlapping that member's field type. `None` if zero or more
    /// than one member survives (ambiguous → fall back to plain inference).
    pub(super) fn narrow_record_literal(
        &mut self,
        fields: &'a [ast::FieldInit],
        members: &[Type],
    ) -> Option<Type> {
        let lit_names: Vec<&str> = fields
            .iter()
            .map(|f| self.former.text(f.name.span))
            .collect();
        let mut chosen: Option<Type> = None;
        for m in members {
            let Type::Record(mf) = m else { continue };
            // Records are exact: the literal must name precisely the member's fields.
            if mf.len() != lit_names.len()
                || !lit_names.iter().all(|n| mf.iter().any(|(mn, _)| mn == n))
            {
                continue;
            }
            // Each plain-literal field the literal supplies must be assignable to
            // the member's field type — this disambiguates `x: int` vs
            // `x: string` members, not just literal-typed discriminants.
            let mut ok = true;
            for field in fields {
                let fname = self.former.text(field.name.span);
                if let Some((_, ft)) = mf.iter().find(|(n, _)| n == fname)
                    && is_plain_literal(&field.value)
                    && decidable_type(ft)
                {
                    // Probe the literal's type WITHOUT emitting diagnostics — the
                    // real per-field check below reports any (once).
                    let before = self.former.diagnostics.len();
                    let lt = self.infer(&field.value);
                    self.former.diagnostics.truncate(before);
                    if decidable_type(&lt) && !type_overlap(&lt, ft) {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                if chosen.is_some() {
                    return None; // ambiguous — more than one member matches
                }
                chosen = Some(m.clone());
            }
        }
        chosen
    }

    /// Joins branch/arm result types. Under a context the literal
    /// arm types survive (each arm already checked against the
    /// expectation); in a bare position the join widens (§4) and
    /// partially solved arms (§22.1) first solve against each other
    /// — only an unsolvable join reports TPZ5020, and any true
    /// Unknown keeps the join silent (ambient suppression).
    pub(super) fn join_branches(
        &mut self,
        contributing: Vec<Type>,
        ctx: Option<&Type>,
        bare: bool,
        span: Span,
    ) -> Type {
        if contributing.is_empty() || contributing.iter().any(contains_true_unknown) {
            return Type::Unknown;
        }
        if contributing.iter().any(type_has_var) {
            // Compress the partial vars to a dense local space so
            // the substitution vector stays small.
            let mut index: Vec<u32> = Vec::new();
            for t in &contributing {
                collect_vars_into(t, &mut index);
            }
            let dense: Vec<Type> = contributing.iter().map(|t| remap_vars(t, &index)).collect();
            let mut subst: Vec<Option<Type>> = vec![None; index.len()];
            for i in 0..dense.len() {
                for j in 0..dense.len() {
                    if i != j {
                        unify_with(&dense[i], &dense[j], &mut subst, false);
                    }
                }
            }
            // Solve to a fixed point: bindings may chain
            // (`v0 := Array<v1>`, `v1 := int`); the occurs check
            // guarantees termination within |subst| rounds.
            let solved: Vec<Type> = dense
                .iter()
                .map(|t| {
                    let mut cur = substitute(t, &subst);
                    for _ in 0..subst.len() {
                        let next = substitute(&cur, &subst);
                        if next == cur {
                            break;
                        }
                        cur = next;
                    }
                    cur
                })
                .collect();
            if solved.iter().any(type_has_var) {
                if bare {
                    self.former.error(
                        codes::UNSOLVED,
                        "this expression needs a contextual type (§22.1): annotate the binding or surrounding expectation"
                            .to_string(),
                        span,
                    );
                }
                return Type::Unknown;
            }
            self.resolve_recorded_dense_inference(&index, &subst);
            return Type::union(solved.into_iter().map(Type::widen).collect());
        }
        if ctx.is_some() {
            Type::union(contributing)
        } else {
            Type::union(contributing.into_iter().map(Type::widen).collect())
        }
    }

    /// A literal integer constant through parens and unary signs —
    /// just enough to judge the §10 constant-zero-step rule.
    /// A fresh RIGID projection of an abstract member — a generic `T`
    /// (`Type::Skolem`) or a `Foreign` type — e.g. `FieldOf<T, x>` for a record
    /// field or `ElemOf<T>` for a list element. Represented as a synthetic
    /// `Type::Skolem`, so it is rigid: `has_unknown` is false and subtyping
    /// accepts it only by equality. Thus it can neither discharge a CONCRETE
    /// expectation (the parametricity hole — `case { x } => x` on `{x:int} | T`
    /// can no longer be returned as `int`) NOR be used as the base `T` itself
    /// (over `T = { x: string }`, the binding is the field, not `T`). Its id
    /// comes from `skolem_counter` but is NEVER recorded in a type-param map, so
    /// `skolems_to_vars` leaves it rigid instead of reviving it as an inference
    /// variable.
    pub(super) fn project(&mut self, name: String) -> Type {
        self.skolem_counter += 1;
        self.projection_ids.push(self.skolem_counter);
        Type::Skolem {
            origin: format!("projection:{name}"),
            name,
            id: self.skolem_counter,
        }
    }

    /// If typing (C-5): the §15/§5 branch-compatibility rule shared
    /// with match — branches are context sites, divergent branches
    /// drop out of the join, a missing else contributes `()`.
    pub(super) fn check_if(
        &mut self,
        cond: &'a ast::Expr,
        then_block: &'a ast::Block,
        else_branch: Option<&'a ast::Expr>,
        ctx: Option<&Type>,
        bare: bool,
        span: Span,
    ) -> Type {
        let cond_ty = self.infer(cond);
        self.expect_bool(&cond_ty, cond.span);
        let saved_collect = self.collect_partial;
        if ctx.is_none() {
            self.collect_partial = true;
        }
        let mut branches: Vec<Option<Type>> = Vec::new();
        if block_diverges(then_block) {
            self.check_block(then_block);
            branches.push(None);
        } else {
            branches.push(Some(self.check_block_with(then_block, ctx)));
        }
        match else_branch {
            Some(e) if arm_diverges(e) => {
                self.infer(e);
                branches.push(None);
            }
            Some(e) => {
                let ty = match ctx {
                    Some(expected) => self.check_expr(e, expected),
                    None => self.infer(e),
                };
                branches.push(Some(ty));
            }
            None => branches.push(Some(Type::Prim(Prim::Unit))),
        }
        self.collect_partial = saved_collect;
        let contributing: Vec<Type> = branches.into_iter().flatten().collect();
        self.join_branches(contributing, ctx, bare, span)
    }

    /// A block whose tail is a context site: statements check as
    /// usual, the tail checks against the expectation.
    pub(super) fn check_block_with(&mut self, block: &'a ast::Block, ctx: Option<&Type>) -> Type {
        let Some(expected) = ctx else {
            return self.check_block(block);
        };
        self.push_scope();
        self.former.push_alias_frame();
        self.former.collect_frame(&block.stmts);
        let base = self.tyenv();
        self.former.validate_aliases_in(&base);
        self.hoist_functions(&block.stmts);
        for stmt in &block.stmts {
            self.check_stmt(stmt);
        }
        let ty = match &block.tail {
            Some(tail) => self.check_expr(tail, expected),
            None => Type::Prim(Prim::Unit),
        };
        self.former.pop_alias_frame();
        self.pop_scope();
        ty
    }

    /// Match typing (CDR-004 §5, phase C-4): pattern bindings narrow
    /// from the scrutinee, arm bodies are §22.1 context sites, the
    /// result joins the arm types, and decidable scrutinee domains
    /// (bool, literal unions, Option, Result) check exhaustiveness.
    pub(super) fn check_match(
        &mut self,
        scrutinee: &'a ast::Expr,
        cases: &'a [ast::CaseClause],
        ctx: Option<&Type>,
        span: Span,
        bare: bool,
    ) -> Type {
        let scrutinee_ty = self.infer(scrutinee);
        let saved_collect = self.collect_partial;
        if ctx.is_none() {
            self.collect_partial = true;
        }
        let mut coverage = Coverage::default();
        let mut arm_types: Vec<Option<Type>> = Vec::new();
        for case in cases {
            self.push_scope();
            let cov = self.bind_match_pattern(&case.pattern, &scrutinee_ty);
            if let Some(guard) = &case.guard {
                let guard_ty = self.infer(guard);
                self.expect_bool(&guard_ty, guard.span);
            } else {
                coverage.merge(cov);
            }
            match &case.body {
                ast::CaseArmBody::Expr(e) if arm_diverges(e) => {
                    // A block arm ending in `return` diverges like a
                    // return arm: it is not checked against the
                    // context and contributes nothing to the join.
                    self.infer(e);
                    arm_types.push(None);
                }
                ast::CaseArmBody::Expr(e) => {
                    let ty = match ctx {
                        Some(expected) => self.check_expr(e, expected),
                        None => self.infer(e),
                    };
                    arm_types.push(Some(ty));
                }
                ast::CaseArmBody::Return { value, span: rspan } => {
                    // A return arm diverges out of the match; it does
                    // not contribute to the join (CDR-004 §4).
                    // Like a `return` statement, a `return` arm at the module top
                    // level is "`return` outside a function" — reject it statically
                    // (TPZ5001) so `check` matches `run` here too.
                    if self.ret_ctx.is_empty() {
                        self.former.error(
                            codes::TYPE_MISMATCH,
                            "`return` outside a function".to_string(),
                            *rspan,
                        );
                    }
                    let expected = self.ret_ctx.last().cloned().flatten();
                    match (value, expected) {
                        (Some(v), Some(ret)) => {
                            self.check_expr(v, &ret);
                        }
                        (Some(v), None) => {
                            let ty = self.infer(v);
                            if let Some(join) = self.ret_join.last_mut() {
                                join.push(ty);
                            }
                        }
                        (None, Some(ret)) => {
                            // Bare `return` yields `()` (§7).
                            self.expect(&Type::Prim(Prim::Unit), &ret, *rspan);
                        }
                        (None, None) => {
                            if let Some(join) = self.ret_join.last_mut() {
                                join.push(Type::Prim(Prim::Unit));
                            }
                        }
                    }
                    arm_types.push(None);
                }
            }
            self.pop_scope();
        }
        self.collect_partial = saved_collect;
        // §3 (v5.3) enum exhaustiveness: every declared variant must be covered
        // by an unguarded arm, unless a wildcard makes the match irrefutable. The
        // enum's variant set lives in the former (out of `Coverage`'s reach), so
        // the missing-set is computed here.
        let enums = self.former.enum_table();
        if let Type::Enum { base, args } = &scrutinee_ty {
            let id = nominal_instance_id(base, args);
            if !coverage.irrefutable
                && let Some(info) = enums.get(&id)
            {
                // PAYLOAD-AWARE coverage (`variant_covered`): payload-less → tag
                // present; single-payload → nested coverage exhausts the payload
                // type (`Circle(_)` covers, `Circle(1)` does not); multi-payload →
                // conservative (every position irrefutable).
                let missing: Vec<String> = info
                    .variants
                    .iter()
                    .filter(|v| !coverage.variant_covered(v, enums))
                    .map(|v| format!("`{}.{}`", id, v.name))
                    .collect();
                if !missing.is_empty() {
                    self.former.error(
                        codes::NON_EXHAUSTIVE,
                        format!("non-exhaustive match: missing {}", missing.join(", ")),
                        span,
                    );
                }
            }
        } else if let Some(missing) = coverage.missing(&scrutinee_ty, enums)
            && !missing.is_empty()
        {
            self.former.error(
                codes::NON_EXHAUSTIVE,
                format!("non-exhaustive match: missing {}", missing.join(", ")),
                span,
            );
        }
        let contributing: Vec<Type> = arm_types.into_iter().flatten().collect();
        self.join_branches(contributing, ctx, bare, span)
    }
}
