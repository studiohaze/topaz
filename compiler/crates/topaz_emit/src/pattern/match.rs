use crate::*;

/// Lower a `match` expression (§5) to a Rust block: bind the scrutinee
/// once, then test cases IN ORDER. A literal case tests `values_equal`
/// against the scrutinee (the cmp fault is mapped exactly as the interpreter's
/// `pat` does, at the match's span). The first IRREFUTABLE case (`_` or a
/// binding) is the catch-all and closes the chain (later cases are unreachable,
/// exactly as the interpreter, which matches the first case, would treat them).
/// With no catch-all, an unmatched scrutinee faults `FAULT_MATCH_MISS` — the
/// interpreter's §5 behavior.
/// The Rust expression for a §5 case-arm body: an `Expr` arm lowers to its
/// value; a `case … => return e` arm (`CaseArmBody::Return`) lowers to `return
/// Ok(e)` (a bare `return` to `return Ok(Value::Unit)`), returning from the
/// enclosing function's `async move` block — exactly the interpreter's `KReturn`
/// from a case arm. A `return` arm at the TOP LEVEL (a match outside any
/// function/lambda) is refused at `emit_entry_body` (`expr_has_bare_return`
/// flags a `CaseArmBody::Return`), so this `return` is only reached inside a
/// function/lambda body.
pub(crate) fn emit_case_arm_value(
    body: &CaseArmBody,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    locals: &[(String, Bind)],
    in_loop: bool,
) -> Result<String, EmitError> {
    match body {
        CaseArmBody::Expr(e) => emit_expr(e, src, aliases, locals, in_loop),
        CaseArmBody::Return { value, .. } => {
            let v = match value {
                Some(e) => emit_expr(e, src, aliases, locals, in_loop)?,
                None => "Value::Unit".to_string(),
            };
            // §14 a `case … => return e` crosses every enclosing BLOCK defer stack —
            // evaluate the value FIRST (into a temp), THEN drain inner→outer, THEN return
            // (same order as `StmtKind::Return`: a side-effecting/faulting value orders
            // before/avoids the defers). The function's `__defers` is drained by the
            // wrapper, so skip `stacks[..fn_base]`.
            let drain = {
                let f = aliases.flow.borrow();
                f.drain_from(f.fn_base)
            };
            if drain.is_empty() {
                Ok(format!("return Ok({v})"))
            } else {
                Ok(format!(
                    "{{ let __ret_v = {v}; {drain}return Ok(__ret_v) }}"
                ))
            }
        }
    }
}

impl MatchCaseEmission<'_, '_, '_> {
    /// Complete one refutable arm after its pattern-specific lowering has produced
    /// an `Option` of owned bindings. Structural patterns use this owned tuple to
    /// end any scrutinee borrow before the guard or body; or-patterns use the same
    /// boundary to carry the first matching alternative's bindings. A guard is a
    /// second phase over that tuple, and `None` continues the existing arm chain.
    pub(crate) fn emit_extracted_arm(
        self,
        scope: &[(String, Bind)],
        tuple: &str,
        extraction: &str,
        guard: Option<&Expr>,
    ) -> Result<bool, EmitError> {
        let body =
            emit_case_arm_value(&self.case.body, self.src, self.aliases, scope, self.in_loop)?;
        let guard = guard
            .map(|guard| emit_expr(guard, self.src, self.aliases, scope, self.in_loop))
            .transpose()?;
        let extraction = match guard {
            None => format!("{{ {extraction} }}"),
            Some(guard) => format!(
                "{{ let __m = {extraction}; if let Some({tuple}) = __m {{ if case_guard_bool(&({guard}), {span})? {{ Some({tuple}) }} else {{ None }} }} else {{ None }} }}",
                span = self.span,
            ),
        };
        self.arms.push_str(&format!(
            "if let Some({tuple}) = {extraction} {{ {body} }} else "
        ));
        Ok(false)
    }

    pub(crate) fn emit_simple_arm(
        self,
        scope: &[(String, Bind)],
        condition: Option<&str>,
        binding: Option<&str>,
        guard: Option<&Expr>,
    ) -> Result<bool, EmitError> {
        let body =
            emit_case_arm_value(&self.case.body, self.src, self.aliases, scope, self.in_loop)?;
        let guard = guard
            .map(|guard| emit_expr(guard, self.src, self.aliases, scope, self.in_loop))
            .transpose()?;
        let body = match binding {
            Some(binding) => format!("{binding} {body}"),
            None => body,
        };
        if condition.is_none() && guard.is_none() {
            self.arms.push_str(&format!("{{ {body} }}"));
            return Ok(true);
        }
        let mut test = condition.unwrap_or_default().to_string();
        if let Some(guard) = guard {
            if !test.is_empty() {
                test.push_str(" && ");
            }
            let guard = format!("case_guard_bool(&({guard}), {span})?", span = self.span,);
            let guard = match binding {
                Some(binding) => format!("{{ {binding} {guard} }}"),
                None => guard,
            };
            test.push_str(&guard);
        }
        self.arms.push_str(&format!("if {test} {{ {body} }} else "));
        Ok(false)
    }

    /// Emit an ordinary catch-all or typed single-binding arm. The optional
    /// condition is absent only for the irrefutable ordinary binding.
    pub(crate) fn emit_single_binding(
        self,
        bound: &str,
        condition: Option<&str>,
        guard: Option<&Expr>,
    ) -> Result<bool, EmitError> {
        let mut scope = self.locals.to_vec();
        scope.push((bound.to_string(), Bind::Imm));
        let rust_name = mangle(bound);
        let bind = format!("let {rust_name} = __scrut.clone();");
        self.emit_simple_arm(&scope, condition, Some(&bind), guard)
    }

    /// Emit a binding-free arm. The optional condition is absent only for the
    /// irrefutable wildcard.
    pub(crate) fn emit_binding_free(
        self,
        condition: Option<String>,
        guard: Option<&Expr>,
    ) -> Result<bool, EmitError> {
        let scope = self.locals;
        self.emit_simple_arm(scope, condition.as_deref(), None, guard)
    }

    pub(crate) fn emit_literal(
        self,
        literal: &Expr,
        guard: Option<&Expr>,
    ) -> Result<bool, EmitError> {
        let condition = literal_pattern_condition(
            literal,
            "__scrut",
            self.span,
            self.src,
            self.aliases,
            self.locals,
            self.in_loop,
        )
        .map_err(|error| error.at(self.case.pattern.span))?;
        self.emit_binding_free(Some(condition), guard)
    }

    pub(crate) fn emit_range(
        self,
        lo: &Expr,
        hi: &Expr,
        inclusive: bool,
        guard: Option<&Expr>,
    ) -> Result<bool, EmitError> {
        let condition = range_pattern_condition(lo, hi, inclusive, "__scrut", self.src)
            .map_err(|error| error.at(self.case.pattern.span))?;
        self.emit_binding_free(Some(condition), guard)
    }

    pub(crate) fn emit_or(
        self,
        alternatives: &[Pattern],
        guard: Option<&Expr>,
    ) -> Result<bool, EmitError> {
        let prepared = prepare_or_pattern(
            alternatives,
            "__scrut",
            self.src,
            self.aliases,
            self.span,
            self.locals,
            self.in_loop,
        )
        .map_err(|error| error.at(self.case.pattern.span))?;
        let canonical = &prepared.bound_names;
        let tuple = &prepared.binding_tuple;
        let mut scope = self.locals.to_vec();
        for name in canonical {
            scope.push((name.clone(), Bind::Imm));
        }
        let chain = &prepared.first_match_chain;
        self.emit_extracted_arm(&scope, tuple, chain, guard)
    }

    pub(crate) fn emit_binding(
        self,
        name: &Ident,
        guard: Option<&Expr>,
    ) -> Result<bool, EmitError> {
        let bound = text(self.src, name.span);
        let owners = enums_declaring_variant(self.aliases, bound);
        if owners.is_empty() {
            return self.emit_single_binding(bound, None, guard);
        }

        let mangled = mangle(bound);
        let owners_list = owners.join(", ");
        let is_owner = format!(
            "matches!(&__scrut, Value::Enum {{ enum_id, declaration_identity, .. }} if [{owners_list}].contains(&nominal_declaration_identity(enum_id.as_ref(), declaration_identity.as_deref())))"
        );
        let tag_match = format!(
            "matches!(&__scrut, Value::Enum {{ enum_id, declaration_identity, variant, .. }} if [{owners_list}].contains(&nominal_declaration_identity(enum_id.as_ref(), declaration_identity.as_deref())) && variant.as_ref() == {bound:?})"
        );
        let owner_body = emit_case_arm_value(
            &self.case.body,
            self.src,
            self.aliases,
            self.locals,
            self.in_loop,
        )?;
        let mut binding_scope = self.locals.to_vec();
        binding_scope.push((bound.to_string(), Bind::Imm));
        let binding_body = emit_case_arm_value(
            &self.case.body,
            self.src,
            self.aliases,
            &binding_scope,
            self.in_loop,
        )?;
        if let Some(guard) = guard {
            let owner_guard = emit_expr(guard, self.src, self.aliases, self.locals, self.in_loop)?;
            let binding_guard =
                emit_expr(guard, self.src, self.aliases, &binding_scope, self.in_loop)?;
            self.arms.push_str(&format!(
                "if {tag_match} && case_guard_bool(&({owner_guard}), {span})? {{ {owner_body} }} else if !{is_owner} && {{ let {mangled} = __scrut.clone(); case_guard_bool(&({binding_guard}), {span})? }} {{ let {mangled} = __scrut.clone(); {binding_body} }} else ",
                span = self.span,
            ));
        } else {
            self.arms.push_str(&format!(
                "if {tag_match} {{ {owner_body} }} else if !{is_owner} {{ let {mangled} = __scrut.clone(); {binding_body} }} else "
            ));
        }
        Ok(false)
    }

    pub(crate) fn finish_scoped_pattern(
        self,
        scope: Vec<(String, Bind)>,
        conditions: Vec<String>,
        bindings: Vec<String>,
        guard: Option<&Expr>,
    ) -> Result<bool, EmitError> {
        let bound: Vec<String> = scope[self.locals.len()..]
            .iter()
            .map(|(name, _)| mangle(name))
            .collect();
        // A duplicate pattern binding (`case [x, x]`) would emit a `(x, x)` Rust
        // tuple pattern. The checker rejects duplicates; unchecked emission refuses
        // them here instead of producing Rust that does not compile.
        ensure_distinct_binding_names(bound.iter().map(String::as_str))?;
        let tuple = rust_binding_tuple(bound.iter().map(String::as_str));
        let condition = conditions.join(" && ");
        let bindings = bindings.join(" ");
        let extraction = format!("if {condition} {{ {bindings} Some({tuple}) }} else {{ None }}");
        self.emit_extracted_arm(&scope, &tuple, &extraction, guard)
    }

    pub(crate) fn emit_scoped_pattern(self, guard: Option<&Expr>) -> Result<bool, EmitError> {
        let mut scope = self.locals.to_vec();
        let mut counter = 0usize;
        let (conditions, bindings) = SubpatternEmitter::new(
            self.src,
            self.aliases,
            &mut scope,
            self.span,
            &mut counter,
            self.in_loop,
            self.locals,
        )
        .emit(&self.case.pattern, "__scrut")?;
        self.finish_scoped_pattern(scope, conditions, bindings, guard)
    }

    pub(crate) fn emit_typed(
        self,
        name: &Ident,
        ty: &Type,
        guard: Option<&Expr>,
    ) -> Result<bool, EmitError> {
        let mut type_counter = 0u32;
        let test = type_test(
            ty,
            self.src,
            "&__scrut",
            &mut type_counter,
            self.aliases,
            self.locals,
            &mut Vec::new(),
        )
        .ok_or_else(|| EmitError::unsupported("typed pattern type").at(self.case.pattern.span))?;
        let bound = text(self.src, name.span);
        self.emit_single_binding(bound, Some(&test), guard)
    }

    pub(crate) fn emit(self) -> Result<bool, EmitError> {
        let case = self.case;
        // A §5 `case` GUARD `if cond`: the case matches only when the pattern
        // matches AND the guard is `true`; a `false` guard FALLS THROUGH to the
        // next case (so a guarded wildcard/binding no longer closes the chain),
        // and a non-`bool` guard faults through the SHARED `case_guard_bool`
        // leaf at the match span (the interpreter's `KMatchGuard`). The guard
        // is evaluated only AFTER the pattern matches (the `&&` short-circuit /
        // the binding's `if`), so its own faults/effects mirror the
        // interpreter's order.
        let guard = case.guard.as_ref();
        match &case.pattern.kind {
            PatternKind::Literal(literal) => self.emit_literal(literal, guard),
            PatternKind::Wildcard => self.emit_binding_free(None, guard),
            PatternKind::Binding(name) => self.emit_binding(name, guard),
            // Every structural top-level pattern uses the same recursive dispatcher as
            // nested subpatterns; failed shapes and false guards fall through.
            PatternKind::Constructor { .. }
            | PatternKind::List(_)
            | PatternKind::Record(_)
            | PatternKind::NominalRecord { .. } => self.emit_scoped_pattern(guard),
            PatternKind::Range { lo, hi, inclusive } => self.emit_range(lo, hi, *inclusive, guard),
            // Or alternatives use independent test-and-bind blocks. The first
            // match wins; the checker guarantees a shared binding shape.
            PatternKind::Or(alternatives) => self.emit_or(alternatives, guard),
            PatternKind::Typed { name, ty } => self.emit_typed(name, ty, guard),
        }
    }
}

pub(crate) fn emit_match(
    scrutinee: &Expr,
    cases: &[CaseClause],
    match_span: Span,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    locals: &[(String, Bind)],
    in_loop: bool,
) -> Result<String, EmitError> {
    let scrut_rs = emit_expr(scrutinee, src, aliases, locals, in_loop)?;
    let span = emit_span(match_span);
    let mut arms = String::new();
    let mut closed = false;
    for case in cases {
        if (MatchCaseEmission {
            case,
            span: &span,
            src,
            aliases,
            locals,
            in_loop,
            arms: &mut arms,
        })
        .emit()?
        {
            closed = true;
            break;
        }
    }
    if !closed {
        arms.push_str(&format!(
            "{{ return Err(fault(codes::FAULT_MATCH_MISS, {:?}, {})); }}",
            "no `case` matched and no catch-all exists (§5)",
            emit_span(match_span)
        ));
    }
    Ok(format!("{{ let __scrut = {scrut_rs}; {arms} }}"))
}
