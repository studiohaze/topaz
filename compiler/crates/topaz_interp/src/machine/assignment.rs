use super::*;

impl<'a> Machine<'a> {
    /// Splits an assignment target at its LAST index segment (§4):
    /// everything after it is a pure record-member chain. With no
    /// index segment the chain roots at a binding and the write is a
    /// functional record update + rebind; with one, the array cell
    /// is the shared mutable anchor and base/index evaluate exactly
    /// once.
    /// The identifier a member/index/optional chain bottoms out at,
    /// for the §4/§9 mutable-root requirement; None when it roots at
    /// a non-binding (a temporary, literal, or call result).
    /// Parentheses and optional access (`?.`) are transparent, so a
    /// mutation through a nested optional chain (`r?.xs?.push(..)`)
    /// still anchors at the root binding.
    pub(super) fn mutation_root(&self, target: &Expr) -> Option<&str> {
        let mut cursor = target;
        loop {
            match &cursor.kind {
                ExprKind::Member { object, .. }
                | ExprKind::Index { object, .. }
                | ExprKind::OptionalAccess { object, .. }
                | ExprKind::Paren(object) => {
                    cursor = object;
                }
                ExprKind::Ident => return Some(self.text(cursor.span)),
                _ => return None,
            }
        }
    }

    /// The §9 mutator root for a `recv.field` access: the receiver's
    /// root binding when `field` names an in-place mutator, else
    /// None (only the collection arms of `member_access` actually
    /// enforce it, so non-collection receivers are unaffected).
    pub(super) fn mutator_root_of(&self, object: &Expr, field: &Ident) -> Option<&str> {
        // §6 (v5.4) `clear` (Map/Set) and `update` (Map) join the in-place mutators —
        // a `let mut` root is required, byte-identically to the emitter's gate. The
        // v5.4 array mutation API (`pop`/`reverse`/`removeAt`/`sort`/`sortBy`/`retain`)
        // joins too — EVERY mutator listed in `is_mutator` (builtins.rs) MUST be here, or
        // the static gate fires while the runtime root isn't resolved → run≢build (the
        // Slice A `clear`/`update` bug). `insert`/`clear` are already listed (shared with
        // Map/Set/the index-assign path).
        if matches!(
            self.text(field.span),
            "push"
                | "insert"
                | "add"
                | "remove"
                | "clear"
                | "update"
                | "pop"
                | "reverse"
                | "removeAt"
                | "sort"
                | "sortBy"
                | "retain"
                | "set"
                | "fill"
                | "copy"
        ) {
            self.mutation_root(object)
        } else {
            None
        }
    }

    /// §9: fault when the mutator's root binding is not `let mut`.
    pub(super) fn require_mut_root(&self, root: Option<&str>, span: Span) -> Result<(), RtError> {
        if let Some(name) = root
            && !is_mutable(&self.env, name)
        {
            return Err(fault(
                codes::GUARD_IMMUTABLE,
                format!(
                    "`{name}` is not `let mut`; in-place collection mutation requires a mutable binding (§9)"
                ),
                span,
            ));
        }
        Ok(())
    }

    pub(super) fn schedule_path_assign(
        &mut self,
        target: Rc<Expr>,
        op: AssignOp,
        value: Rc<Expr>,
        span: Span,
    ) -> Result<(), RtError> {
        // §4: an assignment target cannot route through optional
        // access — `?.` is conditional and not assignable.
        if target_has_optional(&target) {
            return Err(fault(
                codes::GUARD_TYPE,
                "cannot assign through optional access `?.` (§4)",
                span,
            ));
        }
        // §4/§9: an in-place member or index assignment requires a
        // mutable root binding (collections included — the cell
        // write is a collection mutation).
        if let Some(name) = self.mutation_root(&target)
            && !is_mutable(&self.env, name)
        {
            return Err(fault(
                codes::GUARD_IMMUTABLE,
                format!("`{name}` is not `let mut` and cannot be assigned"),
                span,
            ));
        }
        let mut fields: Vec<Ident> = Vec::new();
        let mut cursor = target;
        loop {
            match &cursor.kind {
                ExprKind::Member { object, field } => {
                    fields.push(*field);
                    cursor = object.clone();
                }
                ExprKind::Index { object, index } => {
                    fields.reverse();
                    // Stack discipline: [.., base, index]; the RHS
                    // evaluates only after the reference resolves.
                    self.frames.push(Frame::KCellPathAssign {
                        fields,
                        op,
                        value,
                        span,
                    });
                    self.frames.push(Frame::Eval(index.clone()));
                    self.frames.push(Frame::Eval(object.clone()));
                    return Ok(());
                }
                ExprKind::Ident => {
                    fields.reverse();
                    // The reference (root + leaf read where the op
                    // needs one) resolves before the RHS evaluates.
                    let current = match op {
                        AssignOp::Coalesce => {
                            // §12: only None/null evaluates and
                            // writes the RHS.
                            let name = self.text(cursor.span);
                            let root = lookup(&self.env, name).ok_or_else(|| {
                                fault(codes::GUARD_UNBOUND, format!("`{name}` is not bound"), span)
                            })?;
                            let cur = walk_fields(&root, &fields, &self.src, span)?;
                            if !matches!(cur, Value::Null | Value::None) {
                                return Ok(());
                            }
                            None
                        }
                        AssignOp::Assign => None,
                        _ => {
                            let name = self.text(cursor.span);
                            let root = lookup(&self.env, name).ok_or_else(|| {
                                fault(codes::GUARD_UNBOUND, format!("`{name}` is not bound"), span)
                            })?;
                            Some(walk_fields(&root, &fields, &self.src, span)?)
                        }
                    };
                    self.frames.push(Frame::KRecordPathAssign {
                        root: cursor.clone(),
                        fields,
                        op,
                        span,
                        current,
                    });
                    return self.eval_expr(&value);
                }
                _ => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        "an assignment target must be rooted at a binding or an index slot (§5)",
                        span,
                    ));
                }
            }
        }
    }

    /// Pure record chain on a binding: functional update + rebind.
    pub(super) fn apply_record_path(
        &mut self,
        root: &Expr,
        fields: &[Ident],
        op: AssignOp,
        span: Span,
        current: Option<Value>,
    ) -> Result<(), RtError> {
        let mut value = self.values.pop().expect("assign value");
        if let Some(old) = current {
            // The leaf was read before the RHS evaluated (§2
            // read-operation-write); the rebuild below still starts
            // from the root as it stands NOW, so sibling writes made
            // by the RHS survive.
            value = apply_op(&old, value, op, span)?;
        }
        let name = self.text(root.span).to_string();
        let root_now = lookup(&self.env, &name)
            .ok_or_else(|| fault(codes::GUARD_UNBOUND, format!("`{name}` is not bound"), span))?;
        if !is_mutable(&self.env, &name) {
            return Err(fault(
                codes::GUARD_IMMUTABLE,
                format!("`{name}` is not `let mut` and cannot be assigned"),
                span,
            ));
        }
        let updated = self.update_fields(&root_now, fields, value, span)?;
        match rebind(&self.env, &name, updated) {
            Ok(()) => Ok(()),
            Err("immutable") => Err(fault(
                codes::GUARD_IMMUTABLE,
                format!("`{name}` is not `let mut` and cannot be assigned"),
                span,
            )),
            Err(_) => Err(fault(
                codes::GUARD_UNBOUND,
                format!("`{name}` is not bound"),
                span,
            )),
        }
    }

    /// Array-cell anchor: the cell mutates in place; any record
    /// chain past the cell updates functionally into it.
    pub(super) fn apply_cell_path(
        &mut self,
        base: Value,
        index: Value,
        fields: &[Ident],
        value: Value,
        span: Span,
    ) -> Result<(), RtError> {
        let (items, i) = index_slot(&base, &index, span)?;
        let current = items.borrow()[i].clone();
        let updated = self.update_fields(&current, fields, value, span)?;
        items.borrow_mut()[i] = updated;
        Ok(())
    }

    pub(super) fn read_cell_path(
        &mut self,
        base: &Value,
        index: &Value,
        fields: &[Ident],
        span: Span,
    ) -> Result<Value, RtError> {
        let (items, i) = index_slot(base, index, span)?;
        let cell = items.borrow()[i].clone();
        walk_fields(&cell, fields, &self.src, span)
    }

    /// Functionally replaces `current.f1.f2... = value` (any compound op was
    /// already applied to the pre-RHS leaf read); with no fields the value
    /// replaces `current` itself. Resolves the field spans, then delegates to
    /// the shared `update_fields_value` leaf so the interpreter and the emitted
    /// code rebuild and fault identically (CDR-006 §2).
    pub(super) fn update_fields(
        &self,
        current: &Value,
        fields: &[Ident],
        value: Value,
        span: Span,
    ) -> Result<Value, RtError> {
        let names: Vec<&str> = fields.iter().map(|f| self.text(f.span)).collect();
        update_fields_value(current, &names, value, span)
    }

    pub(super) fn apply_assign(
        &mut self,
        target: &Expr,
        op: AssignOp,
        span: Span,
        current: Option<Value>,
    ) -> Result<(), RtError> {
        let rhs = self.values.pop().expect("assign value");
        let ExprKind::Ident = &target.kind else {
            return Err(fault(
                codes::GUARD_UNIMPLEMENTED,
                "invalid assignment target",
                span,
            ));
        };
        let name = self.text(target.span);
        let new_value = match op {
            AssignOp::Assign => rhs,
            AssignOp::Coalesce => unreachable!("`??=` desugars before KAssign"),
            _ => {
                // The pre-RHS read (§2 read-operation-write).
                let lhs = current.ok_or_else(|| {
                    fault(codes::GUARD_UNBOUND, format!("`{name}` is not bound"), span)
                })?;
                let bop = match op {
                    AssignOp::Add => BinaryOp::Add,
                    AssignOp::Sub => BinaryOp::Sub,
                    AssignOp::Mul => BinaryOp::Mul,
                    AssignOp::Div => BinaryOp::Div,
                    AssignOp::Rem => BinaryOp::Rem,
                    _ => unreachable!(),
                };
                binary_value(bop, lhs, rhs, span)?
            }
        };
        match rebind(&self.env, name, new_value) {
            Ok(()) => Ok(()),
            Err("immutable") => Err(fault(
                codes::GUARD_IMMUTABLE,
                format!("`{name}` is not `let mut` and cannot be assigned"),
                span,
            )),
            Err(_) => Err(fault(
                codes::GUARD_UNBOUND,
                format!("`{name}` is not bound"),
                span,
            )),
        }
    }

    pub(super) fn apply_unary(&mut self, op: UnaryOp, span: Span) -> Result<(), RtError> {
        let v = self.values.pop().expect("unary operand");
        let out = unary_value(op, v, span)?;
        self.values.push(out);
        Ok(())
    }

    pub(super) fn apply_binary(&mut self, op: BinaryOp, span: Span) -> Result<(), RtError> {
        let rhs = self.values.pop().expect("binary rhs");
        let lhs = self.values.pop().expect("binary lhs");
        let out = binary_value(op, lhs, rhs, span)?;
        self.values.push(out);
        Ok(())
    }
}

/// The final write of an assignment chain: plain, compound, never
/// `??=` (handled before scheduling).
pub(super) fn apply_op(
    current: &Value,
    value: Value,
    op: AssignOp,
    span: Span,
) -> Result<Value, RtError> {
    let bop = match op {
        AssignOp::Assign => return Ok(value),
        AssignOp::Add => BinaryOp::Add,
        AssignOp::Sub => BinaryOp::Sub,
        AssignOp::Mul => BinaryOp::Mul,
        AssignOp::Div => BinaryOp::Div,
        AssignOp::Rem => BinaryOp::Rem,
        AssignOp::Coalesce => unreachable!("`??=` desugars before the write"),
    };
    binary_value(bop, current.clone(), value, span)
}
