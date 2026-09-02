use super::*;

impl<'a> Machine<'a> {
    pub(super) fn step_pattern_control_frame(&mut self, frame: Frame) -> Result<(), RtError> {
        match frame {
            Frame::KMatchDispatch { cases, span } => {
                let scrutinee = self.values.pop().expect("match scrutinee");
                self.frames.push(Frame::KMatchCase {
                    scrutinee,
                    cases,
                    idx: 0,
                    span,
                });
                Ok(())
            }
            Frame::KComposePair => {
                let g = self.values.pop().expect("compose rhs");
                let f = self.values.pop().expect("compose lhs");
                self.values.push(Value::Composed(Rc::new((f, g))));
                Ok(())
            }
            Frame::KForStart {
                pattern,
                body,
                span,
                is_stmt,
            } => {
                let iterable = self.values.pop().expect("for iterable");
                // The §10 `for`-materialization is the SHARED leaf both
                // engines call (CDR-006 §2), so the element sequence and
                // the not-iterable faults cannot drift from emitted code.
                let items: Vec<Value> = for_items(&iterable, span)?;
                self.frames.push(Frame::KForNext {
                    pattern,
                    body,
                    items: Rc::new(items),
                    next: 0,
                    acc: Vec::new(),
                    span,
                    is_stmt,
                });
                Ok(())
            }
            Frame::KMatchCase {
                scrutinee,
                cases,
                idx,
                span,
            } => {
                if idx >= cases.len() {
                    return Err(fault(
                        codes::FAULT_MATCH_MISS,
                        "no `case` matched and no catch-all exists (§5)",
                        span,
                    ));
                }
                let case = &cases[idx];
                match self.match_pattern(&case.pattern, &scrutinee, span)? {
                    Some(binds) => {
                        let saved = self.env.clone();
                        self.env = child_env(&saved);
                        for (name, value) in binds {
                            self.bind(name, value, false);
                        }
                        self.frames.push(Frame::PopScope(saved));
                        match &case.guard {
                            Some(guard) => {
                                self.frames.push(Frame::KMatchGuard {
                                    scrutinee,
                                    cases: cases.clone(),
                                    idx,
                                    span,
                                });
                                self.eval_expr(guard)?;
                            }
                            None => self.push_case_body(&case.body)?,
                        }
                    }
                    None => {
                        self.frames.push(Frame::KMatchCase {
                            scrutinee,
                            cases,
                            idx: idx + 1,
                            span,
                        });
                    }
                }
                Ok(())
            }
            Frame::KMatchGuard {
                scrutinee,
                cases,
                idx,
                span,
            } => {
                let test = self.values.pop().expect("guard value");
                // The bool-or-fault classification is the SHARED leaf both
                // engines call, so the guard's GUARD_TYPE message cannot drift.
                if case_guard_bool(&test, span)? {
                    self.push_case_body(&cases[idx].body)
                } else {
                    // Drop this case's bindings scope (the PopScope frame we
                    // pushed) and move on to the next case.
                    match self.frames.pop() {
                        Some(Frame::PopScope(saved)) => self.env = saved,
                        _ => unreachable!("guard scope frame"),
                    }
                    self.frames.push(Frame::KMatchCase {
                        scrutinee,
                        cases,
                        idx: idx + 1,
                        span,
                    });
                    Ok(())
                }
            }
            Frame::KTry { span } => {
                // §13 the value decision (unwrap / propagate / fault) is the
                // SHARED `try_value` leaf; the control flow (push the unwrapped
                // value vs unwind a Return of the propagated `Err`) stays here.
                let v = self.values.pop().expect("try value");
                match try_value(v, span)? {
                    Ok(unwrapped) => {
                        self.values.push(unwrapped);
                        Ok(())
                    }
                    Err(propagated) => {
                        self.start_unwind(UnwindAction::Return {
                            value: propagated,
                            span,
                        });
                        Ok(())
                    }
                }
            }
            Frame::KComposeAfter { g, span } => {
                let mid = self.values.pop().expect("compose mid");
                self.values.push(g);
                self.values.push(mid);
                self.apply_call(1, Vec::new(), Vec::new(), false, span)
            }
            Frame::KRange {
                inclusive,
                step,
                span,
            } => match step {
                Some(e) => {
                    self.frames.push(Frame::KRangeStep { inclusive, span });
                    self.frames.push(Frame::Eval(e));
                    Ok(())
                }
                None => self.finish_range(inclusive, None, span),
            },
            Frame::KRangeStep { inclusive, span } => {
                let step = self.values.pop().expect("range step");
                self.finish_range(inclusive, Some(step), span)
            }
            Frame::KForNext {
                pattern,
                body,
                items,
                next,
                mut acc,
                span,
                is_stmt,
            } => {
                // The previous body value (if any) was collected by
                // ForBody; this frame receives control with acc ready.
                if next >= items.len() {
                    self.values.push(Value::array(acc));
                    return Ok(());
                }
                let item = items[next].clone();
                match self.match_pattern(&pattern, &item, span)? {
                    Some(binds) => {
                        let saved = self.env.clone();
                        self.env = child_env(&saved);
                        for (name, value) in binds {
                            self.bind(name, value, false);
                        }
                        acc.reserve(0);
                        self.frames.push(Frame::ForBody {
                            pattern,
                            body: body.clone(),
                            items,
                            next: next + 1,
                            acc,
                            span,
                            vstack: self.values.len(),
                            is_stmt,
                        });
                        self.frames.push(Frame::PopScope(saved));
                        self.frames.push(Frame::KBlock {
                            block: body,
                            idx: 0,
                        });
                        Ok(())
                    }
                    None => Err(fault(
                        codes::GUARD_TYPE,
                        "`for` pattern did not match an element",
                        span,
                    )),
                }
            }
            Frame::ForBody {
                pattern,
                body,
                items,
                next,
                mut acc,
                span,
                is_stmt,
                ..
            } => {
                let v = self.values.pop().expect("for body value");
                acc.push(v);
                self.frames.push(Frame::KForNext {
                    pattern,
                    body,
                    items,
                    next,
                    acc,
                    span,
                    is_stmt,
                });
                Ok(())
            }
            _ => unreachable!("frame family changed after classification"),
        }
    }

    pub(super) fn push_case_body(&mut self, body: &CaseArmBody) -> Result<(), RtError> {
        match body {
            CaseArmBody::Expr(e) => self.eval_expr(e),
            CaseArmBody::Return { value, span } => {
                self.frames.push(Frame::KReturn { span: *span });
                match value {
                    Some(e) => self.eval_expr(e)?,
                    None => self.values.push(Value::Unit),
                }
                Ok(())
            }
        }
    }

    pub(super) fn finish_range(
        &mut self,
        inclusive: bool,
        step: Option<Value>,
        span: Span,
    ) -> Result<(), RtError> {
        let hi = self.values.pop().expect("range hi");
        let lo = self.values.pop().expect("range lo");
        // The §10 range builder is the SHARED leaf both engines call
        // (CDR-006 §2), so the endpoint/step guards cannot drift.
        let range = make_range(lo, hi, inclusive, step, span)?;
        self.values.push(range);
        Ok(())
    }

    /// Structural pattern match (§6); bindings on success.
    pub(super) fn match_pattern(
        &self,
        pattern: &Pattern,
        value: &Value,
        span: Span,
    ) -> Result<Option<Vec<(String, Value)>>, RtError> {
        let mut binds = Vec::new();
        if self.pat(pattern, value, &mut binds, span)? {
            Ok(Some(binds))
        } else {
            Ok(None)
        }
    }

    pub(super) fn pat(
        &self,
        pattern: &Pattern,
        value: &Value,
        binds: &mut Vec<(String, Value)>,
        span: Span,
    ) -> Result<bool, RtError> {
        match &pattern.kind {
            PatternKind::Wildcard => Ok(true),
            PatternKind::Binding(name) => {
                let text = self.text(name.span);
                // §3 (v5.3): a bare name matched against a user-enum value is a
                // VARIANT pattern (`case Red =>`) when it names a variant of that
                // value's enum — refutable, matching by tag, binding nothing. The
                // parser yields `Binding` for any bare ident, so the enum
                // disambiguation lives here, against the value's own `enum_id`
                // (the checker validated this against the scrutinee type). A name
                // that is NOT a variant of this enum is an ordinary binding.
                if let Value::Enum { variant, .. } = value
                    && self
                        .enum_variants_for_value(value)
                        .is_some_and(|variants| variants.contains_key(text))
                {
                    return Ok(variant.as_ref() == text);
                }
                binds.push((text.to_string(), value.clone()));
                Ok(true)
            }
            PatternKind::Or(alts) => {
                // §6 (v5.4) BINDING or-patterns: the FIRST matching alternative wins
                // and its OWN bindings are captured into the arm scope. Each
                // alternative is tried against a SCRATCH buffer so a partially-bound
                // alternative that then refutes leaves NO bindings behind (the next
                // alternative — or the next case arm — starts clean). The checker
                // (TPZ5710/TPZ5711) guarantees every alternative binds the same names
                // at unifying types, so whichever alternative matches supplies a
                // complete, well-typed binding set. (At `< V5_4` the parser admits no
                // bindings here, so `scratch` is always empty — same observable
                // behavior as before.)
                for alt in alts {
                    let mut scratch = Vec::new();
                    if self.pat(alt, value, &mut scratch, span)? {
                        binds.append(&mut scratch);
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            PatternKind::Literal(lit) => {
                let expected = self.literal_value(lit)?;
                values_equal(&expected, value).map_err(|e| cmp_guard(e, span))
            }
            PatternKind::Range { lo, hi, inclusive } => {
                let (Value::Int(lo), Value::Int(hi)) =
                    (self.literal_value(lo)?, self.literal_value(hi)?)
                else {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        "range-pattern endpoints must be `int` in this build (§6)",
                        span,
                    ));
                };
                let Value::Int(v) = value else {
                    return Ok(false);
                };
                Ok(*v >= lo && if *inclusive { *v <= hi } else { *v < hi })
            }
            PatternKind::Typed { name, ty } => {
                if !self.value_matches_type(ty, &self.src, value, span)? {
                    return Ok(false);
                }
                binds.push((self.text(name.span).to_string(), value.clone()));
                Ok(true)
            }
            PatternKind::Constructor { name, args } => {
                let ctor = self.text(name.span);
                // §3 (v5.3/v5.4): a constructor pattern against a user-enum value —
                // the parenthesized variant form `case Circle(r) =>` /
                // `case Bin(op, l, r) =>` (a bare `Circle` is a `Binding`, handled
                // above). Matches by enum_id+variant tag; then each of the N
                // subpatterns binds the corresponding payload POSITION-WISE
                // (recursing like `Some(x)`). The tag itself binds nothing. The
                // subpattern count must equal the value's payload arity (a mismatch
                // faults GUARD_ARITY identically to the emitter).
                if let Value::Enum {
                    variant, payloads, ..
                } = value
                    && self
                        .enum_variants_for_value(value)
                        .is_some_and(|variants| variants.contains_key(ctor))
                {
                    // Tag must match first; a non-matching variant simply does not
                    // match (falls to the next case), regardless of arity.
                    if variant.as_ref() != ctor {
                        return Ok(false);
                    }
                    if args.len() != payloads.len() {
                        return Err(fault(
                            codes::GUARD_ARITY,
                            format!(
                                "enum variant `{ctor}` pattern takes {} subpattern{}",
                                payloads.len(),
                                if payloads.len() == 1 { "" } else { "s" }
                            ),
                            span,
                        ));
                    }
                    for (sub, p) in args.iter().zip(payloads.iter()) {
                        if !self.pat(sub, p, binds, span)? {
                            return Ok(false);
                        }
                    }
                    return Ok(true);
                }
                // §3 (v5.4): a constructor pattern `case UserId(x)` against a NEWTYPE
                // value — `ctor` names a declared newtype. Matches by newtype_id;
                // then the single subpattern binds the inner base value (recursing
                // like `Some(x)`). A non-matching id does not match (falls to the next
                // case); a wrong subpattern count faults GUARD_ARITY identically to
                // the emitter.
                if let Some(definition) = self.newtype_definition_in(&self.src, ctor)
                    && let Value::Newtype { inner, .. } = value
                {
                    let expected = self.nominal_definition_identity(
                        definition.method_identity.as_ref(),
                        &definition.runtime_id,
                    );
                    if value.nominal_declaration_id() != Some(expected) {
                        return Ok(false);
                    }
                    if args.len() != 1 {
                        return Err(fault(
                            codes::GUARD_ARITY,
                            format!("newtype `{ctor}` pattern takes 1 subpattern"),
                            span,
                        ));
                    }
                    return self.pat(&args[0], inner, binds, span);
                }
                match (ctor, value) {
                    ("Some", Value::Some(inner))
                    | ("Ok", Value::Ok(inner))
                    | ("Err", Value::Err(inner)) => {
                        if args.len() != 1 {
                            return Err(fault(
                                codes::GUARD_ARITY,
                                format!("`{ctor}` pattern takes one subpattern"),
                                span,
                            ));
                        }
                        self.pat(&args[0], inner, binds, span)
                    }
                    ("None", Value::None) => Ok(args.is_empty()),
                    ("Some" | "Ok" | "Err" | "None", _) => Ok(false),
                    _ => Err(fault(
                        codes::GUARD_TYPE,
                        format!("unknown constructor pattern `{ctor}`"),
                        span,
                    )),
                }
            }
            PatternKind::List(elems) => {
                let Value::Array(items) = value else {
                    return Ok(false);
                };
                let items = items.borrow();
                let rest_at = elems
                    .iter()
                    .position(|e| matches!(e, ListPatternElem::Rest(_)));
                match rest_at {
                    None => {
                        if items.len() != elems.len() {
                            return Ok(false);
                        }
                        for (elem, item) in elems.iter().zip(items.iter()) {
                            let ListPatternElem::Pattern(p) = elem else {
                                unreachable!()
                            };
                            if !self.pat(p, item, binds, span)? {
                                return Ok(false);
                            }
                        }
                        Ok(true)
                    }
                    Some(pos) => {
                        let after = elems.len() - pos - 1;
                        if items.len() < pos + after {
                            return Ok(false);
                        }
                        for (elem, item) in elems[..pos].iter().zip(items.iter()) {
                            let ListPatternElem::Pattern(p) = elem else {
                                unreachable!()
                            };
                            if !self.pat(p, item, binds, span)? {
                                return Ok(false);
                            }
                        }
                        for (elem, item) in elems[pos + 1..]
                            .iter()
                            .zip(items[items.len() - after..].iter())
                        {
                            let ListPatternElem::Pattern(p) = elem else {
                                unreachable!()
                            };
                            if !self.pat(p, item, binds, span)? {
                                return Ok(false);
                            }
                        }
                        if let ListPatternElem::Rest(Some(restp)) = &elems[pos] {
                            let mid: Vec<Value> = items[pos..items.len() - after].to_vec();
                            if !self.pat(restp, &Value::array(mid), binds, span)? {
                                return Ok(false);
                            }
                        }
                        Ok(true)
                    }
                }
            }
            PatternKind::Record(fields) => {
                let Value::Record(map) = value else {
                    return Ok(false);
                };
                for field in fields {
                    let name = self.text(field.name.span);
                    let Some(fv) = map.get(name) else {
                        return Ok(false);
                    };
                    match &field.pattern {
                        Some(p) => {
                            if !self.pat(p, fv, binds, span)? {
                                return Ok(false);
                            }
                        }
                        None => binds.push((name.to_string(), fv.clone())),
                    }
                }
                Ok(true)
            }
            // §3 (v5.4) NOMINAL record pattern `User { name, age }` — matches ONLY a
            // `Value::NominalRecord` with the SAME id (a structural record / other
            // record id does NOT match). Each field subpattern binds by NAME.
            PatternKind::NominalRecord { name, fields } => {
                let rec_name = self.text(name.span);
                let Value::NominalRecord {
                    fields: rec_fields, ..
                } = value
                else {
                    return Ok(false);
                };
                let expected = self
                    .record_defs
                    .get(rec_name)
                    .map_or(rec_name, |definition| {
                        self.nominal_definition_identity(
                            definition.method_identity.as_ref(),
                            &definition.runtime_id,
                        )
                    });
                if value.nominal_declaration_id() != Some(expected) {
                    return Ok(false);
                }
                for field in fields {
                    let fname = self.text(field.name.span);
                    let Some((_, fv)) = rec_fields.iter().find(|(n, _)| n.as_ref() == fname) else {
                        return Ok(false);
                    };
                    let fv = fv.clone();
                    match &field.pattern {
                        Some(p) => {
                            if !self.pat(p, &fv, binds, span)? {
                                return Ok(false);
                            }
                        }
                        None => binds.push((fname.to_string(), fv)),
                    }
                }
                Ok(true)
            }
        }
    }

    /// Literal pattern / range-endpoint evaluation (literal-only).
    pub(super) fn literal_value(&self, expr: &Expr) -> Result<Value, RtError> {
        match &expr.kind {
            ExprKind::Int => self
                .text(expr.span)
                .parse()
                .map(Value::Int)
                .map_err(|_| fault(codes::GUARD_TYPE, "malformed integer literal", expr.span)),
            ExprKind::Float => self
                .text(expr.span)
                .parse()
                .map(Value::Float)
                .map_err(|_| fault(codes::GUARD_TYPE, "malformed float literal", expr.span)),
            ExprKind::Bool(b) => Ok(Value::Bool(*b)),
            ExprKind::Null => Ok(Value::Null),
            ExprKind::Unit => Ok(Value::Unit),
            ExprKind::Unary {
                op: UnaryOp::Minus,
                operand,
            } => match self.literal_value(operand)? {
                Value::Int(v) => Ok(Value::Int(-v)),
                Value::Float(v) => Ok(Value::Float(-v)),
                _ => Err(fault(codes::GUARD_TYPE, "malformed literal", expr.span)),
            },
            ExprKind::String(lit) if lit.tag.is_none() => {
                let mut buf = String::new();
                for part in &lit.parts {
                    match part {
                        StringPart::Text(s) => decode_escapes(self.text(*s), &mut buf, *s)?,
                        StringPart::Interpolation(_) => {
                            return Err(fault(
                                codes::GUARD_TYPE,
                                "interpolation is not allowed in pattern literals",
                                expr.span,
                            ));
                        }
                    }
                }
                Ok(Value::str(buf))
            }
            _ => Err(fault(
                codes::GUARD_TYPE,
                "pattern literals must be literal expressions",
                expr.span,
            )),
        }
    }
}

/// Reads through a pure record-member chain — resolves the field spans, then
/// delegates to the shared `walk_fields_value` leaf (so the interpreter and the
/// emitted code read and fault identically; CDR-006 §2).
pub(super) fn walk_fields(
    root: &Value,
    fields: &[Ident],
    src: &str,
    span: Span,
) -> Result<Value, RtError> {
    let names: Vec<&str> = fields
        .iter()
        .map(|f| &src[f.span.lo as usize..f.span.hi as usize])
        .collect();
    walk_fields_value(root, &names, span)
}
