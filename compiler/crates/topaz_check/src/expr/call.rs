use super::*;

pub(super) fn specialized_call_callee_type(scheme: &Scheme, subst: &[Option<Type>]) -> Type {
    Type::Func {
        params: scheme
            .params
            .iter()
            .map(|param| substitute(param, subst))
            .collect(),
        variadic: scheme
            .variadic
            .as_ref()
            .map(|param| Box::new(substitute(param, subst))),
        ret: Box::new(substitute(&scheme.ret, subst)),
    }
}

impl<'a> ExprChecker<'a> {
    pub(super) fn report_no_type_args(&mut self, count: usize, span: Span) {
        self.former.error(
            codes::NO_TYPE_ARGS,
            format!(
                "this call is not generic, but {} type argument{} {} supplied",
                count,
                if count == 1 { "" } else { "s" },
                if count == 1 { "was" } else { "were" },
            ),
            span,
        );
    }

    // ---- call typing (C-3) ------------------------------------------

    pub(super) fn infer_call<'context>(&mut self, request: CallRequest<'a, 'context>) -> Type {
        let CallRequest {
            callee,
            args,
            type_args,
            context: ctx,
            span,
            bare,
            leading,
        } = request;
        let lispex_rule_target = match &callee.kind {
            ast::ExprKind::Ident => self
                .lispex_rule_factories
                .get(self.former.text(callee.span))
                .cloned(),
            ast::ExprKind::Member { object, field }
                if matches!(object.kind, ast::ExprKind::Ident) =>
            {
                self.lispex_rule_namespaces
                    .get(self.former.text(object.span))
                    .and_then(|members| members.get(self.former.text(field.span)))
                    .cloned()
            }
            _ => None,
        };
        // §12 OptionalCall: a Call whose callee is an OptionalAccess.
        // §9 is not statically enforced here — `?.` short-circuits on
        // None, so a mutator through an immutable optional root is not
        // a guaranteed mutation; the runtime enforces it on the Some
        // branch.
        if let ast::ExprKind::OptionalAccess { object, field } = &callee.kind {
            if !type_args.is_empty() {
                self.report_no_type_args(type_args.len(), span);
            }
            let object_ty = self.infer(object);
            return self.optional_member(
                &object_ty,
                field,
                span,
                Some(OptionalCallInput {
                    args,
                    leading,
                    callee_span: callee.span,
                }),
            );
        }
        // The builtin `print` is `string`-only (§22.2). Knowing the callee lets
        // `apply_scheme` point a non-string argument at the interpolation form — the
        // single most common slip for a newcomer from Python/JS — instead of a bare
        // mismatch. Gated on `lookup(...).is_none()` so a USER-defined `print` (which
        // shadows the prelude) does not get the builtin's §22.2 hint.
        let callee_is_print = matches!(callee.kind, ast::ExprKind::Ident)
            && self.former.text(callee.span) == "print"
            && self.lookup("print").is_none();
        let site = CallSite {
            args,
            type_args,
            context: ctx,
            span,
            bare,
            leading,
            is_print: callee_is_print,
        };
        // §3 (v5.4) set when the callee is `Set.of(..)`/`Map.new()`: those infer their
        // key/element type from the args (no annotation), so the inferred RESULT is
        // re-inspected for a newtype key after `apply_scheme` — the runtime `freeze`
        // mirror that the `form.rs` annotation check cannot reach.
        let mut gates = CallGates::default();
        // §22 (v5.4) set when the callee is an Array `.sorted()` / `.sortedBy(f)`: the
        // ELEMENT (sorted) or KEY (sortedBy) type must be ORDER-comparable, gated AFTER
        // `apply_scheme` so check==runtime (the runtime `values_compare` leaf agrees) —
        // no more `.sorted()` passing check then faulting at run. `SortGate::Element`
        // carries the receiver element type to gate; `SortGate::Key` marks that the
        // callback's RETURN type (probed from the arg) must be gated.
        // §22 (v5.4) set when the callee is `JSON.stringify(value)`: the single argument's
        // type must be JSON-encodable, gated AFTER `apply_scheme` so check==runtime (the
        // shared `encode_json` leaf would otherwise return a runtime `Err`).
        // §22 (v5.4) set when the callee is `JSON.parseAs<T>(text)` or
        // `JSON.decode<T>(value)`: the explicit target `T` must be JSON-decodable
        // and fully known so both engines can lower the same runtime schema.
        let resolution = match &callee.kind {
            ast::ExprKind::Ident => self.resolve_ident_call(callee, site, &mut gates),
            ast::ExprKind::Member { object, field } => {
                self.resolve_member_call(callee, object, field, site, &mut gates)
            }
            _ => {
                let callee_ty = self.infer(callee);
                match callee_ty {
                    Type::Func {
                        params,
                        variadic,
                        ret,
                    } => CallResolution::Apply(ResolvedCall {
                        scheme: Scheme {
                            vars: 0,
                            names: vec![],
                            names_known: false,
                            required: params.len(),
                            defaulted: Vec::new(),
                            params,
                            variadic: variadic.map(|b| *b),
                            ret: *ret,
                        },
                        iterable_param_fixup: false,
                        target_identity: None,
                    }),
                    Type::Unknown | Type::Var(_) => CallResolution::Unresolved,
                    other => {
                        self.former.error(
                            codes::NOT_CALLABLE,
                            format!("`{other}` is not callable"),
                            callee.span,
                        );
                        CallResolution::Unresolved
                    }
                }
            }
        };
        let resolved = match resolution {
            CallResolution::Apply(resolved) => Some(resolved),
            CallResolution::Complete(result) => return result,
            CallResolution::Unresolved => None,
        };
        self.finish_inferred_call(CallCompletion {
            callee,
            site,
            lispex_rule_target,
            resolved,
            gates,
        })
    }

    pub(super) fn resolve_member_call<'context>(
        &mut self,
        callee: &'a ast::Expr,
        object: &'a ast::Expr,
        field: &'a ast::Ident,
        site: CallSite<'a, 'context>,
        gates: &mut CallGates,
    ) -> CallResolution {
        let CallSite {
            args,
            type_args,
            context,
            span,
            leading,
            ..
        } = site;
        if let ast::ExprKind::Ident = object.kind {
            let head = self.former.text(object.span);
            // A called enum-variant head (`Shape.Circle(3)`) constructs the
            // variant's single payload.
            if self.lookup(head).is_none() && self.former.is_enum(head) {
                if !type_args.is_empty() {
                    self.report_no_type_args(type_args.len(), span);
                }
                let construction = self.enum_construct(head, field, args, span, context);
                if let Some(callee_type) = construction.callee_type {
                    self.record_typed_call_callee(span, &callee_type);
                    self.record_typed_node(
                        topaz_hir::TypedNodeKind::Expression,
                        callee.span,
                        &callee_type,
                    );
                }
                return CallResolution::Complete(construction.result);
            }
            // Declared protocols use their static dispatch contract unless a
            // builtin static namespace member owns the same surface.
            if self.lookup(head).is_none()
                && !self.namespaces.contains_key(head)
                && self.former.protocol(head).is_some()
                && builtins::static_member(head, self.former.text(field.span)).is_none()
            {
                if !type_args.is_empty() {
                    self.report_no_type_args(type_args.len(), span);
                }
                return CallResolution::Complete(self.protocol_call(
                    head,
                    field,
                    args,
                    span,
                    callee.span,
                ));
            }
        }

        let mut static_scheme = None;
        let mut namespace_call: Option<Option<Scheme>> = None;
        if let ast::ExprKind::Ident = object.kind {
            let head = self.former.text(object.span);
            if self.lookup(head).is_none() {
                // An imported namespace shadows prelude static heads.
                if !self.namespaces.contains_key(head) {
                    let member = self.former.text(field.span);
                    gates.check_inferred_key = (head == "Set" && member == "of")
                        || (head == "Map" && matches!(member, "new" | "ofEntries"));
                    gates.json_encode = head == "JSON" && member == "stringify";
                    if head == "JSON" && matches!(member, "parseAs" | "decode") {
                        gates.json_decode = Some(member.to_string());
                    }
                    if head == "Test" && matches!(member, "assertEq" | "assertNe") {
                        gates.equality_assertion = Some(member.to_string());
                    }
                    static_scheme = builtins::static_member(head, member);
                }
                if static_scheme.is_none() && self.namespaces.contains_key(head) {
                    let head = head.to_string();
                    let member_ty = self.namespace_member(&head, field);
                    namespace_call = Some(match member_ty {
                        Type::Func {
                            params,
                            variadic,
                            ret,
                        } => {
                            let member = self.former.text(field.span);
                            let surface = self.namespaces.get(&head).expect("namespace checked");
                            let meta = surface.values.get(member).cloned();
                            gates.protocol_bounds = meta
                                .as_ref()
                                .map(|value| value.bounds.clone())
                                .unwrap_or_default();
                            Some(Scheme {
                                vars: meta.as_ref().map_or(0, |value| value.vars),
                                names: meta
                                    .as_ref()
                                    .map(|value| value.names.clone())
                                    .unwrap_or_default(),
                                names_known: meta.as_ref().is_some_and(|value| value.names_known),
                                required: meta
                                    .as_ref()
                                    .map_or(params.len(), |value| value.required),
                                defaulted: meta
                                    .as_ref()
                                    .map(|value| value.defaulted.clone())
                                    .unwrap_or_default(),
                                params,
                                variadic: variadic.map(|value| *value),
                                ret: *ret,
                            })
                        }
                        ty if ty.has_unknown() => None,
                        other => {
                            self.former.error(
                                codes::NOT_CALLABLE,
                                format!("`{other}` is not callable"),
                                field.span,
                            );
                            None
                        }
                    });
                }
            }
        }
        if let Some(resolved) = namespace_call {
            return resolved.map_or(CallResolution::Unresolved, |scheme| {
                CallResolution::Apply(ResolvedCall {
                    scheme,
                    iterable_param_fixup: false,
                    target_identity: None,
                })
            });
        }
        if let Some(scheme) = static_scheme {
            return CallResolution::Apply(ResolvedCall {
                scheme,
                iterable_param_fixup: false,
                target_identity: None,
            });
        }

        let object_ty = self.infer(object);
        let member = self.former.text(field.span);
        // User receiver methods precede builtins and callable fields.
        if let Some(id) = nominal_type_id(&object_ty)
            && let Some((scheme, target)) = self.method_scheme(&id, member)
        {
            return CallResolution::Apply(ResolvedCall {
                scheme,
                iterable_param_fixup: false,
                target_identity: target,
            });
        }

        let is_mut_call =
            builtins::is_mutator(&object_ty, member) || union_arm_is_mutator(&object_ty, member);
        if let Type::Ctor(Ctor::Array, elements) = &object_ty {
            if matches!(member, "sorted" | "sort") {
                gates.sort = Some(CallSortGate::Element(
                    elements[0].clone(),
                    member.to_string(),
                ));
            } else if matches!(member, "sortedBy" | "sortBy") {
                gates.sort = Some(CallSortGate::Key(member.to_string()));
            }
        }
        let resolution = self.resolve_receiver_member_call(&object_ty, field, args, leading, span);
        if is_mut_call {
            self.check_mutation_root(object);
        }
        resolution
    }

    pub(super) fn resolve_ident_call<'context>(
        &mut self,
        callee: &'a ast::Expr,
        site: CallSite<'a, 'context>,
        gates: &mut CallGates,
    ) -> CallResolution {
        let CallSite {
            args,
            type_args,
            context,
            span,
            ..
        } = site;
        let name = self.former.text(callee.span).to_string();
        // §3 (v5.4) NEWTYPE construction `UserId(5)`: the callee is a declared
        // newtype NOT shadowed by a binding. Type-check the single arg against
        // the base and yield the nominal `Type::Newtype`.
        if self.lookup(&name).is_none() && self.former.is_newtype(&name) {
            if !type_args.is_empty() {
                self.report_no_type_args(type_args.len(), span);
            }
            return CallResolution::Complete(self.newtype_construct(&name, args, span, context));
        }
        if self.resolves_to_pending(&name) {
            self.hit_pending_ret = true;
        }
        if let Some(bound) = self.lookup(&name).cloned() {
            return match bound {
                Type::Func {
                    params,
                    variadic,
                    ret,
                } => {
                    let meta = self.fn_meta_of(&name).cloned();
                    gates.protocol_bounds = meta
                        .as_ref()
                        .map(|metadata| metadata.bounds.clone())
                        .unwrap_or_default();
                    CallResolution::Apply(ResolvedCall {
                        scheme: Scheme {
                            vars: meta.as_ref().map_or(0, |metadata| metadata.vars),
                            names: meta
                                .as_ref()
                                .and_then(|metadata| metadata.names.clone())
                                .unwrap_or_default(),
                            names_known: meta
                                .as_ref()
                                .is_some_and(|metadata| metadata.names.is_some()),
                            required: meta
                                .as_ref()
                                .map_or(params.len(), |metadata| metadata.required),
                            defaulted: meta
                                .as_ref()
                                .map(|metadata| metadata.defaulted.clone())
                                .unwrap_or_default(),
                            params,
                            variadic: variadic.map(|value| *value),
                            ret: *ret,
                        },
                        iterable_param_fixup: false,
                        target_identity: None,
                    })
                }
                Type::Unknown | Type::Var(_) => CallResolution::Unresolved,
                other => {
                    self.former.error(
                        codes::NOT_CALLABLE,
                        format!("`{other}` is not callable"),
                        callee.span,
                    );
                    CallResolution::Unresolved
                }
            };
        }

        let resolved = builtins::free_function(&name)
            .map(|scheme| (scheme, builtins::iterable_param_fixup(&name)));
        if resolved.is_none() && self.module_mode && !self.namespaces.contains_key(&name) {
            // C-6: an unbound callee in a closed unit.
            let hint = self.unbound_callee_hint(&name);
            self.former.error(
                codes::UNBOUND,
                format!("`{name}` is not bound{hint}"),
                callee.span,
            );
        }
        resolved.map_or(CallResolution::Unresolved, |(scheme, fixup)| {
            CallResolution::Apply(ResolvedCall {
                scheme,
                iterable_param_fixup: fixup,
                target_identity: None,
            })
        })
    }

    pub(super) fn resolve_receiver_member_call(
        &mut self,
        object_ty: &Type,
        field: &'a ast::Ident,
        args: &'a [ast::CallArg],
        leading: Option<&Type>,
        call_span: Span,
    ) -> CallResolution {
        let member = self.former.text(field.span);
        let resolved = match builtins::receiver_member(object_ty, member) {
            Some(Member::Method(scheme)) => Some((scheme, false)),
            Some(Member::Property(ty)) => {
                self.former.error(
                    codes::NOT_CALLABLE,
                    format!("`{ty}` is not callable"),
                    field.span,
                );
                None
            }
            None => match object_ty {
                Type::Record(record_fields) => {
                    match record_fields.iter().find(|(n, _)| n == member) {
                        Some((
                            _,
                            Type::Func {
                                params,
                                variadic,
                                ret,
                            },
                        )) => Some((
                            Scheme {
                                vars: 0,
                                names: vec![],
                                names_known: false,
                                required: params.len(),
                                defaulted: Vec::new(),
                                params: params.clone(),
                                variadic: variadic.as_deref().cloned(),
                                ret: (**ret).clone(),
                            },
                            false,
                        )),
                        Some((_, other)) => {
                            let other = other.clone();
                            self.former.error(
                                codes::NOT_CALLABLE,
                                format!("`{other}` is not callable"),
                                field.span,
                            );
                            None
                        }
                        None => {
                            let display = object_ty.to_string();
                            // A record-field CALL suggests only
                            // FUNCTION-typed fields (callable), never a
                            // plain data field — C4.
                            let hint = topaz_diag::suggest::did_you_mean(
                                member,
                                record_fields
                                    .iter()
                                    .filter(|(_, t)| matches!(t, Type::Func { .. }))
                                    .map(|(n, _)| n.as_str()),
                            );
                            self.former.error(
                                codes::NO_FIELD,
                                format!("`{display}` has no field `{member}`{hint}"),
                                field.span,
                            );
                            None
                        }
                    }
                }
                // §3 (v5.4) a NEWTYPE method call `id.value()`: route through
                // `member_type`, which knows the newtype's SOLE member
                // `.value()` (zero-arg, returns the BASE) and emits NO_FIELD
                // for any other member. A `Func` result becomes a real
                // scheme so the RESULT type (the base) flows AND arity is
                // checked at CHECK time — without this the `other` arm below
                // would resolve to `Unknown`, so `let s: string = id.value()`
                // and `id.value(2)` would wrongly pass check then fault at run.
                Type::Newtype { .. } => {
                    match self.member_type(object_ty, field, field.span) {
                        Type::Func {
                            params,
                            variadic,
                            ret,
                        } => Some((
                            Scheme {
                                vars: 0,
                                names: vec![],
                                names_known: false,
                                required: params.len(),
                                defaulted: Vec::new(),
                                params,
                                variadic: variadic.map(|v| *v),
                                ret: *ret,
                            },
                            false,
                        )),
                        // `member_type` already emitted NO_FIELD (an absent
                        // member yields `Unknown`); infer the args and bail.
                        _ => None,
                    }
                }
                // §3 (v5.4) a NOMINAL record's callable field `b.f()`:
                // resolve the declared field type — a Func-typed field is
                // callable, a non-Func field is NOT_CALLABLE, an unknown
                // field is NO_FIELD. Mirrors the structural-record arm.
                Type::NominalRecord { base, args } => {
                    let id = nominal_instance_id(base, args);
                    let info = self.former.record_info(&id).cloned();
                    let field_ty = info
                        .as_ref()
                        .and_then(|i| i.fields.iter().find(|f| f.name == member))
                        .map(|f| f.ty.clone());
                    match field_ty {
                        Some(Type::Func {
                            params,
                            variadic,
                            ret,
                        }) => Some((
                            Scheme {
                                vars: 0,
                                names: vec![],
                                names_known: false,
                                required: params.len(),
                                defaulted: Vec::new(),
                                params,
                                variadic: variadic.map(|v| *v),
                                ret: *ret,
                            },
                            false,
                        )),
                        Some(other) => {
                            self.former.error(
                                codes::NOT_CALLABLE,
                                format!("`{other}` is not callable"),
                                field.span,
                            );
                            None
                        }
                        None => {
                            let known: Vec<&str> = info
                                .as_ref()
                                .map(|i| {
                                    i.fields
                                        .iter()
                                        .filter(|f| matches!(f.ty, Type::Func { .. }))
                                        .map(|f| f.name.as_str())
                                        .collect()
                                })
                                .unwrap_or_default();
                            let hint =
                                topaz_diag::suggest::did_you_mean(member, known.iter().copied());
                            self.former.error(
                                codes::NO_FIELD,
                                format!("record `{id}` has no field `{member}`{hint}"),
                                field.span,
                            );
                            None
                        }
                    }
                }
                // Any non-record receiver: reject an unknown member
                // CALL iff it is DECIDABLY absent — a member-closed
                // builtin/scalar that lacks it (`int`/`float`/`bool`
                // expose no methods — C3; `string`'s only member is
                // `scalars`, so a `string` method call is a static
                // error — C7), or a UNION with a member-closed
                // non-null arm that lacks it (that arm would
                // runtime-fault — C2). Opaque/`Var`/`Unknown`
                // receivers stay staged. `receiver_has_member` is the
                // shared, non-emitting source of truth.
                other => {
                    if receiver_has_member(other, member) == Some(false) {
                        let display = object_ty.to_string();
                        // A member CALL suggests only CALLABLE members
                        // (methods), never an access-only property
                        // like `length` — C4.
                        let callable = builtins::callable_member_names(other);
                        let hint =
                            topaz_diag::suggest::did_you_mean(member, callable.iter().copied());
                        self.former.error(
                            codes::NO_FIELD,
                            format!("`{display}` has no member named `{member}`{hint}"),
                            field.span,
                        );
                        None
                    } else if is_rigid_or_union_rigid(other) {
                        // A call on a rigid generic receiver (a bare
                        // generic, or a union with a rigid arm) has an
                        // unknowable result type; project it rigidly so it
                        // cannot silently discharge a concrete expectation
                        // (`fn steal<T>(t: T) -> int { t.foo() }` must NOT
                        // check). `project_call` checks the args against each
                        // concrete arm and infers them. Honor §9, short-circuit.
                        let member = member.to_string();
                        let callee_type = self
                            .project_member(other, &member)
                            .expect("rigid receiver projects a member");
                        self.record_typed_call_callee(call_span, &callee_type);
                        let result = self
                            .project_call(other, &member, args, leading, field.span)
                            .expect("rigid receiver projects a call");
                        return CallResolution::Complete(result);
                    } else {
                        None
                    }
                }
            },
        };
        resolved.map_or(CallResolution::Unresolved, |(scheme, fixup)| {
            CallResolution::Apply(ResolvedCall {
                scheme,
                iterable_param_fixup: fixup,
                target_identity: None,
            })
        })
    }

    pub(super) fn finish_inferred_call<'context>(
        &mut self,
        completion: CallCompletion<'a, 'context>,
    ) -> Type {
        let CallCompletion {
            callee,
            site,
            lispex_rule_target,
            resolved,
            gates,
        } = completion;
        let CallSite {
            args,
            type_args,
            context: ctx,
            span,
            ..
        } = site;
        let CallGates {
            check_inferred_key,
            sort: sort_gate,
            json_encode: json_gate,
            json_decode: json_decode_gate,
            equality_assertion: eq_assert_gate,
            protocol_bounds,
        } = gates;
        match resolved {
            Some(ResolvedCall {
                scheme,
                iterable_param_fixup,
                target_identity,
            }) => {
                // Retain the exact call-site-instantiated callable type.  Looking
                // up a named/static/member callee above intentionally avoids a
                // second generic expression inference, so without this explicit
                // fact the full Typed IR would know the call result but leave the
                // callee expression unexplained.
                let semantic_scheme = scheme.clone();
                let sort_key_ret = if matches!(sort_gate, Some(CallSortGate::Key(_))) {
                    scheme.params.first().and_then(|param| match param {
                        Type::Func { ret, .. } => Some((**ret).clone()),
                        _ => None,
                    })
                } else {
                    None
                };
                let json_param = if json_gate {
                    scheme.params.first().cloned()
                } else {
                    None
                };
                let eq_assert_param = if eq_assert_gate.is_some() {
                    scheme.params.first().cloned()
                } else {
                    None
                };
                let applied = self.apply_scheme_result(scheme, iterable_param_fixup, site);
                let bounds_ok = self.check_protocol_bounds(&protocol_bounds, &applied.subst, span);
                let result = if bounds_ok {
                    applied.ty.clone()
                } else {
                    Type::Unknown
                };
                let mut semantic_subst = applied.subst.clone();
                // A partial result is renumbered into the branch join's globally
                // fresh variable space. Carry that same identity into the callee
                // fact so the eventual join solution sharpens both facts together.
                unify_with(
                    &semantic_scheme.ret,
                    &applied.ty,
                    &mut semantic_subst,
                    false,
                );
                let callee_type = specialized_call_callee_type(&semantic_scheme, &semantic_subst);
                self.record_typed_call_callee(span, &callee_type);
                self.record_typed_node(
                    topaz_hir::TypedNodeKind::Expression,
                    callee.span,
                    &callee_type,
                );
                self.record_typed_call_target(span, target_identity.or(lispex_rule_target));
                // §3/§6 (v5.4) a `Set.of(...)`/`Map.new()` whose key/element was
                // INFERRED to a non-keyable type: reject statically (TPZ5007), the
                // same way an ANNOTATED `Set<T>`/`Map<K, _>` is rejected in `form.rs`
                // and the runtime `freeze` rejects the key — no check-pass-then-fault.
                // Skip when an expected type is present (`ctx`): the annotation's own
                // `form.rs` check already fired, so this would double-report.
                if check_inferred_key && ctx.is_none() {
                    let key = match &result {
                        Type::Ctor(Ctor::Set, args) => args.first(),
                        Type::Ctor(Ctor::Map, args) => args.first(),
                        _ => None,
                    };
                    if let Some(bad) = key.and_then(|key| self.non_keyable_map_set_key(key)) {
                        self.former.error(
                            codes::INCOMPARABLE,
                            format!(
                                "{} is not a valid Map/Set key ({} keys are not supported yet)",
                                bad.subject, bad.kind
                            ),
                            span,
                        );
                    }
                }
                // §22 (v5.4) the `.sorted()` / `.sortedBy(f)` ORDER-comparability gate:
                // reject a non-order-comparable element (sorted) or key (sortedBy) at
                // CHECK time, so check==runtime (the runtime `values_compare` leaf would
                // otherwise fault — the soundness gap). The gated type:
                //   - sorted:   the receiver element type T (captured in the gate).
                //   - sortedBy: the callback's RETURN type K, PROBED from the callback
                //     argument WITHOUT emitting (the arg was already checked by
                //     `apply_scheme`), so a non-orderable K is rejected without
                //     double-reporting the arg's own (none-expected) errors.
                if let Some(gate) = sort_gate {
                    let gated: Option<(Type, String, bool)> = match gate {
                        CallSortGate::Element(elem, method) => Some((elem, method, false)),
                        CallSortGate::Key(method) => sort_key_ret
                            .as_ref()
                            .map(|ret| (substitute(ret, &applied.subst), method, true)),
                    };
                    if let Some((ty, method, is_key)) = gated {
                        let enums = self.former.enum_table();
                        let records = self.former.record_table();
                        let newtypes = self.former.newtype_table();
                        if order_comparable_gate(&ty, enums, records, newtypes, &mut Vec::new())
                            == GateCheck::Reject
                        {
                            let what = if is_key {
                                format!("the `{method}` key type `{ty}`")
                            } else {
                                format!("`{ty}`")
                            };
                            self.former.error(
                                codes::INCOMPARABLE,
                                format!("{what} is not ordered comparable, so `.{method}()` cannot sort it"),
                                span,
                            );
                        }
                    }
                }
                // §22 (v5.4) the JSON-encodability gate for `JSON.stringify(value)`:
                // reject a non-encodable argument type at CHECK time so check==runtime
                // (the shared `encode_json` leaf would otherwise return a runtime `Err`).
                // Prefer the instantiated parameter that `apply_scheme` already solved.
                // Only an unresolved/unknown substitution needs a diagnostic-suppressed
                // probe of the sole argument; a resolved call must not type the argument
                // expression a second time.
                if json_gate {
                    let arg = args.first().map(|a| match a {
                        ast::CallArg::Positional(e)
                        | ast::CallArg::Named { value: e, .. }
                        | ast::CallArg::Spread(e) => e,
                    });
                    if let Some(e) = arg {
                        let ty = json_param
                            .as_ref()
                            .map(|param| substitute(param, &applied.subst))
                            .filter(|param| !type_has_var(param) && !contains_true_unknown(param))
                            .unwrap_or_else(|| {
                                let before = self.former.diagnostics.len();
                                let probed = self.infer(e);
                                self.former.diagnostics.truncate(before);
                                probed
                            });
                        let enums = self.former.enum_table();
                        let records = self.former.record_table();
                        let newtypes = self.former.newtype_table();
                        let structural =
                            json_encodable_status(&ty, enums, records, newtypes, &mut Vec::new());
                        let bounded = self.type_satisfies_protocol_bound(&ty, "JSON");
                        if structural == GateCheck::Reject && bounded != GateCheck::Accept {
                            self.former.error(
                                codes::NOT_JSON_ENCODABLE,
                                format!("`{ty}` is not JSON-encodable, so `JSON.stringify` cannot encode it"),
                                span,
                            );
                        }
                    }
                }
                if let Some(member) = json_decode_gate {
                    if type_args.len() == 1 {
                        let ty_ast = &type_args[0];
                        let env = self.tyenv();
                        let ty = self.former.form(ty_ast, &env);
                        let enums = self.former.enum_table();
                        let records = self.former.record_table();
                        let newtypes = self.former.newtype_table();
                        if self.former.version() < topaz_syntax::LangVersion::V5_20
                            && self.former.json_schema_crosses_module(ty_ast)
                        {
                            self.former.error(
                                codes::NOT_JSON_DECODABLE,
                                format!(
                                    "`JSON.{member}` cannot materialize an imported nominal or alias schema before Topaz 5.20"
                                ),
                                span,
                            );
                        } else if self.former.json_schema_uses_block_alias(ty_ast) {
                            self.former.error(
                                codes::NOT_JSON_DECODABLE,
                                format!(
                                    "`JSON.{member}` cannot materialize a block-local type alias schema"
                                ),
                                span,
                            );
                        } else if type_has_schema_variable(&ty) {
                            self.former.error(
                                codes::NOT_JSON_DECODABLE,
                                format!(
                                    "`JSON.{member}` needs a fully-known type argument, but `{ty}` has unresolved type variables"
                                ),
                                span,
                            );
                        } else if json_decodable_status(
                            &ty,
                            enums,
                            records,
                            newtypes,
                            &mut Vec::new(),
                        ) == GateCheck::Reject
                        {
                            self.former.error(
                                codes::NOT_JSON_DECODABLE,
                                format!(
                                    "`{ty}` is not JSON-decodable, so `JSON.{member}` cannot decode into it"
                                ),
                                span,
                            );
                        }
                    } else if type_args.is_empty() {
                        self.former.error(
                            codes::NOT_JSON_DECODABLE,
                            format!(
                                "`JSON.{member}` requires an explicit type argument, e.g. `JSON.{member}<User>(...)`"
                            ),
                            span,
                        );
                    }
                }
                if let Some(method) = eq_assert_gate
                    && let Some(param) = eq_assert_param
                {
                    let ty = substitute(&param, &applied.subst);
                    if !type_has_var(&ty) && !contains_true_unknown(&ty) {
                        let enums = self.former.enum_table();
                        let records = self.former.record_table();
                        let newtypes = self.former.newtype_table();
                        if !comparable_in(&ty, enums, records, newtypes, &mut Vec::new()) {
                            self.former.error(
                                codes::INCOMPARABLE,
                                format!(
                                    "`{ty}` is not comparable, so `Test.{method}` cannot compare it"
                                ),
                                span,
                            );
                        }
                    }
                }
                result
            }
            None => {
                if !type_args.is_empty() {
                    self.report_no_type_args(type_args.len(), span);
                }
                for arg in args {
                    match arg {
                        ast::CallArg::Positional(e)
                        | ast::CallArg::Spread(e)
                        | ast::CallArg::Named { value: e, .. } => {
                            self.infer(e);
                        }
                    }
                }
                Type::Unknown
            }
        }
    }

    pub(super) fn apply_scheme<'context>(
        &mut self,
        scheme: Scheme,
        iterable_fixup: bool,
        site: CallSite<'a, 'context>,
    ) -> Type {
        self.apply_scheme_result(scheme, iterable_fixup, site).ty
    }

    pub(super) fn seed_iterable_elem(
        &mut self,
        subst: &mut [Option<Type>],
        elem: Type,
        span: Span,
    ) {
        if contains_rigid(&elem) || !elem.has_unknown() {
            if let Some(slot) = subst.get_mut(0) {
                if let Some(expected) = slot.clone() {
                    self.expect(&elem, &expected, span);
                } else {
                    *slot = Some(elem);
                }
            }
        } else {
            unify(&Type::Var(0), &elem, subst);
        }
    }

    // The call-typing kernel plus the solved substitution, for post-call gates
    // that need an instantiated scheme variable (e.g. `sortedBy`'s key K).
    pub(super) fn apply_scheme_result<'context>(
        &mut self,
        scheme: Scheme,
        iterable_fixup: bool,
        site: CallSite<'a, 'context>,
    ) -> ApplyOutcome {
        let CallSite {
            args,
            type_args,
            context: ctx,
            span,
            bare,
            leading,
            is_print,
        } = site;
        // §11 pipelines insert the saved left-hand value as the
        // first positional argument; it fills parameter slot 0 (or
        // the variadic tail of a parameterless variadic).
        let lead_slot = leading.is_some() && !scheme.params.is_empty();
        // Spread arguments are canonical only at variadic call tails
        // (SPEC §5): they splice into the variadic region, never
        // fixed slots; a spread at a fixed-arity callee is an arity
        // error.
        let has_spread = args.iter().any(|a| matches!(a, ast::CallArg::Spread(_)));
        if has_spread && scheme.variadic.is_none() {
            self.former.error(
                codes::ARITY,
                "spread arguments require a variadic parameter".to_string(),
                span,
            );
            for arg in args {
                match arg {
                    ast::CallArg::Positional(e)
                    | ast::CallArg::Spread(e)
                    | ast::CallArg::Named { value: e, .. } => {
                        self.infer(e);
                    }
                }
            }
            return ApplyOutcome {
                ty: Type::Unknown,
                subst: Vec::new(),
            };
        }

        // Order arguments: positionals first, named ones matched to
        // parameter names (§5). User-function parameter names are not
        // tracked yet, so named arguments to them stay staged.
        let mut ordered: Vec<Option<&'a ast::Expr>> = vec![None; scheme.params.len()];
        let mut variadic_args: Vec<&'a ast::Expr> = Vec::new();
        let mut spread_args: Vec<&'a ast::Expr> = Vec::new();
        let mut recovery_args: Vec<&'a ast::Expr> = Vec::new();
        let mut seen_spread = false;
        let mut positional = lead_slot as usize;
        let mut bad = false;
        if leading.is_some() && scheme.params.is_empty() && scheme.variadic.is_none() {
            self.former.error(
                codes::ARITY,
                "the pipeline inserts the piped value, but this function takes no parameters"
                    .to_string(),
                span,
            );
            bad = true;
        }
        let mut named_unmatched = false;
        let mut saw_named = false;
        for arg in args {
            match arg {
                ast::CallArg::Positional(e) => {
                    if saw_named {
                        // SPEC §5: positional arguments may not
                        // follow named ones.
                        bad = true;
                        self.former.error(
                            codes::ARITY,
                            "positional arguments may not follow named arguments".to_string(),
                            e.span,
                        );
                    }
                    if seen_spread {
                        // §5: after a spread every value belongs to
                        // the variadic tail region.
                        variadic_args.push(e);
                    } else if positional < ordered.len() {
                        if saw_named && let Some(displaced) = ordered[positional].take() {
                            recovery_args.push(displaced);
                        }
                        ordered[positional] = Some(e);
                        positional += 1;
                    } else if scheme.variadic.is_some() {
                        variadic_args.push(e);
                        positional += 1;
                    } else {
                        bad = true;
                        recovery_args.push(e);
                        positional += 1;
                    }
                }
                ast::CallArg::Named { name, value } => {
                    // SPEC §5: named arguments FOLLOW all positional
                    // and spread arguments — named-after-spread is
                    // the required order, not a violation.
                    saw_named = true;
                    let n = self.former.text(name.span);
                    match scheme.names.iter().position(|p| p.as_str() == n) {
                        Some(0) if lead_slot => {
                            // §11: slot 0 is already the piped value.
                            bad = true;
                            self.former.error(
                                codes::ARITY,
                                format!("`{n}` is already supplied by the pipeline (§11)"),
                                name.span,
                            );
                            recovery_args.push(value);
                        }
                        Some(i) if ordered[i].is_none() => ordered[i] = Some(value),
                        _ if !scheme.names_known => {
                            // No authoritative name table (a bare
                            // function type): stay staged rather
                            // than guess.
                            named_unmatched = true;
                        }
                        _ => {
                            bad = true;
                            self.former.error(
                                codes::ARITY,
                                format!("no parameter named `{n}`"),
                                name.span,
                            );
                            recovery_args.push(value);
                        }
                    }
                }
                ast::CallArg::Spread(e) => {
                    if saw_named {
                        // SPEC §5: a named argument preceding a
                        // spread argument is a static error.
                        bad = true;
                        self.former.error(
                            codes::ARITY,
                            "named arguments must follow spread arguments".to_string(),
                            e.span,
                        );
                    }
                    let skipped_required = (positional..scheme.params.len()).any(|i| {
                        ordered[i].is_none() && scheme.slot_required(i) && !(i == 0 && lead_slot)
                    });
                    if !seen_spread && skipped_required {
                        // SPEC §5: a spread cannot skip an
                        // unsatisfied fixed parameter (defaulted
                        // slots may be skipped; required ones cannot
                        // be rescued by a later named argument).
                        bad = true;
                        self.former.error(
                            codes::ARITY,
                            "a spread argument cannot skip an unsatisfied fixed parameter"
                                .to_string(),
                            e.span,
                        );
                    }
                    seen_spread = true;
                    spread_args.push(e);
                }
            }
        }
        if !named_unmatched {
            let supplied = ordered.iter().filter(|o| o.is_some()).count() + lead_slot as usize;
            // Required parameters are the leading non-defaulted slots;
            // each must actually be filled (a named optional cannot
            // satisfy a missing required one). Slot 0 may be filled by
            // the piped value.
            let missing_required = ordered
                .iter()
                .enumerate()
                .any(|(i, o)| o.is_none() && scheme.slot_required(i) && !(i == 0 && lead_slot));
            if supplied < scheme.required
                || missing_required
                || (positional > scheme.params.len() && scheme.variadic.is_none())
            {
                self.former.error(
                    codes::ARITY,
                    format!(
                        "this call needs {}{} argument{}, found {}",
                        scheme.required,
                        if scheme.variadic.is_some() || scheme.params.len() > scheme.required {
                            "+"
                        } else {
                            ""
                        },
                        if scheme.required == 1 { "" } else { "s" },
                        args.len() + usize::from(leading.is_some())
                    ),
                    span,
                );
                bad = true;
            }
        }

        let mut subst: Vec<Option<Type>> = vec![None; scheme.vars as usize];
        // §3 (v5.4) EXPLICIT call-site type arguments `f<T, U>(args)`: the
        // parser only produces these at `>= V5_4`. They become GROUND TRUTH for
        // the callee scheme's type variables — seeded into `subst` BEFORE any
        // ctx/argument unification, so a conflicting argument surfaces through
        // the existing `expect` mismatch (the `unify_with` `slot.is_none()` guard
        // means a later ctx/arg unification can only FILL remaining `None`
        // slots, never override an explicit seed). CHECK-ONLY: the seed shapes
        // the inferred result and argument expectations; the call still lowers
        // type-erased in interp/emit (`f<int>(x)` ≡ `f(x)` at run/build).
        if !type_args.is_empty() {
            if scheme.vars == 0 {
                // A non-generic callee accepts no explicit type arguments.
                self.report_no_type_args(type_args.len(), span);
            } else if type_args.len() != scheme.vars as usize {
                self.former.error(
                    codes::TYPE_ARG_ARITY,
                    format!(
                        "this call expects {} type argument{}, but {} {} supplied",
                        scheme.vars,
                        if scheme.vars == 1 { "" } else { "s" },
                        type_args.len(),
                        if type_args.len() == 1 { "was" } else { "were" },
                    ),
                    span,
                );
            } else {
                // Form each explicit type in the CURRENT type env so a type
                // parameter in scope (`f<T>(xs)` inside `fn g<T>(...)`) resolves.
                let env = self.tyenv();
                for (i, ty) in type_args.iter().enumerate() {
                    let formed = self.former.form(ty, &env);
                    subst[i] = Some(formed);
                }
            }
        }
        if named_unmatched {
            self.infer_call_args(args);
            return ApplyOutcome {
                ty: Type::Unknown,
                subst,
            };
        }
        for arg in recovery_args {
            self.infer(arg);
        }
        if let Some(expected) = ctx {
            unify_with(&scheme.ret, expected, &mut subst, false);
        }
        let mut any_unknown_input = false;
        if let Some(lt) = leading {
            if lead_slot {
                if iterable_fixup {
                    // Slot 0 of map/filter/reduce is the iterable; a rigid generic
                    // refines the element to a rigid `ElemOf<T>` projection.
                    if let Some(elem) = self.iter_elem(lt) {
                        // The iterable's element is ground truth for slot 0, but
                        // an explicit call-site type arg may already have seeded
                        // that slot. Fill only an empty slot; otherwise compare so
                        // a conflict surfaces as TPZ5001 instead of being clobbered.
                        self.seed_iterable_elem(&mut subst, elem, span);
                    } else if lt.has_unknown() {
                        any_unknown_input = true;
                    } else {
                        self.former.error(
                            codes::TYPE_MISMATCH,
                            format!("`{lt}` is not iterable (§10)"),
                            span,
                        );
                    }
                } else {
                    if lt.has_unknown() {
                        any_unknown_input = true;
                    }
                    let expected = substitute(&scheme.params[0], &subst);
                    if type_has_var(&expected) {
                        unify(&expected, lt, &mut subst);
                        let solved = substitute(&expected, &subst);
                        if !type_has_var(&solved) && !lt.has_unknown() {
                            self.expect(lt, &solved, span);
                        }
                    } else if !lt.has_unknown() {
                        self.expect(lt, &expected, span);
                    }
                }
            } else if let Some(velem) = &scheme.variadic {
                let expected = substitute(velem, &subst);
                if type_has_var(&expected) {
                    unify(&expected, lt, &mut subst);
                } else if !lt.has_unknown() {
                    self.expect(lt, &expected, span);
                }
            }
        }
        for (i, slot) in ordered.iter().enumerate() {
            let Some(arg) = slot else { continue };
            if iterable_fixup && i == 0 {
                // Iterable<T> (§22.1): refine T from the argument's
                // element type; concrete non-iterables are errors.
                let arg_ty = self.infer(arg);
                if let Some(elem) = self.iter_elem(&arg_ty) {
                    // Fill only an empty slot; if ctx or an explicit type arg has
                    // already seeded it, compare against that seed rather than
                    // overwriting it.
                    self.seed_iterable_elem(&mut subst, elem, arg.span);
                } else if arg_ty.has_unknown() {
                    any_unknown_input = true;
                } else {
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        format!("`{arg_ty}` is not iterable (§10)"),
                        arg.span,
                    );
                }
                continue;
            }
            let expected = substitute(&scheme.params[i], &subst);
            if type_has_var(&expected) {
                // Partially solved expectations still carry context
                // (a lambda whose parameter types are concrete even
                // when the return variable is not).
                let t = self.check_expr(arg, &expected);
                if contains_true_unknown(&t) {
                    any_unknown_input = true;
                }
                unify(&expected, &t, &mut subst);
                let solved = substitute(&expected, &subst);
                if !type_has_var(&solved) {
                    self.expect(&t, &solved, arg.span);
                }
            } else if is_print && matches!(expected, Type::Prim(Prim::String)) {
                // `print(value)`: §22.2 print is string-only, and interpolation is the
                // canonical way to print a value. Type the argument ONCE (surfacing any
                // inner error), and for a concrete non-string point at the interpolation
                // form instead of a bare mismatch. Handled fully here — we never also run
                // the generic check below, which would re-type the arg and double-report
                // an inner error.
                let t = self.infer(arg);
                if !t.has_unknown() && !usable(&t, &expected) {
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        "`print` takes a `string`; interpolate the value instead — \
                         `print(\"{…}\")` (§22.2)"
                            .to_string(),
                        arg.span,
                    );
                }
                if contains_true_unknown(&t) {
                    any_unknown_input = true;
                }
            } else {
                let t = self.check_expr(arg, &expected);
                if contains_true_unknown(&t) {
                    any_unknown_input = true;
                }
            }
        }
        if let Some(velem) = &scheme.variadic {
            for arg in &spread_args {
                // §5: a spread argument is an Array of the variadic
                // element type.
                let expected = Type::Ctor(Ctor::Array, vec![substitute(velem, &subst)]);
                if type_has_var(&expected) {
                    let arg_ty = self.infer(arg);
                    if contains_true_unknown(&arg_ty) {
                        any_unknown_input = true;
                    }
                    unify(&expected, &arg_ty, &mut subst);
                    let solved = substitute(&expected, &subst);
                    if !type_has_var(&solved) && !arg_ty.has_unknown() {
                        self.expect(&arg_ty, &solved, arg.span);
                    }
                } else {
                    let arg_ty = self.check_expr(arg, &expected);
                    if contains_true_unknown(&arg_ty) {
                        any_unknown_input = true;
                    }
                }
            }
            for arg in &variadic_args {
                let expected = substitute(velem, &subst);
                if type_has_var(&expected) {
                    let t = self.infer(arg).widen();
                    if t.has_unknown() {
                        any_unknown_input = true;
                    }
                    unify(&expected, &t, &mut subst);
                } else {
                    let t = self.check_expr(arg, &expected);
                    if t.has_unknown() {
                        any_unknown_input = true;
                    }
                }
            }
        }

        let mut result = substitute(&scheme.ret, &subst);
        if type_has_var(&result)
            && let Some(expected) = ctx
        {
            unify_with(&result, expected, &mut subst, false);
            result = substitute(&scheme.ret, &subst);
        }
        if type_has_var(&result) {
            // A context that itself carries inference vars is a
            // pass-through hole, not a user expectation: it neither
            // fails (§22.1) nor blocks partial collection.
            let solving_ctx = ctx.is_some_and(|c| !type_has_var(c));
            // Inside a contextless branch join, hand the partial
            // back for cross-arm solving instead of dissolving it.
            if self.collect_partial && !solving_ctx && !bare && !bad && !any_unknown_input {
                return ApplyOutcome {
                    ty: self.renumber_partial(&result),
                    subst,
                };
            }
            // (§22.1) When a context EXISTS but could not solve the
            // variables, this is the context site and it failed —
            // diagnose here instead of letting the outer expectation
            // silently admit Unknown.
            if !bad && !any_unknown_input && (bare || solving_ctx) {
                self.former.error(
                    codes::UNSOLVED,
                    "this expression needs a contextual type (§22.1): annotate the binding or surrounding expectation"
                        .to_string(),
                    span,
                );
            }
            return ApplyOutcome {
                ty: Type::Unknown,
                subst,
            };
        }
        ApplyOutcome {
            ty: if bad { Type::Unknown } else { result },
            subst,
        }
    }

    /// `None` and other polymorphic constructor values (§22.1).
    pub(super) fn instantiate_value(
        &mut self,
        scheme: Scheme,
        ctx: Option<&Type>,
        span: Span,
        bare: bool,
    ) -> Type {
        let mut subst: Vec<Option<Type>> = vec![None; scheme.vars as usize];
        let mut result = substitute(&scheme.ret, &subst);
        if type_has_var(&result)
            && let Some(expected) = ctx
        {
            unify_with(&result, expected, &mut subst, false);
            result = substitute(&scheme.ret, &subst);
        }
        if type_has_var(&result) {
            let solving_ctx = ctx.is_some_and(|c| !type_has_var(c));
            if !bare && !solving_ctx {
                if self.collect_partial {
                    return self.renumber_partial(&result);
                }
                // The element type is unsolved, but the CONSTRUCTOR is still
                // known: `None` is an `Option<_>`, not an opaque `Unknown`.
                // Keep the Ctor shape (its var lowered to `Unknown`) so a
                // member call on a bare `None` resolves and TYPE-CHECKS its
                // arguments — `(None).okOrElse(5)` must still reject `5` as a
                // non-`() -> E`. `has_unknown()` stays true on the lowered
                // shape, so a `has_unknown`-gated guard still skips it.
                //
                // One downstream behavior INTENTIONALLY differs from the former
                // bare `Unknown`: match-EXHAUSTIVENESS. A `match None { … }` now
                // scrutinizes an `Option<Unknown>`, so the checker requires the
                // arms to cover the Option type (a `Some` arm as well as `None`);
                // `match None { case None => 0 }` reports TPZ5021. This is the
                // type-consistent rule — a match on an `Option` is exhaustive over
                // the TYPE, not over the one constructor literally written — and is
                // the more correct behavior. (c7.rs pins both the TPZ5021 case and
                // the exhaustive `None`+`Some` clean case.)
                return unknown_for_vars(&result);
            }
            self.former.error(
                codes::UNSOLVED,
                "this expression needs a contextual type (§22.1): annotate the binding or surrounding expectation"
                    .to_string(),
                span,
            );
            return Type::Unknown;
        }
        result
    }

    /// Renames every distinct inference var in a collected partial
    /// to a fresh slot in the join's var space — collision-free for
    /// scheme-local vars and embedded partial vars alike.
    pub(super) fn renumber_partial(&mut self, t: &Type) -> Type {
        let mut map: Vec<(u32, u32)> = Vec::new();
        self.renumber_walk(t, &mut map)
    }

    pub(super) fn renumber_walk(&mut self, t: &Type, map: &mut Vec<(u32, u32)>) -> Type {
        t.transform_components(&mut |component| match component {
            Type::Var(index) => {
                let to = match map.iter().find(|(from, _)| from == index) {
                    Some((_, to)) => *to,
                    None => {
                        let to = PARTIAL_VAR_OFFSET + self.partial_base;
                        self.partial_base += 1;
                        map.push((*index, to));
                        to
                    }
                };
                Some(Type::Var(to))
            }
            _ => None,
        })
    }

    /// §11 unary application: a non-call pipeline stage of callable
    /// type takes the piped value as its single argument.
    pub(super) fn pipe_apply(&mut self, value: Type, stage: Type, span: Span) -> Type {
        if stage.has_unknown() {
            return Type::Unknown;
        }
        match stage {
            Type::Func {
                params,
                variadic,
                ret,
            } => {
                if params.len() == 1 {
                    if !value.has_unknown() {
                        self.expect(&value, &params[0], span);
                    }
                    *ret
                } else if params.is_empty() && variadic.is_some() {
                    let velem = variadic.expect("checked");
                    if !value.has_unknown() {
                        self.expect(&value, &velem, span);
                    }
                    *ret
                } else {
                    self.former.error(
                        codes::ARITY,
                        "a pipeline stage takes the piped value as its only argument (§11)"
                            .to_string(),
                        span,
                    );
                    Type::Unknown
                }
            }
            other => {
                self.former.error(
                    codes::NOT_CALLABLE,
                    format!("`{other}` is not callable"),
                    span,
                );
                Type::Unknown
            }
        }
    }
}
