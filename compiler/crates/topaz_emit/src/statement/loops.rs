use crate::*;

pub(crate) fn emit_loop_control(
    emission: LoopControlEmission<'_, '_, '_>,
) -> Result<(), EmitError> {
    let LoopControlEmission {
        stmt,
        kind,
        label,
        src,
        aliases,
        locals,
        in_loop,
        lines,
    } = emission;
    let is_break = matches!(kind, LoopControlKind::Break(_));
    let label_text = label.map(|identifier| text(src, identifier.span));
    if !in_loop && label_text.is_none() {
        let message = if is_break {
            "break outside loop"
        } else {
            "continue outside loop"
        };
        return Err(EmitError::unsupported(message).at(stmt.span));
    }
    let target = aliases.flow.borrow().loop_target(label_text);
    let Some(index) = target else {
        let message = if label_text.is_some() && is_break {
            "break to a loop label not in scope"
        } else if label_text.is_some() {
            "continue to a loop label not in scope"
        } else if is_break {
            "break outside loop"
        } else {
            "continue outside loop"
        };
        return Err(EmitError::unsupported(message).at(stmt.span));
    };
    let (drain, frame) = {
        let flow = aliases.flow.borrow();
        (
            flow.drain_from(flow.loop_markers[index]),
            flow.loop_frames[index].clone(),
        )
    };

    match (kind, frame) {
        (LoopControlKind::Break(value), LoopFrameKind::Value { rust_label, .. }) => {
            let value = match value {
                Some(value) => emit_expr(value, src, aliases, locals, in_loop)?,
                None => "Value::Unit".to_string(),
            };
            lines.push_str(&format!(
                "    {{ let __brk = {value}; {drain}break {rust_label} __brk; }}\n"
            ));
        }
        (LoopControlKind::Break(value), LoopFrameKind::Plain) => {
            if let Some(value) = value {
                let value = emit_expr(value, src, aliases, locals, in_loop)?;
                lines.push_str(&format!("    let _ = {value};\n"));
            }
            lines.push_str(&format!("    {drain}break;\n"));
        }
        (LoopControlKind::Continue, LoopFrameKind::Value { rust_label, .. }) => {
            lines.push_str(&format!("    {drain}continue {rust_label};\n"));
        }
        (LoopControlKind::Continue, LoopFrameKind::Plain) => {
            lines.push_str(&format!("    {drain}continue;\n"));
        }
    }
    Ok(())
}

pub(crate) fn emit_while_statement(
    stmt: &Stmt,
    condition: &Expr,
    body: &Block,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    locals: &[(String, Bind)],
) -> Result<String, EmitError> {
    let condition = emit_expr(condition, src, aliases, locals, false)?;
    let body = emit_loop_body(body, src, aliases, locals)?;
    Ok(format!(
        "while condition_bool(&{condition}, \"while\", {})? {body}",
        emit_span(stmt.span)
    ))
}

pub(crate) fn emit_block(
    block: &Block,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    locals: &[(String, Bind)],
    in_loop: bool,
) -> Result<String, EmitError> {
    emit_block_with_mode(block, src, aliases, locals, in_loop, true)
}

/// Lower a source block used as an ordinary expression. A statement-free block
/// has no bindings or control-transfer boundary of its own, so its tail can be
/// emitted directly. Rust constructs whose grammar requires a block continue to
/// use [`emit_block`] and retain braces.
pub(crate) fn emit_block_expr(
    block: &Block,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    locals: &[(String, Bind)],
    in_loop: bool,
) -> Result<String, EmitError> {
    emit_block_with_mode(block, src, aliases, locals, in_loop, false)
}

pub(crate) fn emit_block_with_mode(
    block: &Block,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    locals: &[(String, Bind)],
    in_loop: bool,
    require_braces: bool,
) -> Result<String, EmitError> {
    let mut scope = locals.to_vec();
    let base = scope.len();
    let (lines, result) = emit_stmt_seq(StatementSequenceEmission {
        stmts: &block.stmts,
        tail: block.tail.as_deref(),
        src,
        aliases,
        locals: &mut scope,
        base,
        in_loop,
        defer_scope: false,
        at_module_top: false,
    })?;
    if require_braces || !lines.trim().is_empty() {
        Ok(format!("{{ {lines}{result} }}"))
    } else {
        Ok(result)
    }
}

/// The statement-position discard of a loop body's value. `while`/`for`
/// are statements, so the body value is thrown away — but a body with no
/// tail is already `Value::Unit` with no side effect, so emit nothing
/// rather than a dead `let _ = Value::Unit;`. A tail expression still
/// lowers to `let _ = …;` so its effects run without a dead Unit store.
pub(crate) fn loop_discard(tail: Option<&Expr>, result: &str) -> String {
    match tail {
        None => String::new(),
        Some(_) => format!("let _ = {result}; "),
    }
}

/// Lower a loop body to a Rust block evaluating to `()`. A `while`/`for`
/// body is a STATEMENT, so its value is discarded (the interpreter
/// pushes `KDiscard` after the body) — a tail, if any, lowers to a
/// `let _ = …;` (its effects still run). It is `in_loop` (so
/// `break`/`continue` are legal), and has its OWN lexical scope (a COPY of
/// the visible bindings), fresh per iteration like the interpreter's
/// per-iteration child env.
pub(crate) fn emit_loop_body(
    block: &Block,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    locals: &[(String, Bind)],
) -> Result<String, EmitError> {
    let mut scope = locals.to_vec();
    let base = scope.len();
    // §14 mark the block-defer stack depth at the loop-body entry — a `break`/`continue`
    // inside drains only the stacks opened at/after this marker (the body's own scopes).
    let marker = aliases.flow.borrow().stacks.len();
    {
        let mut f = aliases.flow.borrow_mut();
        f.loop_markers.push(marker);
        // A `while` is an unlabeled, value-less loop.
        f.loop_frames.push(LoopFrameKind::Plain);
    }
    let seq = emit_stmt_seq(StatementSequenceEmission {
        stmts: &block.stmts,
        tail: block.tail.as_deref(),
        src,
        aliases,
        locals: &mut scope,
        base,
        in_loop: true,
        defer_scope: false,
        at_module_top: false,
    });
    {
        let mut f = aliases.flow.borrow_mut();
        f.loop_markers.pop();
        f.loop_frames.pop();
    }
    let (lines, result) = seq?;
    // §15 a cooperative YIELD at the `while` back-edge, placed at the body's START so it
    // is reached on EVERY iteration — INCLUDING after a `continue`, which jumps to the
    // condition and re-enters the body here (a body-END checkpoint a `continue` would
    // skip). A long-running (even infinite, even `while true { continue }`) loop thus
    // suspends each iteration so the round-robin concurrent scheduler can advance sibling
    // arms. Under `block_on` it is a transparent re-poll, so the loop is unobservably
    // unchanged for non-concurrent programs.
    Ok(format!(
        "{{ checkpoint().await; {lines}{discard} }}",
        discard = loop_discard(block.tail.as_deref(), &result),
    ))
}

/// Lower a `loop (label)? { body }` expression to a labeled Rust `loop`.
/// The loop expression's VALUE is the break value: `'lN: loop { ... }` yields
/// whatever a `break 'lN <value>` carries (`Value::Unit` for a value-less break),
/// exactly mirroring the interpreter's `LoopExprBody` boundary. The body is a
/// per-iteration STATEMENT scope (its tail value discarded) with a cooperative
/// `checkpoint().await` at the start (every iteration, like `while`), and is
/// `in_loop` so its `break`/`continue` are legal. The unique `'lN` Rust label is
/// pushed onto the flow's `loop_frames` (in lockstep with `loop_markers`) so a
/// `break`/`continue` targeting this loop — labeled or innermost — finds it.
pub(crate) fn emit_loop(
    label: Option<Ident>,
    body: &Block,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    locals: &[(String, Bind)],
) -> Result<String, EmitError> {
    let src_label = label.map(|l| text(src, l.span).to_string());
    let rust_label = {
        let mut f = aliases.flow.borrow_mut();
        let id = f.next_loop_label;
        f.next_loop_label += 1;
        format!("'l{id}")
    };
    let mut scope = locals.to_vec();
    let base = scope.len();
    // §14 mark the block-defer stack depth at the loop-body entry (as `while`/`for`).
    let marker = aliases.flow.borrow().stacks.len();
    {
        let mut f = aliases.flow.borrow_mut();
        f.loop_markers.push(marker);
        f.loop_frames.push(LoopFrameKind::Value {
            src_label,
            rust_label: rust_label.clone(),
        });
    }
    let seq = emit_stmt_seq(StatementSequenceEmission {
        stmts: &body.stmts,
        tail: body.tail.as_deref(),
        src,
        aliases,
        locals: &mut scope,
        base,
        in_loop: true,
        defer_scope: false,
        at_module_top: false,
    });
    {
        let mut f = aliases.flow.borrow_mut();
        f.loop_markers.pop();
        f.loop_frames.pop();
    }
    let (lines, result) = seq?;
    // The body value is DISCARDED (a `loop` exits only via `break`/`return`/`?`);
    // a tail still lowers to `let _ = …;` so its effects run. The `checkpoint().await`
    // is the same cooperative yield `while` places at the body start.
    Ok(format!(
        "{rust_label}: loop {{ checkpoint().await; {lines}{discard} }}",
        discard = loop_discard(body.tail.as_deref(), &result),
    ))
}

/// Render the Rust tuple that carries owned pattern bindings out of a guard block.
/// The one-element trailing comma is semantic: without it Rust parses parentheses,
/// not a tuple pattern/value. All refutable binding routes share this 0/1/N shape.
pub(crate) fn rust_binding_tuple<I, S>(names: I) -> String
where
    I: ExactSizeIterator<Item = S>,
    S: AsRef<str>,
{
    let singleton = names.len() == 1;
    let mut tuple = String::from("(");
    for (index, name) in names.enumerate() {
        if index != 0 {
            tuple.push_str(", ");
        }
        tuple.push_str(name.as_ref());
    }
    if singleton {
        tuple.push(',');
    }
    tuple.push(')');
    tuple
}

/// Refutable pattern lowering may run after the checker under `--unchecked`.
/// Refuse duplicate names before they become an invalid Rust tuple pattern.
pub(crate) fn ensure_distinct_binding_names<'a, I>(names: I) -> Result<(), EmitError>
where
    I: Clone + Iterator<Item = &'a str>,
{
    let mut remaining = names;
    while let Some(name) = remaining.next() {
        if remaining.clone().any(|candidate| candidate == name) {
            return Err(EmitError::unsupported("same-scope redeclaration"));
        }
    }
    Ok(())
}

/// Prepare the binding that every statement, expression, or comprehension for-loop
/// applies to one item. This is the single owner of iteration-pattern admission,
/// immutable scope extension, refutable extraction, and the common miss fault.
pub(crate) fn prepare_for_pattern(
    emission: ForPatternEmission<'_, '_, '_>,
    scope: &mut Vec<(String, Bind)>,
    existing_refutable_locals: Option<&[(String, Bind)]>,
) -> Result<ForPatternBinding, EmitError> {
    let ForPatternEmission {
        pattern,
        src,
        aliases,
        span,
        in_loop,
        typed_unsupported,
    } = emission;
    let base = scope.len();
    if matches!(
        pattern.kind,
        PatternKind::List(_)
            | PatternKind::Record(_)
            | PatternKind::NominalRecord { .. }
            | PatternKind::Constructor { .. }
            | PatternKind::Range { .. }
            | PatternKind::Or(_)
            | PatternKind::Literal(_)
    ) {
        // Subpattern lowering reads the already-visible bindings while appending the
        // new iteration bindings. A standalone for can borrow its caller's original
        // scope; a comprehension snapshots its accumulated clause scope once here.
        let locals_snapshot;
        let locals = if let Some(existing) = existing_refutable_locals {
            existing
        } else {
            locals_snapshot = scope.clone();
            &locals_snapshot
        };
        let mut counter = 0usize;
        let (conditions, bindings) =
            SubpatternEmitter::new(src, aliases, scope, span, &mut counter, in_loop, locals)
                .emit(pattern, "__loop")?;
        let bound = &scope[base..];
        ensure_distinct_binding_names(bound.iter().map(|(name, _)| name.as_str()))?;
        let condition = if conditions.is_empty() {
            "true".to_string()
        } else {
            conditions.join(" && ")
        };
        let bindings = bindings.join(" ");
        let tuple = rust_binding_tuple(bound.iter().map(|(name, _)| mangle(name)));
        let miss = format!(
            "return Err(fault(codes::GUARD_TYPE, {message:?}, {span}));",
            message = "`for` pattern did not match an element",
        );
        // The guard block returns owned bindings so any RefCell borrows from
        // refutable matching end before the loop body can mutate an alias.
        let extraction = format!(
            "let {tuple} = {{ if {condition} {{ {bindings} {tuple} }} else {{ {miss} }} }}; "
        );
        return Ok(ForPatternBinding {
            loop_variable: "__loop".to_string(),
            prelude: extraction,
        });
    }

    if let PatternKind::Typed { name, ty } = &pattern.kind {
        let mut type_counter = 0u32;
        let test = type_test(
            ty,
            src,
            "&__loop",
            &mut type_counter,
            aliases,
            scope,
            &mut Vec::new(),
        )
        .ok_or_else(|| EmitError::unsupported(typed_unsupported).at(pattern.span))?;
        let bound = text(src, name.span);
        scope.push((bound.to_string(), Bind::Imm));
        let check = format!(
            "let {mangled} = {{ if {test} {{ __loop }} else {{ return Err(fault(codes::GUARD_TYPE, {message:?}, {span})); }} }}; ",
            mangled = mangle(bound),
            message = "`for` pattern did not match an element",
        );
        return Ok(ForPatternBinding {
            loop_variable: "__loop".to_string(),
            prelude: check,
        });
    }

    let (loop_variable, bound) = for_loop_var(pattern, src)?;
    if let Some(name) = bound {
        scope.push((name.to_string(), Bind::Imm));
    }
    Ok(ForPatternBinding {
        loop_variable,
        prelude: String::new(),
    })
}

pub(crate) fn emit_for(emission: ForEmission<'_, '_, '_>) -> Result<String, EmitError> {
    let ForEmission {
        pattern,
        iter,
        body,
        span: for_span,
        src,
        aliases,
        locals,
        collect,
    } = emission;
    let iter_rs = emit_expr(iter, src, aliases, locals, false)?;
    let span = emit_span(for_span);
    let mut scope = locals.to_vec();
    // The iteration binding and body top-level lets share one scope, matching the
    // interpreter per-iteration environment and its same-scope redeclaration rule.
    let base = scope.len();
    let ForPatternBinding {
        loop_variable,
        prelude,
    } = prepare_for_pattern(
        ForPatternEmission {
            pattern,
            src,
            aliases,
            span: &span,
            in_loop: !collect,
            typed_unsupported: "typed for type",
        },
        &mut scope,
        Some(locals),
    )?;

    // A statement for is an unlabeled value-less loop-control target. An expression
    // for collects values and therefore does not admit bare break or continue.
    let marker = aliases.flow.borrow().stacks.len();
    if !collect {
        let mut flow = aliases.flow.borrow_mut();
        flow.loop_markers.push(marker);
        flow.loop_frames.push(LoopFrameKind::Plain);
    }
    let sequence = emit_stmt_seq(StatementSequenceEmission {
        stmts: &body.stmts,
        tail: body.tail.as_deref(),
        src,
        aliases,
        locals: &mut scope,
        base,
        in_loop: !collect,
        defer_scope: false,
        at_module_top: false,
    });
    if !collect {
        let mut flow = aliases.flow.borrow_mut();
        flow.loop_markers.pop();
        flow.loop_frames.pop();
    }
    let (lines, result) = sequence?;
    let iter_call = format!("for_items(&({iter_rs}), {span})?");
    Ok(if collect {
        format!(
            "{{ let mut __acc = Vec::new(); for {loop_variable} in {iter_call} {{ {prelude}{lines}__acc.push({result}); }} Value::array(__acc) }}"
        )
    } else {
        format!(
            "for {loop_variable} in {iter_call} {{ {prelude}{lines}{discard} }}",
            discard = loop_discard(body.tail.as_deref(), &result),
        )
    })
}

pub(crate) fn emit_comp_clauses(
    clauses: &[CompClause],
    scope: &mut Vec<(String, Bind)>,
    emission: ComprehensionEmission<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ComprehensionEmission {
        body,
        kind,
        span: comp_span,
        src,
        aliases,
    } = emission;
    let span = emit_span(comp_span);
    let Some((clause, rest)) = clauses.split_first() else {
        // Base case: append the body. Map keys/values and array/set elements all
        // lower in source order (left-to-right faults), exactly as the interpreter.
        return Ok(match (kind, body) {
            (CompKind::Map, CompBody::Entry { key, value }) => {
                let key = emit_expr(key, src, aliases, scope, false)?;
                let value = emit_expr(value, src, aliases, scope, false)?;
                format!("__cacc.push(({key}, {value}));")
            }
            (_, CompBody::Elem(element)) => {
                let element = emit_expr(element, src, aliases, scope, false)?;
                format!("__cacc.push({element});")
            }
            // The parser pairs map comprehensions with entries and array/set
            // comprehensions with elements.
            _ => unreachable!("comprehension kind/body shape paired by the parser"),
        });
    };
    match clause {
        CompClause::If(condition) => {
            let condition = emit_expr(condition, src, aliases, scope, false)?;
            let inner = emit_comp_clauses(rest, scope, emission)?;
            Ok(format!(
                "if condition_bool(&{condition}, \"if\", {span})? {{ {inner} }}"
            ))
        }
        CompClause::For { pattern, iter } => {
            // The iterable is lowered before the clause adds its iteration binding.
            let iter = emit_expr(iter, src, aliases, scope, false)?;
            let iter_call = format!("for_items(&({iter}), {span})?");
            let base = scope.len();
            let ForPatternBinding {
                loop_variable,
                prelude,
            } = prepare_for_pattern(
                ForPatternEmission {
                    pattern,
                    src,
                    aliases,
                    span: &span,
                    in_loop: false,
                    typed_unsupported: "typed comprehension for type",
                },
                scope,
                None,
            )?;
            let inner = emit_comp_clauses(rest, scope, emission)?;
            scope.truncate(base);
            Ok(format!(
                "for {loop_variable} in {iter_call} {{ {prelude}{inner} }}"
            ))
        }
    }
}

/// The Rust loop-variable pattern for a `for` loop and the Topaz name it
/// binds (if any): a simple identifier → its mangled local; `_` → the
/// Rust wildcard, binding nothing. Refutable and typed patterns are consumed by
/// `prepare_for_pattern` before this simple binding-only fallback.
pub(crate) fn for_loop_var<'a>(
    pattern: &'a Pattern,
    src: &'a LoweredText,
) -> Result<(String, Option<&'a str>), EmitError> {
    match binding_name(pattern, src)? {
        Some(name) => Ok((mangle(name), Some(name))),
        None => Ok(("_".to_string(), None)),
    }
}

/// A §6 range-pattern endpoint as an `i64`: an `int` literal or a NEGATED `int`
/// literal (`-5`, a `Unary` minus the parser does not fold), matching the
/// interpreter's `literal_value` (which evaluates `-int`). A non-int / non-literal
/// endpoint (a const-expression) is `None` (the caller refuses it — a later
/// slice).
pub(crate) fn range_endpoint(expr: &Expr, src: &LoweredText) -> Option<i64> {
    match &expr.kind {
        ExprKind::Int => text(src, expr.span).parse().ok(),
        ExprKind::Unary {
            op: UnaryOp::Minus,
            operand,
        } if matches!(operand.kind, ExprKind::Int) => {
            text(src, operand.span).parse::<i64>().ok().map(|n| -n)
        }
        _ => None,
    }
}

/// A numeric-literal constant: an `int`/`float` literal, or a `- +` / parenthesized
/// wrapping of one (recursive). Used to gate arithmetic unary defaults so they cannot
/// type-fault at `unary_value` (`-"x"`, `-null`, `-()` are NOT numeric and are rejected).
pub(crate) fn is_numeric_literal_const(d: &Expr) -> bool {
    match &d.kind {
        ExprKind::Int | ExprKind::Float => true,
        ExprKind::Unary {
            op: UnaryOp::Minus | UnaryOp::Plus,
            operand,
        } => is_numeric_literal_const(operand),
        ExprKind::Paren(e) => is_numeric_literal_const(e),
        _ => false,
    }
}

/// A bool-literal constant: a `bool` literal, or a `!` / parenthesized wrapping of one
/// (recursive). Gates logical-not defaults so they cannot type-fault (`!1`, `!"x"`,
/// `!null` are NOT bool and are rejected).
pub(crate) fn is_bool_literal_const(d: &Expr) -> bool {
    match &d.kind {
        ExprKind::Bool(_) => true,
        ExprKind::Unary {
            op: UnaryOp::Not,
            operand,
        } => is_bool_literal_const(operand),
        ExprKind::Paren(e) => is_bool_literal_const(e),
        _ => false,
    }
}
