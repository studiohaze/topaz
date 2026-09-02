use super::*;

/// Preserve the §12 container kind for both optional-access values and their
/// call-site-specialized callable evidence. A member that is already optional
/// flat-maps instead of gaining a second Option layer.
pub(super) fn wrap_optional_member_type(nullable: bool, raw: Type) -> Type {
    if raw.has_unknown() {
        Type::Unknown
    } else if nullable {
        Type::union(vec![raw, Type::Literal(Lit::Null)])
    } else if matches!(raw, Type::Ctor(Ctor::Option, _)) {
        raw
    } else {
        Type::Ctor(Ctor::Option, vec![raw])
    }
}

/// §12: the single unwrapped layer of `Option<T>` or `T | null`.
/// Returns None when the type is neither.
pub(super) fn unwrap_optional(ty: &Type) -> Option<Type> {
    match ty {
        Type::Ctor(Ctor::Option, args) => Some(args[0].clone()),
        Type::Union(members) if members.contains(&Type::Literal(Lit::Null)) => {
            let rest: Vec<Type> = members
                .iter()
                .filter(|m| **m != Type::Literal(Lit::Null))
                .cloned()
                .collect();
            if rest.is_empty() {
                None
            } else {
                Some(Type::union(rest))
            }
        }
        _ => None,
    }
}

impl<'a> ExprChecker<'a> {
    /// §12 `?.`: unwraps one optional layer, resolves the member on
    /// the inner type, and re-wraps in the same container kind.
    /// `call` carries the complete OptionalCall input when present.
    pub(super) fn optional_member<'context>(
        &mut self,
        object_ty: &Type,
        field: &'a ast::Ident,
        span: Span,
        call: Option<OptionalCallInput<'a, 'context>>,
    ) -> Type {
        let nullable = matches!(object_ty, Type::Union(_));
        let inner = match unwrap_optional(object_ty) {
            Some(t) => t,
            None if object_ty.has_unknown() => {
                if let Some(call) = call {
                    for arg in call.args {
                        match arg {
                            ast::CallArg::Positional(e)
                            | ast::CallArg::Spread(e)
                            | ast::CallArg::Named { value: e, .. } => {
                                self.infer(e);
                            }
                        }
                    }
                }
                return Type::Unknown;
            }
            None => {
                let display = object_ty.clone();
                self.former.error(
                    codes::TYPE_MISMATCH,
                    format!("`?.` needs an Option or nullable receiver, found `{display}`"),
                    span,
                );
                return Type::Unknown;
            }
        };
        let raw = match call {
            None => self.member_type(&inner, field, span),
            Some(call) => {
                let site = CallSite {
                    args: call.args,
                    type_args: &[],
                    context: None,
                    span,
                    bare: false,
                    leading: call.leading,
                    is_print: false,
                };
                let member = self.former.text(field.span);
                match builtins::receiver_member(&inner, member) {
                    Some(Member::Method(scheme)) => {
                        self.apply_optional_call_scheme(scheme, site, nullable, call.callee_span)
                    }
                    Some(Member::Property(ty)) => {
                        // §12/§22: a property is not callable.
                        self.former.error(
                            codes::NOT_CALLABLE,
                            format!("`{ty}` is not callable"),
                            field.span,
                        );
                        for arg in call.args {
                            match arg {
                                ast::CallArg::Positional(e)
                                | ast::CallArg::Spread(e)
                                | ast::CallArg::Named { value: e, .. } => {
                                    self.infer(e);
                                }
                            }
                        }
                        return Type::Unknown;
                    }
                    None => {
                        // Record-field functions and the not-callable
                        // diagnostics ride the plain member path.
                        let callee_ty = self.member_type(&inner, field, span);
                        match callee_ty {
                            Type::Func {
                                params,
                                variadic,
                                ret,
                            } => {
                                let scheme = Scheme {
                                    vars: 0,
                                    names: vec![],
                                    names_known: false,
                                    required: params.len(),
                                    defaulted: Vec::new(),
                                    params,
                                    variadic: variadic.map(|b| *b),
                                    ret: *ret,
                                };
                                self.apply_optional_call_scheme(
                                    scheme,
                                    site,
                                    nullable,
                                    call.callee_span,
                                )
                            }
                            other => {
                                if is_rigid_or_union_rigid(&inner) {
                                    // A call on a rigid generic optional receiver
                                    // projects a rigid result — the tail wraps it as
                                    // `Option<CallOf<T, m>>`. Without this the rigid
                                    // member projection `member_type` just produced would
                                    // wrongly trip NOT_CALLABLE; `project_call` checks the
                                    // args against each concrete arm and infers them.
                                    let callee_type =
                                        wrap_optional_member_type(nullable, other.clone());
                                    self.record_typed_node(
                                        topaz_hir::TypedNodeKind::Expression,
                                        call.callee_span,
                                        &callee_type,
                                    );
                                    self.record_typed_call_callee(span, &callee_type);
                                    let member = member.to_string();
                                    self.project_call(
                                        &inner,
                                        &member,
                                        call.args,
                                        call.leading,
                                        field.span,
                                    )
                                    .expect("rigid inner projects a call")
                                } else {
                                    if !other.has_unknown() {
                                        self.former.error(
                                            codes::NOT_CALLABLE,
                                            format!("`{other}` is not callable"),
                                            field.span,
                                        );
                                    }
                                    for arg in call.args {
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
                    }
                }
            }
        };
        wrap_optional_member_type(nullable, raw)
    }

    pub(super) fn apply_optional_call_scheme<'context>(
        &mut self,
        scheme: Scheme,
        site: CallSite<'a, 'context>,
        nullable: bool,
        callee_span: Span,
    ) -> Type {
        let semantic_scheme = scheme.clone();
        let applied = self.apply_scheme_result(scheme, false, site);
        let callee_type = wrap_optional_member_type(
            nullable,
            specialized_call_callee_type(&semantic_scheme, &applied.subst),
        );
        self.record_typed_node(
            topaz_hir::TypedNodeKind::Expression,
            callee_span,
            &callee_type,
        );
        self.record_typed_call_callee(site.span, &callee_type);
        applied.ty
    }

    /// The rigid member projection for an opaque-but-rigid receiver, or
    /// `None` for a gradual one. A bare generic (`Skolem`/`Foreign`) projects
    /// `MemberOf<T, x>`; a UNION with any rigid arm splits per non-null arm
    /// (concrete arms keep their precise field type, rigid arms project, gradual
    /// arms stay `Unknown`) so `{x:int} | T` types `.x` as `int | MemberOf<T, x>`,
    /// which cannot silently discharge `int` alone. `None` leaves a truly gradual
    /// receiver (`Unknown`/`Var`) gradual.
    pub(super) fn project_member(&mut self, ty: &Type, name: &str) -> Option<Type> {
        match ty {
            Type::Skolem { .. } | Type::Foreign { .. } => {
                Some(self.project(format!("MemberOf<{ty}, {name}>")))
            }
            Type::Union(arms) if arms.iter().any(is_rigid_or_union_rigid) => {
                let mut parts = Vec::new();
                for arm in arms {
                    if matches!(arm, Type::Literal(Lit::Null)) {
                        continue;
                    }
                    parts.push(self.member_arm_type(arm, name));
                }
                Some(Type::union(parts))
            }
            _ => None,
        }
    }

    /// The member type of a single union arm (already non-null and not
    /// decidably-absent), emitting no diagnostics — the per-arm worker behind
    /// `project_member`.
    pub(super) fn member_arm_type(&mut self, arm: &Type, name: &str) -> Type {
        if let Some(m) = builtins::receiver_member(arm, name) {
            return match m {
                Member::Property(ty) => ty,
                Member::Method(scheme) => Type::Func {
                    params: scheme.params,
                    variadic: scheme.variadic.map(Box::new),
                    ret: Box::new(scheme.ret),
                },
            };
        }
        match arm {
            Type::Record(fields) => fields
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, t)| t.clone())
                .unwrap_or(Type::Unknown),
            Type::Skolem { .. } | Type::Foreign { .. } => {
                self.project(format!("MemberOf<{arm}, {name}>"))
            }
            Type::Union(_) => self.project_member(arm, name).unwrap_or(Type::Unknown),
            _ => Type::Unknown,
        }
    }

    pub(super) fn infer_call_args(&mut self, args: &'a [ast::CallArg]) {
        for arg in args {
            match arg {
                ast::CallArg::Positional(e)
                | ast::CallArg::Spread(e)
                | ast::CallArg::Named { value: e, .. } => {
                    self.infer(e);
                }
            }
        }
    }

    /// The iteration element of `ty` (§10) — for a concrete iterable its
    /// real element, for a rigid generic (`Skolem`/`Foreign`) a rigid `ElemOf<T>`
    /// projection so the bound element cannot silently discharge a concrete
    /// expectation (`for x in t { let y: int = x }` must NOT check). `None` for a
    /// concrete non-iterable or a gradual receiver — the caller keeps its existing
    /// "not iterable" error / `Unknown` staging.
    pub(super) fn iter_elem(&mut self, ty: &Type) -> Option<Type> {
        if let Some(e) = builtins::iterable_elem(ty) {
            return Some(e);
        }
        if matches!(ty, Type::Skolem { .. } | Type::Foreign { .. }) {
            return Some(self.project(format!("ElemOf<{ty}>")));
        }
        None
    }

    /// The element of an array SPREAD `[...e]`. Unlike `iter_elem`, array spread
    /// is Array-ONLY (the runtime faults "array spread needs an `Array`" on a
    /// `range`/`set`/scalar): a concrete `Array<E>` contributes `E`; a rigid
    /// generic (or a union with a rigid arm) may BE an array at runtime, so it
    /// projects a rigid `ElemOf<T>` element; a union/gradual receiver is staged
    /// (it may resolve to an array); a decidably non-array is rejected.
    pub(super) fn spread_elem(&mut self, ty: &Type) -> SpreadElem {
        match ty {
            Type::Ctor(Ctor::Array, args) => SpreadElem::Elem(args[0].clone()),
            Type::Skolem { .. } | Type::Foreign { .. } => {
                SpreadElem::Elem(self.project(format!("ElemOf<{ty}>")))
            }
            Type::Union(arms) => {
                // Every arm must be spreadable. A decidably non-array arm (it
                // would fault at runtime) rejects the whole spread; a gradual arm
                // stages it; otherwise the element is the JOIN of the per-arm
                // elements, so a rigid-element arm like `Array<T>` still
                // contributes its `ElemOf<T>` projection.
                let mut elems = Vec::new();
                let mut staged = false;
                for arm in arms {
                    match self.spread_elem(arm) {
                        SpreadElem::Elem(e) => elems.push(e),
                        SpreadElem::Stage => staged = true,
                        SpreadElem::NotArray => return SpreadElem::NotArray,
                    }
                }
                if staged {
                    SpreadElem::Stage
                } else {
                    SpreadElem::Elem(Type::union(elems))
                }
            }
            Type::Unknown | Type::Var(_) => SpreadElem::Stage,
            _ => SpreadElem::NotArray,
        }
    }

    /// Resolve a member CALL against one concrete (non-null, non-rigid) union
    /// arm, preserving the REAL method scheme so named parameters / defaults are
    /// checked exactly as a direct concrete receiver would (lowering through
    /// `Type::Func` would drop them, so `xs.get(j: 0)` would slip). Emits
    /// NOT_CALLABLE for a member that exists but is not callable.
    pub(super) fn resolve_arm_call(&mut self, arm: &Type, member: &str, span: Span) -> ArmCall {
        match builtins::receiver_member(arm, member) {
            Some(Member::Method(scheme)) => ArmCall::Callable(scheme),
            Some(Member::Property(ty)) => {
                self.former
                    .error(codes::NOT_CALLABLE, format!("`{ty}` is not callable"), span);
                ArmCall::NotCallable
            }
            None => match arm {
                Type::Record(fields) => match fields.iter().find(|(n, _)| n == member) {
                    Some((
                        _,
                        Type::Func {
                            params,
                            variadic,
                            ret,
                        },
                    )) => ArmCall::Callable(Scheme {
                        vars: 0,
                        names: vec![],
                        names_known: false,
                        required: params.len(),
                        defaulted: Vec::new(),
                        params: params.clone(),
                        variadic: variadic.as_deref().cloned(),
                        ret: (**ret).clone(),
                    }),
                    Some((_, other)) => {
                        let other = other.clone();
                        self.former.error(
                            codes::NOT_CALLABLE,
                            format!("`{other}` is not callable"),
                            span,
                        );
                        ArmCall::NotCallable
                    }
                    None => ArmCall::Absent,
                },
                _ => ArmCall::Absent,
            },
        }
    }

    /// The rigid call result for a rigid/union-rigid receiver, or `None`
    /// if the receiver is not projectable. A bare generic infers the args (for
    /// their own diagnostics) and projects `CallOf<T, m>`.
    ///
    /// For a UNION with a rigid arm, the call is resolved concrete-first: every
    /// CONCRETE (non-null, non-rigid) arm's member is fully checked via
    /// `apply_scheme` (arity, argument types, the whole §5/§11 surface), since
    /// the receiver may BE that arm at runtime. A concrete member that is not
    /// callable is a real NOT_CALLABLE the rigid arm cannot excuse. The RESULT is
    /// the per-arm `Type::union`: each concrete arm's real call result joined with
    /// `CallOf<rigid arm, m>` (a gradual arm bails to the coarse whole-union
    /// `CallOf<ty, m>`). The rigid `CallOf<...>` is NEVER dropped, so the result
    /// still cannot silently discharge a concrete expectation — the concrete arms
    /// only sharpen the rendered type (and feed downstream inference). Concrete
    /// arms are known to HAVE the member here — a missing one already failed the
    /// `receiver_has_member == Some(false)` guard upstream.
    pub(super) fn project_call(
        &mut self,
        ty: &Type,
        member: &str,
        args: &'a [ast::CallArg],
        leading: Option<&Type>,
        span: Span,
    ) -> Option<Type> {
        if !is_rigid_or_union_rigid(ty) {
            return None;
        }
        let site = CallSite {
            args,
            type_args: &[],
            context: None,
            span,
            bare: false,
            leading,
            is_print: false,
        };
        let mut args_checked = false;
        if let Type::Union(arms) = ty {
            // Build a precise per-arm result — the union of each
            // arm's contribution — instead of the coarse `CallOf<whole union, m>`,
            // so a concrete arm surfaces its REAL call result in diagnostics while a
            // rigid arm keeps an opaque `CallOf<arm, m>` possibility. The
            // rigid/gradual possibility is NEVER dropped: dropping it would let
            // `{ foo: () -> int } | T` project a bare `int` and reopen the precision hole
            // (a concrete expectation would be silently discharged). The union still
            // carries the `CallOf<...>` Skolem, so it cannot discharge a concrete
            // expectation — only the rendered "found" type gets sharper.
            let mut results: Vec<Type> = Vec::new();
            for arm in arms {
                if matches!(arm, Type::Literal(Lit::Null)) {
                    continue;
                }
                if is_rigid_or_union_rigid(arm) {
                    results.push(self.project(format!("CallOf<{arm}, {member}>")));
                    continue;
                }
                match self.resolve_arm_call(arm, member, span) {
                    ArmCall::Callable(scheme) => {
                        // Check the args against this arm's REAL signature (named
                        // params, defaults, arity, types, the pipeline `leading`
                        // value) AND keep its result type as this arm's contribution.
                        let ret = self.apply_scheme(scheme, false, site);
                        args_checked = true;
                        results.push(ret);
                    }
                    ArmCall::NotCallable => {
                        if !args_checked {
                            self.infer_call_args(args);
                        }
                        return Some(Type::Unknown);
                    }
                    ArmCall::Absent => {
                        // A gradual arm whose member is unresolvable: fall back to the
                        // coarse whole-union projection rather than contribute an
                        // `Unknown` that could silently discharge a concrete
                        // expectation (the opaque possibility must stay rigid).
                        if !args_checked {
                            self.infer_call_args(args);
                        }
                        return Some(self.project(format!("CallOf<{ty}, {member}>")));
                    }
                }
            }
            if !args_checked {
                self.infer_call_args(args);
            }
            return Some(Type::union(results));
        }
        if !args_checked {
            self.infer_call_args(args);
        }
        Some(self.project(format!("CallOf<{ty}, {member}>")))
    }

    /// §4 (v5.4) the call SCHEME for a user receiver method `(type id, method)`, if
    /// declared — over the method's NON-self parameters (the receiver supplies
    /// `self`). `names_known` is true (a user declaration), so named/defaulted method
    /// arguments check exactly like a free function.
    pub(super) fn method_scheme(
        &self,
        type_id: &str,
        method: &str,
    ) -> Option<(Scheme, Option<String>)> {
        let info = self.former.method_info(type_id, method)?;
        let scheme = Scheme {
            vars: 0,
            params: info.params.clone(),
            names: info.names.clone(),
            names_known: true,
            required: info.required,
            defaulted: info.defaulted.clone(),
            variadic: info.variadic.clone(),
            ret: info.ret.clone(),
        };
        let target = self
            .former
            .receiver_method_dispatch_id(type_id, method)
            .map(str::to_string);
        Some((scheme, target))
    }

    /// §4 (v5.4) types a PROTOCOL static dispatch `Show.show(x)` / `Order.compare(a,
    /// b)` (`head` is a declared protocol). STATIC dispatch only — never `x.show()`.
    /// Steps:
    ///   1. The protocol must declare the method (else TPZ5522 "P has no method m").
    ///   2. The CONFORMING value is the FIRST argument — infer its type, read its
    ///      runtime nominal id. A non-nominal first arg ⇒ "type X does not conform".
    ///   3. `(P, type_id)` must conform (derive OR manual `impl`), else TPZ5522.
    ///   4. Build the CONCRETE signature: derived ⇒ substitute the receiver type for
    ///      the protocol's `T` (= `Var(0)`); manual ⇒ the registered `protocol_method`
    ///      signature. Check arity + every arg against the concrete param type; the
    ///      result is the concrete return type.
    ///
    /// Args are ALL-POSITIONAL in this slice (matching the runtime dispatch); a
    /// named/spread protocol argument is reported as unsupported (arity).
    pub(super) fn protocol_call(
        &mut self,
        protocol: &str,
        field: &ast::Ident,
        args: &'a [ast::CallArg],
        span: Span,
        callee_span: Span,
    ) -> Type {
        let method = self.former.text(field.span).to_string();
        // 1. The protocol must declare the method.
        let Some(sig) = self
            .former
            .protocol(protocol)
            .and_then(|p| p.methods.get(&method).cloned())
        else {
            self.infer_call_args(args);
            self.former.error(
                codes::NO_CONFORMANCE,
                format!("protocol `{protocol}` has no method `{method}`"),
                field.span,
            );
            return Type::Unknown;
        };
        // All-positional only (the runtime dispatch is positional).
        let positional: Vec<&'a ast::Expr> = args
            .iter()
            .filter_map(|a| match a {
                ast::CallArg::Positional(e) => Some(e),
                _ => None,
            })
            .collect();
        if positional.len() != args.len() {
            self.infer_call_args(args);
            self.former.error(
                codes::ARITY,
                format!("a protocol call `{protocol}.{method}(…)` takes positional arguments only"),
                span,
            );
            return self.protocol_ret(&sig, &Type::Unknown);
        }
        // 2. The conforming value is the FIRST argument.
        let Some(first) = positional.first() else {
            self.former.error(
                codes::ARITY,
                format!(
                    "`{protocol}.{method}` takes {} argument{}, found 0",
                    sig.params.len(),
                    if sig.params.len() == 1 { "" } else { "s" }
                ),
                span,
            );
            return self.protocol_ret(&sig, &Type::Unknown);
        };
        let recv_ty = self.infer(first);
        // A receiver whose type is still Unknown (gradual) defers: check the rest
        // against the protocol's generic shape without a conformance verdict.
        let Some(type_id) = nominal_type_id(&recv_ty) else {
            if self.type_satisfies_protocol_bound(&recv_ty, protocol) == GateCheck::Accept {
                let subst = [recv_ty.clone()];
                let params: Vec<Type> = sig
                    .params
                    .iter()
                    .map(|p| crate::form::substitute(p, &subst))
                    .collect();
                let ret = crate::form::substitute(&sig.ret, &subst);
                if positional.len() != params.len() {
                    self.former.error(
                        codes::ARITY,
                        format!(
                            "`{protocol}.{method}` takes {} argument{}, found {}",
                            params.len(),
                            if params.len() == 1 { "" } else { "s" },
                            positional.len()
                        ),
                        span,
                    );
                }
                self.check_protocol_call_arguments(&positional, &params, &recv_ty);
                return self.complete_protocol_call(protocol, span, callee_span, params, ret);
            }
            if !recv_ty.has_unknown() {
                let message = if matches!(recv_ty, Type::Skolem { .. } | Type::Foreign { .. }) {
                    format!("`{recv_ty}` does not conform to `{protocol}`")
                } else {
                    format!(
                        "`{recv_ty}` does not conform to `{protocol}`: a protocol receiver must be a record/enum/newtype"
                    )
                };
                self.former
                    .error(codes::NO_CONFORMANCE, message, first.span);
            }
            // Still check the remaining args against the generic param shape.
            for (i, arg) in positional.iter().enumerate().skip(1) {
                let want = sig.params.get(i).cloned().unwrap_or(Type::Unknown);
                self.check_expr(arg, &want);
            }
            return self.protocol_ret(&sig, &Type::Unknown);
        };
        // 3. The receiver's type must conform.
        if !self.former.conforms(protocol, &type_id) {
            self.former.error(
                codes::NO_CONFORMANCE,
                format!("`{type_id}` does not conform to `{protocol}`"),
                first.span,
            );
            for (i, arg) in positional.iter().enumerate().skip(1) {
                let want = sig.params.get(i).cloned().unwrap_or(Type::Unknown);
                self.check_expr(arg, &want);
            }
            return self.protocol_ret(&sig, &recv_ty);
        }
        // 4. The CONCRETE signature — manual impl uses the user's actual signature;
        // derived uses the protocol shape with `T` = the receiver type.
        let (params, ret): (Vec<Type>, Type) =
            if let Some(info) = self.former.protocol_method(protocol, &type_id, &method) {
                (info.params.clone(), info.ret.clone())
            } else {
                let subst = [recv_ty.clone()];
                (
                    sig.params
                        .iter()
                        .map(|p| crate::form::substitute(p, &subst))
                        .collect(),
                    crate::form::substitute(&sig.ret, &subst),
                )
            };
        // Arity.
        if positional.len() != params.len() {
            self.former.error(
                codes::ARITY,
                format!(
                    "`{protocol}.{method}` takes {} argument{}, found {}",
                    params.len(),
                    if params.len() == 1 { "" } else { "s" },
                    positional.len()
                ),
                span,
            );
        }
        self.check_protocol_call_arguments(&positional, &params, &recv_ty);
        self.complete_protocol_call(protocol, span, callee_span, params, ret)
    }

    pub(super) fn complete_protocol_call(
        &mut self,
        protocol: &str,
        call_span: Span,
        callee_span: Span,
        params: Vec<Type>,
        result: Type,
    ) -> Type {
        let callee_type = Type::Func {
            params,
            variadic: None,
            ret: Box::new(result.clone()),
        };
        self.record_typed_call_callee(call_span, &callee_type);
        self.record_typed_node(
            topaz_hir::TypedNodeKind::Expression,
            callee_span,
            &callee_type,
        );
        self.record_typed_call_target(call_span, Some(format!("builtin::{protocol}")));
        result
    }

    /// Check a protocol call's concrete parameters while retaining the receiver
    /// type already inferred for conformance dispatch. The first argument must not
    /// be traversed again; comparing that observed type with parameter zero keeps
    /// the same mismatch gate while the remaining arguments receive their normal
    /// contextual checks.
    pub(super) fn check_protocol_call_arguments(
        &mut self,
        positional: &[&'a ast::Expr],
        params: &[Type],
        recv_ty: &Type,
    ) {
        for (i, arg) in positional.iter().enumerate() {
            match (i, params.get(i)) {
                (0, Some(want)) => self.expect(recv_ty, want, arg.span),
                (0, None) => {}
                (_, Some(want)) => {
                    self.check_expr(arg, want);
                }
                (_, None) => {
                    self.infer(arg);
                }
            }
        }
    }

    /// §4 (v5.4) the result type of a protocol call whose receiver type is `recv` —
    /// the protocol method's return type with `T` substituted (Unknown receiver ⇒
    /// the return stays generic/Unknown). Used on the diagnostic paths so the call
    /// site still gets a sensible result type.
    pub(super) fn protocol_ret(&self, sig: &crate::form::ProtocolMethodSig, recv: &Type) -> Type {
        if matches!(recv, Type::Unknown) {
            // No concrete receiver: a prim return (string/bool/int) is still concrete;
            // a `T` return becomes Unknown.
            return match &sig.ret {
                Type::Var(_) => Type::Unknown,
                other => other.clone(),
            };
        }
        let subst = [recv.clone()];
        crate::form::substitute(&sig.ret, &subst)
    }

    pub(super) fn member_type(&mut self, object_ty: &Type, field: &ast::Ident, span: Span) -> Type {
        let name = self.former.text(field.span);
        // §3 (v5.4) a NEWTYPE exposes EXACTLY ONE builtin method: `.value()`, a
        // zero-arg method returning the BASE type (`id.value()` → int). Any other
        // member is absent (no implicit base-method forwarding — you must `.value()`
        // first). Dispatched here because the base lives in the former's table.
        if let Type::Newtype { base, args } = object_ty {
            let id = nominal_instance_id(base, args);
            if name == "value" {
                let base = self
                    .former
                    .newtype_info(&id)
                    .map(|i| i.base.clone())
                    .unwrap_or(Type::Unknown);
                return Type::Func {
                    params: Vec::new(),
                    variadic: None,
                    ret: Box::new(base),
                };
            }
            self.former.error(
                codes::NO_FIELD,
                format!(
                    "newtype `{id}` has no member named `{name}` (use `.value()` to unwrap the base)"
                ),
                span,
            );
            return Type::Unknown;
        }
        match builtins::receiver_member(object_ty, name) {
            Some(Member::Property(ty)) => return ty,
            Some(Member::Method(scheme)) => {
                return Type::Func {
                    params: scheme.params,
                    variadic: scheme.variadic.map(Box::new),
                    ret: Box::new(scheme.ret),
                };
            }
            None => {}
        }
        // A record returns its field type, or a NO_FIELD with a field-name hint.
        if let Type::Record(fields) = object_ty {
            return match fields.iter().find(|(n, _)| n == name) {
                Some((_, ty)) => ty.clone(),
                None => {
                    let display = object_ty.to_string();
                    let hint = topaz_diag::suggest::did_you_mean(
                        name,
                        fields.iter().map(|(n, _)| n.as_str()),
                    );
                    self.former.error(
                        codes::NO_FIELD,
                        format!("`{display}` has no field `{name}`{hint}"),
                        span,
                    );
                    Type::Unknown
                }
            };
        }
        // §3 (v5.4) a NOMINAL record returns its DECLARED field type (looked up by
        // name from the record table), or a NO_FIELD for an unknown field. A
        // callable (Func-typed) field types as a Func so `b.f()` resolves; this
        // also makes a bad field-access type (e.g. `return u.name` for `-> int`)
        // a CHECK error rather than a runtime fault.
        if let Type::NominalRecord { base, args } = object_ty {
            let id = nominal_instance_id(base, args);
            let info = self.former.record_info(&id).cloned();
            return match info
                .as_ref()
                .and_then(|i| i.fields.iter().find(|f| f.name == name))
            {
                Some(f) => f.ty.clone(),
                None => {
                    let known: Vec<&str> = info
                        .as_ref()
                        .map(|i| i.fields.iter().map(|f| f.name.as_str()).collect())
                        .unwrap_or_default();
                    let hint = topaz_diag::suggest::did_you_mean(name, known.iter().copied());
                    self.former.error(
                        codes::NO_FIELD,
                        format!("record `{id}` has no field `{name}`{hint}"),
                        span,
                    );
                    Type::Unknown
                }
            };
        }
        // Any other receiver: emit NO_FIELD iff the member is DECIDABLY absent —
        // a member-closed builtin/scalar that lacks it (`int`/`float`/`bool` and
        // `string`, whose only member is `scalars` — C3/C7), OR a UNION with a
        // member-closed non-null arm that lacks it (that arm would runtime-fault
        // — C2). An opaque/undecidable receiver (`Unknown`/`Var`, a null-only or
        // open union) stays staged. `receiver_has_member` is the single source
        // of truth.
        if receiver_has_member(object_ty, name) == Some(false) {
            let display = object_ty.to_string();
            let hint = topaz_diag::suggest::did_you_mean(
                name,
                builtins::receiver_member_names(object_ty).iter().copied(),
            );
            self.former.error(
                codes::NO_FIELD,
                format!("`{display}` has no member named `{name}`{hint}"),
                span,
            );
            return Type::Unknown;
        }
        // An opaque-but-rigid receiver (a bare generic, or a union with a
        // rigid arm) projects a rigid member type instead of tainting to
        // `Unknown` — `Unknown` would silently discharge a concrete expectation
        // (`function steal<T>(t: T) -> int { return t.field }` must NOT check).
        // A truly gradual receiver (`Unknown`/`Var`) stays `Unknown`.
        let name = name.to_string();
        if let Some(proj) = self.project_member(object_ty, &name) {
            return proj;
        }
        Type::Unknown
    }
}
