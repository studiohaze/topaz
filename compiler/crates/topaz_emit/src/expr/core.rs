use crate::*;

/// §9 the mutation ROOT of a mutator receiver path — the leftmost `Ident` under
/// a Member/Index/OptionalAccess/Paren chain (mirroring the interpreter's
/// `mutation_root`). `None` for a non-`Ident`-rooted path (a call, a literal).
/// The `mut`-root requirement (`require_mut_root`) keys on this root binding.
pub(crate) fn mutation_root<'s>(target: &Expr, src: &'s LoweredText) -> Option<&'s str> {
    let mut cursor = target;
    loop {
        match &cursor.kind {
            ExprKind::Member { object, .. }
            | ExprKind::Index { object, .. }
            | ExprKind::OptionalAccess { object, .. } => cursor = object,
            ExprKind::Paren(inner) => cursor = inner,
            ExprKind::Ident => return Some(text(src, cursor.span)),
            _ => return None,
        }
    }
}

/// Whether `name` is bound anywhere in scope (mutability irrelevant).
pub(crate) fn has_local(locals: &[(String, Bind)], name: &str) -> bool {
    locals.iter().any(|(n, _)| n == name)
}

/// Whether an assignment target routes through optional access (`?.`), which
/// the interpreter faults as unassignable (§4). The emitter REFUSES such a
/// target rather than emit a write the interpreter would never perform
/// (mirrors the interpreter's `target_has_optional`).
pub(crate) fn target_has_optional(target: &Expr) -> bool {
    match &target.kind {
        ExprKind::OptionalAccess { .. } => true,
        ExprKind::Member { object, .. }
        | ExprKind::Index { object, .. }
        | ExprKind::Paren(object) => target_has_optional(object),
        _ => false,
    }
}

/// The Rust expression that READS an in-scope local of the given kind: a `Cell`
/// drops its borrow through `cell_get`; any other binding clones its `Value`
/// (the carrier is `Rc`-shared). The caller classifies `name` with the
/// INNERMOST [`lookup_bind`], so an inner immutable `x` shadowing an outer cell
/// `x` reads plainly.
/// §22 a name with a PRELUDE meaning resolved on a `lookup_bind` miss: the
/// bare-value `Ident` fallbacks (`None` + the free builtins
/// `toInt`/`print`/`open`/`map`/`filter`/`reduce`), the free constructor CALLS
/// (`Some`/`Ok`/`Err`), and the static heads (`Array`/`Map`/`Set`, for `Array.of`
/// etc.). A top-level `function` shadowing one of these is REFUSED (§7): its
/// resolution is POSITIONAL/dynamic in the interpreter (the prelude before its
/// declaration runs, the user function after — looked up in the shared env at CALL
/// time), and the emitter can reproduce neither — leaving it positional bakes ONE
/// resolution into every earlier function body (a call after the declaration then
/// diverges), and celling it would fault `GUARD_UNBOUND` where the interpreter
/// falls back to the prelude. Refusing is the sound narrow slice (no binary, loud);
/// such user functions are vanishingly rare and absent from the corpus. A
/// NON-prelude name (incl. a method name like `push`, which has no free fallback)
/// IS celled — a pre-declaration reference then faults `GUARD_UNBOUND` in BOTH
/// engines. Keep in sync with the `Ident`/`Call` prelude-fallback arms.
pub(crate) fn is_prelude_name(name: &str) -> bool {
    matches!(
        name,
        "None"
            | "Some"
            | "Ok"
            | "Err"
            | "toInt"
            | "toIntRadix"
            | "fromCodePoint"
            | "toFloat"
            | "input"
            | "print"
            | "open"
            | "map"
            | "filter"
            | "reduce"
            | "Array"
            | "Map"
            | "Set"
    )
}

/// §7 refuse a TOP-LEVEL `function` (or `export function`) whose name shadows a
/// prelude value ([`is_prelude_name`]) — its prelude-vs-user resolution is dynamic
/// in the interpreter and the emitter cannot reproduce it (a function body that
/// names it would otherwise bake the prelude meaning while a call after the
/// declaration uses the user function). Applied in BOTH the entry and every
/// non-entry module so the divergence class is closed unit-wide; a loud refusal
/// (no binary), never a silent run≠build.
pub(crate) fn refuse_prelude_named_top_functions(
    items: &[Stmt],
    src: &LoweredText,
) -> Result<(), EmitError> {
    for stmt in items {
        let decl = match &stmt.kind {
            StmtKind::Function(decl) => Some(decl),
            StmtKind::Export(inner) => match &inner.kind {
                StmtKind::Function(decl) => Some(decl),
                _ => None,
            },
            _ => None,
        };
        if let Some(decl) = decl
            && is_prelude_name(text(src, decl.name.span))
        {
            return Err(
                EmitError::unsupported("top-level function shadows a prelude name")
                    .at(decl.name.span),
            );
        }
    }
    Ok(())
}

pub(crate) fn emit_expr(
    expr: &Expr,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    locals: &[(String, Bind)],
    in_loop: bool,
) -> Result<String, EmitError> {
    // Locate any lowering failure at the INNERMOST expression that produced it:
    // `.at` is first-wins, so as the error unwinds each enclosing `emit_expr`
    // call leaves the deepest span in place (CDR-001 §5 TPZ6001; per-node
    // precision, refined as coverage grows).
    emit_expr_inner(expr, src, aliases, locals, in_loop).map_err(|e| e.at(expr.span))
}

pub(crate) fn emit_member_expr(
    expr: &Expr,
    object: &Expr,
    field: &Ident,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let field_name = text(src, field.span);
    // §3 (v5.3) enum construction `Color.Red`: a declared enum head NOT
    // shadowed by a local, and `Red` a PAYLOAD-LESS variant → a payload-less
    // `Value::Enum`, mirroring the interpreter. A bare reference to a
    // PAYLOADFUL variant (`Shape.Circle`, no call) is NOT a value: it falls
    // through to ordinary member access (which faults), so `--unchecked`
    // run≡build (checked mode rejects it as an arity error). Intercepts
    // before ordinary member access.
    if let ExprKind::Ident = &object.kind {
        let head = text(src, object.span);
        // §15/§17 unchecked/runtime parity for bare static-member values. The
        // checked surface still rejects static members as first-class values,
        // but the interpreter's member evaluator can produce this `Builtin`
        // under `--unchecked`, so boxed emit must do the same.
        if !locals.iter().any(|(n, _)| n == head) {
            if let Some(variant) = Builtin::static_namespace(head, field_name) {
                return Ok(format!(
                    "Value::Builtin {{ kind: Builtin::{variant:?}, recv: None }}"
                ));
            }
            if head == "RoundingMode" {
                let variant = match field_name {
                    "Down" => Some("Down"),
                    "Up" => Some("Up"),
                    "TowardZero" => Some("TowardZero"),
                    "AwayFromZero" => Some("AwayFromZero"),
                    "HalfUp" => Some("HalfUp"),
                    "HalfEven" => Some("HalfEven"),
                    _ => None,
                };
                if let Some(variant) = variant {
                    return Ok(format!("rounding_mode_value(RoundingMode::{variant})"));
                }
            }
        }
        if !locals.iter().any(|(n, _)| n == head)
            && let Some(def) = aliases.enums.get(head)
            && let Some(&(0, variant_index)) = def.variants.get(field_name)
        {
            let method_identity = def
                .method_identity
                .as_ref()
                .map_or("None".to_string(), |id| format!("Some(Rc::from({id:?}))"));
            let declaration_identity = def
                .declaration_identity
                .as_ref()
                .map_or("None".to_string(), |id| format!("Some(Rc::from({id:?}))"));
            return Ok(format!(
                "Value::Enum {{ enum_id: Rc::from({:?}), declaration_identity: {declaration_identity}, method_identity: {method_identity}, variant: Rc::from({field_name:?}), variant_index: {variant_index}, payloads: Rc::from([] as [Value; 0]) }}",
                def.id
            ));
        }
    }
    // §17 namespace imports are statically known module export records, so a
    // field whose NAME collides with a receiver method (`add`, `map`, `clear`,
    // ...) must still read the export value. Ordinary receiver member values
    // keep the guard below.
    if let ExprKind::Ident = &object.kind {
        let head = text(src, object.span);
        if matches!(lookup_bind(locals, head), Some(Bind::Namespace)) {
            let obj_rs = emit_expr(object, src, aliases, locals, in_loop)?;
            return Ok(format!(
                "member_value_required(&({obj_rs}), {field_name:?}, {})?",
                emit_span(expr.span)
            ));
        }
    }
    let obj_rs = emit_expr(object, src, aliases, locals, in_loop)?;
    if let Some(bound) =
        try_emit_receiver_member_value(object, &obj_rs, field_name, expr.span, src, locals)?
    {
        return Ok(bound);
    }
    Ok(format!(
        "member_value_required(&({obj_rs}), {field_name:?}, {})?",
        emit_span(expr.span)
    ))
}

pub(crate) fn emit_identifier_expr(
    expr: &Expr,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        ..
    } = ctx;
    Ok({
        let name = text(src, expr.span);
        // The INNERMOST binding decides the read: a `Cell` drops its borrow
        // through `cell_get`, any other binding clones its `Value`. An inner
        // immutable `x` shadowing an outer cell `x` therefore reads plainly.
        match lookup_bind(locals, name) {
            // §7 a top-level forward-reference cell: read fallibly at THIS
            // identifier's span — an unfilled cell (a forward call reached
            // before the declaration ran) faults `GUARD_UNBOUND` exactly as the
            // interpreter's positional `ExprKind::Ident` miss.
            Some(Bind::TopFnCell | Bind::TopValueCell | Bind::TopMutValueCell) => {
                format!(
                    "top_cell_get(&{}, {name:?}, {})?",
                    mangle(name),
                    emit_span(expr.span)
                )
            }
            Some(bind) => read_local(name, bind),
            // §22.1 a bare `None` (not shadowed by a local) is the prelude
            // nullary constructor VALUE — exactly the interpreter's
            // `ExprKind::Ident` `None` arm, reached after `lookup` misses
            // (so a local `None` shadows it, as `lookup_bind` is checked
            // first).
            None if name == "None" => "Value::None".to_string(),
            // §22 EVERY free builtin (`toInt`/`print`/`open`/`map`/`filter`/
            // `reduce`) used as a first-class VALUE (`let f = toInt`,
            // `xs |> map(toInt)`) is the interpreter's `free_builtin` →
            // `Value::Builtin { recv: None }` (the `ExprKind::Ident` arm
            // after `lookup` misses), dispatched by `call_value`. Direct and
            // first-class callback HOFs converge on `call_callback_hof`; this arm
            // is reached only for the bare VALUE form. A genuinely unbound name
            // is a checker error.
            None if name == "toInt" => {
                "Value::Builtin { kind: Builtin::ToInt, recv: None }".to_string()
            }
            None if name == "toIntRadix" => {
                "Value::Builtin { kind: Builtin::ToIntRadix, recv: None }".to_string()
            }
            None if name == "fromCodePoint" => {
                "Value::Builtin { kind: Builtin::FromCodePoint, recv: None }".to_string()
            }
            None if name == "toFloat" => {
                "Value::Builtin { kind: Builtin::ToFloat, recv: None }".to_string()
            }
            None if name == "input" => {
                "Value::Builtin { kind: Builtin::Input, recv: None }".to_string()
            }
            None if name == "assert" => {
                "Value::Builtin { kind: Builtin::TestAssert, recv: None }".to_string()
            }
            None if lispex_intrinsic_kind(aliases, name).is_some() => {
                let kind = lispex_intrinsic_kind(aliases, name)
                    .expect("guard established the generated intrinsic");
                format!("Value::Builtin {{ kind: Builtin::{kind}, recv: None }}")
            }
            None if name == "print" => {
                "Value::Builtin { kind: Builtin::Print, recv: None }".to_string()
            }
            None if name == "open" => {
                "Value::Builtin { kind: Builtin::Open, recv: None }".to_string()
            }
            // §22 the HOF free builtins `map`/`filter`/`reduce` as VALUES.
            // `call_value` and direct calls both use the shared callback-HOF driver.
            None if matches!(name, "map" | "filter" | "reduce") => {
                let variant = match name {
                    "map" => "MapFn",
                    "filter" => "FilterFn",
                    _ => "ReduceFn",
                };
                format!("Value::Builtin {{ kind: Builtin::{variant}, recv: None }}")
            }
            None => return Err(EmitError::unsupported("free identifier")),
        }
    })
}

pub(crate) fn emit_lambda_expr(
    params: &[topaz_hir::emission::LambdaParam],
    body: &Expr,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        ..
    } = ctx;
    Ok({
        // Lambda param TYPES are ignored here — the checker handles them
        // statically; the emitted callable binds slots by name/position
        // and emits no §6 boundary guards for lambdas. A `LambdaParam`
        // has no default or variadic, so the name is all the emitter needs.
        let param_names: Vec<&str> = params.iter().map(|p| text(src, p.name.span)).collect();
        let param_locals: Vec<(String, Bind)> = param_names
            .iter()
            .map(|n| (n.to_string(), Bind::Imm))
            .collect();
        let captures = lambda_captures(body, &param_locals, locals, src)?;
        // The body sees the params (immutable) and the captures. A captured
        // `Cell` carries its cell-ness in (reads go through `cell_get`); a
        // captured plain `Mut` is refused (the safety gate).
        let mut body_locals = param_locals;
        push_capture_locals(&captures, locals, &mut body_locals)?;
        // §3/§7 a lambda's own type-param set is EMPTY (the interpreter builds
        // its `ClosureData.type_params` empty and swaps to it on call), so an
        // enclosing function's params do NOT erase inside the lambda body —
        // reset to empty to match (a typed binding over an outer `T` declines
        // in both engines, the same as before this slice).
        // §17 a lambda body's emit locals are capture-pruned, and a lambda can be
        // declared in any scope (incl. a block with a shadowing local the head
        // does not capture) — refuse a qualified type in it this slice (`true`).
        let lambda_aliases = aliases.with_body(&[], true);
        // §14 a lambda body is its OWN closure (its own `__defers` via the wrapper,
        // its own async block): its `return`/`?` must drain ITS block defers, NOT the
        // enclosing scope's (whose stacks aren't even in the lambda's captures). RESET
        // the shared flow for the body, restore after.
        let body_rs = with_reset_flow(&lambda_aliases, |a| {
            emit_expr(body, src, a, &body_locals, false)
        })?;
        // A lambda has no §7 default parameters (`LambdaParam` carries no
        // default), no §5 variadic, and no declared §6 return type, so it
        // passes none of them — and is never return-guarded.
        emit_closure_value(ClosureEmission {
            param_names: &param_names,
            captures: &captures,
            defaults: &[],
            variadic: None,
            variadic_guard: None,
            param_guards: "",
            body: &body_rs,
            return_guard: None,
            has_defers: false,
        })
    })
}

pub(crate) fn emit_string_expr(
    literal: &topaz_hir::emission::StringLit,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    if let Some(tag_span) = literal.tag {
        let tag = text(src, tag_span);
        let mut parts = Vec::new();
        let mut current = String::new();
        let mut values = Vec::new();
        for part in &literal.parts {
            match part {
                StringPart::Text(span) => {
                    decode_escapes(text(src, *span), &mut current, *span)
                        .map_err(|_| EmitError::malformed_literal("string escape"))?;
                }
                StringPart::Interpolation(value) => {
                    parts.push(std::mem::take(&mut current));
                    values.push(emit_expr(value, src, aliases, locals, in_loop)?);
                }
            }
        }
        parts.push(current);
        let rendered_parts = parts
            .iter()
            .map(|part| format!("{part:?}.to_string()"))
            .collect::<Vec<_>>()
            .join(", ");
        return Ok(format!(
            "make_template({tag:?}.to_string(), vec![{rendered_parts}], vec![{}])",
            values.join(", ")
        ));
    }

    let has_interpolation = literal
        .parts
        .iter()
        .any(|part| matches!(part, StringPart::Interpolation(_)));
    if !has_interpolation {
        let mut decoded = String::new();
        for part in &literal.parts {
            if let StringPart::Text(span) = part {
                decode_escapes(text(src, *span), &mut decoded, *span)
                    .map_err(|_| EmitError::malformed_literal("string escape"))?;
            }
        }
        return Ok(format!("Value::str({decoded:?})"));
    }

    let mut statements = String::new();
    for part in &literal.parts {
        match part {
            StringPart::Text(span) => {
                let mut decoded = String::new();
                decode_escapes(text(src, *span), &mut decoded, *span)
                    .map_err(|_| EmitError::malformed_literal("string escape"))?;
                statements.push_str(&format!("__s.push_str({decoded:?}); "));
            }
            StringPart::Interpolation(value) => {
                let value = emit_expr(value, src, aliases, locals, in_loop)?;
                statements.push_str(&format!("__s.push_str(&render(&({value}))); "));
            }
        }
    }
    Ok(format!(
        "{{ let mut __s = String::new(); {statements}Value::str(__s) }}"
    ))
}

pub(crate) fn emit_operator_expr(
    expr: &Expr,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    Ok(match &expr.kind {
        ExprKind::Binary { op, lhs, rhs } => {
            let lhs = emit_expr(lhs, src, aliases, locals, in_loop)?;
            let rhs = emit_expr(rhs, src, aliases, locals, in_loop)?;
            if matches!(op, BinaryOp::And | BinaryOp::Or | BinaryOp::Coalesce) {
                format!(
                    "match short_circuit_lhs({lhs}, BinaryOp::{op:?}, {})? {{ Some(__v) => __v, None => {rhs} }}",
                    emit_span(expr.span)
                )
            } else {
                format!(
                    "binary_value(BinaryOp::{op:?}, {lhs}, {rhs}, {})?",
                    emit_span(expr.span)
                )
            }
        }
        ExprKind::Unary { op, operand } => {
            let operand = emit_expr(operand, src, aliases, locals, in_loop)?;
            format!(
                "unary_value(UnaryOp::{op:?}, {operand}, {})?",
                emit_span(expr.span)
            )
        }
        _ => unreachable!("operator helper received another expression kind"),
    })
}

pub(crate) fn emit_index_expr(
    expr: &Expr,
    object: &Expr,
    index: &Expr,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let object = emit_expr(object, src, aliases, locals, in_loop)?;
    let index = emit_expr(index, src, aliases, locals, in_loop)?;
    Ok(format!(
        "index_value({object}, {index}, {})?",
        emit_span(expr.span)
    ))
}

pub(crate) fn emit_optional_access_expr(
    expr: &Expr,
    object: &Expr,
    field: &Ident,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let field = text(src, field.span);
    let root = receiver_builtin_name_shape(field)
        .filter(|shape| shape.mutates)
        .and_then(|_| mutation_root(object, src));
    let object_rs = emit_expr(object, src, aliases, locals, in_loop)?;
    if let Some(shape) = receiver_builtin_name_shape(field) {
        let span = emit_span(expr.span);
        let dispatch = render_receiver_member_binding(field, &span, shape.mutates, root, locals)?;
        return Ok(format!(
            "{{ let __obj = {object_rs}; match __obj {{ \
             Value::None => Value::None, \
             Value::Null => Value::Null, \
             Value::Some(__inner) => {{ let __recv = (*__inner).clone(); wrap_optional({dispatch}) }}, \
             __other => {{ let __recv = __other; {dispatch} }}, \
             }} }}"
        ));
    }
    Ok(format!(
        "optional_member({object_rs}, {field:?}, {})?",
        emit_span(expr.span)
    ))
}

pub(crate) fn emit_try_expr(
    expr: &Expr,
    inner: &Expr,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let inner = emit_expr(inner, src, aliases, locals, in_loop)?;
    let drain = {
        let flow = aliases.flow.borrow();
        flow.drain_from(flow.fn_base)
    };
    Ok(format!(
        "match try_value({inner}, {})? {{ Ok(__v) => __v, Err(__early) => {{ {drain}return Ok(__early) }} }}",
        emit_span(expr.span)
    ))
}

pub(crate) fn emit_compose_expr(
    lhs: &Expr,
    rhs: &Expr,
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let lhs = emit_expr(lhs, src, aliases, locals, in_loop)?;
    let rhs = emit_expr(rhs, src, aliases, locals, in_loop)?;
    Ok(format!("Value::Composed(Rc::new(({lhs}, {rhs})))"))
}

pub(crate) fn emit_expr_inner(
    expr: &Expr,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    locals: &[(String, Bind)],
    in_loop: bool,
) -> Result<String, EmitError> {
    Ok(match &expr.kind {
        ExprKind::Int => {
            let n: i64 = text(src, expr.span)
                .parse()
                .map_err(|_| EmitError::malformed_literal("integer"))?;
            format!("Value::Int({n})")
        }
        ExprKind::Float => {
            // Parsed exactly as the interpreter reads it. A finite
            // value emits round-trippably (`{:?}` is the shortest form
            // that parses back to the same f64), so it is bit-identical.
            // A lexer-valid OVERSIZED literal (e.g. `2` then 308 `0`s
            // then `.0`) parses to `f64::INFINITY` in BOTH engines — but
            // `{:?}` renders that as `inf`, which is not a Rust literal,
            // so the const is emitted instead. A bare float TOKEN is
            // unsigned (a leading `-` is a `Unary`, unsupported here) and
            // never `NaN`, so positive infinity is the only non-finite
            // case.
            let x: f64 = text(src, expr.span)
                .parse()
                .map_err(|_| EmitError::malformed_literal("float"))?;
            if x.is_finite() {
                format!("Value::Float({x:?})")
            } else {
                "Value::Float(f64::INFINITY)".to_string()
            }
        }
        ExprKind::Bool(b) => format!("Value::Bool({b})"),
        ExprKind::Unit => "Value::Unit".to_string(),
        ExprKind::Null => "Value::Null".to_string(),
        // An identifier read: a `let` local clones its current value
        // (the `Value` carrier is `Rc`-shared). A name that is NOT a
        // local is a free name — the bare prelude `None` value, else a
        // §22 builtin used as a value without value support, or an unbound name
        // (a checker error) — refuse rather than emit an undefined Rust local.
        ExprKind::Ident => emit_identifier_expr(
            expr,
            ExprEmitContext {
                src,
                aliases,
                locals,
                in_loop,
            },
        )?,
        ExprKind::String(literal) => emit_string_expr(
            literal,
            ExprEmitContext {
                src,
                aliases,
                locals,
                in_loop,
            },
        )?,
        // `(e)` is transparent to the value — emit the inner expression.
        ExprKind::Paren(inner) => emit_expr(inner, src, aliases, locals, in_loop)?,
        // §2 operators lower to a call into the SHARED leaf so the
        // result AND any §13a fault (overflow, div-by-zero, …) are
        // byte-identical to the interpreter — same function, same span.
        // The span is threaded as a literal so an emitted fault points
        // at exactly the operator the interpreter would.
        ExprKind::Binary { .. } | ExprKind::Unary { .. } => emit_operator_expr(
            expr,
            ExprEmitContext {
                src,
                aliases,
                locals,
                in_loop,
            },
        )?,
        ExprKind::Array(_)
        | ExprKind::SetLiteral(_)
        | ExprKind::MapLiteral(_)
        | ExprKind::RecordLiteral { .. } => emit_aggregate_literal_expr(
            expr,
            ExprEmitContext {
                src,
                aliases,
                locals,
                in_loop,
            },
        )?,
        // A block `{ … }` is its own lexical scope; it lowers to a Rust
        // block expression yielding the block's tail value (`Unit` if
        // there is none).
        ExprKind::Block(block) => emit_block_expr(block, src, aliases, locals, in_loop)?,
        // `if cond { … } (else …)?` (§5). The condition runs through the
        // SHARED `condition_bool` guard so a non-`bool` faults identically
        // to the interpreter — same code, message, and span (the whole
        // `if` expression's span, exactly as the interpreter threads it).
        // Each arm is a block value; a missing `else` yields `Unit`. An
        // `else if` chains naturally because the else branch lowers as an
        // expression (another `if`).
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => emit_if_expr(
            expr,
            cond,
            then_block,
            else_branch.as_deref(),
            ExprEmitContext {
                src,
                aliases,
                locals,
                in_loop,
            },
        )?,
        // §6.4 (v5.4) a COMPREHENSION: a fresh Rust accumulator `__cacc`, the
        // clause list lowered to a nested Rust `for`/`if` (the SAME `for_items` /
        // pattern machinery a real loop uses), the body appended per surviving
        // iteration, then FINALIZED through the SAME shared leaf the literal uses —
        // `Value::array` (array), `builtin_set_of` (set, duplicate collapse), or
        // `builtin_map_of` (map, duplicate-key fault TPZ4601). So the result, its
        // order, and its faults are byte-identical to the interpreter.
        ExprKind::Comprehension {
            kind,
            clauses,
            body,
        } => emit_comprehension_expr(
            expr,
            *kind,
            clauses,
            body,
            ExprEmitContext {
                src,
                aliases,
                locals,
                in_loop,
            },
        )?,
        // A `for` in EXPRESSION position is value-collecting (§5): it
        // gathers each body value into `Value::array(acc)`. It may NOT be
        // targeted by bare `break`/`continue` (a static error), so its body is
        // lowered as not-in-loop; labeled control may pass through to an outer
        // labeled `loop`.
        ExprKind::For {
            pattern,
            iter,
            body,
        } => emit_for(ForEmission {
            pattern,
            iter,
            body,
            span: expr.span,
            src,
            aliases,
            locals,
            collect: true,
        })?,
        // `loop (label)? { body }` is an infinite-loop expression. It lowers
        // to a LABELED Rust `loop` whose break value IS the expression's value:
        // `'lN: loop { checkpoint().await; <body discarded>; }`. The body is a
        // statement context (its tail value discarded, matching the interpreter's
        // `KDiscard`); only a `break <value>` targeting this loop yields a result.
        // The unique `'lN` label lets `break 'lN value` reach it from a NESTED
        // loop, and is always present so a labeled break can cross a `while`/`for`.
        ExprKind::Loop { label, body } => emit_loop(*label, body, src, aliases, locals)?,
        // §10 range `lo .. hi (by step)?` lowers to the SHARED `make_range`
        // builder, so the int-endpoint and int/non-zero-step guards and
        // their faults are byte-identical to the interpreter. Endpoints
        // then the step lower left-to-right, matching the interpreter's
        // evaluation order, so an evaluation fault fires in source order.
        ExprKind::Range {
            lo,
            hi,
            inclusive,
            step,
        } => emit_range_expr(
            expr,
            lo,
            hi,
            *inclusive,
            step.as_deref(),
            ExprEmitContext {
                src,
                aliases,
                locals,
                in_loop,
            },
        )?,
        ExprKind::Call {
            callee,
            args,
            type_args,
        } => emit_call_expr(
            expr,
            callee,
            args,
            type_args,
            ExprEmitContext {
                src,
                aliases,
                locals,
                in_loop,
            },
        )?,
        // §5 `match` — bind the scrutinee once, test cases in order. The
        // span threaded for the cmp/match-miss faults is the whole match
        // expression's span (the interpreter's `KMatchDispatch` span).
        ExprKind::Match { scrutinee, cases } => {
            emit_match(scrutinee, cases, expr.span, src, aliases, locals, in_loop)?
        }
        // §5 lambda (CDR-006 §4 async callable ABI). The body lowers with
        // the params PLUS the CAPTURES as locals; a capture is an enclosing
        // local the body references free. An immutable capture is a value
        // snapshot; a mutable capture must have been lifted to a rebinding
        // cell by escape analysis, or the capture safety gate refuses the
        // plain `Mut`. Each capture is cloned at lambda creation, owned by
        // the `move` closure, and re-cloned per call into the param-style
        // local the body reads. The body is NOT in a loop (a `break` cannot
        // cross the lambda).
        ExprKind::Lambda { params, body } => emit_lambda_expr(
            params,
            body,
            ExprEmitContext {
                src,
                aliases,
                locals,
                in_loop,
            },
        )?,
        // §8/§22.2 member access `obj.field` → the shared
        // `member_value_required` leaf: record fields, access-only properties,
        // string-`.length` faults, and every receiver builtin represented as a
        // bound value. Mutators retain their acquisition-time root check;
        // callback and resource values re-enter their runtime routes when
        // called. The span is the whole member expression's, matching the
        // interpreter's `KMember`.
        ExprKind::Member { object, field } => emit_member_expr(
            expr,
            object,
            field,
            ExprEmitContext {
                src,
                aliases,
                locals,
                in_loop,
            },
        )?,
        // §1 index `object[index]` → the shared `index_value` leaf (an array
        // by an `int`, with the out-of-bounds / not-indexable faults). The
        // object lowers before the index (the interpreter's order); the leaf
        // takes both by value; the fault span is the whole index expression's.
        ExprKind::Index { object, index } => emit_index_expr(
            expr,
            object,
            index,
            ExprEmitContext {
                src,
                aliases,
                locals,
                in_loop,
            },
        )?,
        // §8 record update `base { field: value, … }` → STAGE 1 checks the
        // base is a record (the `record_update_base` leaf, which runs BEFORE
        // the field values evaluate, matching the interpreter's order), STAGE 2
        // merges the evaluated fields through `record_update_merge` (only an
        // EXISTING field may be overridden — an unknown field faults
        // GUARD_NO_FIELD). Fields lower left to right.
        ExprKind::RecordUpdate { .. } => emit_record_update_expr(
            expr,
            ExprEmitContext {
                src,
                aliases,
                locals,
                in_loop,
            },
        )?,
        // §12 optional member access `object?.field` → the shared
        // `optional_member` leaf: `None`/`null` short-circuit (preserved), a
        // `Some(inner)` unwraps + accesses + re-wraps, any other value accesses
        // directly. Receiver builtin values use the same catalog binding as a
        // plain member; mutators apply their root check only on the non-empty
        // branch, and callback/resource routes remain attached to the value.
        ExprKind::OptionalAccess { object, field } => emit_optional_access_expr(
            expr,
            object,
            field,
            ExprEmitContext {
                src,
                aliases,
                locals,
                in_loop,
            },
        )?,
        // §13 the `?` operator `e?` (`ExprKind::Try`): the SHARED `try_value`
        // leaf makes the value decision — an `Ok` unwraps, a non-`Result`
        // faults (its `?` propagates the `RtError`), an `Err` is handed back for
        // EARLY-RETURN of the propagated value from the enclosing function. The
        // emitter supplies that control flow: `return Ok(__early)` returns from
        // the nearest `async move` block (the function/lambda body), exactly as
        // the interpreter's `KTry` unwinds a `Return` to the nearest function
        // boundary. A TOP-LEVEL `?` (outside any function/lambda) is refused at
        // `emit_entry_body` (it runtime-faults "return outside a function",
        // like a top-level `return`), so this `return` is only reached inside a
        // function/lambda body.
        ExprKind::Try(inner) => emit_try_expr(
            expr,
            inner,
            ExprEmitContext {
                src,
                aliases,
                locals,
                in_loop,
            },
        )?,
        // §11 function composition `f >> g` → a `Value::Composed((f, g))` (the
        // interpreter's `KComposePair`). The operands are evaluated left-to-right
        // (the Rust tuple's order = the interpreter's frame order) and are NOT
        // required to be callable here — callability is checked when the composed
        // value is CALLED (through `call_value`'s `Composed` arm), exactly as the
        // interpreter constructs `Composed` eagerly and dispatches on call.
        ExprKind::Compose { lhs, rhs } => emit_compose_expr(
            lhs,
            rhs,
            ExprEmitContext {
                src,
                aliases,
                locals,
                in_loop,
            },
        )?,
        // §11 a pipeline stage `lhs |> rhs`. The piped value is bound to a fresh
        // `__piped` FIRST (so the lhs → stage → args order matches the
        // interpreter's KPipe, which evaluates the lhs before scheduling the
        // stage). Two stage shapes lower here, both ending in `call_value`:
        //   • a NON-call stage `lhs |> f` → unary application `f(lhs)` (the
        //     interpreter's `KCallApplyWithArg`, at the PIPE span);
        //   • a CALL stage `lhs |> f(args)` → §11 first-argument insertion: the
        //     piped value is the call's FIRST positional, then the explicit
        //     positional args (the interpreter's `schedule_call(.., Some(lhs))`,
        //     at the CALL span).
        // A FIELD pipe `lhs |> .field` is §11 sugar for the pure member access
        // `lhs.field` — it lowers like the `Member` arm (the shared
        // member-value leaf at the PIPE span, including receiver builtin values).
        // A placeholder `_` in a call-stage argument binds `_`
        // to the piped value and skips first-argument insertion; a placeholder
        // in callee position is refused, as are named/spread stage arguments.
        // A stage whose callee is a free builtin
        // VALUE (`xs |> map(toInt)`) now lowers — `emit_expr` produces the
        // `Value::Builtin`, so the first-argument insertion dispatches it through
        // `call_value`. A bare CONSTRUCTOR name callee (`Some`/`Ok`/`Err`) is
        // still declined by `emit_expr` (a free identifier — not first-class).
        ExprKind::Pipe { lhs, rhs } => emit_pipe_expr(
            expr,
            lhs,
            rhs,
            ExprEmitContext {
                src,
                aliases,
                locals,
                in_loop,
            },
        )?,
        ExprKind::Placeholder => match lookup_bind(locals, "_") {
            Some(bind) => read_local("_", bind),
            None => return Err(EmitError::unsupported("placeholder outside pipe stage")),
        },
        // §15 `concurrent { a: e1, b: e2 }`. Each arm is a no-parameter closure capturing
        // its enclosing scope (lowered exactly like a lambda body, so a nested closure /
        // free identifier is analysed identically), called for its `CallFuture`. The
        // no-`timeout` JOIN form runs them with `concurrent_join`, collecting a record keyed
        // by arm name — the value `concurrent` evaluates to (§15). The `timeout`/`else`
        // deadline form (the parser guarantees the two appear together) parses the duration
        // literal to milliseconds, lowers the `else` block as its own no-parameter captured
        // closure, and runs `concurrent_join_timeout`: the round-robin with a deadline that,
        // on expiry with arms still pending, abandons them and yields the else value.
        ExprKind::Concurrent { .. } => emit_concurrent_expr(
            expr,
            ExprEmitContext {
                src,
                aliases,
                locals,
                in_loop,
            },
        )?,
        _ => return Err(EmitError::unsupported("expression kind")),
    })
}
