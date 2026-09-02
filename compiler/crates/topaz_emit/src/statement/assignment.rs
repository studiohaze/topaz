use crate::*;

/// The mangled local an assignment targets — only a bare identifier
/// that is a MUTABLE `let` local in scope. A member/index target, a
/// free name or an immutable local is a static error.
pub(crate) fn local_target(
    target: &Expr,
    src: &LoweredText,
    locals: &[(String, Bind)],
) -> Result<(String, Bind), EmitError> {
    // Locate any assignment-target refusal at the offending target expression.
    local_target_inner(target, src, locals).map_err(|e| e.at(target.span))
}

pub(crate) fn local_target_inner(
    target: &Expr,
    src: &LoweredText,
    locals: &[(String, Bind)],
) -> Result<(String, Bind), EmitError> {
    if let ExprKind::Ident = &target.kind {
        let name = text(src, target.span);
        // The INNERMOST binding of this name wins (a child scope shadows an
        // enclosing one), via `lookup_bind`. A `Mut` assigns in place; a `Cell`
        // writes through `cell_set` (the caller dispatches on the returned
        // `Bind`); an `Imm` is a static error.
        match lookup_bind(locals, name) {
            Some(bind @ (Bind::Mut | Bind::Cell | Bind::TopMutValueCell)) => {
                return Ok((mangle(name), bind));
            }
            // An immutable `let` AND an immutable recursion `ImmCell` (a
            // `function` name) both refuse assignment — the interpreter faults
            // (`is not let mut`/`TPZ5003`); the emitter over-refuses to compile.
            Some(
                Bind::Imm | Bind::ImmCell | Bind::TopFnCell | Bind::TopValueCell | Bind::Namespace,
            ) => {
                return Err(EmitError::unsupported("assign to immutable"));
            }
            None => {}
        }
    }
    Err(EmitError::unsupported("assignment target"))
}

pub(crate) fn emit_classified_assignment(
    target: &Expr,
    emission: &AssignmentEmission<'_, '_, '_>,
    writable: impl FnOnce() -> Result<String, EmitError>,
) -> Result<String, EmitError> {
    match classify_assign_root(target, emission.src, emission.locals) {
        AssignRoot::Writable => writable(),
        AssignRoot::Immutable(name) => Ok(immutable_assign_fault(name, emission.span)),
        AssignRoot::Refuse => Err(EmitError::unsupported("assignment target").at(target.span)),
    }
}

pub(crate) fn emit_assignment_statement(
    target: &Expr,
    emission: &AssignmentEmission<'_, '_, '_>,
) -> Result<String, EmitError> {
    if matches!(target.kind, ExprKind::Index { .. }) {
        // §9 index-assign. An optional in the path is unassignable; the root
        // binding then classifies the write.
        if target_has_optional(target) {
            return Err(EmitError::unsupported("assignment target").at(target.span));
        }
        return emit_classified_assignment(target, emission, || {
            emit_index_assign(target, emission)
        });
    }
    if let Some((base, index, fields)) = cell_path(target, emission.src) {
        // §4/§9 member chains rooted at an index slot share the same optional
        // refusal and root classification as a direct index assignment.
        if target_has_optional(target) {
            return Err(EmitError::unsupported("assignment target").at(target.span));
        }
        return emit_classified_assignment(target, emission, || {
            emit_cell_path_assign(base, index, &fields, emission)
        });
    }
    if let Some((root_name, fields)) = record_path(target, emission.src) {
        return emit_classified_assignment(target, emission, || {
            emit_record_path_assign(root_name, &fields, emission)
        });
    }
    emit_simple_assign(target, emission)
}

pub(crate) fn emit_simple_assign(
    target: &Expr,
    emission: &AssignmentEmission<'_, '_, '_>,
) -> Result<String, EmitError> {
    let AssignmentEmission {
        op,
        value,
        span,
        src,
        aliases,
        locals,
        in_loop,
    } = emission;
    let (local, bind) = local_target(target, src, locals)?;
    let value_rs = emit_expr(value, src, aliases, locals, *in_loop)?;
    let cell = bind == Bind::Cell;
    let top_cell = bind == Bind::TopMutValueCell;
    let read = if cell {
        format!("cell_get(&{local})")
    } else if top_cell {
        format!(
            "top_cell_get(&{local}, {:?}, {})?",
            text(src, target.span),
            emit_span(target.span)
        )
    } else {
        format!("{local}.clone()")
    };
    let mut out = String::new();
    match *op {
        AssignOp::Assign if cell => out.push_str(&format!("    cell_set(&{local}, {value_rs});\n")),
        AssignOp::Assign if top_cell => {
            out.push_str(&format!("    top_cell_set(&{local}, {value_rs});\n"))
        }
        AssignOp::Assign => out.push_str(&format!("    {local} = {value_rs};\n")),
        // §12 `x ??= e` writes `e` ONLY when `x` is currently null/None (the RHS
        // lowers INTO the branch, so it evaluates only then — short-circuit).
        AssignOp::Coalesce if cell => out.push_str(&format!(
            "    if matches!(&{read}, Value::Null | Value::None) {{ cell_set(&{local}, {value_rs}); }}\n"
        )),
        AssignOp::Coalesce if top_cell => out.push_str(&format!(
            "    if matches!(&{read}, Value::Null | Value::None) {{ top_cell_set(&{local}, {value_rs}); }}\n"
        )),
        AssignOp::Coalesce => out.push_str(&format!(
            "    if matches!(&{local}, Value::Null | Value::None) {{ {local} = {value_rs}; }}\n"
        )),
        // §2 compound op reads the target BEFORE the RHS (read-operation-write),
        // then the shared `binary_value` leaf — result AND any §13a fault are
        // byte-identical to the interpreter (same op, same span). A faulting op
        // propagates BEFORE the write.
        _ => {
            let bop = match *op {
                AssignOp::Add => "Add",
                AssignOp::Sub => "Sub",
                AssignOp::Mul => "Mul",
                AssignOp::Div => "Div",
                AssignOp::Rem => "Rem",
                AssignOp::Assign | AssignOp::Coalesce => unreachable!("handled above"),
            };
            let combined =
                format!("binary_value(BinaryOp::{bop}, {read}, {value_rs}, {})?", emit_span(*span));
            if cell {
                out.push_str(&format!("    cell_set(&{local}, {combined});\n"));
            } else if top_cell {
                out.push_str(&format!("    top_cell_set(&{local}, {combined});\n"));
            } else {
                out.push_str(&format!("    {local} = {combined};\n"));
            }
        }
    }
    Ok(out)
}

/// §9 index-assign `xs[i] (op)= v` into a mutable Array root. The
/// object and index evaluate (in that order), then the shared `index_slot`
/// leaf validates the slot and faults identically to the interpreter BEFORE
/// the RHS evaluates; the cell is written in place through its `Rc`-shared
/// store, so the mutation is visible through the root binding. A compound op
/// reads the current element BEFORE the RHS (§2 read-operation-write); `??=`
/// writes — and evaluates the RHS — only when the element is null/None. Each
/// read borrow is dropped before the in-place write.
pub(crate) fn emit_index_assign(
    target: &Expr,
    emission: &AssignmentEmission<'_, '_, '_>,
) -> Result<String, EmitError> {
    let AssignmentEmission {
        op,
        value,
        span,
        src,
        aliases,
        locals,
        in_loop,
    } = emission;
    let ExprKind::Index { object, index } = &target.kind else {
        return Err(EmitError::unsupported("assignment target").at(target.span));
    };
    let obj_rs = emit_expr(object, src, aliases, locals, *in_loop)?;
    let idx_rs = emit_expr(index, src, aliases, locals, *in_loop)?;
    let value_rs = emit_expr(value, src, aliases, locals, *in_loop)?;
    let sp = emit_span(*span);
    let slot = format!(
        "let __ia_base = {obj_rs}; let __ia_idx = {idx_rs}; let (__ia_store, __ia_k) = index_slot(&__ia_base, &__ia_idx, {sp})?;"
    );
    let body = match *op {
        AssignOp::Assign => {
            format!("{slot} let __ia_v = {value_rs}; __ia_store.borrow_mut()[__ia_k] = __ia_v;")
        }
        AssignOp::Coalesce => format!(
            "{slot} let __ia_empty = matches!(&__ia_store.borrow()[__ia_k], Value::Null | Value::None); if __ia_empty {{ let __ia_v = {value_rs}; __ia_store.borrow_mut()[__ia_k] = __ia_v; }}"
        ),
        _ => {
            let bop = match *op {
                AssignOp::Add => "Add",
                AssignOp::Sub => "Sub",
                AssignOp::Mul => "Mul",
                AssignOp::Div => "Div",
                AssignOp::Rem => "Rem",
                AssignOp::Assign | AssignOp::Coalesce => unreachable!("handled above"),
            };
            format!(
                "{slot} let __ia_cur = __ia_store.borrow()[__ia_k].clone(); let __ia_v = {value_rs}; let __ia_new = binary_value(BinaryOp::{bop}, __ia_cur, __ia_v, {sp})?; __ia_store.borrow_mut()[__ia_k] = __ia_new;"
            )
        }
    };
    Ok(format!("    {{ {body} }}\n"))
}

pub(crate) fn classify_assign_root<'a>(
    target: &Expr,
    src: &'a LoweredText,
    locals: &[(String, Bind)],
) -> AssignRoot<'a> {
    match mutation_root(target, src) {
        None => AssignRoot::Writable,
        Some(name) => match lookup_bind(locals, name) {
            Some(Bind::Mut | Bind::Cell | Bind::TopMutValueCell) => AssignRoot::Writable,
            Some(
                Bind::Imm | Bind::ImmCell | Bind::TopFnCell | Bind::TopValueCell | Bind::Namespace,
            ) => AssignRoot::Immutable(name),
            None => AssignRoot::Refuse,
        },
    }
}

/// The GUARD_IMMUTABLE fault an immutable-rooted assignment emits, BEFORE any
/// object/index/RHS evaluation — the interpreter faults at the mut-root check in
/// `schedule_path_assign` before pushing any eval frame. Same wording for
/// index-assign and record-path.
pub(crate) fn immutable_assign_fault(name: &str, span: Span) -> String {
    let msg = format!("`{name}` is not `let mut` and cannot be assigned");
    format!(
        "    return Err(fault(codes::GUARD_IMMUTABLE, {msg:?}, {}));\n",
        emit_span(span)
    )
}

/// A pure record-path assignment target `r.f1.f2…` — the root identifier plus
/// its field-name chain (root-first). `None` if the target is a bare identifier
/// (a simple assign) or routes through anything but `Member` links (an
/// Index/Call/optional in the path is a cell-path or unassignable — handled
/// elsewhere or refused).
pub(crate) fn record_path<'s>(
    target: &Expr,
    src: &'s LoweredText,
) -> Option<(&'s str, Vec<&'s str>)> {
    let mut fields = Vec::new();
    let mut cursor = target;
    loop {
        match &cursor.kind {
            ExprKind::Member { object, field } => {
                fields.push(text(src, field.span));
                cursor = object;
            }
            ExprKind::Ident if !fields.is_empty() => {
                fields.reverse();
                return Some((text(src, cursor.span), fields));
            }
            _ => return None,
        }
    }
}

/// §4/§8 record-path assign `r.f… (op)= v` into a mutable-rooted record chain.
/// Mirrors the interpreter's `apply_record_path`: the RHS evaluates, the root is
/// read, the record is rebuilt functionally through the shared
/// `update_fields_value` leaf, and the root is rebound. A compound op reads the
/// current leaf (`walk_fields_value`) BEFORE the RHS and re-reads the root after
/// (§2 read-operation-write); `??=` reads the leaf and writes — evaluating the
/// RHS — only when it is null/None. The root is a mutable LOCAL (the caller
/// classified it `Writable`, and a record path is always `Ident`-rooted).
pub(crate) fn emit_record_path_assign(
    root_name: &str,
    fields: &[&str],
    emission: &AssignmentEmission<'_, '_, '_>,
) -> Result<String, EmitError> {
    let AssignmentEmission {
        op,
        value,
        span,
        src,
        aliases,
        locals,
        in_loop,
    } = emission;
    let bind = lookup_bind(locals, root_name).expect("record-path root is a classified local");
    let read = read_local(root_name, bind);
    let rebind = |new: &str| -> String {
        if bind == Bind::Cell {
            format!("cell_set(&{}, {new})", mangle(root_name))
        } else {
            format!("{} = {new}", mangle(root_name))
        }
    };
    let value_rs = emit_expr(value, src, aliases, locals, *in_loop)?;
    let sp = emit_span(*span);
    let fields_lit = fields
        .iter()
        .map(|f| format!("{f:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let body = match *op {
        AssignOp::Assign => format!(
            "let __rp_v = {value_rs}; let __rp_root = {read}; let __rp_new = update_fields_value(&__rp_root, &[{fields_lit}], __rp_v, {sp})?; {};",
            rebind("__rp_new")
        ),
        // §12 `??=` reads the current leaf; writes — and evaluates the RHS — only
        // when it is null/None. The leaf read can fault (non-record / no-field)
        // and that fault propagates BEFORE the null test.
        AssignOp::Coalesce => format!(
            "let __rp_cur = walk_fields_value(&{read}, &[{fields_lit}], {sp})?; if matches!(__rp_cur, Value::Null | Value::None) {{ let __rp_v = {value_rs}; let __rp_root = {read}; let __rp_new = update_fields_value(&__rp_root, &[{fields_lit}], __rp_v, {sp})?; {}; }}",
            rebind("__rp_new")
        ),
        // §2 compound: read the current leaf BEFORE the RHS, combine through the
        // shared `binary_value` leaf, then re-read the root and rebuild.
        _ => {
            let bop = match *op {
                AssignOp::Add => "Add",
                AssignOp::Sub => "Sub",
                AssignOp::Mul => "Mul",
                AssignOp::Div => "Div",
                AssignOp::Rem => "Rem",
                AssignOp::Assign | AssignOp::Coalesce => unreachable!("handled above"),
            };
            format!(
                "let __rp_cur = walk_fields_value(&{read}, &[{fields_lit}], {sp})?; let __rp_v = {value_rs}; let __rp_combined = binary_value(BinaryOp::{bop}, __rp_cur, __rp_v, {sp})?; let __rp_root = {read}; let __rp_new = update_fields_value(&__rp_root, &[{fields_lit}], __rp_combined, {sp})?; {};",
                rebind("__rp_new")
            )
        }
    };
    Ok(format!("    {{ {body} }}\n"))
}

/// A cell-path assignment target `arr[i].f1.f2…` — the array sub-expression, the
/// index expression, and the field chain rooted AT the index slot (root-first).
/// `None` unless the target is a `Member` chain whose innermost link is an
/// `Index` (a bare `Ident` root is a record-path and a bare `Index` is an
/// index-assign — both handled elsewhere).
pub(crate) fn cell_path<'t, 's>(
    target: &'t Expr,
    src: &'s LoweredText,
) -> Option<(&'t Expr, &'t Expr, Vec<&'s str>)> {
    let mut fields = Vec::new();
    let mut cursor = target;
    loop {
        match &cursor.kind {
            ExprKind::Member { object, field } => {
                fields.push(text(src, field.span));
                cursor = object;
            }
            ExprKind::Index { object, index } if !fields.is_empty() => {
                fields.reverse();
                return Some((object, index, fields));
            }
            _ => return None,
        }
    }
}

/// §4/§9 cell-path assign `arr[i].f… (op)= v` — a member chain rooted at an INDEX
/// access (the combination of index assignment and record path). Mirrors the
/// interpreter's `KCellPathAssign`/`apply_cell_path`: the array cell is resolved
/// in place (`index_slot`), the record chain past the cell updates functionally
/// through the shared `update_fields_value` leaf, and the rebuilt record is
/// written back to the same slot. The bounds-check resolves the slot BEFORE the
/// RHS and AGAIN at write time (so an RHS that resizes the array refaults
/// identically). A compound op reads the leaf (`walk_fields_value`) before the
/// RHS; `??=` reads the leaf and writes — evaluating the RHS — only when null/None.
pub(crate) fn emit_cell_path_assign(
    base: &Expr,
    index: &Expr,
    fields: &[&str],
    emission: &AssignmentEmission<'_, '_, '_>,
) -> Result<String, EmitError> {
    let AssignmentEmission {
        op,
        value,
        span,
        src,
        aliases,
        locals,
        in_loop,
    } = emission;
    let obj_rs = emit_expr(base, src, aliases, locals, *in_loop)?;
    let idx_rs = emit_expr(index, src, aliases, locals, *in_loop)?;
    let value_rs = emit_expr(value, src, aliases, locals, *in_loop)?;
    let sp = emit_span(*span);
    let fields_lit = fields
        .iter()
        .map(|f| format!("{f:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let anchor = format!("let __cp_base = {obj_rs}; let __cp_idx = {idx_rs};");
    // `read_cell_path`: re-resolve the slot, clone the cell, walk to the leaf.
    let read_leaf = format!(
        "{{ let (__cp_s, __cp_i) = index_slot(&__cp_base, &__cp_idx, {sp})?; let __cp_cell = __cp_s.borrow()[__cp_i].clone(); walk_fields_value(&__cp_cell, &[{fields_lit}], {sp})? }}"
    );
    // `apply_cell_path`: re-resolve the slot, rebuild the element, write it back.
    let write = |v: &str| -> String {
        format!(
            "let (__cp_store, __cp_k) = index_slot(&__cp_base, &__cp_idx, {sp})?; let __cp_cur = __cp_store.borrow()[__cp_k].clone(); let __cp_new = update_fields_value(&__cp_cur, &[{fields_lit}], {v}, {sp})?; __cp_store.borrow_mut()[__cp_k] = __cp_new;"
        )
    };
    let body = match *op {
        // The reference resolves (bounds-check only) BEFORE the RHS; the field
        // rebuild happens at write time, after the RHS.
        AssignOp::Assign => format!(
            "{anchor} index_slot(&__cp_base, &__cp_idx, {sp})?; let __cp_v = {value_rs}; {}",
            write("__cp_v")
        ),
        // §12 `??=` reads the leaf first; the write (and RHS) run only when null/None.
        AssignOp::Coalesce => format!(
            "{anchor} let __cp_pre = {read_leaf}; if matches!(__cp_pre, Value::Null | Value::None) {{ let __cp_v = {value_rs}; {} }}",
            write("__cp_v")
        ),
        // §2 compound: read the leaf BEFORE the RHS, combine through `binary_value`,
        // then re-read the element and rebuild (sibling writes by the RHS survive).
        _ => {
            let bop = match *op {
                AssignOp::Add => "Add",
                AssignOp::Sub => "Sub",
                AssignOp::Mul => "Mul",
                AssignOp::Div => "Div",
                AssignOp::Rem => "Rem",
                AssignOp::Assign | AssignOp::Coalesce => unreachable!("handled above"),
            };
            format!(
                "{anchor} let __cp_old = {read_leaf}; let __cp_v = {value_rs}; let __cp_comb = binary_value(BinaryOp::{bop}, __cp_old, __cp_v, {sp})?; {}",
                write("__cp_comb")
            )
        }
    };
    Ok(format!("    {{ {body} }}\n"))
}
