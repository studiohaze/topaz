use super::*;

impl<'a> Machine<'a> {
    pub(super) fn step_value_frame(&mut self, frame: Frame) -> Result<(), RtError> {
        match frame {
            Frame::KIf {
                then_block,
                else_branch,
                span,
            } => {
                let cond = self.values.pop().expect("if cond");
                // The §5 condition guard is the SHARED leaf both engines
                // call (CDR-006 §2), so a non-`bool` faults identically
                // here and in emitted code.
                if condition_bool(&cond, "if", span)? {
                    self.push_block(then_block);
                } else {
                    match else_branch {
                        Some(e) => self.frames.push(Frame::Eval(e)),
                        None => self.values.push(Value::Unit),
                    }
                }
                Ok(())
            }
            Frame::KWhile { cond, body, span } => {
                let test = self.values.pop().expect("while cond");
                // Same SHARED §5 condition guard as `if` (CDR-006 §2).
                if condition_bool(&test, "while", span)? {
                    self.frames.push(Frame::LoopBody {
                        cond,
                        body: body.clone(),
                        span,
                        vstack: self.values.len(),
                    });
                    let saved = self.env.clone();
                    self.env = child_env(&saved);
                    self.frames.push(Frame::PopScope(saved));
                    // Body statements; while is a statement (§5),
                    // its body value is discarded.
                    self.frames.push(Frame::KDiscard);
                    self.frames.push(Frame::KBlock {
                        block: body,
                        idx: 0,
                    });
                }
                Ok(())
            }
            Frame::LoopBody {
                cond, body, span, ..
            } => {
                // Body completed normally: re-test.
                self.frames.push(Frame::KWhile {
                    cond: cond.clone(),
                    body,
                    span,
                });
                self.frames.push(Frame::Eval(cond));
                Ok(())
            }
            // A `loop` body completed normally, so re-enter it (infinite).
            // A `loop` never falls through; it exits only via `break`/`return`/`?`,
            // each of which unwinds PAST this frame (the `break` catch is in
            // `step_unwind`), so reaching here means "iterate again".
            Frame::LoopExprBody {
                body, label, span, ..
            } => {
                self.enter_loop_expr_body(body, label, span);
                Ok(())
            }
            Frame::KUnary { op, span } => self.apply_unary(op, span),
            Frame::KBinaryRhs { op, rhs, span } => {
                let lhs = self.values.pop().expect("binary lhs");
                if matches!(op, BinaryOp::And | BinaryOp::Or | BinaryOp::Coalesce) {
                    // §2/§12 short-circuit: the LHS decides (through the shared
                    // leaf) whether to short-circuit to a value or evaluate the
                    // RHS — so both engines share the bool guard / `??` unwrap /
                    // non-bool fault.
                    match short_circuit_lhs(lhs, op, span)? {
                        Some(value) => self.values.push(value),
                        None => self.frames.push(Frame::Eval(rhs)),
                    }
                } else {
                    self.values.push(lhs);
                    self.frames.push(Frame::KBinaryApply { op, span });
                    self.frames.push(Frame::Eval(rhs));
                }
                Ok(())
            }
            Frame::KBinaryApply { op, span } => self.apply_binary(op, span),
            Frame::KInterp { lit, idx, mut buf } => {
                if idx > 0 {
                    let v = self.values.pop().expect("interp value");
                    buf.push_str(&render(&v));
                }
                self.continue_interp(lit, idx, buf)
            }
            Frame::KTemplate {
                lit,
                idx,
                tag,
                mut parts,
                buf,
                mut values,
            } => {
                let v = self.values.pop().expect("template value");
                values.push(v);
                parts.push(buf);
                self.continue_template(lit, idx, tag, parts, String::new(), values)
            }
            Frame::KArray {
                elements,
                idx,
                mut acc,
                spread,
                span,
            } => {
                if idx > 0 {
                    let v = self.values.pop().expect("array element");
                    if spread {
                        // §9 spread-extend through the shared leaf so the
                        // flatten and the non-array fault match emitted code.
                        array_spread_extend(&mut acc, v, span)?;
                    } else {
                        acc.push(v);
                    }
                }
                if idx < elements.len() {
                    let (expr, is_spread) = match &elements[idx] {
                        ArrayElement::Expr(e) => (e, false),
                        ArrayElement::Spread(e) => (e, true),
                    };
                    self.frames.push(Frame::KArray {
                        elements: elements.clone(),
                        idx: idx + 1,
                        acc,
                        spread: is_spread,
                        span,
                    });
                    self.eval_expr(expr)?;
                } else {
                    self.values.push(Value::array(acc));
                }
                Ok(())
            }
            Frame::KSetLiteral {
                elements,
                idx,
                mut acc,
                span,
            } => {
                if idx > 0 {
                    acc.push(self.values.pop().expect("set element"));
                }
                if idx < elements.len() {
                    self.frames.push(Frame::KSetLiteral {
                        elements: elements.clone(),
                        idx: idx + 1,
                        acc,
                        span,
                    });
                    self.eval_expr(&elements[idx])?;
                } else {
                    // §6 build through the SHARED leaf so duplicate-collapse and the
                    // non-keyable fault match the emitted code byte-for-byte.
                    self.values.push(builtin_set_of(acc, span)?);
                }
                Ok(())
            }
            Frame::KMapLiteral {
                entries,
                idx,
                mut acc,
                pending_key,
                span,
            } => {
                // The state machine evaluates each entry's KEY then VALUE in source
                // order. `pending_key == None` means "about to evaluate the KEY of
                // `entries[idx]`" (no value on the stack to consume — except the prior
                // entry's value, folded in when it returns via the `Some` branch).
                // `pending_key == Some(k)` means "the VALUE of `entries[idx]` is on the
                // stack; pair it with `k` and advance".
                match pending_key {
                    None => {
                        if idx >= entries.len() {
                            // §6 all entries consumed: build through the SHARED leaf so
                            // the duplicate-key fault (TPZ4601) and the non-keyable
                            // fault match the emitted code byte-for-byte.
                            self.values.push(builtin_map_of(acc, span)?);
                        } else {
                            // Evaluate this entry's KEY, then re-enter to evaluate its
                            // VALUE — pushing a KMapKey marker so the key lands in the
                            // VALUE step's `pending_key`. We model this directly: push a
                            // continuation that pops the key and starts the value.
                            self.frames.push(Frame::KMapLiteralKey {
                                entries: entries.clone(),
                                idx,
                                acc,
                                span,
                            });
                            self.eval_expr(&entries[idx].0)?;
                        }
                    }
                    Some(key) => {
                        let value = self.values.pop().expect("map value");
                        acc.push((key, value));
                        // Advance to the next entry's KEY.
                        self.frames.push(Frame::KMapLiteral {
                            entries: entries.clone(),
                            idx: idx + 1,
                            acc,
                            pending_key: None,
                            span,
                        });
                    }
                }
                Ok(())
            }
            Frame::KMapLiteralKey {
                entries,
                idx,
                acc,
                span,
            } => {
                // The KEY of `entries[idx]` is on the value stack. Pop it, then
                // evaluate the VALUE; the result re-enters `KMapLiteral` with the key
                // pending so the pair is recorded in source order.
                let key = self.values.pop().expect("map key");
                self.frames.push(Frame::KMapLiteral {
                    entries: entries.clone(),
                    idx,
                    acc,
                    pending_key: Some(key),
                    span,
                });
                self.eval_expr(&entries[idx].1)?;
                Ok(())
            }
            // §6.4 (v5.4) COMPREHENSION clause driver. At `idx == len` the clauses are
            // exhausted for this iteration: evaluate the body and append it to the top
            // accumulator. Otherwise dispatch the clause.
            _ => unreachable!("frame family changed after classification"),
        }
    }

    pub(super) fn step_aggregate_frame(&mut self, frame: Frame) -> Result<(), RtError> {
        match frame {
            Frame::KCompClause {
                kind,
                clauses,
                idx,
                body,
                span,
            } => {
                if idx >= clauses.len() {
                    // Base case: evaluate the body element/entry and append.
                    match &*body {
                        CompBody::Elem(e) => {
                            self.frames.push(Frame::KCompYield { pending_key: None });
                            self.frames.push(Frame::Eval(e.clone()));
                        }
                        CompBody::Entry { key, value } => {
                            // Evaluate the KEY, then (KCompYieldMapValue) the VALUE,
                            // then append the pair — source order, like the literal.
                            self.frames.push(Frame::KCompYieldMapValue {
                                value: value.clone(),
                            });
                            self.frames.push(Frame::Eval(key.clone()));
                        }
                    }
                    return Ok(());
                }
                match &clauses[idx] {
                    CompClause::For { iter, .. } => {
                        self.frames.push(Frame::KCompForStart {
                            kind,
                            clauses: clauses.clone(),
                            idx,
                            body,
                            span,
                        });
                        self.frames.push(Frame::Eval(iter.clone()));
                    }
                    CompClause::If(cond) => {
                        self.frames.push(Frame::KCompIf {
                            kind,
                            clauses: clauses.clone(),
                            idx,
                            body,
                            span,
                        });
                        self.frames.push(Frame::Eval(cond.clone()));
                    }
                }
                Ok(())
            }
            // §6.4 `for`-clause start: materialize the iterable via the SHARED
            // `for_items` leaf (the SAME not-iterable fault a real `for` raises).
            Frame::KCompForStart {
                kind,
                clauses,
                idx,
                body,
                span,
            } => {
                let iterable = self.values.pop().expect("comprehension for iterable");
                let items: Vec<Value> = for_items(&iterable, span)?;
                self.frames.push(Frame::KCompForNext {
                    kind,
                    clauses,
                    idx,
                    body,
                    items: Rc::new(items),
                    next: 0,
                    span,
                });
                Ok(())
            }
            // §6.4 `for`-clause driver: bind `items[next]` against the clause pattern
            // in a fresh per-iteration scope, recurse into the next clause, then
            // advance. A non-matching element FAULTS GUARD_TYPE — exactly the
            // interpreter's (and emitter's) real-`for` destructure behavior.
            Frame::KCompForNext {
                kind,
                clauses,
                idx,
                body,
                items,
                next,
                span,
            } => {
                if next >= items.len() {
                    return Ok(());
                }
                let CompClause::For { pattern, .. } = &clauses[idx] else {
                    unreachable!("KCompForNext over a non-for clause");
                };
                let pattern = pattern.clone();
                let item = items[next].clone();
                match self.match_pattern(&pattern, &item, span)? {
                    Some(binds) => {
                        let saved = self.env.clone();
                        self.env = child_env(&saved);
                        for (name, value) in binds {
                            self.bind(name, value, false);
                        }
                        // After this item's inner clauses finish (and its scope is
                        // popped), advance to the next item.
                        self.frames.push(Frame::KCompForNext {
                            kind,
                            clauses: clauses.clone(),
                            idx,
                            body: body.clone(),
                            items,
                            next: next + 1,
                            span,
                        });
                        self.frames.push(Frame::PopScope(saved));
                        self.frames.push(Frame::KCompClause {
                            kind,
                            clauses,
                            idx: idx + 1,
                            body,
                            span,
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
            // §6.4 `if`-clause: recurse into the next clause only when the condition is
            // `true` (the §5 bool guard is the SHARED `condition_bool` leaf).
            Frame::KCompIf {
                kind,
                clauses,
                idx,
                body,
                span,
            } => {
                let cond = self.values.pop().expect("comprehension if cond");
                if condition_bool(&cond, "if", span)? {
                    self.frames.push(Frame::KCompClause {
                        kind,
                        clauses,
                        idx: idx + 1,
                        body,
                        span,
                    });
                }
                Ok(())
            }
            // §6.4 MAP yield helper: the KEY is on the stack; evaluate the VALUE, then
            // append `(key, value)` — source order.
            Frame::KCompYieldMapValue { value } => {
                let key = self.values.pop().expect("comprehension map key");
                self.frames.push(Frame::KCompYield {
                    pending_key: Some(key),
                });
                self.frames.push(Frame::Eval(value));
                Ok(())
            }
            // §6.4 yield: pop the body element (or map value) and append it to the TOP
            // accumulator. Leaves nothing on the value stack — iteration continues.
            Frame::KCompYield { pending_key } => {
                let v = self.values.pop().expect("comprehension body value");
                let acc = self
                    .comp_accs
                    .last_mut()
                    .expect("comprehension accumulator");
                match (acc, pending_key) {
                    (CompAccum::Pairs(pairs), Some(key)) => pairs.push((key, v)),
                    (CompAccum::List(list), None) => list.push(v),
                    _ => unreachable!("comprehension accumulator shape matches its kind"),
                }
                Ok(())
            }
            // §6.4 finalize: pop the top accumulator and build the final value through
            // the SAME shared leaf the literal uses (so order/collapse/dup-key faults
            // are byte-identical to emitted code).
            Frame::KCompFinish { kind, span } => {
                let acc = self
                    .comp_accs
                    .pop()
                    .expect("comprehension accumulator to finalize");
                let value = match (kind, acc) {
                    (CompKind::Array, CompAccum::List(list)) => Value::array(list),
                    (CompKind::Set, CompAccum::List(list)) => builtin_set_of(list, span)?,
                    (CompKind::Map, CompAccum::Pairs(pairs)) => builtin_map_of(pairs, span)?,
                    _ => unreachable!("comprehension kind matches its accumulator"),
                };
                self.values.push(value);
                Ok(())
            }
            Frame::KRecord {
                fields,
                idx,
                mut acc,
                base,
                span,
            } => {
                if idx > 0 {
                    let v = self.values.pop().expect("record field");
                    let name = self.text(fields[idx - 1].name.span).to_string();
                    acc.push((name, v));
                }
                if idx < fields.len() {
                    self.frames.push(Frame::KRecord {
                        fields: fields.clone(),
                        idx: idx + 1,
                        acc,
                        base,
                        span,
                    });
                    self.eval_expr(&fields[idx].value)?;
                } else {
                    // §8 a plain literal builds directly; a record UPDATE
                    // merges through the shared leaf (the field-existence
                    // guard), so both engines agree.
                    let record = match base {
                        Some(b) => record_update_merge(b, acc, span)?,
                        None => Value::record(acc),
                    };
                    self.values.push(record);
                }
                Ok(())
            }
            Frame::KRecordUpdateBase { fields, span } => {
                let base = self.values.pop().expect("record base");
                // §8 the base is type-checked through the shared leaf BEFORE
                // the field values evaluate (the emitter does the same).
                let map = record_update_base(base, span)?;
                self.frames.push(Frame::KRecord {
                    fields,
                    idx: 0,
                    acc: Vec::new(),
                    base: Some(map),
                    span,
                });
                Ok(())
            }
            Frame::KNominalRecord {
                record_id,
                declaration_identity,
                method_identity,
                plan,
                idx,
                mut acc,
                decl_order,
                span,
            } => {
                if idx > 0 {
                    let v = self.values.pop().expect("nominal-record field value");
                    acc.push((plan[idx - 1].name.clone(), v));
                }
                if idx < plan.len() {
                    let item = &plan[idx];
                    self.frames.push(Frame::KNominalRecord {
                        record_id,
                        declaration_identity,
                        method_identity,
                        plan: plan.clone(),
                        idx: idx + 1,
                        acc,
                        decl_order,
                        span,
                    });
                    if !Rc::ptr_eq(&self.src, &item.src) {
                        let saved_src = self.src.clone();
                        self.src = item.src.clone();
                        self.frames.push(Frame::RestoreSource(saved_src));
                    }
                    if let Some(env) = &item.env {
                        let saved_env = self.env.clone();
                        self.env = env.clone();
                        self.frames.push(Frame::RestoreEnv(saved_env));
                    }
                    if item.is_default {
                        self.record_default_depth += 1;
                        self.frames.push(Frame::KRecordDefaultExit);
                    }
                    self.frames.push(Frame::Eval(item.expr.clone()));
                } else {
                    // Assemble in DECLARATION order from the accumulated values. The
                    // LAST entry per name wins, so an EXPLICIT field (pushed after a
                    // spread's seeded fields) overrides the spread value.
                    let fields: Vec<(Rc<str>, Value)> = decl_order
                        .iter()
                        .map(|name| {
                            nominal_record_field_required(&acc, &record_id, name, span)
                                .map(|value| (name.clone(), value))
                        })
                        .collect::<Result<_, _>>()?;
                    self.values.push(Value::NominalRecord {
                        record_id,
                        declaration_identity,
                        method_identity,
                        fields: Rc::from(fields),
                    });
                }
                Ok(())
            }
            Frame::KRecordDefaultExit => {
                self.record_default_depth = self.record_default_depth.saturating_sub(1);
                Ok(())
            }
            Frame::KNominalSpread {
                record_id,
                declaration_identity,
                method_identity,
                plan,
                decl_order,
                span,
            } => {
                let base = self.values.pop().expect("nominal-spread base");
                // The spread base MUST be a `NominalRecord` of the SAME id; a
                // wrong-id / non-record base faults (GUARD_TYPE) — byte-identical to
                // the emitter under `--unchecked` (both call `nominal_spread_base`).
                // Seed the accumulator with its fields IN DECLARATION ORDER so
                // explicit fields can override them.
                let required_fields: Vec<&str> =
                    decl_order.iter().map(|name| name.as_ref()).collect();
                let acc = nominal_spread_base_required(
                    base,
                    &record_id,
                    declaration_identity.as_deref(),
                    &required_fields,
                    span,
                )?;
                self.frames.push(Frame::KNominalRecord {
                    record_id,
                    declaration_identity,
                    method_identity,
                    plan,
                    idx: 0,
                    acc,
                    decl_order,
                    span,
                });
                Ok(())
            }
            _ => unreachable!("frame family changed after classification"),
        }
    }

    pub(super) fn eval_expr(&mut self, expr: &Expr) -> Result<(), RtError> {
        match &expr.kind {
            ExprKind::Int => {
                let text = self.text(expr.span);
                let v: i64 = text.parse().map_err(|_| {
                    fault(
                        codes::GUARD_TYPE,
                        format!("integer literal `{text}` overflows `int`"),
                        expr.span,
                    )
                })?;
                self.values.push(Value::Int(v));
                Ok(())
            }
            ExprKind::Float => {
                let text = self.text(expr.span);
                let v: f64 = text.parse().map_err(|_| {
                    fault(
                        codes::GUARD_TYPE,
                        format!("malformed float literal `{text}`"),
                        expr.span,
                    )
                })?;
                self.values.push(Value::Float(v));
                Ok(())
            }
            ExprKind::Bool(b) => {
                self.values.push(Value::Bool(*b));
                Ok(())
            }
            ExprKind::Null => {
                self.values.push(Value::Null);
                Ok(())
            }
            ExprKind::Unit => {
                self.values.push(Value::Unit);
                Ok(())
            }
            ExprKind::Ident => {
                let name = self.text(expr.span);
                match lookup(&self.env, name) {
                    Some(v) => {
                        self.values.push(v);
                        Ok(())
                    }
                    None => {
                        // §22.1/§22.2 prelude is the outermost
                        // scope: constructors and builtins apply
                        // only when no user binding shadows them.
                        if name == "None" {
                            self.values.push(Value::None);
                            return Ok(());
                        }
                        if let Some(kind) = Builtin::free(name) {
                            self.values.push(Value::Builtin { kind, recv: None });
                            return Ok(());
                        }
                        Err(fault(
                            codes::GUARD_UNBOUND,
                            format!("`{name}` is not bound"),
                            expr.span,
                        ))
                    }
                }
            }
            ExprKind::String(lit) => {
                let lit = lit.clone();
                match lit.tag {
                    Some(tag_span) => {
                        let tag = self.text(tag_span).to_string();
                        self.continue_template(lit, 0, tag, Vec::new(), String::new(), Vec::new())
                    }
                    None => self.continue_interp(lit, 0, String::new()),
                }
            }
            ExprKind::Paren(inner) => {
                self.frames.push(Frame::Eval(inner.clone()));
                Ok(())
            }
            ExprKind::Block(block) => {
                self.push_block(block.clone());
                Ok(())
            }
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                self.frames.push(Frame::KIf {
                    then_block: then_block.clone(),
                    else_branch: else_branch.clone(),
                    span: expr.span,
                });
                self.frames.push(Frame::Eval(cond.clone()));
                Ok(())
            }
            ExprKind::Unary { op, operand } => {
                self.frames.push(Frame::KUnary {
                    op: *op,
                    span: expr.span,
                });
                self.frames.push(Frame::Eval(operand.clone()));
                Ok(())
            }
            ExprKind::Binary { op, lhs, rhs } => {
                self.frames.push(Frame::KBinaryRhs {
                    op: *op,
                    rhs: rhs.clone(),
                    span: expr.span,
                });
                self.frames.push(Frame::Eval(lhs.clone()));
                Ok(())
            }
            ExprKind::Array(elements) => {
                self.frames.push(Frame::KArray {
                    elements: Rc::from(elements.as_slice()),
                    idx: 0,
                    acc: Vec::new(),
                    spread: false,
                    span: expr.span,
                });
                Ok(())
            }
            ExprKind::SetLiteral(elements) => {
                self.frames.push(Frame::KSetLiteral {
                    elements: Rc::from(elements.as_slice()),
                    idx: 0,
                    acc: Vec::new(),
                    span: expr.span,
                });
                Ok(())
            }
            ExprKind::MapLiteral(entries) => {
                self.frames.push(Frame::KMapLiteral {
                    entries: Rc::from(entries.as_slice()),
                    idx: 0,
                    acc: Vec::new(),
                    pending_key: None,
                    span: expr.span,
                });
                Ok(())
            }
            // §6.4 (v5.4) COMPREHENSION: push a fresh accumulator (popped+finalized by
            // `KCompFinish` through the SAME shared leaf the literal uses), then drive
            // the clause list from clause 0. The clauses run for effect (each surviving
            // iteration appends to the accumulator); the value comes from KCompFinish.
            ExprKind::Comprehension {
                kind,
                clauses,
                body,
            } => {
                self.comp_accs.push(match kind {
                    CompKind::Map => CompAccum::Pairs(Vec::new()),
                    CompKind::Array | CompKind::Set => CompAccum::List(Vec::new()),
                });
                self.frames.push(Frame::KCompFinish {
                    kind: *kind,
                    span: expr.span,
                });
                self.frames.push(Frame::KCompClause {
                    kind: *kind,
                    clauses: Rc::from(clauses.as_slice()),
                    idx: 0,
                    body: body.clone(),
                    span: expr.span,
                });
                Ok(())
            }
            ExprKind::RecordLiteral { fields } => {
                self.frames.push(Frame::KRecord {
                    fields: Rc::from(fields.as_slice()),
                    idx: 0,
                    acc: Vec::new(),
                    base: None,
                    span: expr.span,
                });
                Ok(())
            }
            ExprKind::RecordUpdate {
                base,
                spread,
                fields,
            } => {
                // §3 (v5.4) NOMINAL record construction `User { name: …, age: … }`
                // (optionally with a leading spread `User { ...u, … }`): the parser
                // models it as a RecordUpdate whose `base` is the record NAME. When
                // that name is a declared record NOT shadowed by a binding, build
                // the deterministic eval plan + assemble nominally.
                if let ExprKind::Ident = &base.kind {
                    let head = self.text(base.span).to_string();
                    if lookup(&self.env, &head).is_none()
                        && let Some(definition) =
                            self.record_definition_in(&self.src, &head).cloned()
                    {
                        return self.start_nominal_record(
                            &head,
                            definition,
                            spread.clone(),
                            fields,
                            expr.span,
                        );
                    }
                }
                // A leading `...` spread is nominal-record construction only. If
                // the head name is shadowed (or not a declared record), this would
                // fall to the structural update path, which has no spread semantics;
                // fault before evaluating the base, spread, or fields.
                if spread.is_some() {
                    let name = self.text(base.span);
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!("record spread `...` needs a declared record; `{name}` is not one"),
                        expr.span,
                    ));
                }
                self.frames.push(Frame::KRecordUpdateBase {
                    fields: Rc::from(fields.as_slice()),
                    span: expr.span,
                });
                self.frames.push(Frame::Eval(base.clone()));
                Ok(())
            }
            ExprKind::Member { object, field } => {
                // §22.2 collection constructors: `Array.of`,
                // `Map.new`, `Set.of` — prelude heads unless
                // shadowed by a user binding.
                if let ExprKind::Ident = &object.kind {
                    let head = self.text(object.span);
                    let member = self.text(field.span);
                    if lookup(&self.env, head).is_none() {
                        let kind = Builtin::static_namespace(head, member);
                        if let Some(kind) = kind {
                            self.values.push(Value::Builtin { kind, recv: None });
                            return Ok(());
                        }
                        if head == "RoundingMode"
                            && let Some(mode) = rounding_mode_variant(member)
                        {
                            self.values.push(rounding_mode_value(mode));
                            return Ok(());
                        }
                        // §3 (v5.3) enum construction: `Color.Red` where `Color` is
                        // a declared enum NOT shadowed by a binding and `Red` is a
                        // PAYLOAD-LESS variant → a payload-less enum value. A bare
                        // reference to a PAYLOADFUL variant (`Shape.Circle` with no
                        // call) is NOT a valid value — it falls through to ordinary
                        // member access (which faults), exactly as the emitter does,
                        // so `--unchecked` run≡build (checked mode rejects it as an
                        // arity error). An unknown variant likewise falls through.
                        if let Some(definition) = self.enum_definition_in(&self.src, head)
                            && let Some(&(0, variant_index)) = definition.variants.get(member)
                        {
                            let identities = self
                                .nominal_identity_projection(definition.method_identity.clone());
                            self.values.push(Value::Enum {
                                enum_id: definition.runtime_id.clone(),
                                declaration_identity: identities.declaration,
                                method_identity: identities.method,
                                variant: Rc::from(member),
                                variant_index,
                                payloads: Rc::from([] as [Value; 0]),
                            });
                            return Ok(());
                        }
                    }
                }
                let root = self.mutator_root_of(object, field);
                self.frames.push(Frame::KMember {
                    field: *field,
                    span: expr.span,
                    root: root.map(Rc::from),
                });
                self.frames.push(Frame::Eval(object.clone()));
                Ok(())
            }
            ExprKind::OptionalAccess { object, field } => {
                let root = self.mutator_root_of(object, field);
                self.frames.push(Frame::KOptional {
                    field: *field,
                    span: expr.span,
                    root: root.map(Rc::from),
                });
                self.frames.push(Frame::Eval(object.clone()));
                Ok(())
            }
            ExprKind::Index { object, index } => {
                self.frames.push(Frame::KIndexObj {
                    index: index.clone(),
                    span: expr.span,
                });
                self.frames.push(Frame::Eval(object.clone()));
                Ok(())
            }
            ExprKind::Call {
                callee,
                args,
                type_args,
            } => {
                if let Some((schema, parse_text, param)) =
                    self.json_typed_decode_member(callee, type_args)?
                {
                    let member = match parse_text {
                        true => "parseAs",
                        false => "decode",
                    };
                    let arg = self.json_typed_decode_arg(expr.span, member, args, param)?;
                    self.frames.push(Frame::KJsonDecode {
                        schema: Rc::new(schema),
                        span: expr.span,
                        parse_text,
                    });
                    return self.eval_expr(arg);
                }
                self.schedule_call(callee, args, expr.span, None)
            }
            ExprKind::Lambda { params, body } => {
                self.values.push(Value::Closure(Rc::new(ClosureData {
                    name: None,
                    params: ClosureParams::Lambda(Rc::from(params.as_slice())),
                    body: ClosureBody::Expr(body.clone()),
                    env: self.env.clone(),
                    src: self.src.clone(),
                    // A lambda has no declared type params or return type — both
                    // boundaries are unguardable, so it is skipped wholesale.
                    type_params: Rc::from([] as [Ident; 0]),
                    return_type: None,
                })));
                Ok(())
            }
            ExprKind::Pipe { lhs, rhs } => {
                let root = if let PipeRhs::Field(field) = rhs.as_ref() {
                    self.mutator_root_of(lhs, field)
                } else {
                    None
                };
                self.frames.push(Frame::KPipe {
                    rhs: rhs.clone(),
                    span: expr.span,
                    root: root.map(Rc::from),
                });
                self.frames.push(Frame::Eval(lhs.clone()));
                Ok(())
            }
            ExprKind::Match { scrutinee, cases } => {
                self.frames.push(Frame::KMatchDispatch {
                    cases: Rc::from(cases.as_slice()),
                    span: expr.span,
                });
                self.frames.push(Frame::Eval(scrutinee.clone()));
                Ok(())
            }
            ExprKind::Try(inner) => {
                self.frames.push(Frame::KTry { span: expr.span });
                self.frames.push(Frame::Eval(inner.clone()));
                Ok(())
            }
            ExprKind::Compose { lhs, rhs } => {
                self.frames.push(Frame::KComposePair);
                self.frames.push(Frame::Eval(rhs.clone()));
                self.frames.push(Frame::Eval(lhs.clone()));
                Ok(())
            }
            ExprKind::Range {
                lo,
                hi,
                inclusive,
                step,
            } => {
                self.frames.push(Frame::KRange {
                    inclusive: *inclusive,
                    step: step.clone(),
                    span: expr.span,
                });
                self.frames.push(Frame::Eval(hi.clone()));
                self.frames.push(Frame::Eval(lo.clone()));
                Ok(())
            }
            ExprKind::For {
                pattern,
                iter,
                body,
            } => {
                self.frames.push(Frame::KForStart {
                    pattern: pattern.clone(),
                    body: body.clone(),
                    span: expr.span,
                    is_stmt: false,
                });
                self.frames.push(Frame::Eval(iter.clone()));
                Ok(())
            }
            // `loop (label)? { body }` is an infinite-loop expression.
            // Enter the body once (a fresh per-iteration scope, value discarded);
            // its `LoopExprBody` boundary re-enters it forever on normal completion
            // and yields a `break <value>` as the loop expression's result.
            ExprKind::Loop { label, body } => {
                let label = label.map(|l| self.text(l.span).to_string());
                self.enter_loop_expr_body(body.clone(), label, expr.span);
                Ok(())
            }
            ExprKind::Concurrent {
                timeout,
                arms,
                else_block,
            } => self.start_concurrent(timeout.as_deref(), arms, else_block.clone(), expr.span),
            ExprKind::Duration(_) => Err(fault(
                codes::GUARD_TYPE,
                "duration literals exist only in the `concurrent` timeout clause (§15)",
                expr.span,
            )),
            ExprKind::Placeholder => match lookup(&self.env, "_") {
                Some(v) => {
                    self.values.push(v);
                    Ok(())
                }
                None => Err(fault(
                    codes::GUARD_TYPE,
                    "`_` is only valid inside a pipeline stage (§11)",
                    expr.span,
                )),
            },
        }
    }

    /// §16: walk the template literal collecting decoded text parts
    /// and scheduling interpolation evaluation.
    pub(super) fn continue_template(
        &mut self,
        lit: Rc<StringLit>,
        idx: usize,
        tag: String,
        mut parts: Vec<String>,
        mut buf: String,
        values: Vec<Value>,
    ) -> Result<(), RtError> {
        let mut i = idx;
        while i < lit.parts.len() {
            match &lit.parts[i] {
                StringPart::Text(span) => {
                    decode_escapes(self.text(*span), &mut buf, *span)?;
                    i += 1;
                }
                StringPart::Interpolation(e) => {
                    self.frames.push(Frame::KTemplate {
                        lit: lit.clone(),
                        idx: i + 1,
                        tag,
                        parts,
                        buf,
                        values,
                    });
                    return self.eval_expr(e);
                }
            }
        }
        parts.push(buf);
        // §16 build the tagged-template value through the SHARED leaf (the
        // `p`-tag normalization + the diagnostic rendering live there, so the
        // emitter cannot drift).
        self.values.push(make_template(tag, parts, values));
        Ok(())
    }

    pub(super) fn continue_interp(
        &mut self,
        lit: Rc<StringLit>,
        idx: usize,
        mut buf: String,
    ) -> Result<(), RtError> {
        let mut i = idx;
        while i < lit.parts.len() {
            match &lit.parts[i] {
                StringPart::Text(span) => {
                    decode_escapes(self.text(*span), &mut buf, *span)?;
                    i += 1;
                }
                StringPart::Interpolation(e) => {
                    self.frames.push(Frame::KInterp {
                        lit: lit.clone(),
                        idx: i + 1,
                        buf,
                    });
                    return self.eval_expr(e);
                }
            }
        }
        self.values.push(Value::str(buf));
        Ok(())
    }

    /// §3 (v5.4) build the DETERMINISTIC nominal-record construction plan and push
    /// the build frames. The eval order is: the SPREAD base (if any) FIRST, then
    /// EXPLICIT fields left-to-right (source order), then any still-MISSING fields'
    /// DEFAULTS in declaration order — the SAME order the emitter uses (run≡build).
    /// `decl_fields` is the record's declaration (field name + optional default
    /// expr); `spread` is an optional `...base` source; `inits` are the explicit
    /// `{ … }` field inits. The checker already validated names/arity/spread-id for
    /// a checked program; under `--unchecked` a bad field/missing/wrong-spread
    /// faults here, byte-identically to the emitter.
    pub(super) fn start_nominal_record(
        &mut self,
        head: &str,
        definition: RecordRuntimeDef,
        spread: Option<Rc<Expr>>,
        inits: &[FieldInit],
        span: Span,
    ) -> Result<(), RtError> {
        let RecordRuntimeDef {
            runtime_id: record_id,
            method_identity,
            fields: decl_fields,
            ..
        } = definition;
        let NominalIdentityProjection {
            declaration: declaration_identity,
            method: method_identity,
        } = self.nominal_identity_projection(method_identity);
        // VALIDATE the explicit field set BEFORE evaluating any field, so an
        // UNKNOWN or DUPLICATE field faults (GUARD) rather than silently being
        // dropped/double-evaluated. This runs even under `--unchecked` and matches
        // the emitter byte-identically. (The checker rejects these for a checked
        // program; this keeps the runtime sound on the unchecked path.)
        let mut explicit: Vec<Rc<str>> = Vec::new();
        for f in inits {
            let fname: Rc<str> = Rc::from(self.text(f.name.span));
            if !decl_fields
                .iter()
                .any(|(n, _)| n.as_str() == fname.as_ref())
            {
                return Err(fault(
                    codes::GUARD_NO_FIELD,
                    format!("record `{head}` has no field `{fname}`"),
                    f.span,
                ));
            }
            if explicit.iter().any(|e| e.as_ref() == fname.as_ref()) {
                return Err(fault(
                    codes::GUARD_ARITY,
                    format!("field `{fname}` is given twice in `{head}`"),
                    f.span,
                ));
            }
            explicit.push(fname);
        }
        let has_spread = spread.is_some();
        // Explicit fields, in SOURCE order (left-to-right).
        let mut plan: Vec<NominalFieldPlan> = Vec::new();
        for f in inits {
            let fname: Rc<str> = Rc::from(self.text(f.name.span));
            plan.push(NominalFieldPlan {
                name: fname,
                expr: f.value.clone(),
                src: self.src.clone(),
                env: None,
                is_default: false,
            });
        }
        // Still-missing fields, in DECLARATION order. With a SPREAD, every
        // non-explicit field is supplied BY THE SPREAD BASE (same nominal id, so it
        // carries a value for every field) — a default is NEITHER evaluated NOR
        // needed. WITHOUT a spread, a non-explicit field falls to its default (or
        // faults if it has none — the `--unchecked` path; the checker rejects it).
        for (fname, default) in decl_fields.iter() {
            if explicit.iter().any(|e| e.as_ref() == fname.as_str()) {
                continue;
            }
            if has_spread {
                continue; // filled by the spread base, overriding the default
            }
            match default {
                Some(default) => plan.push(NominalFieldPlan {
                    name: Rc::from(fname.as_str()),
                    expr: default.expr.clone(),
                    src: default.src.clone(),
                    env: Some(default.env.clone()),
                    is_default: true,
                }),
                None => {
                    return Err(fault(
                        codes::GUARD_ARITY,
                        format!("record `{head}` is missing field `{fname}`"),
                        span,
                    ));
                }
            }
        }
        let decl_order: Rc<[Rc<str>]> = decl_fields
            .iter()
            .map(|(n, _)| Rc::from(n.as_str()))
            .collect();
        // With a spread, evaluate the base FIRST (KNominalSpread validates its id +
        // seeds the accumulator with its fields), THEN the explicit/default plan.
        if let Some(spread) = spread {
            self.frames.push(Frame::KNominalSpread {
                record_id,
                declaration_identity,
                method_identity,
                plan: Rc::from(plan),
                decl_order,
                span,
            });
            self.frames.push(Frame::Eval(spread));
            return Ok(());
        }
        self.frames.push(Frame::KNominalRecord {
            record_id,
            declaration_identity,
            method_identity,
            plan: Rc::from(plan),
            idx: 0,
            acc: Vec::new(),
            decl_order,
            span,
        });
        Ok(())
    }

    pub(super) fn member_access(
        &self,
        object: Value,
        field: &str,
        span: Span,
        root: Option<&str>,
    ) -> Result<Value, RtError> {
        if let Value::Namespace(target) = &object {
            // §17: a namespace lookup consumes exactly one exported
            // member name.
            let module_scope = self
                .module_scopes
                .get(target.as_ref())
                .expect("initialized module");
            if self.record_default_depth > 0
                && module_scope.runtime.private_default_values.contains(field)
                && let Some(value) = lookup(&module_scope.runtime.env, field)
            {
                return Ok(value);
            }
            return self.exported_value(module_scope, target, field, span);
        }
        // §8/§22.2 PURE member values — a record field, `.length`, `.keys`,
        // and the string-`.length` fault — come from the shared leaf so both
        // engines agree. (The record arm matches ANY field, so a record field
        // named like a method is a field access, exactly as before.) The
        // bound methods below are engine-specific (a receiver-bound `Builtin`).
        if let Some(value) = member_value(&object, field, span)? {
            return Ok(value);
        }
        let receiver = receiver_builtin(&object, field)
            .ok_or_else(|| no_member_fault(&object, field, span))?;
        if receiver.mutates {
            self.require_mut_root(root, span)?;
        }
        Ok(Value::Builtin {
            kind: receiver.kind,
            recv: Some(Rc::new(object)),
        })
    }
}
