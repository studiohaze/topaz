use super::*;

pub(super) fn resolve_inference(ty: &Type, solutions: &HashMap<u32, Type>) -> Type {
    ty.transform_components(&mut |component| match component {
        Type::Var(index) => solutions.get(index).cloned(),
        _ => None,
    })
}

impl<'a> ExprChecker<'a> {
    pub(super) fn check_expr(&mut self, expr: &'a ast::Expr, expected: &Type) -> Type {
        let ty = self.check_expr_inner(expr, expected);
        self.record_typed_node(topaz_hir::TypedNodeKind::Expression, expr.span, &ty);
        ty
    }

    pub(super) fn check_expr_inner(&mut self, expr: &'a ast::Expr, expected: &Type) -> Type {
        match &expr.kind {
            ast::ExprKind::Paren(inner) => self.check_expr(inner, expected),
            ast::ExprKind::Block(block) => self.check_block_with(block, Some(expected)),
            ast::ExprKind::Lambda { params, body } => {
                // A lambda's arity is FIXED (lambdas are never variadic). If a
                // concrete, non-variadic function type is expected, a differing
                // parameter count cannot satisfy it — e.g. a 1-param lambda for a
                // `() -> E` callback, which would fault "missing argument" at
                // runtime. (When the counts DO match, contextual param types flow in
                // below.)
                if let Type::Func {
                    params: ep,
                    variadic: None,
                    ..
                } = expected
                    && ep.len() != params.len()
                {
                    self.former.error(
                        codes::ARITY,
                        format!(
                            "this lambda takes {} parameter(s), but a function taking {} is expected",
                            params.len(),
                            ep.len()
                        ),
                        expr.span,
                    );
                }
                let expected_fn = match expected {
                    Type::Func { params: ep, .. } if ep.len() == params.len() => Some(ep.clone()),
                    _ => None,
                };
                self.push_scope();
                let mut param_types = Vec::new();
                for (i, param) in params.iter().enumerate() {
                    let ty = match &param.ty {
                        Some(annot) => {
                            let env = self.tyenv();
                            self.former.form(annot, &env)
                        }
                        None => expected_fn
                            .as_ref()
                            .map(|ep| ep[i].clone())
                            .unwrap_or(Type::Unknown),
                    };
                    let name = self.former.text(param.name.span).to_string();
                    self.record_typed_local(&name, param.name.span, &ty);
                    self.bind(name, ty.clone());
                    param_types.push(ty);
                }
                let body_ty = self.lambda_body_type(body);
                self.pop_scope();
                let produced = Type::Func {
                    params: param_types,
                    variadic: None,
                    ret: Box::new(body_ty),
                };
                self.expect(&produced, expected, expr.span);
                produced
            }
            ast::ExprKind::Array(elements) if !elements.is_empty() => {
                if let Type::Ctor(Ctor::Array, args) = expected {
                    let elem = args[0].clone();
                    for e in elements {
                        match e {
                            ast::ArrayElement::Expr(item) => {
                                self.check_expr(item, &elem);
                            }
                            ast::ArrayElement::Spread(item) => {
                                // A spread's element must match the expected
                                // array element — `let xs: Array<int> = [...strs]`
                                // (and the rigid `[...t]`) must NOT slip.
                                let spread_ty = self.infer(item);
                                match self.spread_elem(&spread_ty) {
                                    SpreadElem::Elem(spread_elem) => {
                                        self.expect(&spread_elem, &elem, item.span);
                                    }
                                    SpreadElem::Stage => {}
                                    SpreadElem::NotArray => {
                                        self.former.error(
                                            codes::TYPE_MISMATCH,
                                            format!(
                                                "array spread needs an `Array`, found `{spread_ty}`"
                                            ),
                                            item.span,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    expected.clone()
                } else {
                    let t = self.infer(expr);
                    self.expect(&t, expected, expr.span);
                    t
                }
            }
            ast::ExprKind::RecordLiteral { fields } => {
                // The expected record: a bare `Record`, or — when a UNION is
                // expected — the single union member this literal narrows to (its
                // field names match, and any literal discriminant it supplies is
                // satisfied). Checking against that member keeps literal-typed
                // fields (e.g. `kind: "circle"`) from widening to their primitive,
                // so a tagged/closed union accepts a matching record literal.
                let chosen: Option<Type> = match expected {
                    Type::Record(_) => Some(expected.clone()),
                    Type::Union(members) => self.narrow_record_literal(fields, members),
                    _ => None,
                };
                if let Some(Type::Record(expected_fields)) = &chosen {
                    let expected_fields = expected_fields.clone();
                    let mut formed: Vec<(String, Type)> = Vec::new();
                    for field in fields {
                        let name = self.former.text(field.name.span).to_string();
                        let ty = match expected_fields.iter().find(|(n, _)| *n == name) {
                            Some((_, ft)) => {
                                let ft = ft.clone();
                                self.check_expr(&field.value, &ft);
                                ft
                            }
                            None => self.infer(&field.value).widen(),
                        };
                        if formed.iter().any(|(n, _)| *n == name) {
                            self.former.error(
                                codes::MALFORMED_TYPE,
                                format!("record literal declares field `{name}` twice"),
                                field.span,
                            );
                            continue;
                        }
                        formed.push((name, ty));
                    }
                    formed.sort_by(|(a, _), (b, _)| a.cmp(b));
                    let produced = Type::Record(formed);
                    self.expect(&produced, expected, expr.span);
                    produced
                } else {
                    let t = self.infer(expr);
                    self.expect(&t, expected, expr.span);
                    t
                }
            }
            // Context-needing forms route the expectation through.
            ast::ExprKind::Call { .. }
            | ast::ExprKind::Ident
            | ast::ExprKind::Member { .. }
            | ast::ExprKind::Match { .. }
            | ast::ExprKind::If { .. }
            | ast::ExprKind::Concurrent { .. }
            | ast::ExprKind::Pipe { .. }
            | ast::ExprKind::Array(_)
            | ast::ExprKind::RecordUpdate { .. }
            // §6 (v5.4) set/map literals route the expectation through so an
            // empty `set {}`/`map {}` resolves its element/key/value type from the
            // annotation (the empty-`[]` path), and a non-empty literal's element
            // type can still be checked against the expected `Set<T>`/`Map<K,V>`.
            | ast::ExprKind::SetLiteral(_)
            | ast::ExprKind::MapLiteral(_) => {
                let t = self.infer_with(expr, Some(expected));
                self.expect(&t, expected, expr.span);
                t
            }
            _ => {
                let t = self.infer(expr);
                self.expect(&t, expected, expr.span);
                t
            }
        }
    }

    pub fn infer(&mut self, expr: &'a ast::Expr) -> Type {
        self.infer_with(expr, None)
    }

    pub(super) fn infer_with(&mut self, expr: &'a ast::Expr, ctx: Option<&Type>) -> Type {
        let ty = self.infer_with_inner(expr, ctx);
        self.record_typed_node(topaz_hir::TypedNodeKind::Expression, expr.span, &ty);
        ty
    }

    pub(super) fn infer_with_inner(&mut self, expr: &'a ast::Expr, ctx: Option<&Type>) -> Type {
        // One-shot: only the outermost initializer expression of a
        // bare binding may report §22.1; nested subexpressions sit in
        // positions whose context sites arrive in later phases.
        let bare = std::mem::take(&mut self.at_bare_binding);
        match expression_family(&expr.kind) {
            ExpressionFamily::Atomic => self.infer_atomic_expression(expr, ctx, bare),
            ExpressionFamily::Control => self.infer_control_expression(expr, ctx, bare),
            ExpressionFamily::Operation => self.infer_operation_expression(expr, ctx, bare),
            ExpressionFamily::Aggregate => self.infer_aggregate_expression(expr, ctx, bare),
        }
    }

    pub(super) fn infer_atomic_expression(
        &mut self,
        expr: &'a ast::Expr,
        ctx: Option<&Type>,
        bare: bool,
    ) -> Type {
        match &expr.kind {
            ast::ExprKind::Int => self.literal_from_span(expr.span),
            ast::ExprKind::Float => {
                Type::Literal(Lit::Float(self.former.text(expr.span).to_string()))
            }
            ast::ExprKind::Bool(b) => Type::Literal(Lit::Bool(*b)),
            ast::ExprKind::Null => Type::Literal(Lit::Null),
            ast::ExprKind::Unit => Type::Prim(Prim::Unit),
            // Durations type with `concurrent` in a later phase.
            ast::ExprKind::Duration(_) => Type::Unknown,
            ast::ExprKind::String(lit) => {
                let mut interpolated = false;
                let mut text = String::new();
                for part in &lit.parts {
                    match part {
                        ast::StringPart::Interpolation(inner) => {
                            // §1 interpolation accepts ordinary values, but ADR-108
                            // deliberately keeps mutable ByteBuffer storage out of
                            // both plain rendering and tagged-template capture.
                            interpolated = true;
                            let inferred = self.infer(inner);
                            let contains_byte_buffer = {
                                let enums = self.former.enum_table();
                                let records = self.former.record_table();
                                let newtypes = self.former.newtype_table();
                                contains_byte_buffer_in(
                                    &inferred,
                                    enums,
                                    records,
                                    newtypes,
                                    &mut Vec::new(),
                                )
                            };
                            if contains_byte_buffer {
                                self.former.error(
                                    codes::TYPE_MISMATCH,
                                    "`ByteBuffer` values cannot be interpolated; snapshot to `Bytes` and encode explicitly"
                                        .to_string(),
                                    inner.span,
                                );
                            }
                        }
                        ast::StringPart::Text(span) => {
                            text.push_str(self.former.text(*span));
                        }
                    }
                }
                if lit.tag.is_some() {
                    // §16: opaque, non-comparable template value.
                    Type::Template
                } else if interpolated {
                    Type::Prim(Prim::String)
                } else {
                    // A plain string literal keeps its literal type
                    // (CDR-004 §4) under raw-text identity.
                    Type::Literal(Lit::Str(text))
                }
            }
            ast::ExprKind::Ident => {
                let name = self.former.text(expr.span);
                if self.lispex_rule_factories.contains_key(name) {
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        "a generated Lispex rule factory may only appear as the direct callee of a zero-argument call"
                            .to_string(),
                        expr.span,
                    );
                    return Type::Unknown;
                }
                if let Some(t) = self.lookup(name).cloned() {
                    if let Some(expected) = ctx
                        && let Some(instantiated) =
                            self.instantiate_generic_value_against(name, &t, expected, expr.span)
                    {
                        return instantiated;
                    }
                    return t;
                }
                if let Some(scheme) = builtins::constant(name) {
                    return self.instantiate_value(scheme, ctx, expr.span, bare);
                }
                if let Some(scheme) = builtins::free_function(name) {
                    // A builtin referenced as a value: its mono type
                    // when concrete, Unknown when generic.
                    return if scheme.vars == 0 {
                        Type::Func {
                            params: scheme.params,
                            variadic: scheme.variadic.map(Box::new),
                            ret: Box::new(scheme.ret),
                        }
                    } else {
                        Type::Unknown
                    };
                }
                if self.namespaces.contains_key(name) {
                    // A namespace used as a bare value stays opaque;
                    // member access intercepts before this point.
                    return Type::Unknown;
                }
                if self.module_mode {
                    // A name that IS a top-level binding but is not yet
                    // in scope here is a FORWARD reference — the
                    // init-order pass (`check_init_order`) owns that
                    // verdict, so the bare type check stays silent
                    // rather than double-report it as "not bound".
                    if self
                        .top_level
                        .as_ref()
                        .is_some_and(|t| t.is_forward_runtime_name(name))
                    {
                        return Type::Unknown;
                    }
                    // C-6: the unit's name space is closed (§17).
                    let hint = self.unbound_hint(name);
                    self.former.error(
                        codes::UNBOUND,
                        format!("`{name}` is not bound{hint}"),
                        expr.span,
                    );
                    return Type::Unknown;
                }
                // Fragment mode: ambient names stay Unknown (the
                // docs corpus posture).
                Type::Unknown
            }
            ast::ExprKind::Placeholder => match self.pipe_value.clone() {
                Some(value) => value,
                None => {
                    // §11: `_` is valid only as a pipeline
                    // placeholder inside the stage being typed.
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        "a placeholder `_` is valid only inside a pipeline stage (§11)".to_string(),
                        expr.span,
                    );
                    Type::Unknown
                }
            },
            ast::ExprKind::Paren(inner) => {
                self.at_bare_binding = bare;
                self.infer_with(inner, ctx)
            }
            ast::ExprKind::Block(block) => self.check_block_bare(block, bare),
            _ => unreachable!("expression family changed after classification"),
        }
    }

    pub(super) fn infer_control_expression(
        &mut self,
        expr: &'a ast::Expr,
        ctx: Option<&Type>,
        bare: bool,
    ) -> Type {
        match &expr.kind {
            ast::ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => self.check_if(
                cond,
                then_block,
                else_branch.as_deref(),
                ctx,
                bare,
                expr.span,
            ),
            ast::ExprKind::Match { scrutinee, cases } => {
                self.check_match(scrutinee, cases, ctx, expr.span, bare)
            }
            ast::ExprKind::For {
                pattern,
                iter,
                body,
            } => self.check_for(pattern, iter, body, false),
            // `loop (label)? { body }` is an infinite-loop expression.
            // Its type is the JOIN of every `break <value>` targeting it (Unit
            // when no break carries a value), inferred exactly as an omitted
            // return type is inferred from the body's `return` join. The optional
            // label lets an inner `break 'l <value>` (from a nested loop) target
            // it; the body always runs at least once but never falls THROUGH (a
            // `loop` exits only via `break`/`return`/`?`), so the body's own value
            // never contributes — only breaks do.
            ast::ExprKind::Loop { label, body } => {
                let label_text = label.map(|l| self.former.text(l.span).to_string());
                self.loop_ctx.push(LoopFrame {
                    label: label_text,
                    value_loop: true,
                    bare_target: true,
                    bare_error: None,
                    expected: ctx.cloned(),
                    breaks: Vec::new(),
                });
                self.push_scope();
                self.check_block(body);
                self.pop_scope();
                let frame = self.loop_ctx.pop().expect("loop_ctx stack");
                if frame.breaks.is_empty() {
                    // No value-or-Unit break reached this loop (no `break` at all,
                    // or it diverges) — the loop yields Unit.
                    Type::Prim(Prim::Unit)
                } else {
                    self.join_branches(frame.breaks, ctx, bare, expr.span)
                }
            }
            ast::ExprKind::Concurrent {
                timeout,
                arms,
                else_block,
            } => {
                if let Some(t) = timeout {
                    let duration_fits = match &t.kind {
                        ast::ExprKind::Duration(_) => {
                            topaz_syntax::parse_duration_milliseconds(self.former.text(t.span))
                                .is_some()
                        }
                        _ => true,
                    };
                    if !duration_fits {
                        self.former.error(
                            codes::TYPE_MISMATCH,
                            "`concurrent` timeout duration must fit in u64 milliseconds (§15)"
                                .to_string(),
                            t.span,
                        );
                    }
                    self.infer(t);
                }
                // §15: the success result is a record with one field
                // per arm; duplicate arm names are a static error; a
                // record context routes per-arm field expectations.
                let mut fields: Vec<(String, Type)> = Vec::new();
                for arm in arms {
                    let name = self.former.text(arm.name.span).to_string();
                    if fields.iter().any(|(n, _)| *n == name) {
                        self.former.error(
                            codes::REDECLARE,
                            format!("duplicate concurrent arm `{name}`"),
                            arm.name.span,
                        );
                    }
                    let expected = match ctx {
                        Some(Type::Record(ctx_fields)) => ctx_fields
                            .iter()
                            .find(|(n, _)| *n == name)
                            .map(|(_, t)| t.clone()),
                        _ => None,
                    };
                    let ty = match expected {
                        Some(x) => self.check_expr(&arm.value, &x),
                        None => self.infer(&arm.value).widen(),
                    };
                    fields.push((name, ty));
                }
                fields.sort_by(|a, b| a.0.cmp(&b.0));
                let record = Type::Record(fields);
                if let Some(b) = else_block {
                    // §15: the else result joins under the same
                    // branch-compatibility rule as if/match.
                    if block_diverges(b) || record.has_unknown() {
                        self.check_block(b);
                    } else {
                        self.check_block_with(b, Some(&record));
                    }
                }
                if record.has_unknown() {
                    Type::Unknown
                } else {
                    record
                }
            }
            _ => unreachable!("expression family changed after classification"),
        }
    }

    pub(super) fn infer_operation_expression(
        &mut self,
        expr: &'a ast::Expr,
        ctx: Option<&Type>,
        bare: bool,
    ) -> Type {
        match &expr.kind {
            ast::ExprKind::Call {
                callee,
                args,
                type_args,
            } => self.infer_call(CallRequest {
                callee,
                args,
                type_args,
                context: ctx,
                span: expr.span,
                bare,
                leading: None,
            }),
            ast::ExprKind::Member { object, field } => {
                if let ast::ExprKind::Ident = object.kind {
                    let head = self.former.text(object.span);
                    let member = self.former.text(field.span);
                    if self.lookup(head).is_none()
                        && head == "RoundingMode"
                        && builtins::ROUNDING_MODE_VALUE_NAMES.contains(&member)
                    {
                        return Type::RoundingMode;
                    }
                    if self.lookup(head).is_none() && self.namespaces.contains_key(head) {
                        if self
                            .lispex_rule_namespaces
                            .get(head)
                            .is_some_and(|members| members.contains_key(member))
                        {
                            self.former.error(
                                codes::TYPE_MISMATCH,
                                "a generated Lispex rule factory may only appear as the direct callee of a zero-argument call"
                                    .to_string(),
                                expr.span,
                            );
                            return Type::Unknown;
                        }
                        return self.namespace_member(head, field);
                    }
                    // §3 enum CONSTRUCTION (v5.3): `Color.Red` where `Color` is a
                    // declared enum NOT shadowed by a binding, and `Red` is one of
                    // its variants, types as the nominal `Type::Enum`. Intercepts
                    // before ordinary member access. This bare (un-called) form is a
                    // payload-LESS construction; `enum_construct` reports an arity
                    // error if the variant actually carries a payload.
                    if self.lookup(head).is_none() && self.former.is_enum(head) {
                        return self.enum_construct(head, field, &[], expr.span, ctx).result;
                    }
                }
                let object_ty = self.infer(object);
                self.check_mutator_access(object, &object_ty, field);
                self.member_type(&object_ty, field, expr.span)
            }
            ast::ExprKind::OptionalAccess { object, field } => {
                if let ast::ExprKind::Ident = object.kind {
                    let head = self.former.text(object.span);
                    if self.lookup(head).is_none() && self.namespaces.contains_key(head) {
                        self.former.error(
                            codes::TYPE_MISMATCH,
                            "`?.` needs an Option or nullable receiver, found `namespace`"
                                .to_string(),
                            expr.span,
                        );
                        return Type::Unknown;
                    }
                }
                // §9 on an optional receiver is value-dependent: `?.`
                // short-circuits on None at runtime (no mutation, no
                // fault), so the checker does NOT statically reject a
                // mutator through an immutable optional root — that
                // would over-reject the None case. The runtime
                // enforces §9 precisely on the Some branch.
                let object_ty = self.infer(object);
                self.optional_member(&object_ty, field, expr.span, None)
            }
            ast::ExprKind::Index { object, index } => {
                let object_ty = self.infer(object);
                let index_ty = self.infer(index);
                match object_ty {
                    Type::Ctor(Ctor::Array, args) => {
                        self.expect(&index_ty, &Type::Prim(Prim::Int), index.span);
                        args.into_iter().next().expect("Array arity 1")
                    }
                    // §9: only arrays are index-readable. A concrete non-array
                    // receiver faults at runtime ("cannot index …"), so reject it
                    // statically — `check` should gate what `run` does. A type still
                    // carrying unknowns/vars stays staged (it may resolve to Array).
                    other if other.has_unknown() => Type::Unknown,
                    // A union with an array member may BE that array at runtime, which
                    // the interpreter indexes — so do not reject it (stage instead).
                    // The index must still be an `int`, exactly as for a plain array.
                    Type::Union(members)
                        if members
                            .iter()
                            .any(|m| matches!(m, Type::Ctor(Ctor::Array, _))) =>
                    {
                        self.expect(&index_ty, &Type::Prim(Prim::Int), index.span);
                        Type::Unknown
                    }
                    other => {
                        let msg = if matches!(other, Type::Ctor(Ctor::Map, _)) {
                            "a Map is not index-readable; use `m.get` (§22)".to_string()
                        } else {
                            format!(
                                "`{other}` is not index-readable; only arrays support `[…]` indexing (§9)"
                            )
                        };
                        self.former.error(codes::TYPE_MISMATCH, msg, expr.span);
                        Type::Unknown
                    }
                }
            }
            ast::ExprKind::Try(inner) => {
                // `?` desugars to a conditional `return`, so at the module top level
                // it is "`return` outside a function" — the same fault the interpreter
                // raises. Gate it statically (TPZ5001) so `check` matches `run`.
                if self.ret_ctx.is_empty() {
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        "`return` outside a function".to_string(),
                        expr.span,
                    );
                }
                let inner_ty = self.infer(inner);
                match inner_ty {
                    Type::Ctor(Ctor::Result, args) => {
                        let (value, error) = (args[0].clone(), args[1].clone());
                        // §13: the enclosing function's return must
                        // carry the same error value.
                        if let Some(Some(ret)) = self.ret_ctx.last() {
                            match ret {
                                Type::Ctor(Ctor::Result, ret_args) => {
                                    let expected_err = ret_args[1].clone();
                                    self.expect(&error, &expected_err, expr.span);
                                }
                                ret if !ret.has_unknown() => {
                                    let display = ret.clone();
                                    self.former.error(
                                        codes::TYPE_MISMATCH,
                                        format!(
                                            "`?` propagates an error, but the function returns `{display}`, not a Result"
                                        ),
                                        expr.span,
                                    );
                                }
                                _ => {}
                            }
                        }
                        value
                    }
                    ty if ty.has_unknown() => Type::Unknown,
                    other => {
                        self.former.error(
                            codes::TYPE_MISMATCH,
                            format!("`?` requires a Result value, found `{other}`"),
                            expr.span,
                        );
                        Type::Unknown
                    }
                }
            }
            ast::ExprKind::Unary { op, operand } => {
                let operand_ty = self.infer(operand).widen();
                self.unary_type(*op, operand_ty, operand.span)
            }
            ast::ExprKind::Binary {
                op: ast::BinaryOp::Coalesce,
                lhs,
                rhs,
            } => {
                // §12 `??`: typed by the static type of the left
                // operand, unwrapping exactly one layer. The right
                // operand is a context site checked against the
                // inner type, so contextual literals survive.
                let lhs_ty = self.infer(lhs);
                match unwrap_optional(&lhs_ty) {
                    Some(inner) => {
                        self.check_expr(rhs, &inner);
                        inner
                    }
                    None if !lhs_ty.has_unknown() => {
                        self.former.error(
                            codes::TYPE_MISMATCH,
                            format!(
                                "`??` needs an Option or nullable left operand, found `{lhs_ty}`"
                            ),
                            expr.span,
                        );
                        self.infer(rhs);
                        Type::Unknown
                    }
                    None => {
                        self.infer(rhs);
                        Type::Unknown
                    }
                }
            }
            ast::ExprKind::Binary { op, lhs, rhs } => {
                let mut lhs_ty = self.infer(lhs).widen();
                let mut rhs_ty = self.infer(rhs).widen();
                if matches!(op, ast::BinaryOp::Eq | ast::BinaryOp::Ne) {
                    if type_has_var(&lhs_ty)
                        && !type_has_var(&rhs_ty)
                        && !contains_true_unknown(&rhs_ty)
                    {
                        lhs_ty = self.solve_recorded_inference_against(&lhs_ty, &rhs_ty);
                    } else if type_has_var(&rhs_ty)
                        && !type_has_var(&lhs_ty)
                        && !contains_true_unknown(&lhs_ty)
                    {
                        rhs_ty = self.solve_recorded_inference_against(&rhs_ty, &lhs_ty);
                    }
                }
                self.binary_type(*op, lhs_ty, rhs_ty, expr.span)
            }
            ast::ExprKind::Range { lo, hi, step, .. } => {
                // §10: range element types must support ordered
                // stepping; the v5.2 surface is integer ranges.
                let int = Type::Prim(Prim::Int);
                for end in [&**lo, &**hi] {
                    let end_ty = self.infer(end).widen();
                    if !end_ty.has_unknown() && !usable(&end_ty, &int) {
                        self.former.error(
                            codes::TYPE_MISMATCH,
                            format!("range endpoints are `int` in v5.2, found `{end_ty}`"),
                            end.span,
                        );
                    }
                }
                if let Some(s) = step {
                    let s_ty = self.infer(s).widen();
                    if !s_ty.has_unknown() && !usable(&s_ty, &int) {
                        self.former.error(
                            codes::TYPE_MISMATCH,
                            format!("a range step is `int`, found `{s_ty}`"),
                            s.span,
                        );
                    }
                    // §10: a constant step of zero is a static error.
                    if self.const_int(s) == Some(0) {
                        self.former.error(
                            codes::TYPE_MISMATCH,
                            "the range step must not be zero (§10)".to_string(),
                            s.span,
                        );
                    }
                }
                Type::Ctor(Ctor::Range, vec![int])
            }
            ast::ExprKind::Compose { lhs, rhs } => {
                // §11: `((A) -> B) >> ((B) -> C) : (A) -> C`.
                let lhs_ty = self.infer(lhs);
                let rhs_ty = self.infer(rhs);
                if lhs_ty.has_unknown() || rhs_ty.has_unknown() {
                    return Type::Unknown;
                }
                match (lhs_ty, rhs_ty) {
                    (
                        Type::Func {
                            params,
                            variadic,
                            ret: mid,
                        },
                        Type::Func {
                            params: rhs_params,
                            variadic: rhs_variadic,
                            ret,
                        },
                    ) => {
                        // §11: composition is UNARY on both sides — `((A) -> B) >>
                        // ((B) -> C)`, with "no multi-argument composition". An
                        // operand is unary iff it has exactly one fixed parameter
                        // AND is not variadic (a `(int, ...int)` tail is still
                        // multi-argument). Both sides are enforced.
                        if params.len() != 1 || variadic.is_some() {
                            self.former.error(
                                codes::ARITY,
                                "the left side of `>>` takes exactly one argument (§11)"
                                    .to_string(),
                                lhs.span,
                            );
                        }
                        if rhs_params.len() == 1 && rhs_variadic.is_none() {
                            self.expect(&mid, &rhs_params[0], rhs.span);
                        } else {
                            self.former.error(
                                codes::ARITY,
                                "the right side of `>>` takes exactly one argument (§11)"
                                    .to_string(),
                                rhs.span,
                            );
                        }
                        Type::Func {
                            params,
                            variadic,
                            ret,
                        }
                    }
                    (Type::Func { .. }, other) => {
                        self.former.error(
                            codes::NOT_CALLABLE,
                            format!("`{other}` is not callable"),
                            rhs.span,
                        );
                        Type::Unknown
                    }
                    (other, _) => {
                        self.former.error(
                            codes::NOT_CALLABLE,
                            format!("`{other}` is not callable"),
                            lhs.span,
                        );
                        Type::Unknown
                    }
                }
            }
            ast::ExprKind::Pipe { lhs, rhs } => match rhs.as_ref() {
                ast::PipeRhs::Expr(rhs) => {
                    // §11 stage typing, in spec order: placeholder
                    // replacement first, then first-argument insertion
                    // into a call, then unary application of a callable.
                    let lhs_ty = self.infer(lhs);
                    if let ast::ExprKind::Call {
                        callee,
                        args,
                        type_args,
                    } = &rhs.kind
                    {
                        // §11: a placeholder is valid only inside the
                        // stage call's ARGUMENT list — `_` as the callee
                        // (or any non-argument position) is a static
                        // error.
                        if ast::contains_placeholder(callee) {
                            self.former.error(
                            codes::TYPE_MISMATCH,
                            "a placeholder `_` is valid only in a pipeline stage's argument list (§11)".to_string(),
                            callee.span,
                        );
                        }
                        // §11: placeholder replacement has priority and
                        // suppresses first-argument insertion. `_` binds
                        // anywhere in the stage's argument expressions.
                        let has_placeholder = ast::call_args_contain_placeholder(args);
                        let saved = self.pipe_value.take();
                        let result = if has_placeholder {
                            self.pipe_value = Some(lhs_ty);
                            self.infer_call(CallRequest {
                                callee,
                                args,
                                type_args,
                                context: ctx,
                                span: expr.span,
                                bare: false,
                                leading: None,
                            })
                        } else {
                            self.infer_call(CallRequest {
                                callee,
                                args,
                                type_args,
                                context: ctx,
                                span: expr.span,
                                bare: false,
                                leading: Some(&lhs_ty),
                            })
                        };
                        self.pipe_value = saved;
                        result
                    } else {
                        let stage = self.infer(rhs);
                        self.pipe_apply(lhs_ty, stage, rhs.span)
                    }
                }
                ast::PipeRhs::Field(field) => {
                    // §11: `.field` sugar is member access on the saved
                    // left-hand value.
                    let lhs_ty = self.infer(lhs);
                    self.check_mutator_access(lhs, &lhs_ty, field);
                    self.member_type(&lhs_ty, field, expr.span)
                }
            },
            ast::ExprKind::Lambda { params, body } => {
                self.push_scope();
                let mut param_types = Vec::new();
                for param in params {
                    let ty = match &param.ty {
                        Some(annot) => {
                            let env = self.tyenv();
                            self.former.form(annot, &env)
                        }
                        None => Type::Unknown,
                    };
                    let name = self.former.text(param.name.span).to_string();
                    self.record_typed_local(&name, param.name.span, &ty);
                    self.bind(name, ty.clone());
                    param_types.push(ty);
                }
                let body_ty = self.lambda_body_type(body);
                self.pop_scope();
                Type::Func {
                    params: param_types,
                    variadic: None,
                    ret: Box::new(body_ty),
                }
            }
            _ => unreachable!("expression family changed after classification"),
        }
    }

    pub(super) fn infer_aggregate_expression(
        &mut self,
        expr: &'a ast::Expr,
        ctx: Option<&Type>,
        bare: bool,
    ) -> Type {
        match &expr.kind {
            ast::ExprKind::RecordLiteral { fields } => self.record_literal(fields),
            ast::ExprKind::RecordUpdate {
                base,
                spread,
                fields,
            } => {
                // §3 (v5.4) NOMINAL record CONSTRUCTION `User { name: …, age: … }`:
                // the parser models it as a RecordUpdate whose `base` is the record
                // NAME (an `Ident`). When that ident names a declared record NOT
                // shadowed by a binding, this is nominal construction, NOT a
                // structural update — type the explicit fields against the decl,
                // reject unknown/dup, require missing non-default fields. With a
                // LEADING spread (`User { ...u, … }`), the spread base must type to
                // the SAME nominal id; it supplies fields the explicit/default sets
                // do not (see `nominal_construct`).
                if let ast::ExprKind::Ident = base.kind {
                    let name = self.former.text(base.span);
                    if self.lookup(name).is_none() && self.former.is_record(name) {
                        return self.nominal_construct(
                            name,
                            spread.as_deref(),
                            fields,
                            expr.span,
                            ctx,
                        );
                    }
                }
                // A spread `{ ...x, … }` is a NOMINAL-only feature (§3 v5.4). The
                // parser only emits `spread = Some(_)` after a bare ident, so reaching
                // here means that ident is NOT a declared record (it is a local /
                // unknown). Reject rather than silently dropping the spread value —
                // a structural record has no nominal-spread semantics.
                if let Some(spread) = spread {
                    self.infer(spread);
                    let name = self.former.text(base.span);
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        format!("record spread `...` needs a declared record; `{name}` is not one"),
                        expr.span,
                    );
                    return Type::Unknown;
                }
                let base_ty = self.infer(base);
                match &base_ty {
                    Type::Record(base_fields) => {
                        // SPEC §8: with duplicate updates the LAST
                        // one wins; earlier values still infer for
                        // their own diagnostics.
                        for (i, field) in fields.iter().enumerate() {
                            let name = self.former.text(field.name.span);
                            let last = !fields[i + 1..]
                                .iter()
                                .any(|f| self.former.text(f.name.span) == name);
                            match base_fields.iter().find(|(n, _)| n == name) {
                                Some((_, field_ty)) => {
                                    if last {
                                        let expected = field_ty.clone();
                                        self.check_expr(&field.value, &expected);
                                    } else {
                                        self.infer(&field.value);
                                    }
                                }
                                None => {
                                    let display = base_ty.to_string();
                                    let hint = topaz_diag::suggest::did_you_mean(
                                        name,
                                        base_fields.iter().map(|(n, _)| n.as_str()),
                                    );
                                    self.former.error(
                                        codes::NO_FIELD,
                                        format!("`{display}` has no field `{name}`{hint}"),
                                        field.span,
                                    );
                                    self.infer(&field.value);
                                }
                            }
                        }
                        base_ty
                    }
                    _ => {
                        for field in fields {
                            self.infer(&field.value);
                        }
                        match &base_ty {
                            // A record update on a rigid generic projects an
                            // opaque rigid result, so it can't silently discharge a
                            // concrete expectation (`t { a: 1 }` must NOT check as `T`).
                            Type::Skolem { .. } | Type::Foreign { .. } => {
                                self.project(format!("RecordUpdateOf<{base_ty}>"))
                            }
                            // A union: a decidably non-record arm faults at runtime
                            // (reject), a rigid arm makes the whole result rigid
                            // (project), and a record/gradual arm is updatable/staged.
                            Type::Union(arms) => {
                                let mut has_rigid = false;
                                let mut non_record: Option<Type> = None;
                                for arm in arms {
                                    match arm {
                                        Type::Skolem { .. } | Type::Foreign { .. } => {
                                            has_rigid = true
                                        }
                                        Type::Record(_) | Type::Var(_) | Type::Union(_) => {}
                                        a if a.has_unknown() => {}
                                        a => {
                                            if non_record.is_none() {
                                                non_record = Some(a.clone());
                                            }
                                        }
                                    }
                                }
                                if let Some(bad) = non_record {
                                    self.former.error(
                                        codes::TYPE_MISMATCH,
                                        format!("record update needs a record, found `{bad}`"),
                                        base.span,
                                    );
                                    Type::Unknown
                                } else if has_rigid {
                                    self.project(format!("RecordUpdateOf<{base_ty}>"))
                                } else {
                                    Type::Unknown
                                }
                            }
                            // An inference var may BE a record at runtime — stage.
                            Type::Var(_) => Type::Unknown,
                            other if other.has_unknown() => Type::Unknown,
                            // A decidably non-record base faults at runtime ("record
                            // update needs a record"), so reject statically (check == run).
                            other => {
                                self.former.error(
                                    codes::TYPE_MISMATCH,
                                    format!("record update needs a record, found `{other}`"),
                                    base.span,
                                );
                                Type::Unknown
                            }
                        }
                    }
                }
            }
            ast::ExprKind::Array(elements) => {
                let mut member_types: Vec<Type> = Vec::new();
                let mut opaque = false;
                for elem in elements {
                    match elem {
                        ast::ArrayElement::Expr(e) => {
                            member_types.push(self.infer(e).widen());
                        }
                        ast::ArrayElement::Spread(e) => {
                            // A concrete `Array<E>` spread contributes `E`
                            // (not a poison `Unknown`); a rigid generic contributes
                            // a rigid `ElemOf<T>`; a decidably non-array is rejected
                            // (matching the runtime "array spread needs an `Array`").
                            let spread_ty = self.infer(e);
                            match self.spread_elem(&spread_ty) {
                                SpreadElem::Elem(elem) => member_types.push(elem),
                                SpreadElem::Stage => opaque = true,
                                SpreadElem::NotArray => {
                                    self.former.error(
                                        codes::TYPE_MISMATCH,
                                        format!(
                                            "array spread needs an `Array`, found `{spread_ty}`"
                                        ),
                                        e.span,
                                    );
                                    opaque = true;
                                }
                            }
                        }
                    }
                }
                if member_types.is_empty() && !opaque {
                    // `[]` demands a contextual type (§22.1).
                    match ctx {
                        Some(Type::Ctor(Ctor::Array, args)) => {
                            Type::Ctor(Ctor::Array, vec![args[0].clone()])
                        }
                        Some(t)
                            if self.collect_partial
                                && type_has_var(t)
                                && !contains_true_unknown(t) =>
                        {
                            // A pass-through inference hole: keep
                            // `Some([])` partial for the branch join.
                            let v = Type::Var(PARTIAL_VAR_OFFSET + self.partial_base);
                            self.partial_base += 1;
                            Type::Ctor(Ctor::Array, vec![v])
                        }
                        Some(t) if t.has_unknown() => Type::Ctor(Ctor::Array, vec![Type::Unknown]),
                        _ if bare => {
                            self.former.error(
                                codes::UNSOLVED,
                                "`[]` needs a contextual type (§22.1): annotate the binding or the expected element type"
                                    .to_string(),
                                expr.span,
                            );
                            Type::Ctor(Ctor::Array, vec![Type::Unknown])
                        }
                        _ if self.collect_partial => {
                            let v = Type::Var(PARTIAL_VAR_OFFSET + self.partial_base);
                            self.partial_base += 1;
                            Type::Ctor(Ctor::Array, vec![v])
                        }
                        _ => Type::Ctor(Ctor::Array, vec![Type::Unknown]),
                    }
                } else if opaque {
                    Type::Ctor(Ctor::Array, vec![Type::Unknown])
                } else {
                    Type::Ctor(Ctor::Array, vec![Type::union(member_types)])
                }
            }
            // §6 (v5.4) `set { e, … }` — a SET literal. Elements unify to the element
            // type `T` (each widened, like an array literal's elements); the runtime
            // requires `T` keyable. The key gate mirrors `freeze`, recursively through
            // unions/structural keys, so check==runtime. Empty `set {}` demands a
            // contextual `Set<T>` (mirrors empty `[]`).
            ast::ExprKind::SetLiteral(elements) => {
                if elements.is_empty() {
                    match ctx {
                        Some(Type::Ctor(Ctor::Set, args)) => {
                            Type::Ctor(Ctor::Set, vec![args[0].clone()])
                        }
                        _ if bare => {
                            self.former.error(
                                codes::UNSOLVED,
                                "`set {}` needs a contextual type (§6): annotate the binding or the expected element type"
                                    .to_string(),
                                expr.span,
                            );
                            Type::Ctor(Ctor::Set, vec![Type::Unknown])
                        }
                        _ => Type::Ctor(Ctor::Set, vec![Type::Unknown]),
                    }
                } else {
                    let mut member_types: Vec<Type> = Vec::with_capacity(elements.len());
                    for e in elements {
                        member_types.push(self.infer(e).widen());
                    }
                    let elem = Type::union(member_types);
                    self.gate_collection_key(&elem, expr.span);
                    Type::Ctor(Ctor::Set, vec![elem])
                }
            }
            // §6 (v5.4) `map { k: v, … }` — a MAP literal. Keys unify to `K`
            // (keyable by the same recursive `freeze` mirror), values to `V`. Empty
            // `map {}` demands a contextual `Map<K, V>`.
            ast::ExprKind::MapLiteral(entries) => {
                if entries.is_empty() {
                    match ctx {
                        Some(Type::Ctor(Ctor::Map, args)) => {
                            Type::Ctor(Ctor::Map, vec![args[0].clone(), args[1].clone()])
                        }
                        _ if bare => {
                            self.former.error(
                                codes::UNSOLVED,
                                "`map {}` needs a contextual type (§6): annotate the binding or the expected key/value types"
                                    .to_string(),
                                expr.span,
                            );
                            Type::Ctor(Ctor::Map, vec![Type::Unknown, Type::Unknown])
                        }
                        _ => Type::Ctor(Ctor::Map, vec![Type::Unknown, Type::Unknown]),
                    }
                } else {
                    let mut key_types: Vec<Type> = Vec::with_capacity(entries.len());
                    let mut value_types: Vec<Type> = Vec::with_capacity(entries.len());
                    for (k, v) in entries {
                        key_types.push(self.infer(k).widen());
                        value_types.push(self.infer(v).widen());
                    }
                    // §6 statically-obvious duplicate CONSTANT keys are a CHECK error
                    // (TPZ5602) — distinct from the runtime fault (TPZ4601) for
                    // dynamically-equal keys.
                    self.check_static_dup_map_keys(entries);
                    let key = Type::union(key_types);
                    self.gate_collection_key(&key, expr.span);
                    let value = Type::union(value_types);
                    Type::Ctor(Ctor::Map, vec![key, value])
                }
            }
            // §6.4 (v5.4) a COMPREHENSION `[ for … => body ]` / `set { … }` / `map { … }`.
            // Each `for`-clause infers its iterable's element type and binds the pattern
            // in a fresh scope (visible to later clauses + the body); each `if`-clause's
            // condition must be `bool`. The body — inferred under those bindings, with
            // the contextual element/key/value type routed in — gives the
            // element/key/value type, assembled into `Array<T>` / `Set<T>` / `Map<K, V>`.
            ast::ExprKind::Comprehension {
                kind,
                clauses,
                body,
            } => self.check_comprehension(*kind, clauses, body, ctx, bare, expr.span),
            _ => unreachable!("expression family changed after classification"),
        }
    }

    pub(super) fn check_comprehension(
        &mut self,
        kind: ast::CompKind,
        clauses: &'a [ast::CompClause],
        body: &'a ast::CompBody,
        ctx: Option<&Type>,
        bare: bool,
        span: Span,
    ) -> Type {
        // The contextual ELEMENT / KEY / VALUE type (from an annotated binding), so an
        // unconstrained body (`=> []`) and the empty-collection check have something to
        // fix on — mirrors the empty-`set {}` / empty-`map {}` path.
        let (elem_ctx, val_ctx) = match (kind, ctx) {
            (ast::CompKind::Array, Some(Type::Ctor(Ctor::Array, a)))
            | (ast::CompKind::Set, Some(Type::Ctor(Ctor::Set, a))) => (Some(a[0].clone()), None),
            (ast::CompKind::Map, Some(Type::Ctor(Ctor::Map, a))) => {
                (Some(a[0].clone()), Some(a[1].clone()))
            }
            _ => (None, None),
        };
        self.loop_ctx.push(LoopFrame {
            label: None,
            value_loop: false,
            bare_target: false,
            bare_error: Some("a comprehension (§6.4)"),
            expected: None,
            breaks: Vec::new(),
        });
        let mut pushed = 0usize;
        for clause in clauses {
            match clause {
                ast::CompClause::For { pattern, iter } => {
                    let iter_ty = self.infer(iter);
                    let elem = match self.iter_elem(&iter_ty) {
                        Some(elem) => elem,
                        None => {
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
                    pushed += 1;
                    self.bind_match_pattern_at(pattern, &elem, false);
                }
                ast::CompClause::If(cond) => {
                    let cond_ty = self.infer(cond);
                    self.expect_bool(&cond_ty, cond.span);
                }
            }
        }
        // The body is inferred UNDER the clause bindings, with the contextual
        // element/key/value type routed in (so `=> []` against `Array<int>` solves).
        let result = match (kind, body) {
            (ast::CompKind::Array, ast::CompBody::Elem(e))
            | (ast::CompKind::Set, ast::CompBody::Elem(e)) => {
                let elem = self.infer_with(e, elem_ctx.as_ref()).widen();
                let ctor = if matches!(kind, ast::CompKind::Set) {
                    self.gate_collection_key(&elem, span);
                    Ctor::Set
                } else {
                    Ctor::Array
                };
                self.comprehension_result(
                    ctor,
                    vec![elem],
                    elem_ctx.into_iter().collect(),
                    bare,
                    span,
                )
            }
            (ast::CompKind::Map, ast::CompBody::Entry { key, value }) => {
                let key_ty = self.infer_with(key, elem_ctx.as_ref()).widen();
                let val_ty = self.infer_with(value, val_ctx.as_ref()).widen();
                self.gate_collection_key(&key_ty, span);
                let fallbacks: Vec<Type> = elem_ctx.into_iter().chain(val_ctx).collect();
                self.comprehension_result(Ctor::Map, vec![key_ty, val_ty], fallbacks, bare, span)
            }
            // The parser pairs map↔Entry and array/set↔Elem, so a mismatch is
            // unreachable; report TPZ5611 defensively if a future body form appears.
            _ => {
                self.former.error(
                    codes::COMP_MAP_BODY,
                    "a `map { … }` comprehension body must be `key: value` (§6.4)".to_string(),
                    span,
                );
                Type::Ctor(Ctor::Map, vec![Type::Unknown, Type::Unknown])
            }
        };
        for _ in 0..pushed {
            self.pop_scope();
        }
        self.loop_ctx.pop();
        result
    }

    /// §6.4 assemble a comprehension's result `Ctor<args>`, reporting TPZ5612 when an
    /// argument is unconstrained (`Unknown`) and there is neither a contextual fallback
    /// nor any other constraint — the empty/unconstrained comprehension case.
    pub(super) fn comprehension_result(
        &mut self,
        ctor: Ctor,
        mut args: Vec<Type>,
        fallbacks: Vec<Type>,
        bare: bool,
        span: Span,
    ) -> Type {
        // Fill any unconstrained argument from the matching contextual type; if one is
        // STILL unknown and this is a bare binding, the type cannot be inferred.
        let mut unresolved = false;
        for (i, arg) in args.iter_mut().enumerate() {
            if arg.has_unknown() {
                if let Some(fb) = fallbacks.get(i).filter(|fb| !fb.has_unknown()) {
                    *arg = fb.clone();
                } else {
                    unresolved = true;
                }
            }
        }
        if unresolved && bare {
            self.former.error(
                codes::COMP_EMPTY,
                "this comprehension's element type cannot be inferred (§6.4): annotate the binding with its collection type"
                    .to_string(),
                span,
            );
        }
        Type::Ctor(ctor, args)
    }

    /// §6 (v5.4) a `set`/`map` literal or comprehension's element/key type must be
    /// keyable. Mirrors the runtime `freeze` leaf exactly for known types, including
    /// recursive structural keys and union members.
    pub(super) fn gate_collection_key(&mut self, key: &Type, span: Span) {
        if let Some(bad) = self.non_keyable_map_set_key(key) {
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

    pub(super) fn non_keyable_map_set_key(&self, key: &Type) -> Option<NonKeyableKey> {
        non_keyable_map_set_key_with_nominals(
            key,
            |id| self.former.newtype_info(id).map(|info| info.base.clone()),
            |id| {
                self.former
                    .record_info(id)
                    .map(|info| info.fields.iter().map(|field| field.ty.clone()).collect())
            },
            |id| {
                self.former.enum_info(id).map(|info| {
                    info.variants
                        .iter()
                        .flat_map(|variant| variant.payloads.iter().cloned())
                        .collect()
                })
            },
        )
    }

    /// §6 (v5.4) report TPZ5602 for a `map { … }` literal whose CONSTANT keys are
    /// statically equal (e.g. `map { "a": 1, "a": 2 }`). Only keys whose value is
    /// decidable at check time (string / int / bool literals) are compared — a
    /// runtime-valued duplicate is caught by the TPZ4601 runtime fault instead. The
    /// SECOND (and later) occurrence is reported, at its key span.
    pub(super) fn check_static_dup_map_keys(&mut self, entries: &'a [(ast::Expr, ast::Expr)]) {
        let mut seen: Vec<ConstKey> = Vec::new();
        for (k, _) in entries {
            let Some(ck) = self.const_key_of(k) else {
                continue;
            };
            if seen.contains(&ck) {
                self.former.error(
                    codes::DUPLICATE_MAP_KEY,
                    "duplicate key in `map { … }` literal".to_string(),
                    k.span,
                );
            } else {
                seen.push(ck);
            }
        }
    }

    /// The compile-time-constant VALUE of a map-literal key, when it is a simple
    /// literal (string / int / bool). `None` for any non-constant key (a call, an
    /// identifier, an interpolated string) — those are compared at runtime.
    pub(super) fn const_key_of(&self, key: &ast::Expr) -> Option<ConstKey> {
        match &key.kind {
            ast::ExprKind::Int => self
                .former
                .text(key.span)
                .parse::<i64>()
                .ok()
                .map(ConstKey::Int),
            ast::ExprKind::Bool(b) => Some(ConstKey::Bool(*b)),
            ast::ExprKind::String(lit) => {
                // Only a literal with NO interpolation is a constant key.
                let mut text = String::new();
                for part in &lit.parts {
                    match part {
                        ast::StringPart::Text(span) => text.push_str(self.former.text(*span)),
                        ast::StringPart::Interpolation(_) => return None,
                    }
                }
                Some(ConstKey::Str(text))
            }
            ast::ExprKind::Paren(inner) => self.const_key_of(inner),
            _ => None,
        }
    }
}
