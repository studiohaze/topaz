use crate::*;

pub(crate) fn is_pure_literal_default(d: &Expr) -> bool {
    match &d.kind {
        ExprKind::Int | ExprKind::Float | ExprKind::Bool(_) | ExprKind::Unit | ExprKind::Null => {
            true
        }
        ExprKind::String(s) => {
            s.tag.is_none()
                && !s
                    .parts
                    .iter()
                    .any(|p| matches!(p, StringPart::Interpolation(_)))
        }
        ExprKind::Unary {
            op: UnaryOp::Minus | UnaryOp::Plus,
            operand,
        } => is_numeric_literal_const(operand),
        ExprKind::Unary {
            op: UnaryOp::Not,
            operand,
        } => is_bool_literal_const(operand),
        ExprKind::Paren(e) => is_pure_literal_default(e),
        _ => false,
    }
}

pub(crate) fn literal_pattern_condition(
    literal: &Expr,
    access: &str,
    span: &str,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    locals: &[(String, Bind)],
    in_loop: bool,
) -> Result<String, EmitError> {
    if let ExprKind::String(string) = &literal.kind
        && string
            .parts
            .iter()
            .any(|part| matches!(part, StringPart::Interpolation(_)))
    {
        return Err(EmitError::unsupported("interpolation in pattern literal"));
    }
    let literal = emit_expr(literal, src, aliases, locals, in_loop)?;
    Ok(format!(
        "values_equal(&({literal}), &{access}).map_err(|e| cmp_guard(e, {span}))?"
    ))
}

pub(crate) fn range_pattern_condition(
    lo: &Expr,
    hi: &Expr,
    inclusive: bool,
    access: &str,
    src: &LoweredText,
) -> Result<String, EmitError> {
    let (Some(lo), Some(hi)) = (range_endpoint(lo, src), range_endpoint(hi, src)) else {
        return Err(EmitError::unsupported("range pattern endpoint shape"));
    };
    let comparison = if inclusive { "<=" } else { "<" };
    Ok(format!(
        "matches!(&{access}, Value::Int(__v) if *__v >= {lo} && *__v {comparison} {hi})"
    ))
}

/// Lower ONE §6 OR-pattern ALTERNATIVE (against `access`) to `(cond,
/// bind_lines, bound_names)` — the test, the `let` bind statements, and the
/// (user) names it binds. (v5.4) an alternative MAY bind names; the per-alternative
/// blocks are chained in `emit_match` so the first matching alternative captures
/// THAT alternative's bindings. A binding-free alternative (`1 | 2 | 3`, `_`,
/// `Some(_)`) returns an empty bind/bound set, so the non-binding or-pattern lowers
/// exactly as before. Every other form routes through, or mirrors, the SAME
/// recursive `emit_subpattern` machinery as a constructor arm — including a
/// nested or-subpattern — so the per-alternative tests/binds are byte-identical
/// to the interpreter's `pat`.
pub(crate) fn emit_or_alternative(
    alt: &Pattern,
    access: &str,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    span: &str,
    locals: &[(String, Bind)],
    in_loop: bool,
) -> Result<(String, Vec<String>, Vec<String>), EmitError> {
    // Locate any refusal at the offending alternative pattern (`alt.span` — NOT
    // the `span: &str` arg, which is emitted-code text).
    emit_or_alternative_inner(alt, access, src, aliases, span, locals, in_loop)
        .map_err(|e| e.at(alt.span))
}

pub(crate) fn emit_or_alternative_inner(
    alt: &Pattern,
    access: &str,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    span: &str,
    locals: &[(String, Bind)],
    in_loop: bool,
) -> Result<(String, Vec<String>, Vec<String>), EmitError> {
    match &alt.kind {
        // A `_` alternative makes the whole or-pattern irrefutable (it binds
        // nothing, so the test is simply `true`).
        PatternKind::Wildcard => Ok(("true".to_string(), Vec::new(), Vec::new())),
        // A RANGE alternative (`lo..hi` / `lo..<hi`) — the same `matches!`
        // int-in-range test as a top-level range arm; binds nothing.
        PatternKind::Range { lo, hi, inclusive } => Ok((
            range_pattern_condition(lo, hi, *inclusive, access, src)?,
            Vec::new(),
            Vec::new(),
        )),
        // Every other alternative form routes through the SAME subpattern compiler
        // as a constructor arm (so the tests/binds match the interpreter): a fresh
        // scope captures the names this alternative binds, the conds join into the
        // alternative's test, and the bind statements run only when it matched. This
        // deliberately includes PatternKind::Or, which lets nested or-subpatterns
        // reuse the same first-match-wins bound-tuple extraction recursively.
        _ => {
            let mut scope = locals.to_vec();
            let mut counter = 0usize;
            let (conds, binds) = SubpatternEmitter::new(
                src,
                aliases,
                &mut scope,
                span,
                &mut counter,
                in_loop,
                locals,
            )
            .emit(alt, access)?;
            let cond = if conds.is_empty() {
                "true".to_string()
            } else {
                conds.join(" && ")
            };
            let bound: Vec<String> = scope[locals.len()..]
                .iter()
                .map(|(n, _)| n.clone())
                .collect();
            Ok((cond, binds, bound))
        }
    }
}

/// Prepare one §6 or-pattern for either top-level or recursive nested extraction.
/// This is the single owner of alternative lowering, duplicate-name admission,
/// canonical binding order, equal-name-set admission, and first-match chain inputs.
pub(crate) fn prepare_or_pattern(
    alternatives: &[Pattern],
    access: &str,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    span: &str,
    locals: &[(String, Bind)],
    in_loop: bool,
) -> Result<OrPatternPreparation, EmitError> {
    let mut prepared = Vec::with_capacity(alternatives.len());
    let mut canonical = None;
    for alternative in alternatives {
        let (condition, bindings, mut bound_names) =
            emit_or_alternative(alternative, access, src, aliases, span, locals, in_loop)?;
        ensure_distinct_binding_names(bound_names.iter().map(String::as_str))?;
        bound_names.sort();
        match canonical.as_ref() {
            None => canonical = Some(bound_names),
            Some(expected) if expected != &bound_names => {
                return Err(EmitError::unsupported(
                    "or-pattern alternatives bind different names",
                ));
            }
            Some(_) => {}
        }
        prepared.push((condition, bindings));
    }
    let bound_names = canonical.unwrap_or_default();
    let binding_tuple = rust_binding_tuple(bound_names.iter().map(|name| mangle(name)));
    let mut first_match_chain = String::new();
    for (condition, bindings) in prepared {
        let bindings = bindings.join(" ");
        first_match_chain.push_str(&format!(
            "if {condition} {{ {bindings} Some({binding_tuple}) }} else "
        ));
    }
    first_match_chain.push_str("{ None }");
    Ok(OrPatternPreparation {
        bound_names,
        binding_tuple,
        first_match_chain,
    })
}

impl<'ctx, 'a, 'c> SubpatternEmitter<'ctx, 'a, 'c> {
    pub(crate) fn new(
        src: &'ctx LoweredText,
        aliases: &'ctx Aliases<'a, 'c>,
        scope: &'ctx mut Vec<(String, Bind)>,
        span: &'ctx str,
        counter: &'ctx mut usize,
        in_loop: bool,
        locals: &'ctx [(String, Bind)],
    ) -> Self {
        Self {
            src,
            aliases,
            scope,
            span,
            counter,
            in_loop,
            locals,
        }
    }

    pub(crate) fn emit_enum_subpatterns(
        &mut self,
        access: &str,
        owners_list: &str,
        ctor: &str,
        args: &[Pattern],
    ) -> Result<(Vec<String>, Vec<String>), EmitError> {
        let span = self.span;
        let arity = args.len();
        *self.counter += 1;
        let id = *self.counter;
        let enum_id = format!("__eid{id}");
        let declaration_identity = format!("__edecl{id}");
        let variant = format!("__evariant{id}");
        let payloads = format!("__epayloads{id}");
        // SHAPE + TAG-MATCH FIRST, then ARITY-FAULT (the interpreter's exact GUARD_ARITY,
        // computing the VALUE's `payloads.len()` at runtime), BEFORE any payload
        // extraction — so a wrong-arity value at a matching tag FAULTS (run≡build with
        // the interpreter under `--unchecked`) rather than falling through. A
        // NON-matching tag short-circuits to `false` (no fault — the value isn't this
        // variant). A payload-less variant has the same fault on a stray payload.
        let arity_fault = format!(
            "{{ if {payloads}.len() != {arity} {{ return Err(fault(codes::GUARD_ARITY, format!(\"enum variant `{ctor}` pattern takes {{}} subpattern{{}}\", {payloads}.len(), if {payloads}.len() == 1 {{ \"\" }} else {{ \"s\" }}), {span})); }} true }}"
        );
        let tag = format!(
            "[{owners_list}].contains(&nominal_declaration_identity({enum_id}.as_ref(), {declaration_identity}.as_deref())) && {variant}.as_ref() == {ctor:?} && {arity_fault}"
        );
        let mut conds = vec![
            format!(
                "let Value::Enum {{ enum_id: {enum_id}, declaration_identity: {declaration_identity}, variant: {variant}, payloads: {payloads}, .. }} = &{access}"
            ),
            tag,
        ];
        let mut binds = Vec::new();
        for (i, sub) in args.iter().enumerate() {
            *self.counter += 1;
            let payload = format!("__epayload{}", self.counter);
            // The tag condition already established exact arity; this refutable bind
            // gives nested matching and final bindings the same single payload read.
            conds.push(format!("let Some({payload}) = {payloads}.get({i})"));
            let (sc, sb) = self.emit(sub, &format!("(*{payload})"))?;
            conds.extend(sc);
            binds.extend(sb);
        }
        Ok((conds, binds))
    }

    pub(crate) fn emit_list(
        &mut self,
        elements: &[ListPatternElem],
        access: &str,
    ) -> Result<(Vec<String>, Vec<String>), EmitError> {
        enum BoundListSubpattern<'pattern> {
            Element(&'pattern Pattern, String),
            Rest(&'pattern Pattern, String),
        }

        if elements
            .iter()
            .filter(|element| matches!(element, ListPatternElem::Rest(_)))
            .count()
            > 1
        {
            return Err(EmitError::unsupported("constructor subpattern shape"));
        }
        *self.counter += 1;
        let array = format!("__arr{}", self.counter);
        let mut conditions = vec![format!("let Value::Array({array}) = &{access}")];
        let mut bound_subpatterns = Vec::with_capacity(elements.len());
        let mut slice_pattern = Vec::with_capacity(elements.len());
        for element in elements {
            match element {
                ListPatternElem::Pattern(pattern) => {
                    *self.counter += 1;
                    let item = format!("__aitem{}", self.counter);
                    slice_pattern.push(item.clone());
                    bound_subpatterns.push(BoundListSubpattern::Element(pattern, item));
                }
                ListPatternElem::Rest(Some(pattern)) => {
                    *self.counter += 1;
                    let rest = format!("__arest{}", self.counter);
                    slice_pattern.push(format!("{rest} @ .."));
                    bound_subpatterns.push(BoundListSubpattern::Rest(pattern, rest));
                }
                ListPatternElem::Rest(None) => {
                    slice_pattern.push("..".to_string());
                }
            }
        }
        conditions.push(format!(
            "let [{}] = &{array}.borrow()[..]",
            slice_pattern.join(", ")
        ));
        let mut bindings = Vec::new();
        for bound_subpattern in bound_subpatterns {
            match bound_subpattern {
                BoundListSubpattern::Element(pattern, bound) => {
                    let (nested_conditions, nested_bindings) =
                        self.emit(pattern, &format!("(*{bound})"))?;
                    conditions.extend(nested_conditions);
                    bindings.extend(nested_bindings);
                }
                BoundListSubpattern::Rest(rest_pattern, bound) => {
                    let middle = format!("Value::array({bound}.to_vec())");
                    let (nested_conditions, nested_bindings) = self.emit(rest_pattern, &middle)?;
                    conditions.extend(nested_conditions);
                    bindings.extend(nested_bindings);
                }
            }
        }
        Ok((conditions, bindings))
    }

    pub(crate) fn emit_record(
        &mut self,
        fields: &[RecordPatternField],
        access: &str,
    ) -> Result<(Vec<String>, Vec<String>), EmitError> {
        *self.counter += 1;
        let record = format!("__rec{}", self.counter);
        let mut conditions = vec![format!("let Value::Record({record}) = &{access}")];
        let mut bindings = Vec::new();
        for field in fields {
            let key = text(self.src, field.name.span);
            *self.counter += 1;
            let field_value = format!("__rfield{}", self.counter);
            conditions.push(format!("let Some({field_value}) = {record}.get({key:?})"));
            let field_access = format!("(*{field_value})");
            match &field.pattern {
                None => {
                    self.scope.push((key.to_string(), Bind::Imm));
                    bindings.push(format!("let {} = {field_access}.clone();", mangle(key)));
                }
                Some(pattern) => {
                    let (nested_conditions, nested_bindings) = self.emit(pattern, &field_access)?;
                    conditions.extend(nested_conditions);
                    bindings.extend(nested_bindings);
                }
            }
        }
        Ok((conditions, bindings))
    }

    pub(crate) fn emit_nominal_record(
        &mut self,
        name: &Ident,
        fields: &[RecordPatternField],
        access: &str,
    ) -> Result<(Vec<String>, Vec<String>), EmitError> {
        let record_name = text(self.src, name.span);
        let record_def = self.aliases.records.get(record_name);
        let record_identity = record_def.map_or(record_name, |definition| {
            nominal_declaration_identity(definition.id, definition.declaration_identity.as_deref())
        });
        *self.counter += 1;
        let record = format!("__nrec{}", self.counter);
        let identity_test = format!("{record}.is_nominal_record_declaration({record_identity:?})");
        let mut conditions = vec![format!("let {record} = &{access}"), identity_test];
        let mut bindings = Vec::new();
        for field in fields {
            let key = text(self.src, field.name.span);
            *self.counter += 1;
            let field_value = format!("__nfield{}", self.counter);
            conditions.push(format!(
                "let Some({field_value}) = {record}.nominal_field({key:?})"
            ));
            let field_access = format!("(*{field_value})");
            match &field.pattern {
                None => {
                    self.scope.push((key.to_string(), Bind::Imm));
                    bindings.push(format!("let {} = {field_access}.clone();", mangle(key)));
                }
                Some(pattern) => {
                    let (nested_conditions, nested_bindings) = self.emit(pattern, &field_access)?;
                    conditions.extend(nested_conditions);
                    bindings.extend(nested_bindings);
                }
            }
        }
        Ok((conditions, bindings))
    }

    pub(crate) fn emit_newtype(
        &mut self,
        newtype: &NewtypeDef<'_>,
        subpattern: &Pattern,
        access: &str,
    ) -> Result<(Vec<String>, Vec<String>), EmitError> {
        *self.counter += 1;
        let id = *self.counter;
        let newtype_id = format!("__ntid{id}");
        let declaration_identity = format!("__ntdecl{id}");
        let inner = format!("__ntinner{id}");
        let identity =
            nominal_declaration_identity(newtype.id, newtype.declaration_identity.as_deref());
        let identity_test = format!(
            "nominal_declaration_identity({newtype_id}.as_ref(), {declaration_identity}.as_deref()) == {identity:?}"
        );
        let mut conditions = vec![
            format!(
                "let Value::Newtype {{ newtype_id: {newtype_id}, declaration_identity: {declaration_identity}, inner: {inner}, .. }} = &{access}"
            ),
            identity_test,
        ];
        let (nested_conditions, bindings) = self.emit(subpattern, &format!("(**{inner})"))?;
        conditions.extend(nested_conditions);
        Ok((conditions, bindings))
    }

    pub(crate) fn emit(
        &mut self,
        sub: &Pattern,
        access: &str,
    ) -> Result<(Vec<String>, Vec<String>), EmitError> {
        // Locate any subpattern refusal at the offending subpattern (`sub.span` —
        // NOT the `span: &str` arg, which is emitted-code text). First-wins, so a
        // tighter span from a nested `emit_subpattern`/`emit_expr` is preserved.
        self.emit_inner(sub, access).map_err(|e| e.at(sub.span))
    }

    pub(crate) fn emit_inner(
        &mut self,
        sub: &Pattern,
        access: &str,
    ) -> Result<(Vec<String>, Vec<String>), EmitError> {
        let src = self.src;
        let aliases = self.aliases;
        let span = self.span;
        let in_loop = self.in_loop;
        let locals = self.locals;
        match &sub.kind {
            PatternKind::Wildcard => Ok((Vec::new(), Vec::new())),
            PatternKind::Binding(n) => {
                let name = text(src, n.span);
                // §3 (v5.3): a bare variant name nested in a subpattern. When `name` is
                // a declared variant, this is a REFUTABLE tag test that BINDS NOTHING —
                // mirroring the interpreter's variant population (the only checked-
                // reachable case, since the checker now rejects a non-variant bare name
                // over an enum scrutinee). A name that is NOT any enum's variant is the
                // ordinary nested binding.
                let owners = enums_declaring_variant(aliases, name);
                if !owners.is_empty() {
                    let cond = format!(
                        "matches!(&{access}, Value::Enum {{ enum_id, declaration_identity, variant, .. }} if [{}].contains(&nominal_declaration_identity(enum_id.as_ref(), declaration_identity.as_deref())) && variant.as_ref() == {name:?})",
                        owners.join(", ")
                    );
                    return Ok((vec![cond], Vec::new()));
                }
                self.scope.push((name.to_string(), Bind::Imm));
                Ok((
                    Vec::new(),
                    vec![format!("let {} = {access}.clone();", mangle(name))],
                ))
            }
            PatternKind::Literal(lit) => Ok((
                vec![literal_pattern_condition(
                    lit, access, span, src, aliases, locals, in_loop,
                )?],
                Vec::new(),
            )),
            PatternKind::Range { lo, hi, inclusive } => Ok((
                vec![range_pattern_condition(lo, hi, *inclusive, access, src)?],
                Vec::new(),
            )),
            PatternKind::Or(alts) => {
                let prepared =
                    prepare_or_pattern(alts, access, src, aliases, span, locals, in_loop)?;
                let canonical = &prepared.bound_names;

                *self.counter += 1;
                let id = *self.counter;
                let temps: Vec<String> = canonical
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("__or{id}_{i}"))
                    .collect();
                let pat = rust_binding_tuple(temps.iter().map(String::as_str));
                let chain = &prepared.first_match_chain;
                let mut binds = Vec::new();
                for (name, temp) in canonical.iter().zip(&temps) {
                    self.scope.push((name.clone(), Bind::Imm));
                    binds.push(format!("let {} = {temp};", mangle(name)));
                }
                Ok((vec![format!("let Some({pat}) = {{ {chain} }}")], binds))
            }
            PatternKind::Constructor { name, args } => {
                let ctor = text(src, name.span);
                // §3 (v5.3/v5.4): a parenthesized user-enum variant nested in a
                // subpattern — payload-less `Dot()` is a pure tag test; an N-payload
                // `Bin(op, l, r)` tag-tests + extracts each payload position into its
                // subpattern via the shared N-payload lowering. The tag binds nothing.
                let owners = enums_declaring_variant(aliases, ctor);
                if !owners.is_empty() && !matches!(ctor, "Some" | "Ok" | "Err" | "None") {
                    let owners_list = owners.join(", ");
                    return self.emit_enum_subpatterns(access, &owners_list, ctor, args);
                }
                // §3 (v5.4): a NEWTYPE nested in a subpattern `… UserId(x) …` — one
                // refutable destructure binds its identity operands and inner value,
                // then tests identity before recursing over the single subpattern.
                if let Some(newtype) = aliases.newtypes.get(ctor) {
                    let [subsub] = &args[..] else {
                        return Err(EmitError::unsupported("constructor pattern shape"));
                    };
                    return self.emit_newtype(newtype, subsub, access);
                }
                let variant = match ctor {
                    "Some" => "Some",
                    "Ok" => "Ok",
                    "Err" => "Err",
                    "None" => "None",
                    _ => return Err(EmitError::unsupported("constructor pattern")),
                };
                if variant == "None" {
                    if !args.is_empty() {
                        return Err(EmitError::unsupported("constructor pattern shape"));
                    }
                    Ok((vec![format!("let Value::None = &{access}")], Vec::new()))
                } else {
                    let [subsub] = &args[..] else {
                        return Err(EmitError::unsupported("constructor pattern shape"));
                    };
                    *self.counter += 1;
                    let inner = format!("__inner{}", self.counter);
                    let mut conds = vec![format!("let Value::{variant}({inner}) = &{access}")];
                    let (sc, sb) = self.emit(subsub, &format!("(**{inner})"))?;
                    conds.extend(sc);
                    Ok((conds, sb))
                }
            }
            // §6/§8 a nested RECORD subpattern `{ a: p, … }`: destructure the value as
            // a record (`let Value::Record(__recK) = &access`), then — exactly the
            // `case` record pattern — each NAMED field binds one `get` result and
            // recurses over that value (a subset; extra fields are fine). A
            // shorthand `{ a }` binds the field name.
            PatternKind::Record(fields) => self.emit_record(fields, access),
            PatternKind::NominalRecord { name, fields } => {
                self.emit_nominal_record(name, fields, access)
            }
            // §6 a nested LIST subpattern `[p, …]` / `[p, ..rest, q]`: destructure as
            // an array (`let Value::Array(__arrK) = &access`), then — exactly the `case`
            // list pattern and the top-level destructure — bind prefix, optional middle,
            // and suffix through one refutable Rust slice pattern. Without a rest the
            // slice pattern is exact-length; with a rest it admits any middle length.
            PatternKind::List(elements) => self.emit_list(elements, access),
            // §6 a nested TYPED subpattern `n: T` (`Some(n: int)`, `[n: int, …]`): a
            // runtime type TEST on the value at `access` via the shared `type_test`
            // (scalar / union / structural `Option`/`Result`/`Array`/`Set` / record),
            // then bind the name to the value — exactly the interpreter's typed pattern
            // inside a constructor/list/record. The type_test temporaries are scoped to
            // their own expression, so a fresh counter is fine. A `Map` with an
            // undecidable inner type, or an alias `type_test` cannot decide, refuses.
            PatternKind::Typed { name, ty } => {
                let mut tc = 0u32;
                let test = type_test(
                    ty,
                    src,
                    &format!("&{access}"),
                    &mut tc,
                    aliases,
                    locals,
                    &mut Vec::new(),
                )
                .ok_or(EmitError::unsupported("typed subpattern type"))?;
                let n = text(src, name.span);
                self.scope.push((n.to_string(), Bind::Imm));
                Ok((
                    vec![test],
                    vec![format!("let {} = {access}.clone();", mangle(n))],
                ))
            }
        }
    }
}

/// Encode a Topaz identifier as an injective, Rust-keyword-free,
/// ASCII Rust identifier: `_t_` + the lowercase hex of its UTF-8 bytes.
/// (Topaz identifiers may contain Unicode/emoji, which are not valid
/// Rust identifiers — and the prefix dodges Rust keywords.)
pub(crate) fn mangle(name: &str) -> String {
    let mut out = String::from("_t_");
    for byte in name.bytes() {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// The bound name of a simple `let`/`for` pattern, or `None` for `_`.
/// Only an UNtyped identifier binding (or `_`) lowers in this slice.
pub(crate) fn binding_name<'a>(
    pattern: &'a Pattern,
    src: &'a LoweredText,
) -> Result<Option<&'a str>, EmitError> {
    // Locate any binding-shape refusal at the offending pattern.
    binding_name_inner(pattern, src).map_err(|e| e.at(pattern.span))
}

pub(crate) fn binding_name_inner<'a>(
    pattern: &'a Pattern,
    src: &'a LoweredText,
) -> Result<Option<&'a str>, EmitError> {
    match &pattern.kind {
        PatternKind::Binding(name) => Ok(Some(text(src, name.span))),
        PatternKind::Wildcard => Ok(None),
        // A typed pattern (`x: T`) does a RUNTIME type-conformance match
        // in the interpreter (a non-matching value/element faults), so
        // erasing the annotation to a plain binding would silently DROP
        // that check. Refuse until the runtime pattern-match lands.
        PatternKind::Typed { .. } => Err(EmitError::unsupported("typed pattern")),
        _ => Err(EmitError::unsupported("binding pattern")),
    }
}

pub(crate) fn finish_nested_destructure(
    emission: NestedDestructureEmission<'_, '_, '_>,
) -> Result<String, EmitError> {
    let NestedDestructureEmission {
        bound,
        conds,
        binds,
        variant,
        value,
        fault,
        src,
        aliases,
        locals,
        base,
        captured,
        in_loop,
    } = emission;
    ensure_distinct_binding_names(bound.iter().map(String::as_str))?;
    for n in &bound {
        if locals[base..].iter().any(|(ln, _)| ln == n) {
            return Err(EmitError::unsupported("same-scope redeclaration"));
        }
        if captured.contains(&n.as_str()) {
            return Err(EmitError::unsupported(
                "declaration shadows a captured binding",
            ));
        }
    }
    let value_rs = emit_expr(value, src, aliases, locals, in_loop)?;
    let cond = if conds.is_empty() {
        "true".to_string()
    } else {
        conds.join(" && ")
    };
    let cond = match variant {
        Some(variant) => format!("let {variant} = &__dv && {cond}"),
        None => cond,
    };
    let bind_lines = binds.join(" ");
    let tuple = rust_binding_tuple(bound.iter().map(|name| mangle(name)));
    for n in &bound {
        locals.push((n.clone(), Bind::Imm));
    }
    Ok(format!(
        "    let {tuple} = {{ let __dv = {value_rs}; if {cond} {{ {bind_lines} {tuple} }} else {{ {fault} }} }};\n"
    ))
}

pub(crate) fn emit_destructure_let(
    emission: DestructureLetEmission<'_, '_, '_>,
) -> Result<String, EmitError> {
    // Locate any destructuring-let refusal at the offending PATTERN (`pattern.span`
    // is tighter than the `span: Span` statement arg). First-wins, so a tighter span
    // from `emit_subpattern`/`finish_nested_destructure`/`emit_expr` is preserved.
    let pattern_span = emission.pattern.span;
    emit_destructure_let_inner(emission).map_err(|e| e.at(pattern_span))
}

pub(crate) fn emit_destructure_let_inner(
    emission: DestructureLetEmission<'_, '_, '_>,
) -> Result<String, EmitError> {
    let DestructureLetEmission {
        pattern,
        value,
        span,
        mutable,
        src,
        aliases,
        locals,
        base,
        captured,
        in_loop,
    } = emission;
    if mutable {
        return Err(EmitError::unsupported("mutable destructuring let"));
    }
    if !matches!(
        pattern.kind,
        PatternKind::Or(_) | PatternKind::List(_) | PatternKind::Record(_)
    ) {
        return Err(EmitError::unsupported("binding pattern"));
    }

    let fault = format!(
        "return Err(fault(codes::GUARD_TYPE, {:?}, {}));",
        "`let` pattern did not match the value (§4)",
        emit_span(span),
    );
    let mut scope = locals.to_vec();
    let scope_start = scope.len();
    let mut counter = 0usize;
    let span = emit_span(span);
    let (conditions, bindings) = SubpatternEmitter::new(
        src,
        aliases,
        &mut scope,
        &span,
        &mut counter,
        in_loop,
        locals,
    )
    .emit(pattern, "__dv")?;
    let bound = scope[scope_start..]
        .iter()
        .map(|(name, _)| name.clone())
        .collect();
    finish_nested_destructure(NestedDestructureEmission {
        bound,
        conds: conditions,
        binds: bindings,
        variant: None,
        value,
        fault: &fault,
        src,
        aliases,
        locals,
        base,
        captured,
        in_loop,
    })
}
