use super::*;

pub(crate) fn emit_nonvariadic_receiver_spread_branches(
    callee_rs: &str,
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
    call_span: Span,
) -> Result<(String, String), EmitError> {
    let rendered = render_call_args(args, ctx, call_span, "call argument shape")?;
    Ok((
        rendered.value_call(callee_rs, &[]),
        rendered.arity_fault("__tpz_recv_spread"),
    ))
}

pub(crate) fn emit_nonvariadic_receiver_spread_dispatch(
    recv_rs: &str,
    method: &str,
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
    member_span: Span,
    call_span: Span,
) -> Result<String, EmitError> {
    let m_span = emit_span(member_span);
    let (field_call, none_arm) =
        emit_nonvariadic_receiver_spread_branches("__field", args, ctx, call_span)?;
    Ok(format!(
        "{{ let __recv = {recv_rs}; \
         match member_value(&__recv, {method:?}, {m_span})? {{ \
         Some(__field) => {field_call}, \
         None => {{ check_member_method(&__recv, {method:?}, {m_span})?; {none_arm} }}, \
         }} }}"
    ))
}

pub(crate) fn receiver_hof_spread_guard(method: &str) -> Option<&'static str> {
    match method {
        "map" => Some(
            "matches!(&__recv, Value::Array(_) | Value::Some(_) | Value::None | Value::Ok(_) | Value::Err(_))",
        ),
        "filter" => Some("matches!(&__recv, Value::Array(_) | Value::Map(_))"),
        "reduce" | "sortedBy" => Some("matches!(&__recv, Value::Array(_))"),
        "mapValues" => Some("matches!(&__recv, Value::Map(_))"),
        "flatMap" => {
            Some("matches!(&__recv, Value::Some(_) | Value::None | Value::Ok(_) | Value::Err(_))")
        }
        "okOrElse" => Some("matches!(&__recv, Value::Some(_) | Value::None)"),
        _ => None,
    }
}

pub(crate) fn emit_nonvariadic_receiver_hof_spread_dispatch(
    recv_rs: &str,
    method: &str,
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
    member_span: Span,
    call_span: Span,
) -> Result<String, EmitError> {
    let Some(receiver_guard) = receiver_hof_spread_guard(method) else {
        return Err(EmitError::unsupported("receiver HOF spread argument"));
    };
    let m_span = emit_span(member_span);
    let (field_call, none_arm) =
        emit_nonvariadic_receiver_spread_branches("__field", args, ctx, call_span)?;
    Ok(format!(
        "{{ let __recv = {recv_rs}; \
         match member_value(&__recv, {method:?}, {m_span})? {{ \
         Some(__field) => {field_call}, \
         None => {{ if !({receiver_guard}) {{ check_member_method(&__recv, {method:?}, {m_span})?; }} {none_arm} }}, \
         }} }}"
    ))
}

pub(crate) fn emit_nonvariadic_receiver_mutator_spread_dispatch(
    recv_rs: &str,
    method: &str,
    root: Option<&str>,
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
    member_span: Span,
    call_span: Span,
) -> Result<String, EmitError> {
    let ExprEmitContext { locals, .. } = ctx;
    let m_span = emit_span(member_span);
    let (field_call, spread_fault) =
        emit_nonvariadic_receiver_spread_branches("__field", args, ctx, call_span)?;
    let recv_gate = match method {
        "update" => format!(
            "match &__recv {{ Value::Map(_) => {{}}, _ => return Err(no_member_fault(&__recv, {method:?}, {m_span})), }};"
        ),
        "sortBy" | "retain" => format!(
            "match &__recv {{ Value::Array(_) => {{}}, _ => return Err(no_member_fault(&__recv, {method:?}, {m_span})), }};"
        ),
        _ => return Err(EmitError::unsupported("receiver mutator spread argument")),
    };
    let none_arm = match optional_mutator_fault(root, locals, &m_span)? {
        Some(immutable_fault) => format!("{recv_gate} {immutable_fault}"),
        None => format!("{recv_gate} {spread_fault}"),
    };
    Ok(format!(
        "{{ let __recv = {recv_rs}; \
         match member_value(&__recv, {method:?}, {m_span})? {{ \
         Some(__field) => {field_call}, \
         None => {{ {none_arm} }}, \
         }} }}"
    ))
}

/// Render member lookup for every receiver builtin value. A record field wins
/// before builtin binding, exactly as in the interpreter. For a mutator, the
/// runtime receiver is proved before an immutable-root fault, so an unrelated
/// value still reports no-member and an optional `None` can still short-circuit.
pub(crate) fn render_receiver_member_binding(
    field: &str,
    member_span: &str,
    mutates: bool,
    root: Option<&str>,
    locals: &[(String, Bind)],
) -> Result<String, EmitError> {
    if mutates
        && let Some(name) = root
        && lookup_bind(locals, name).is_none()
    {
        return Err(EmitError::unsupported(
            "receiver mutator value on a non-local-rooted receiver",
        ));
    }
    let bound = format!("bind_receiver_builtin(__recv, {field:?}, {member_span})?");
    let builtin = if mutates {
        match root.and_then(|name| lookup_bind(locals, name).map(|bind| (name, bind))) {
            Some((_, Bind::Mut | Bind::Cell | Bind::TopMutValueCell)) | None => bound,
            Some((
                root_name,
                Bind::Imm | Bind::ImmCell | Bind::TopFnCell | Bind::TopValueCell | Bind::Namespace,
            )) => {
                let message = format!(
                    "`{root_name}` is not `let mut`; in-place collection mutation requires a mutable binding (§9)"
                );
                format!(
                    "let _ = {bound}; return Err(fault(codes::GUARD_IMMUTABLE, {message:?}, {member_span}))"
                )
            }
        }
    } else {
        bound
    };
    Ok(format!(
        "match member_value(&__recv, {field:?}, {member_span})? {{ \
         Some(__field) => __field, \
         None => {{ {builtin} }}, \
         }}"
    ))
}

pub(crate) fn try_emit_receiver_member_value(
    object: &Expr,
    object_rs: &str,
    field: &str,
    member_span: Span,
    src: &LoweredText,
    locals: &[(String, Bind)],
) -> Result<Option<String>, EmitError> {
    let Some(shape) = receiver_builtin_name_shape(field) else {
        return Ok(None);
    };
    let root = if shape.mutates {
        mutation_root(object, src)
    } else {
        None
    };
    let span = emit_span(member_span);
    let dispatch = render_receiver_member_binding(field, &span, shape.mutates, root, locals)?;
    Ok(Some(format!("{{ let __recv = {object_rs}; {dispatch} }}")))
}

/// §9 collection mutators by member NAME, mirroring the interpreter's
/// `mutator_root_of` list. Used by optional-call / optional-pipe gates where the
/// receiver type is only known at runtime; callback mutators may still be
/// declined later if that path cannot lower them faithfully.
pub(crate) fn is_collection_mutator_name(name: &str) -> bool {
    receiver_builtin_name_shape(name).is_some_and(|shape| shape.mutates)
}

/// Collection mutators that ride the shared `call_method` leaf. Callback
/// mutators (`update`/`sortBy`/`retain`) have inline lowering so their callback
/// laziness and write-back semantics stay byte-identical to the interpreter.
pub(crate) fn is_call_method_collection_mutator_name(name: &str) -> bool {
    receiver_builtin_name_shape(name)
        .is_some_and(|shape| shape.route == ReceiverBuiltinRoute::Method && shape.mutates)
}

pub(crate) fn is_resource_receiver_method(name: &str) -> bool {
    receiver_builtin_name_shape(name)
        .is_some_and(|shape| shape.route == ReceiverBuiltinRoute::Resource)
}

/// Lower a direct or array-receiver `map`/`filter` call through the same
/// callback-HOF driver used by first-class builtin calls.
pub(crate) fn emit_hof(name: &str, items_rs: &str, f_rs: &str, span: &str) -> String {
    let kind = match name {
        "map" => "Map",
        "filter" => "Filter",
        _ => unreachable!("unsupported callback HOF"),
    };
    format!(
        "call_callback_hof(CallbackHofKind::{kind}, vec![{items_rs}, {f_rs}], cx.clone(), {span}).await?"
    )
}

/// §6 (v5.4) Lower `Map.filter` and `Map.mapValues` through the same
/// receiver extraction, pair snapshot, and callback-map driver boundary.
pub(crate) fn emit_map_callback_hof(kind: &str, recv_rs: &str, f_rs: &str, span: &str) -> String {
    format!(
        "{{ let __m = {recv_rs}; let __f = {f_rs}; \
         let Value::Map(__cell) = __m else {{ unreachable!() }}; \
         let __pairs = __cell.borrow().pairs(); \
         call_callback_map_hof(CallbackMapHofKind::{kind}, __pairs, __f, cx.clone(), {span}).await? }}"
    )
}

/// §6 (v5.4) Lower `m.update(k, initial, f)` through the shared callback
/// transition. Receiver dispatch stays here so a non-`Map` faults `no_member`
/// at `m_span`; the shared cell keeps the mutation attached to the binding.
pub(crate) fn emit_map_update(
    recv_rs: &str,
    k_rs: &str,
    init_rs: &str,
    f_rs: &str,
    m_span: &str,
    c_span: &str,
) -> String {
    format!(
        "{{ let __m = {recv_rs}; let __k = {k_rs}; let __init = {init_rs}; let __f = {f_rs}; \
         let Value::Map(__cell) = __m else {{ return Err(no_member_fault(&__m, \"update\", {m_span})); }}; \
         call_callback_map_update(__cell, __k, __init, __f, cx.clone(), {c_span}).await? }}"
    )
}

/// §22 (v5.4) Lower `xs.sortedBy(f)` through the shared callback-key driver,
/// then stably sort the parallel item/key vectors through `sorted_by_keys`.
pub(crate) fn emit_sorted_by(items_rs: &str, f_rs: &str, span: &str) -> String {
    format!(
        "{{ let __items: Vec<Value> = iterable_items({items_rs}, {span})?; \
         let __f = {f_rs}; \
         let (__items, __keys) = collect_callback_keys(__items, __f, cx.clone(), {span}).await?; \
         Value::array(sorted_by_keys(&__items, &__keys, {span})?) }}"
    )
}

/// §6 (v5.4) Lower the callback Array mutators through one receiver extraction,
/// snapshot, callback-before-writeback, and unit-result boundary. `sortBy`
/// transforms the snapshot through callback keys and a stable sort; `retain`
/// transforms it through the callback predicate driver.
pub(crate) fn emit_array_callback_mutator(
    method: &str,
    recv_rs: &str,
    f_rs: &str,
    m_span: &str,
    span: &str,
) -> String {
    let transform = match method {
        "sortBy" => format!(
            "let (__items, __keys) = collect_callback_keys(__items, __f, cx.clone(), {span}).await?; \
             let __out = sorted_by_keys(&__items, &__keys, {span})?;"
        ),
        "retain" => {
            format!("let __out = collect_retained_items(__items, __f, cx.clone(), {span}).await?;")
        }
        _ => unreachable!("unsupported callback Array mutator"),
    };
    format!(
        "{{ let __recv = {recv_rs}; let __f = {f_rs}; \
         let Value::Array(__cell) = __recv else {{ return Err(no_member_fault(&__recv, {method:?}, {m_span})); }}; \
         let __items: Vec<Value> = __cell.borrow().clone(); \
         {transform} \
         *__cell.borrow_mut() = __out; \
         Value::Unit }}"
    )
}

/// Lower a direct or array-receiver `reduce` call through the same callback-HOF
/// driver used by first-class builtin calls.
pub(crate) fn emit_reduce(items_rs: &str, initial_rs: &str, f_rs: &str, span: &str) -> String {
    format!(
        "call_callback_hof(CallbackHofKind::Reduce, vec![{items_rs}, {initial_rs}, {f_rs}], cx.clone(), {span}).await?"
    )
}

/// Bind one source-ordered HIR call plan to a fixed receiver-method parameter
/// list, then render the written AST arguments once into source-order temps and
/// the original positional/named shape used by a record-field shadow.
pub(crate) fn render_named_receiver_args(
    expr: &Expr,
    args: &[CallArg],
    params: &[&str],
    ctx: ExprEmitContext<'_, '_, '_>,
    errors: NamedReceiverArgErrors,
) -> Result<RenderedNamedReceiverArgs, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let plan = expr
        .call
        .as_ref()
        .ok_or_else(|| EmitError::unsupported(errors.missing_plan))?;
    let mut slots = vec![None; params.len()];
    let mut next_positional = 0usize;
    for argument in &plan.args {
        let source_index = argument
            .source_index
            .ok_or_else(|| EmitError::unsupported(errors.unexpected_shape))?;
        let param_index = match &argument.binding {
            topaz_hir::ArgBinding::Positional => {
                if next_positional >= params.len() {
                    return Err(EmitError::unsupported(errors.too_many));
                }
                let index = next_positional;
                next_positional += 1;
                index
            }
            topaz_hir::ArgBinding::Named(name) => params
                .iter()
                .position(|param| *param == name)
                .ok_or_else(|| EmitError::unsupported(errors.unknown))?,
            topaz_hir::ArgBinding::Spread => {
                return Err(EmitError::unsupported(errors.spread));
            }
            topaz_hir::ArgBinding::InsertedLead => {
                return Err(EmitError::unsupported(errors.unknown));
            }
        };
        if slots[param_index].is_some() {
            return Err(EmitError::unsupported(errors.duplicate));
        }
        slots[param_index] = Some(source_index);
    }
    let slots = slots
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| EmitError::unsupported(errors.missing))?;

    let mut temps = String::new();
    let mut positional = Vec::new();
    let mut named = Vec::new();
    for (index, argument) in args.iter().enumerate() {
        let (expression, label) = match argument {
            CallArg::Positional(expression) => (expression, None),
            CallArg::Named { name, value } => (value, Some(text(src, name.span))),
            CallArg::Spread(_) => return Err(EmitError::unsupported(errors.spread)),
        };
        let rendered = emit_expr(expression, src, aliases, locals, in_loop)?;
        temps.push_str(&format!("let __a{index} = {rendered}; "));
        match label {
            None => positional.push(format!("__a{index}")),
            Some(name) => named.push(format!("({name:?}.to_string(), __a{index})")),
        }
    }
    Ok(RenderedNamedReceiverArgs {
        slots,
        temps,
        positional: positional.join(", "),
        named: named.join(", "),
    })
}

pub(crate) fn emit_reduce_named(
    expr: &Expr,
    args: &[CallArg],
    recv_rs: &str,
    ctx: RenderedCallContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let RenderedCallContext {
        expression,
        member_span: m_span,
        call_span: c_span,
    } = ctx;
    let RenderedNamedReceiverArgs {
        slots,
        temps,
        positional,
        named,
    } = render_named_receiver_args(
        expr,
        args,
        &["initial", "f"],
        expression,
        NamedReceiverArgErrors {
            missing_plan: "reduce: missing lowered call plan",
            unexpected_shape: "reduce: unexpected arg shape",
            too_many: "reduce: too many arguments",
            unknown: "reduce: unknown argument",
            spread: "reduce: unknown argument",
            duplicate: "reduce: argument given twice",
            missing: "reduce: missing argument",
        },
    )?;
    let [si, sf] = slots.as_slice() else {
        unreachable!("reduce receiver parameter inventory")
    };
    // None arm: the SAME fold loop the positional form emits, seeded by the
    // `initial` temp and folding with the `f` temp (semantics identical).
    let none_arm = emit_reduce("__recv", &format!("__a{si}"), &format!("__a{sf}"), c_span);
    // Args are evaluated AFTER member resolution (receiver → member_value → args),
    // matching the interpreter's phase order.
    Ok(format!(
        "{{ let __recv = {recv_rs}; \
         let __member = member_value(&__recv, \"reduce\", {m_span})?; \
         {temps}match __member {{ \
         Some(__field) => call_value_named(__field, vec![{positional}], vec![{named}], cx.clone(), {c_span}).await?, \
         None => {none_arm}, \
         }} }}"
    ))
}

/// Emit `m.update(...)` when the args include a named form (`update(k:, initial:,
/// f:)`, possibly reordered or mixed). Unlike read-only HOFs, this mutator must keep
/// the receiver/type/mutability gate before evaluating builtin-branch arguments, while
/// still forwarding the original labels to a record field that shadows `update`.
pub(crate) fn emit_map_update_named(
    expr: &Expr,
    args: &[CallArg],
    recv_rs: &str,
    root: Option<&str>,
    ctx: RenderedCallContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let RenderedCallContext {
        expression,
        member_span: m_span,
        call_span: c_span,
    } = ctx;
    let locals = expression.locals;
    let RenderedNamedReceiverArgs {
        slots,
        temps,
        positional,
        named,
    } = render_named_receiver_args(
        expr,
        args,
        &["k", "initial", "f"],
        expression,
        NamedReceiverArgErrors {
            missing_plan: "update: missing lowered call plan",
            unexpected_shape: "update: unexpected arg shape",
            too_many: "update: too many arguments",
            unknown: "update: unknown argument",
            spread: "update: spread argument",
            duplicate: "update: argument given twice",
            missing: "update: missing argument",
        },
    )?;
    let [sk, sinitial, sf] = slots.as_slice() else {
        unreachable!("update receiver parameter inventory")
    };
    let recv_gate = format!(
        "match &__recv {{ Value::Map(_) => {{}}, _ => return Err(no_member_fault(&__recv, \"update\", {m_span})), }};"
    );
    let update_body = emit_map_update(
        "__recv",
        &format!("__a{sk}"),
        &format!("__a{sinitial}"),
        &format!("__a{sf}"),
        m_span,
        c_span,
    );
    let none_arm = match root.and_then(|name| lookup_bind(locals, name).map(|b| (name, b))) {
        Some((_, Bind::Mut | Bind::Cell | Bind::TopMutValueCell)) | None => {
            format!("{recv_gate} {temps}{update_body}")
        }
        Some((
            root_name,
            Bind::Imm | Bind::ImmCell | Bind::TopFnCell | Bind::TopValueCell | Bind::Namespace,
        )) => {
            let msg = format!(
                "`{root_name}` is not `let mut`; in-place collection mutation requires a mutable binding (§9)"
            );
            format!("{recv_gate} return Err(fault(codes::GUARD_IMMUTABLE, {msg:?}, {m_span}))")
        }
    };

    Ok(format!(
        "{{ let __recv = {recv_rs}; match member_value(&__recv, \"update\", {m_span})? {{ \
         Some(__field) => {{ {temps}call_value_named(__field, vec![{positional}], vec![{named}], cx.clone(), {c_span}).await? }}, \
         None => {{ {none_arm} }}, \
         }} }}"
    ))
}

pub(crate) fn emit_single_callback_arg(
    args: &[CallArg],
    leading: Option<&str>,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<(String, bool), EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    match (leading, args) {
        (Some(lead), []) => Ok((lead.to_string(), false)),
        (Some(lead), [CallArg::Named { name, value }])
            if text(src, name.span) == "f" && matches!(&value.kind, ExprKind::Placeholder) =>
        {
            Ok((lead.to_string(), true))
        }
        (Some(_), [CallArg::Named { name, value }])
            if text(src, name.span) == "f" && contains_placeholder(value) =>
        {
            Err(EmitError::unsupported("pipe placeholder"))
        }
        (None, [CallArg::Positional(f)]) => {
            Ok((emit_expr(f, src, aliases, locals, in_loop)?, false))
        }
        (None, [CallArg::Named { name, value }]) if text(src, name.span) == "f" => {
            Ok((emit_expr(value, src, aliases, locals, in_loop)?, true))
        }
        _ => Err(EmitError::unsupported("call argument shape")),
    }
}

pub(crate) fn emit_single_callback_shadow_call(f_rs: &str, named: bool, c_span: &str) -> String {
    if named {
        format!(
            "call_value_named(__field, vec![], vec![(\"f\".to_string(), {f_rs})], cx.clone(), {c_span}).await?"
        )
    } else {
        format!("call_value(__field, vec![{f_rs}], cx.clone(), {c_span}).await?")
    }
}

pub(crate) fn emit_resource_method_dispatch(
    method: &str,
    args: &RenderedCallArgs,
    leading_positional: &[&str],
    before_args: &str,
    m_span: &str,
    c_span: &str,
) -> String {
    let field_call = args.value_call("__field", leading_positional);
    let builtin_call = args.resource_call(method, leading_positional, m_span, c_span);
    format!(
        "match member_value(&__recv, {method:?}, {m_span})? {{ \
         Some(__field) => {{ {before_args}{field_call} }}, \
         None => {{ check_member_method(&__recv, {method:?}, {m_span})?; {before_args}{builtin_call} }}, \
         }}"
    )
}

pub(crate) fn emit_receiver_one_callback_body(
    method: &str,
    recv_rs: &str,
    f_rs: &str,
    m_span: &str,
    c_span: &str,
) -> String {
    if method == "map" {
        format!("call_callback_receiver_map({recv_rs}, {f_rs}, cx.clone(), {c_span}).await?")
    } else if method == "filter" {
        format!(
            "{{ let __f = {f_rs}; match {recv_rs} {{ \
             __m @ Value::Map(_) => {}, \
             __it => {}, \
             }} }}",
            emit_map_callback_hof("Filter", "__m", "__f.clone()", c_span),
            emit_hof("filter", "__it", "__f.clone()", c_span)
        )
    } else if method == "sortedBy" {
        emit_sorted_by(recv_rs, f_rs, c_span)
    } else if method == "mapValues" {
        format!(
            "{{ let __f = {f_rs}; match {recv_rs} {{ \
             __m @ Value::Map(_) => {}, \
             __other => {{ check_member_method(&__other, \"mapValues\", {m_span})?; call_method(__other, \"mapValues\", vec![__f], {m_span}, {c_span})? }}, \
             }} }}",
            emit_map_callback_hof("MapValues", "__m", "__f.clone()", c_span)
        )
    } else if method == "flatMap" {
        format!(
            "call_callback_receiver_flat_map({recv_rs}, {f_rs}, cx.clone(), {m_span}, {c_span}).await?"
        )
    } else {
        unreachable!("unsupported one-callback method")
    }
}

pub(crate) fn emit_in_place_collection_mutator_call(
    expr: &Expr,
    callee: &Expr,
    object: &Expr,
    field: &Ident,
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let method = text(src, field.span);
    // member_value FIRST (the interpreter's `member_access` order): a record
    // field named like a mutator SHADOWS the collection mutator, EVEN on an
    // immutable receiver (`let r = { remove: … }; r.remove(x)` calls the field).
    // The `mut`-root requirement (`require_mut_root`) applies ONLY to the actual
    // collection-mutator `None` arm, and keys on the receiver path's ROOT binding
    // (`mutation_root` — the leftmost `Ident` under a member/index/optional/paren
    // chain), so `obj.xs.push(x)` is rooted at `obj`. A `mut`/cell-rooted path
    // mutates; an immutable-rooted path faults GUARD_IMMUTABLE byte-identically to
    // `require_mut_root` (AFTER the `check_member_method` type gate, so a wrong-type
    // receiver still faults `no_member`). A NON-`Ident`-rooted base (`get().push(x)`,
    // a literal) has root `None`: the interpreter's `require_mut_root(None)` PASSES and
    // mutates the receiver in place — via the shared `Rc<RefCell>` this can even reach
    // an immutable binding, so it is NOT a no-op — so the emitter lowers it WITHOUT a
    // mut-root guard (the `None` arm below). An `Ident` root that is not a LOCAL (a
    // const/import) is refused: the interpreter faults GUARD_IMMUTABLE there, a safe
    // over-refusal.
    let root = mutation_root(object, src);
    if let Some(name) = root
        && lookup_bind(locals, name).is_none()
    {
        return Err(EmitError::unsupported(
            "in-place mutator on a non-local-rooted receiver",
        ));
    }
    // The receiver PATH (an `Ident` or a member/index chain) evaluated BEFORE the
    // args (the interpreter's order); the shared `Rc<RefCell>` means the collection
    // is mutated in place, reaching the binding through the path.
    let obj_rs = emit_expr(object, src, aliases, locals, in_loop)?;
    let member_span = emit_span(callee.span);
    let call_span = emit_span(expr.span);
    let rendered_args = render_call_args(args, ctx, expr.span, "call argument shape")?;
    let all_positional = rendered_args.all_positional();
    // ByteBuffer is the binary-media hot path. Once the ordinary
    // member-first shadow rule and mutable-root gate have passed,
    // call its shared semantic leaf directly. This avoids allocating
    // a temporary `Vec<Value>` and entering async builtin dispatch on
    // every byte write while retaining the exact same validation,
    // fault, alias, and overlap implementation as the interpreter.
    let byte_buffer_direct = match (method, all_positional) {
        ("set", Some([index, value])) => Some(format!(
            "builtin_byte_buffer_set(__recv, {index}, {value}, {call_span})?"
        )),
        ("fill", Some([start, length, value])) => Some(format!(
            "builtin_byte_buffer_fill(__recv, {start}, {length}, {value}, {call_span})?"
        )),
        ("copy", Some([source, source_start, target_start, length])) => Some(format!(
            "builtin_byte_buffer_copy(__recv, {source}, {source_start}, {target_start}, {length}, {call_span})?"
        )),
        _ => None,
    };
    let field_call = rendered_args.value_call("__f", &[]);
    let method_call = rendered_args.method_call(method, &[], &member_span, &call_span);
    // The collection-mutator `None` arm: a `mut`/cell ROOT (or a non-`Ident` base,
    // root `None` — no mut-root requirement) mutates; an immutable LOCAL root faults
    // GUARD_IMMUTABLE (the `require_mut_root` fault) after the type gate. (A
    // const/import root was refused above.)
    let none_arm = match root.and_then(|name| lookup_bind(locals, name).map(|b| (name, b))) {
        Some((_, Bind::Mut | Bind::Cell | Bind::TopMutValueCell)) | None => {
            format!("check_member_method(&__recv, {method:?}, {member_span})?; {method_call}")
        }
        // An immutable `let` root AND an immutable recursion `ImmCell`
        // (a `function` name) both fault GUARD_IMMUTABLE — neither is
        // a `let mut`.
        Some((
            root_name,
            Bind::Imm | Bind::ImmCell | Bind::TopFnCell | Bind::TopValueCell | Bind::Namespace,
        )) => {
            let msg = format!(
                "`{root_name}` is not `let mut`; in-place collection mutation requires a mutable binding (§9)"
            );
            format!(
                "check_member_method(&__recv, {method:?}, {member_span})?; return Err(fault(codes::GUARD_IMMUTABLE, {msg:?}, {member_span}))"
            )
        }
    };
    if let Some(direct) = byte_buffer_direct {
        let direct_arm = match root.and_then(|name| lookup_bind(locals, name).map(|b| (name, b))) {
            Some((_, Bind::Mut | Bind::Cell | Bind::TopMutValueCell)) | None => direct,
            Some((
                root_name,
                Bind::Imm | Bind::ImmCell | Bind::TopFnCell | Bind::TopValueCell | Bind::Namespace,
            )) => {
                let msg = format!(
                    "`{root_name}` is not `let mut`; in-place collection mutation requires a mutable binding (§9)"
                );
                format!("return Err(fault(codes::GUARD_IMMUTABLE, {msg:?}, {member_span}))")
            }
        };
        return Ok(format!(
            "{{ let __recv = {obj_rs}; if matches!(&__recv, Value::ByteBuffer(_)) {{ {direct_arm} }} else {{ match member_value(&__recv, {method:?}, {member_span})? {{ \
                         Some(__f) => {field_call}, \
                         None => {{ {none_arm} }}, }} }} }}"
        ));
    }
    Ok(format!(
        "{{ let __recv = {obj_rs}; match member_value(&__recv, {method:?}, {member_span})? {{ \
                     Some(__f) => {field_call}, \
                     None => {{ {none_arm} }}, }} }}"
    ))
}

pub(crate) fn emit_map_update_call(
    expr: &Expr,
    callee: &Expr,
    object: &Expr,
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let method = "update";
    let root = mutation_root(object, src);
    if let Some(name) = root
        && lookup_bind(locals, name).is_none()
    {
        return Err(EmitError::unsupported(
            "in-place mutator on a non-local-rooted receiver",
        ));
    }
    let recv_rs = emit_expr(object, src, aliases, locals, in_loop)?;
    if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        let m_span = emit_span(callee.span);
        let (field_call, spread_fault) =
            emit_nonvariadic_receiver_spread_branches("__field", args, ctx, expr.span)?;
        let recv_gate = format!(
            "match &__recv {{ Value::Map(_) => {{}}, _ => return Err(no_member_fault(&__recv, {method:?}, {m_span})), }};"
        );
        let none_arm = match root.and_then(|name| lookup_bind(locals, name).map(|b| (name, b))) {
            Some((_, Bind::Mut | Bind::Cell | Bind::TopMutValueCell)) | None => {
                format!("{recv_gate} {spread_fault}")
            }
            Some((
                root_name,
                Bind::Imm | Bind::ImmCell | Bind::TopFnCell | Bind::TopValueCell | Bind::Namespace,
            )) => {
                let msg = format!(
                    "`{root_name}` is not `let mut`; in-place collection mutation requires a mutable binding (§9)"
                );
                format!("{recv_gate} return Err(fault(codes::GUARD_IMMUTABLE, {msg:?}, {m_span}))")
            }
        };
        return Ok(format!(
            "{{ let __recv = {recv_rs}; match member_value(&__recv, {method:?}, {m_span})? {{ \
                         Some(__field) => {field_call}, \
                         None => {{ {none_arm} }}, }} }}"
        ));
    }
    let [
        CallArg::Positional(k),
        CallArg::Positional(init),
        CallArg::Positional(f),
    ] = args
    else {
        if args.iter().any(|arg| matches!(arg, CallArg::Named { .. })) {
            let member_span = emit_span(callee.span);
            let call_span = emit_span(expr.span);
            return emit_map_update_named(
                expr,
                args,
                &recv_rs,
                root,
                RenderedCallContext {
                    expression: ctx,
                    member_span: &member_span,
                    call_span: &call_span,
                },
            );
        }
        return Err(EmitError::unsupported("call argument shape"));
    };
    let k_rs = emit_expr(k, src, aliases, locals, in_loop)?;
    let init_rs = emit_expr(init, src, aliases, locals, in_loop)?;
    let f_rs = emit_expr(f, src, aliases, locals, in_loop)?;
    let m_span = emit_span(callee.span);
    let c_span = emit_span(expr.span);
    let recv_gate = format!(
        "match &__recv {{ Value::Map(_) => {{}}, _ => return Err(no_member_fault(&__recv, {method:?}, {m_span})), }};"
    );
    // The collection-mutator `None` arm first proves the receiver is a Map,
    // then applies the `mut`-root gate. This mirrors interpreter
    // `member_access`: a wrong-type immutable root is NO-MEMBER, not
    // GUARD_IMMUTABLE, and args are not evaluated on that path.
    let none_arm = match root.and_then(|name| lookup_bind(locals, name).map(|b| (name, b))) {
        Some((_, Bind::Mut | Bind::Cell | Bind::TopMutValueCell)) | None => format!(
            "{recv_gate} {}",
            emit_map_update("__recv", &k_rs, &init_rs, &f_rs, &m_span, &c_span)
        ),
        Some((
            root_name,
            Bind::Imm | Bind::ImmCell | Bind::TopFnCell | Bind::TopValueCell | Bind::Namespace,
        )) => {
            let msg = format!(
                "`{root_name}` is not `let mut`; in-place collection mutation requires a mutable binding (§9)"
            );
            format!("{recv_gate} return Err(fault(codes::GUARD_IMMUTABLE, {msg:?}, {m_span}))")
        }
    };
    Ok(format!(
        "{{ let __recv = {recv_rs}; match member_value(&__recv, {method:?}, {m_span})? {{ \
                     Some(__field) => call_value(__field, vec![{k_rs}, {init_rs}, {f_rs}], cx.clone(), {c_span}).await?, \
                     None => {{ {none_arm} }}, }} }}"
    ))
}

pub(crate) fn emit_array_callback_mutator_call(
    expr: &Expr,
    callee: &Expr,
    object: &Expr,
    field: &Ident,
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let method = text(src, field.span);
    let root = mutation_root(object, src);
    if let Some(name) = root
        && lookup_bind(locals, name).is_none()
    {
        return Err(EmitError::unsupported(
            "in-place mutator on a non-local-rooted receiver",
        ));
    }
    let recv_rs = emit_expr(object, src, aliases, locals, in_loop)?;
    if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        let m_span = emit_span(callee.span);
        let (field_call, spread_fault) =
            emit_nonvariadic_receiver_spread_branches("__field", args, ctx, expr.span)?;
        let recv_gate = format!(
            "match &__recv {{ Value::Array(_) => {{}}, _ => return Err(no_member_fault(&__recv, {method:?}, {m_span})), }};"
        );
        let none_arm = match root.and_then(|name| lookup_bind(locals, name).map(|b| (name, b))) {
            Some((_, Bind::Mut | Bind::Cell | Bind::TopMutValueCell)) | None => {
                format!("{recv_gate} {spread_fault}")
            }
            Some((
                root_name,
                Bind::Imm | Bind::ImmCell | Bind::TopFnCell | Bind::TopValueCell | Bind::Namespace,
            )) => {
                let msg = format!(
                    "`{root_name}` is not `let mut`; in-place collection mutation requires a mutable binding (§9)"
                );
                format!("{recv_gate} return Err(fault(codes::GUARD_IMMUTABLE, {msg:?}, {m_span}))")
            }
        };
        return Ok(format!(
            "{{ let __recv = {recv_rs}; match member_value(&__recv, {method:?}, {m_span})? {{ \
                         Some(__field) => {field_call}, \
                         None => {{ {none_arm} }}, }} }}"
        ));
    }
    let (f_rs, named) = emit_single_callback_arg(args, None, ctx)?;
    let m_span = emit_span(callee.span);
    let c_span = emit_span(expr.span);
    let recv_gate = format!(
        "match &__recv {{ Value::Array(_) => {{}}, _ => return Err(no_member_fault(&__recv, {method:?}, {m_span})), }};"
    );
    // The collection-mutator `None` arm first proves the receiver is an Array,
    // then applies the `mut`-root gate. Wrong-type immutable receivers must
    // fault NO-MEMBER before GUARD_IMMUTABLE, matching interpreter order.
    let none_arm = match root.and_then(|name| lookup_bind(locals, name).map(|b| (name, b))) {
        Some((_, Bind::Mut | Bind::Cell | Bind::TopMutValueCell)) | None => {
            let body = emit_array_callback_mutator(method, "__recv", &f_rs, &m_span, &c_span);
            format!("{recv_gate} {body}")
        }
        Some((
            root_name,
            Bind::Imm | Bind::ImmCell | Bind::TopFnCell | Bind::TopValueCell | Bind::Namespace,
        )) => {
            let msg = format!(
                "`{root_name}` is not `let mut`; in-place collection mutation requires a mutable binding (§9)"
            );
            format!("{recv_gate} return Err(fault(codes::GUARD_IMMUTABLE, {msg:?}, {m_span}))")
        }
    };
    // member_value-first: a record field named `sortBy`/`retain` SHADOWS the
    // mutator (then `call_value` invokes it, preserving the source arg shape).
    let shadow = emit_single_callback_shadow_call(&f_rs, named, &c_span);
    Ok(format!(
        "{{ let __recv = {recv_rs}; match member_value(&__recv, {method:?}, {m_span})? {{ \
                     Some(__field) => {shadow}, \
                     None => {{ {none_arm} }}, }} }}"
    ))
}

pub(crate) fn emit_read_only_receiver_call(
    expr: &Expr,
    callee: &Expr,
    object: &Expr,
    field: &Ident,
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let method = text(src, field.span);
    let recv_rs = emit_expr(object, src, aliases, locals, in_loop)?;
    if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        return emit_nonvariadic_receiver_spread_dispatch(
            &recv_rs,
            method,
            args,
            ctx,
            callee.span,
            expr.span,
        );
    }
    if positional_after_named(args) {
        return Err(EmitError::unsupported("call argument shape"));
    }
    let m_span = emit_span(callee.span);
    let c_span = emit_span(expr.span);
    let rendered_args = render_call_args(args, ctx, expr.span, "call argument shape")?;
    let all_positional = rendered_args.all_positional();
    // The PNG/LZ77 path performs millions of byte reads.
    // `get` is shared by Array/Map/record receivers, so specialize
    // only after the evaluated runtime receiver proves the exact
    // ByteBuffer variant. The raw leaf retains the shared bounds
    // and fault semantics and returns i64; this uniform boxed
    // backend adds only its stack Value tag at the expression
    // boundary. Named/spread calls deliberately stay generic.
    let byte_buffer_get_direct = if method == "get"
        && let Some([index]) = all_positional
    {
        Some(format!(
            "Value::Int(builtin_byte_buffer_get_i64(&__recv, {index}, {c_span})?)"
        ))
    } else {
        None
    };
    let some_arm = rendered_args.value_call("__f", &[]);
    let method_call = rendered_args.method_call(method, &[], &m_span, &c_span);
    let none_arm = format!("check_member_method(&__recv, {method:?}, {m_span})?; {method_call}");
    let generic = format!(
        "match member_value(&__recv, {method:?}, {m_span})? {{ \
             Some(__f) => {some_arm}, \
             None => {{ {none_arm} }}, \
             }}"
    );
    Ok(match byte_buffer_get_direct {
        Some(direct) => format!(
            "{{ let __recv = {recv_rs}; if matches!(&__recv, Value::ByteBuffer(_)) {{ {direct} }} else {{ {generic} }} }}"
        ),
        None => format!("{{ let __recv = {recv_rs}; {generic} }}"),
    })
}

pub(crate) fn emit_receiver_hof_call(
    expr: &Expr,
    callee: &Expr,
    object: &Expr,
    field: &Ident,
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let method = text(src, field.span);
    let recv_rs = emit_expr(object, src, aliases, locals, in_loop)?;
    let m_span = emit_span(callee.span);
    let c_span = emit_span(expr.span);
    if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        return emit_nonvariadic_receiver_hof_spread_dispatch(
            &recv_rs,
            method,
            args,
            ctx,
            callee.span,
            expr.span,
        );
    }
    // §22 reduce with NAMED/reordered args (`reduce(f: y, initial: x)`):
    // the FIRST consumer of the HIR call-plan. The plan supplies the
    // source-ordered argument shape; a source-order-temp lowering then runs
    // the args' side effects in source order before slot-binding them to
    // `initial`/`f`. The positional `reduce(init, f)` form falls through to
    // the BYTE-IDENTICAL shared path below (difftest stays green).
    if method == "reduce" && args.iter().any(|a| matches!(a, CallArg::Named { .. })) {
        return emit_reduce_named(
            expr,
            args,
            &recv_rs,
            RenderedCallContext {
                expression: ctx,
                member_span: &m_span,
                call_span: &c_span,
            },
        );
    }
    // `shadow_call` is the record-field-shadow arm (member_value-first):
    // it must preserve the SOURCE arg shape (positional or named `f:`), or a
    // mis-named field call diverges — the interpreter faults `no parameter
    // named f`, native emit must too (via `call_value_named`).
    let (shadow_call, hof) = if method == "reduce" {
        let [CallArg::Positional(init), CallArg::Positional(f)] = args else {
            return Err(EmitError::unsupported("call argument shape"));
        };
        let init_rs = emit_expr(init, src, aliases, locals, in_loop)?;
        let f_rs = emit_expr(f, src, aliases, locals, in_loop)?;
        (
            format!("call_value(__field, vec![{init_rs}, {f_rs}], cx.clone(), {c_span}).await?"),
            emit_reduce("__recv", &init_rs, &f_rs, &c_span),
        )
    } else {
        let (f_rs, named) = emit_single_callback_arg(args, None, ctx)?;
        let hof = emit_receiver_one_callback_body(method, "__recv", &f_rs, &m_span, &c_span);
        let shadow = emit_single_callback_shadow_call(&f_rs, named, &c_span);
        (shadow, hof)
    };
    Ok(format!(
        "{{ let __recv = {recv_rs}; \
             match member_value(&__recv, {method:?}, {m_span})? {{ \
             Some(__field) => {shadow_call}, \
             None => {hof}, \
             }} }}"
    ))
}

pub(crate) fn emit_flat_map_call(
    expr: &Expr,
    callee: &Expr,
    object: &Expr,
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let recv_rs = emit_expr(object, src, aliases, locals, in_loop)?;
    if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        return emit_nonvariadic_receiver_hof_spread_dispatch(
            &recv_rs,
            "flatMap",
            args,
            ctx,
            callee.span,
            expr.span,
        );
    }
    let (f_rs, named) = emit_single_callback_arg(args, None, ctx)?;
    let m_span = emit_span(callee.span);
    let c_span = emit_span(expr.span);
    // The record-field shadow must preserve the named label (else divergence).
    let shadow_call = emit_single_callback_shadow_call(&f_rs, named, &c_span);
    let none_arm = emit_receiver_one_callback_body("flatMap", "__recv", &f_rs, &m_span, &c_span);
    Ok(format!(
        "{{ let __recv = {recv_rs}; \
             match member_value(&__recv, \"flatMap\", {m_span})? {{ \
             Some(__field) => {shadow_call}, \
             None => {none_arm}, \
             }} }}"
    ))
}

pub(crate) fn emit_resource_method_call(
    expr: &Expr,
    callee: &Expr,
    object: &Expr,
    field: &Ident,
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let method = text(src, field.span);
    let recv_rs = emit_expr(object, src, aliases, locals, in_loop)?;
    let rendered = render_call_args(args, ctx, expr.span, "call argument shape")?;
    let m_span = emit_span(callee.span);
    let c_span = emit_span(expr.span);
    let dispatch = emit_resource_method_dispatch(method, &rendered, &[], "", &m_span, &c_span);
    Ok(format!("{{ let __recv = {recv_rs}; {dispatch} }}"))
}

pub(crate) fn emit_ok_or_else_call(
    expr: &Expr,
    callee: &Expr,
    object: &Expr,
    field: &Ident,
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    if args.iter().any(|arg| matches!(arg, CallArg::Spread(_))) {
        let recv_rs = emit_expr(object, src, aliases, locals, in_loop)?;
        return emit_nonvariadic_receiver_hof_spread_dispatch(
            &recv_rs,
            "okOrElse",
            args,
            ctx,
            callee.span,
            expr.span,
        );
    }
    let method = text(src, field.span);
    let recv_rs = emit_expr(object, src, aliases, locals, in_loop)?;
    let rendered = render_ok_or_else_args(args, OkOrElseCallMode::Direct, ctx)?;
    let m_span = emit_span(callee.span);
    let c_span = emit_span(expr.span);
    // A record field named `okOrElse` SHADOWS through the `Some` arm, called
    // exactly as the generic lowering would: `call_value_named` when any named
    // arg is present (the runtime binds by name and faults `no parameter named
    // <n>` / `given twice` when the field's parameters differ — EXACTLY the
    // interpreter; the checker does not make a record-held function value's
    // argument names authoritative, so this is a CHECKED path), else `call_value`.
    let some_arm = rendered.shadow_call("__field", &c_span);
    let none_arm = rendered.builtin_arm(&m_span, &c_span);
    Ok(format!(
        "{{ let __recv = {recv_rs}; \
             match member_value(&__recv, {method:?}, {m_span})? {{ \
             Some(__field) => {some_arm}, \
             None => {{ {none_arm} }}, }} }}"
    ))
}

pub(crate) fn is_read_only_receiver_method(name: &str) -> bool {
    receiver_builtin_name_shape(name)
        .is_some_and(|shape| shape.route == ReceiverBuiltinRoute::Method && !shape.mutates)
}
