use super::*;

pub(super) fn list_let_pattern_refutable(pattern: &ast::Pattern) -> bool {
    match &pattern.kind {
        ast::PatternKind::List(elems) => list_elems_refutable_in_let(elems),
        ast::PatternKind::Constructor { args, .. } | ast::PatternKind::Or(args) => {
            args.iter().any(list_let_pattern_refutable)
        }
        ast::PatternKind::Record(fields) | ast::PatternKind::NominalRecord { fields, .. } => fields
            .iter()
            .filter_map(|field| field.pattern.as_ref())
            .any(list_let_pattern_refutable),
        ast::PatternKind::Wildcard
        | ast::PatternKind::Literal(_)
        | ast::PatternKind::Range { .. }
        | ast::PatternKind::Binding(_)
        | ast::PatternKind::Typed { .. } => false,
    }
}

pub(super) fn list_elems_refutable_in_let(elems: &[ast::ListPatternElem]) -> bool {
    let has_rest = elems
        .iter()
        .any(|elem| matches!(elem, ast::ListPatternElem::Rest(_)));
    let required = elems
        .iter()
        .filter(|elem| matches!(elem, ast::ListPatternElem::Pattern(_)))
        .count();
    !has_rest
        || required > 0
        || elems.iter().any(|elem| match elem {
            ast::ListPatternElem::Pattern(p) | ast::ListPatternElem::Rest(Some(p)) => {
                list_let_pattern_refutable(p)
            }
            ast::ListPatternElem::Rest(None) => false,
        })
}

pub(super) fn list_pattern_matches_every_array(elems: &[ast::ListPatternElem]) -> bool {
    if elems.len() != 1 {
        return false;
    }
    match &elems[0] {
        ast::ListPatternElem::Rest(None) => true,
        ast::ListPatternElem::Rest(Some(pattern)) => pattern_matches_every_array_value(pattern),
        ast::ListPatternElem::Pattern(_) => false,
    }
}

pub(super) fn pattern_matches_every_array_value(pattern: &ast::Pattern) -> bool {
    match &pattern.kind {
        ast::PatternKind::Wildcard | ast::PatternKind::Binding(_) => true,
        ast::PatternKind::List(elems) => list_pattern_matches_every_array(elems),
        ast::PatternKind::Or(alts) => alts.iter().any(pattern_matches_every_array_value),
        ast::PatternKind::Literal(_)
        | ast::PatternKind::Range { .. }
        | ast::PatternKind::Typed { .. }
        | ast::PatternKind::Constructor { .. }
        | ast::PatternKind::Record(_)
        | ast::PatternKind::NominalRecord { .. } => false,
    }
}

impl Coverage {
    pub(super) fn merge(&mut self, other: Coverage) {
        self.irrefutable |= other.irrefutable;
        self.literals.extend(other.literals);
        for (tag, sub) in other.ctor_cov {
            self.ctor_cov.entry(tag).or_default().merge(sub);
        }
        self.nominal_records.extend(other.nominal_records);
    }

    /// Whether this coverage exhausts the type. Undecidable domains
    /// are exhausted only by an irrefutable pattern. `enums` is the program's
    /// enum table, so a NESTED user-enum payload is decidable (its declared
    /// variant set), mirroring the top-level enum exhaustiveness path.
    pub(super) fn covers(&self, ty: &Type, enums: &HashMap<String, EnumInfo>) -> bool {
        self.irrefutable || matches!(self.missing(ty, enums), Some(m) if m.is_empty())
    }

    /// `None` when the scrutinee's domain is undecidable; otherwise
    /// the missing-case descriptions (empty = exhaustive).
    pub(super) fn missing(
        &self,
        scrutinee: &Type,
        enums: &HashMap<String, EnumInfo>,
    ) -> Option<Vec<String>> {
        if self.irrefutable {
            return Some(Vec::new());
        }
        let covered_lit = |l: &Lit| self.literals.contains(l);
        match scrutinee {
            Type::Prim(Prim::Bool) => Some(
                [Lit::Bool(true), Lit::Bool(false)]
                    .into_iter()
                    .filter(|l| !covered_lit(l))
                    .map(|l| format!("`{}`", Type::Literal(l)))
                    .collect(),
            ),
            Type::Union(members) if members.iter().all(|m| matches!(m, Type::Literal(_))) => Some(
                members
                    .iter()
                    .filter(|m| matches!(m, Type::Literal(l) if !covered_lit(l)))
                    .map(|m| format!("`{m}`"))
                    .collect(),
            ),
            // A union composed only of nominal-record members and literals
            // is decidable by declaration identity plus literal coverage. This pins
            // `User | null` and `User | Admin` without pretending an arbitrary mixed
            // union is exhaustively modeled.
            Type::Union(members)
                if members.iter().all(|member| {
                    matches!(member, Type::Literal(_) | Type::NominalRecord { .. })
                }) =>
            {
                Some(
                    members
                        .iter()
                        .filter_map(|member| match member {
                            Type::Literal(lit) if !covered_lit(lit) => Some(format!("`{member}`")),
                            Type::NominalRecord { base, args }
                                if !self.nominal_records.contains(base) =>
                            {
                                Some(format!("`{} {{ … }}`", nominal_instance_id(base, args)))
                            }
                            _ => None,
                        })
                        .collect(),
                )
            }
            Type::Ctor(Ctor::Option, a) => {
                let mut miss = Vec::new();
                if !self.tag_covers("Some", Some(&a[0]), enums) {
                    miss.push("`Some`".to_string());
                }
                if !self.tag_covers("None", None, enums) {
                    miss.push("`None`".to_string());
                }
                Some(miss)
            }
            Type::Ctor(Ctor::Result, a) => {
                let mut miss = Vec::new();
                if !self.tag_covers("Ok", Some(&a[0]), enums) {
                    miss.push("`Ok`".to_string());
                }
                if !self.tag_covers("Err", Some(&a[1]), enums) {
                    miss.push("`Err`".to_string());
                }
                Some(miss)
            }
            // §3 (v5.3/v5.4) a NESTED user-enum payload is decidable by its declared
            // variant set (mirrors the top-level enum path in `check_match`): each
            // variant must be tag-covered (payload-aware per `variant_covered`).
            Type::Enum { base, args } => {
                let id = nominal_instance_id(base, args);
                let info = enums.get(&id)?;
                Some(
                    info.variants
                        .iter()
                        .filter(|v| !self.variant_covered(v, enums))
                        .map(|v| format!("`{id}.{}`", v.name))
                        .collect(),
                )
            }
            // §3 (v5.4) a nominal record has ONE shape: it is exhausted only by an
            // IRREFUTABLE pattern (this method is reached only when `!irrefutable`,
            // so a refutable field subpattern leaves the record uncovered).
            Type::NominalRecord { base, args } => {
                let id = nominal_instance_id(base, args);
                Some(if self.nominal_records.contains(base) {
                    Vec::new()
                } else {
                    vec![format!("`{id} {{ … }}`")]
                })
            }
            // §3 (v5.4) a newtype has ONE shape too: exhausted only by an IRREFUTABLE
            // `UserId(x)` (reached only when `!irrefutable`, so a refutable inner
            // subpattern, e.g. `case UserId(5)`, leaves the newtype uncovered).
            Type::Newtype { base, args } => {
                let id = nominal_instance_id(base, args);
                Some(vec![format!("`{id}(_)`")])
            }
            _ => None,
        }
    }

    /// Whether an unguarded arm covers an enum VARIANT, by arity:
    /// - arity 0 (payload-less): covered iff the tag was matched;
    /// - arity 1 (single payload): covered iff the nested coverage exhausts the
    ///   payload type (precise — `Circle(_)` covers, `Circle(1)` does not);
    /// - arity ≥ 2 (multi-payload, v5.4): CONSERVATIVE — covered only when the
    ///   nested coverage is `irrefutable` (set when EVERY position was irrefutable).
    pub(super) fn variant_covered(
        &self,
        v: &EnumVariantInfo,
        enums: &HashMap<String, EnumInfo>,
    ) -> bool {
        match v.payloads.as_slice() {
            [] => self.tag_covers(&v.name, None, enums),
            [single] => self.tag_covers(&v.name, Some(single), enums),
            _ => self
                .ctor_cov
                .get(&v.name)
                .is_some_and(|sub| sub.irrefutable),
        }
    }

    /// A tag is covered when some unguarded arm matched it and the
    /// merged payload coverage exhausts the payload type.
    pub(super) fn tag_covers(
        &self,
        tag: &str,
        payload: Option<&Type>,
        enums: &HashMap<String, EnumInfo>,
    ) -> bool {
        match self.ctor_cov.get(tag) {
            None => false,
            Some(sub) => match payload {
                None => true,
                Some(ty) => sub.covers(ty, enums),
            },
        }
    }
}

/// Operand admission by subtyping: literal types and literal unions
/// are usable as their primitive.
pub(super) fn usable(t: &Type, p: &Type) -> bool {
    is_subtype(t, p)
}

/// The overlap of a scrutinee with a type-pattern annotation: the
/// narrowest type the binding can assume inside the arm.
pub(super) fn narrow_type(s: &Type, formed: &Type) -> Type {
    if s.has_unknown() {
        return formed.clone();
    }
    if is_subtype(s, formed) {
        return s.clone();
    }
    if is_subtype(formed, s) {
        return formed.clone();
    }
    if let Type::Union(members) = s {
        let kept: Vec<Type> = members
            .iter()
            .filter(|m| type_overlap(m, formed))
            .map(|m| narrow_type(m, formed))
            .collect();
        if !kept.is_empty() {
            return Type::union(kept);
        }
    }
    formed.clone()
}

impl<'a> ExprChecker<'a> {
    /// Binds a match pattern's names against the scrutinee type and
    /// reports what the pattern covers. Concrete impossibilities
    /// (a literal outside the scrutinee, a constructor of the wrong
    /// container) diagnose as TPZ5001.
    pub(super) fn bind_match_pattern(&mut self, pattern: &'a ast::Pattern, s: &Type) -> Coverage {
        self.bind_match_pattern_at(pattern, s, true)
    }

    /// `bind_match_pattern` with explicit POSITION sensitivity (`top_level`): a
    /// TOP-LEVEL match-arm pattern (`case Red =>`) over an enum scrutinee gates a
    /// bare name strictly (it must be a declared variant, else it is a likely TYPO
    /// — TPZ5001 — that would otherwise silently become a catch-all binding and
    /// suppress exhaustiveness). A NESTED payload subpattern (`case Bin(op, l, r)`)
    /// is a destructuring position where a bare name BINDS the payload. `let`/`for`
    /// patterns pass `top_level=false` (binding contexts). Recursion into a payload
    /// subpattern always passes `false`.
    pub(super) fn bind_match_pattern_at(
        &mut self,
        pattern: &'a ast::Pattern,
        s: &Type,
        top_level: bool,
    ) -> Coverage {
        let coverage = self.bind_match_pattern_at_inner(pattern, s, top_level);
        self.record_typed_node(topaz_hir::TypedNodeKind::Pattern, pattern.span, s);
        coverage
    }

    pub(super) fn bind_match_pattern_at_inner(
        &mut self,
        pattern: &'a ast::Pattern,
        s: &Type,
        top_level: bool,
    ) -> Coverage {
        match pattern_family(&pattern.kind) {
            PatternFamily::Scalar => self.bind_scalar_pattern(pattern, s, top_level),
            PatternFamily::Constructor => self.bind_constructor_pattern(pattern, s),
            PatternFamily::Record => self.bind_record_pattern(pattern, s),
            PatternFamily::Sequence => self.bind_sequence_pattern(pattern, s, top_level),
        }
    }

    pub(super) fn bind_scalar_pattern(
        &mut self,
        pattern: &'a ast::Pattern,
        s: &Type,
        top_level: bool,
    ) -> Coverage {
        let mut cov = Coverage::default();
        match &pattern.kind {
            ast::PatternKind::Wildcard => {
                cov.irrefutable = true;
            }
            ast::PatternKind::Binding(name) => {
                // Bare `None` never reaches here: the parser emits a
                // zero-argument constructor pattern for it (§22.1).
                let text = self.former.text(name.span).to_string();
                // §3 (v5.3/v5.4): a bare name in a match against a user-enum
                // scrutinee — the parser yields `Binding` for any bare ident (only
                // `None` is special-cased there), so the enum disambiguation happens
                // HERE against the scrutinee's nominal type, mirroring the
                // value-based interpreter: a name that IS a declared variant is a
                // refutable VARIANT pattern (`case Red =>`, binds nothing). A
                // NON-variant bare name BINDS at a NESTED payload position (essential
                // for destructuring an enum-typed payload, `case Bin(op, …)` binds
                // `op`), but at the TOP LEVEL of a match arm it is a likely TYPO and
                // is rejected (TPZ5001) rather than silently becoming a catch-all.
                if let Type::Enum { base, args } = s {
                    let id = nominal_instance_id(base, args);
                    let info = self.former.enum_info(&id);
                    if let Some(v) = info.and_then(|i| i.variants.iter().find(|v| v.name == text)) {
                        // A bare variant pattern requires the variant be payload-less:
                        // a payloadful variant must be matched with subpatterns, e.g.
                        // `case Circle(r) =>` / `case Bin(_, _, _) =>`.
                        if !v.payloads.is_empty() {
                            let arity = v.payloads.len();
                            let underscores = vec!["_"; arity].join(", ");
                            self.former.error(
                                codes::ARITY,
                                format!(
                                    "enum variant `{id}.{text}` carries {arity} payload{} — write `case {text}({underscores})` to match it",
                                    if arity == 1 { "" } else { "s" }
                                ),
                                name.span,
                            );
                        }
                        cov.ctor_cov.entry(text).or_default().irrefutable = true;
                        return cov;
                    }
                    if top_level {
                        // A non-variant bare name at a TOP-LEVEL enum match arm is a
                        // likely typo — reject it (the catch-all is `_`).
                        let known: Vec<&str> = info
                            .map(|i| i.variants.iter().map(|v| v.name.as_str()).collect())
                            .unwrap_or_default();
                        let hint = topaz_diag::suggest::did_you_mean(&text, known.iter().copied());
                        self.former.error(
                            codes::TYPE_MISMATCH,
                            format!(
                                "`{text}` is not a variant of enum `{id}`{hint} (use `_` for a catch-all)"
                            ),
                            name.span,
                        );
                        // Recover as a covering pattern so a follow-on missing-variant
                        // error does not also pile on.
                        cov.irrefutable = true;
                        return cov;
                    }
                    // A non-variant bare name at a NESTED position binds the payload
                    // (matching interp + emit).
                }
                self.record_typed_local(&text, name.span, s);
                self.bind_decl(text, s.clone(), false, name.span);
                cov.irrefutable = true;
            }
            ast::PatternKind::Literal(expr) => {
                let lit_ty = self.infer(expr);
                // Impossibility is judged only on decidable types;
                // Foreign and Skolem scrutinees stay silent.
                if decidable_type(s) && !lit_ty.has_unknown() && !type_overlap(&lit_ty, s) {
                    let display = s.clone();
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        format!("this pattern can never match `{display}`"),
                        expr.span,
                    );
                }
                if let Type::Literal(l) = lit_ty {
                    cov.literals.push(l);
                }
            }
            ast::PatternKind::Range { lo, hi, .. } => {
                self.infer(lo);
                self.infer(hi);
            }
            ast::PatternKind::Typed { name, ty } => {
                let env = self.tyenv();
                let formed = self.former.form(ty, &env);
                // §6: binds when the value conforms; covering the
                // whole scrutinee makes the arm irrefutable. The
                // binding takes the overlap of scrutinee and
                // annotation, not the raw annotation.
                if !s.has_unknown() && !formed.has_unknown() && is_subtype(s, &formed) {
                    cov.irrefutable = true;
                } else if decidable_type(s) && decidable_type(&formed) && !type_overlap(s, &formed)
                {
                    let display = s.clone();
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        format!("this pattern can never match `{display}`"),
                        pattern.span,
                    );
                }
                let bound = narrow_type(s, &formed);
                let text = self.former.text(name.span).to_string();
                self.record_typed_local(&text, name.span, &bound);
                self.bind_decl(text, bound, false, name.span);
            }
            _ => unreachable!("pattern family changed after classification"),
        }
        cov
    }

    pub(super) fn bind_constructor_pattern(
        &mut self,
        pattern: &'a ast::Pattern,
        s: &Type,
    ) -> Coverage {
        let mut cov = Coverage::default();
        match &pattern.kind {
            ast::PatternKind::Constructor { name, args } if matches!(s, Type::Newtype { .. }) => {
                // §3 (v5.4): a constructor pattern against a NEWTYPE scrutinee —
                // `case UserId(x) =>` destructures the wrapper, binding the inner base
                // value. The ctor name must equal the scrutinee's newtype id; exactly
                // ONE subpattern, typed against the BASE type. The pattern is
                // IRREFUTABLE iff the inner subpattern is (a newtype has a single
                // shape — like `Some` on an `Option`, but with no None alternative).
                let Type::Newtype {
                    base,
                    args: nominal_args,
                } = s
                else {
                    unreachable!("guarded by the match arm")
                };
                let id = nominal_instance_id(base, nominal_args);
                let ctor = self.former.text(name.span).to_string();
                let ctor_id = self
                    .former
                    .newtype_info(&ctor)
                    .map(|i| i.id.as_str())
                    .unwrap_or(ctor.as_str());
                let base_ty = self
                    .former
                    .newtype_info(&id)
                    .map(|i| i.base.clone())
                    .unwrap_or(Type::Unknown);
                if ctor_id != id.as_str() && !nominal_ctx_matches(&ctor, base) {
                    // A different ctor name can never match this newtype.
                    for arg in args {
                        self.bind_match_pattern_at(arg, &Type::Unknown, false);
                    }
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        format!("this `{ctor}` pattern can never match `{id}`"),
                        pattern.span,
                    );
                } else if args.len() != 1 {
                    for arg in args {
                        self.bind_match_pattern_at(arg, &Type::Unknown, false);
                    }
                    self.former.error(
                        codes::ARITY,
                        format!(
                            "newtype `{id}` pattern takes 1 subpattern (the wrapped `{base_ty}`), found {}",
                            args.len()
                        ),
                        pattern.span,
                    );
                } else {
                    let sub = self.bind_match_pattern_at(&args[0], &base_ty, false);
                    cov.irrefutable = sub.irrefutable;
                }
            }
            ast::PatternKind::Constructor { name, args } if matches!(s, Type::Enum { .. }) => {
                // §3 (v5.3/v5.4): a constructor pattern against a user-enum
                // scrutinee. The pattern is a parenthesized variant
                // `case Circle(r) =>` / `case Bin(op, l, r) =>` (a bare `case Red =>`
                // is handled by the `Binding` arm) — the AST has no qualified-pattern
                // form, and the scrutinee's nominal type names the enum, exactly as
                // `Some`/`Ok` patterns rely on an Option/Result scrutinee. The
                // variant must belong to the scrutinee's enum; ARITY must match the
                // variant's payload arity (`payloads.len()`); each subpattern is
                // typed POSITION-WISE against the corresponding payload type.
                //
                // Coverage: for a SINGLE-payload variant the precise single nested
                // coverage is preserved (so `Circle(_)` covers `Circle`, `Circle(1)`
                // does not). For a MULTI-payload variant coverage is CONSERVATIVE
                // The tag is exhausted only when every payload subpattern is
                // irrefutable (`Bin(_, _, _)` / `Bin(op, l, r)` covers; any refutable
                // position, e.g. `Bin(Num(_), _, _)`, does not).
                let Type::Enum {
                    base,
                    args: nominal_args,
                } = s
                else {
                    unreachable!("guarded by the match arm")
                };
                let id = nominal_instance_id(base, nominal_args);
                let ctor = self.former.text(name.span).to_string();
                let info = self.former.enum_info(&id);
                let variant = info
                    .and_then(|i| i.variants.iter().find(|v| v.name == ctor))
                    .cloned();
                match variant {
                    None => {
                        let known: Vec<&str> = info
                            .map(|i| i.variants.iter().map(|v| v.name.as_str()).collect())
                            .unwrap_or_default();
                        let hint = topaz_diag::suggest::did_you_mean(&ctor, known.iter().copied());
                        self.former.error(
                            codes::TYPE_MISMATCH,
                            format!("enum `{id}` has no variant `{ctor}`{hint}"),
                            pattern.span,
                        );
                    }
                    Some(v) => {
                        let arity = v.payloads.len();
                        if args.len() != arity {
                            // Bind the given subpatterns against Unknown so their own
                            // names still bind, then report the arity mismatch.
                            for arg in args {
                                self.bind_match_pattern_at(arg, &Type::Unknown, false);
                            }
                            let want = if arity == 0 {
                                "no subpattern".to_string()
                            } else {
                                format!("{arity} subpattern{}", if arity == 1 { "" } else { "s" })
                            };
                            self.former.error(
                                codes::ARITY,
                                format!(
                                    "enum variant `{id}.{ctor}` takes {want}, found {}",
                                    args.len()
                                ),
                                pattern.span,
                            );
                        } else if arity <= 1 {
                            // Payload-less or single-payload: preserve the precise
                            // single nested coverage.
                            let mut sub = Coverage {
                                irrefutable: arity == 0,
                                ..Coverage::default()
                            };
                            for (arg, ty) in args.iter().zip(v.payloads.iter()) {
                                sub = self.bind_match_pattern_at(arg, ty, false);
                            }
                            cov.ctor_cov.entry(ctor).or_default().merge(sub);
                        } else {
                            // Multi-payload (v5.4): CONSERVATIVE coverage — the tag is
                            // exhausted only when EVERY position is irrefutable.
                            let mut all_irrefutable = true;
                            for (arg, ty) in args.iter().zip(v.payloads.iter()) {
                                let sub = self.bind_match_pattern_at(arg, ty, false);
                                all_irrefutable &= sub.irrefutable;
                            }
                            cov.ctor_cov.entry(ctor).or_default().irrefutable |= all_irrefutable;
                        }
                    }
                }
            }
            ast::PatternKind::Constructor { name, args } => {
                let ctor = self.former.text(name.span);
                // A constructor pattern may match the scrutinee, or any MEMBER of
                // a union scrutinee. Per member: a concrete `Option`/`Result`
                // contributes its concrete payload; a RIGID member (a generic `T`
                // — Skolem/Foreign) contributes a rigid `PayloadOf<T, Ctor>`
                // projection (neither discharges a concrete type nor is usable as
                // `T`); a GRADUAL member (Unknown/Var) contributes `Unknown`.
                let known = matches!(ctor, "Some" | "None" | "Ok" | "Err");
                // A known constructor has a FIXED arity (Some/Ok/Err carry one
                // payload, None carries none). Enforce it so a malformed pattern is
                // rejected at check time (matching the interpreter's runtime guard)
                // rather than slipping through a rigid/gradual union member that
                // does not re-check arity.
                let arity_ok = match ctor {
                    "Some" | "Ok" | "Err" => args.len() == 1,
                    "None" => args.is_empty(),
                    _ => true,
                };
                let members: Vec<&Type> = match s {
                    Type::Union(ms) => ms.iter().collect(),
                    other => vec![other],
                };
                let mut payloads: Vec<Type> = Vec::new();
                let mut matchable = false;
                let mut has_payload = false;
                for m in members {
                    match (ctor, m) {
                        ("Some", Type::Ctor(Ctor::Option, a)) if args.len() == 1 => {
                            matchable = true;
                            has_payload = true;
                            payloads.push(a[0].clone());
                        }
                        ("None", Type::Ctor(Ctor::Option, _)) if args.is_empty() => {
                            matchable = true;
                        }
                        ("Ok", Type::Ctor(Ctor::Result, a)) if args.len() == 1 => {
                            matchable = true;
                            has_payload = true;
                            payloads.push(a[0].clone());
                        }
                        ("Err", Type::Ctor(Ctor::Result, a)) if args.len() == 1 => {
                            matchable = true;
                            has_payload = true;
                            payloads.push(a[1].clone());
                        }
                        // A rigid/gradual member can match a known ctor only at the
                        // correct arity (`arity_ok`); a user ctor stays staged.
                        (_, m_ty) if m_ty.has_unknown() && (!known || arity_ok) => {
                            matchable = true;
                            if !args.is_empty() {
                                has_payload = true;
                                payloads.push(Type::Unknown);
                            }
                        }
                        (_, Type::Foreign { .. } | Type::Skolem { .. }) if !known || arity_ok => {
                            matchable = true;
                            if !args.is_empty() {
                                has_payload = true;
                                payloads.push(self.project(format!("PayloadOf<{m}, {ctor}>")));
                            }
                        }
                        _ => {}
                    }
                }
                let (inner, ok): (Option<Type>, bool) = if matchable {
                    let payload =
                        (has_payload && !payloads.is_empty()).then(|| Type::union(payloads));
                    (payload, true)
                } else if known {
                    if arity_ok {
                        // No member can match this known constructor.
                        let display = s.clone();
                        self.former.error(
                            codes::TYPE_MISMATCH,
                            format!("this pattern can never match `{display}`"),
                            pattern.span,
                        );
                    } else {
                        let want = if ctor == "None" {
                            "no subpattern"
                        } else {
                            "one subpattern"
                        };
                        self.former.error(
                            codes::ARITY,
                            format!("`{ctor}` pattern takes {want}"),
                            pattern.span,
                        );
                    }
                    (None, false)
                } else {
                    // User constructors stay staged.
                    (None, true)
                };
                let payload_ty = inner.unwrap_or(Type::Unknown);
                // The payload subpattern's coverage nests under the
                // constructor tag (payload-aware exhaustiveness, so
                // Some(true)/Some(false)/None exhausts Option<bool>).
                let mut sub = Coverage {
                    irrefutable: true, // payload-less tags cover by presence
                    ..Coverage::default()
                };
                for arg in args {
                    sub = self.bind_match_pattern_at(arg, &payload_ty, false);
                }
                if ok && matches!(ctor, "Some" | "None" | "Ok" | "Err") {
                    cov.ctor_cov.entry(ctor.to_string()).or_default().merge(sub);
                }
            }
            _ => unreachable!("pattern family changed after classification"),
        }
        cov
    }

    pub(super) fn bind_record_pattern(&mut self, pattern: &'a ast::Pattern, s: &Type) -> Coverage {
        let mut cov = Coverage::default();
        match &pattern.kind {
            ast::PatternKind::Record(fields) => match s {
                // A single record: unchanged — a missing field is the precise
                // "no field" diagnostic (TPZ5006), not "can never match". A
                // structural record has ONE shape, so the pattern is IRREFUTABLE iff
                // every field subpattern is irrefutable (so a destructuring `let
                // { x, y } = r` is accepted while `let { x: 5 } = r` is refutable,
                // §4 — the same rule the NominalRecord arm uses).
                Type::Record(s_fields) => {
                    let mut all_irrefutable = true;
                    for field in fields {
                        let fname = self.former.text(field.name.span);
                        let field_ty = match s_fields.iter().find(|(n, _)| n == fname) {
                            Some((_, ft)) => ft.clone(),
                            None if !s.has_unknown() => {
                                let display = s.clone();
                                let hint = topaz_diag::suggest::did_you_mean(
                                    fname,
                                    s_fields.iter().map(|(n, _)| n.as_str()),
                                );
                                self.former.error(
                                    codes::NO_FIELD,
                                    format!("`{display}` has no field `{fname}`{hint}"),
                                    field.span,
                                );
                                Type::Unknown
                            }
                            None => Type::Unknown,
                        };
                        match &field.pattern {
                            Some(sub) => {
                                let cov = self.bind_match_pattern_at(sub, &field_ty, false);
                                all_irrefutable &= cov.irrefutable;
                            }
                            None => {
                                // Shorthand `{ name }` binds the field irrefutably.
                                self.bind_decl(fname.to_string(), field_ty, false, field.span);
                            }
                        }
                    }
                    cov.irrefutable = all_irrefutable;
                }
                // A union: the record pattern matches the union MEMBERS it could
                // apply to. Narrow to the CANDIDATE record members — a member must
                // have every named field, and each literal-constrained discriminant
                // (e.g. `kind: "a"`) must overlap the member's field type — so
                // sibling-field types stay correlated to the matched variant (the
                // canonical tagged-union dispatch). "Can never match" only when no
                // member is a candidate.
                Type::Union(ms) => {
                    // An opaque member (a generic `T`/Skolem, Foreign, or one that
                    // could ALSO match this pattern at runtime. SPLIT it: a RIGID
                    // member (a generic `T` — Skolem/Foreign) contributes a rigid
                    // FIELD PROJECTION so the binding can neither discharge a
                    // concrete type nor be used as `T`; a GRADUAL member
                    // (Unknown/Var) contributes `Unknown` (stays permissive).
                    // Either suppresses "can never match" (it might match).
                    let rigid_members: Vec<Type> = ms
                        .iter()
                        .filter(|m| matches!(m, Type::Skolem { .. } | Type::Foreign { .. }))
                        .cloned()
                        .collect();
                    let has_gradual = ms.iter().any(Type::has_unknown);
                    let any_opaque = !rigid_members.is_empty() || has_gradual;
                    let mut candidates: Vec<&Vec<(String, Type)>> = Vec::new();
                    for m in ms {
                        let Type::Record(mf) = m else { continue };
                        let mut ok = true;
                        for field in fields {
                            let fname = self.former.text(field.name.span);
                            match mf.iter().find(|(n, _)| n == fname) {
                                None => {
                                    ok = false;
                                    break;
                                }
                                Some((_, ft)) => {
                                    if let Some(sub) = &field.pattern
                                        && let ast::PatternKind::Literal(lit) = &sub.kind
                                    {
                                        // Probe the literal type WITHOUT emitting
                                        // diagnostics — the real Literal arm below
                                        // reports any (once).
                                        let before = self.former.diagnostics.len();
                                        let lit_ty = self.infer(lit);
                                        self.former.diagnostics.truncate(before);
                                        if decidable_type(ft)
                                            && !lit_ty.has_unknown()
                                            && !type_overlap(&lit_ty, ft)
                                        {
                                            ok = false;
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        if ok {
                            candidates.push(mf);
                        }
                    }
                    if candidates.is_empty() && !any_opaque && decidable_type(s) {
                        let display = s.clone();
                        self.former.error(
                            codes::TYPE_MISMATCH,
                            format!("this pattern can never match `{display}`"),
                            pattern.span,
                        );
                    }
                    for field in fields {
                        let fname = self.former.text(field.name.span);
                        let mut tys: Vec<Type> = candidates
                            .iter()
                            .filter_map(|mf| {
                                mf.iter()
                                    .find(|(n, _)| n == fname)
                                    .map(|(_, ft)| ft.clone())
                            })
                            .collect();
                        // A rigid member projects its (abstract) field; a gradual
                        // member contributes `Unknown`.
                        for rm in &rigid_members {
                            tys.push(self.project(format!("FieldOf<{rm}, {fname}>")));
                        }
                        if has_gradual {
                            tys.push(Type::Unknown);
                        }
                        let field_ty = if tys.is_empty() {
                            Type::Unknown
                        } else {
                            Type::union(tys)
                        };
                        match &field.pattern {
                            Some(sub) => {
                                self.bind_match_pattern_at(sub, &field_ty, false);
                            }
                            None => {
                                self.bind_decl(fname.to_string(), field_ty, false, field.span);
                            }
                        }
                    }
                }
                // Any other decidable, non-opaque scrutinee can never match a
                // record pattern. A pure RIGID scrutinee (`v: T`, a generic
                // Skolem/Foreign) projects each field; a GRADUAL one (Unknown/Var)
                // stays `Unknown`.
                _ => {
                    let rigid = matches!(s, Type::Foreign { .. } | Type::Skolem { .. });
                    let opaque = rigid || s.has_unknown();
                    if !opaque && decidable_type(s) {
                        let display = s.clone();
                        self.former.error(
                            codes::TYPE_MISMATCH,
                            format!("this pattern can never match `{display}`"),
                            pattern.span,
                        );
                    }
                    for field in fields {
                        let fname = self.former.text(field.name.span);
                        let field_ty = if rigid {
                            self.project(format!("FieldOf<{s}, {fname}>"))
                        } else {
                            Type::Unknown
                        };
                        match &field.pattern {
                            Some(sub) => {
                                self.bind_match_pattern_at(sub, &field_ty, false);
                            }
                            None => {
                                self.bind_decl(fname.to_string(), field_ty, false, field.span);
                            }
                        }
                    }
                }
            },
            // §3/§6 (v5.4) a NOMINAL record pattern `User { name, age }` resolves
            // `User` in the record declaration namespace, then matches ONLY a
            // `Type::NominalRecord` of that declaration base (a structural record /
            // different record does NOT). For a union, matching nominal members are
            // selected and their instantiated field types are joined. A rigid member
            // cannot project nominal identity; a genuinely gradual member stays
            // permissive. Unknown heads are TPZ5002 before backend selection.
            ast::PatternKind::NominalRecord { name, fields } => {
                let rec_name = self.former.text(name.span).to_string();
                let declared_info = self.former.record_info(&rec_name).cloned();
                let head_base = self.former.record_base_for_name(&rec_name);
                if declared_info.is_none() {
                    let hint = topaz_diag::suggest::did_you_mean(
                        &rec_name,
                        self.former.record_table().keys().map(String::as_str),
                    );
                    self.former.error(
                        codes::UNBOUND,
                        format!("unbound nominal record pattern head `{rec_name}`{hint}"),
                        name.span,
                    );
                }

                let members: Vec<&Type> = match s {
                    Type::Union(ms) => ms.iter().collect(),
                    other => vec![other],
                };
                let has_rigid = members
                    .iter()
                    .any(|m| matches!(m, Type::Skolem { .. } | Type::Foreign { .. }));
                let has_gradual = members
                    .iter()
                    .any(|m| matches!(m, Type::Unknown | Type::Var(_)));
                let candidate_args: Vec<Vec<Type>> = head_base
                    .as_ref()
                    .map(|expected_base| {
                        members
                            .iter()
                            .filter_map(|member| match member {
                                Type::NominalRecord { base, args } if base == expected_base => {
                                    Some(args.clone())
                                }
                                _ => None,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let all_members_match = head_base.as_ref().is_some_and(|expected_base| {
                    !members.is_empty()
                        && members.iter().all(|member| {
                            matches!(member, Type::NominalRecord { base, .. } if base == expected_base)
                        })
                });
                let mut candidate_infos = Vec::with_capacity(candidate_args.len());
                for args in candidate_args {
                    if let Some(info) = self.former.record_instance(&rec_name, args, pattern.span) {
                        candidate_infos.push(info);
                    }
                }

                if declared_info.is_some() && has_rigid {
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        format!(
                            "a `{rec_name}` pattern cannot establish nominal identity for rigid scrutinee type `{s}`"
                        ),
                        pattern.span,
                    );
                } else if declared_info.is_some() && candidate_infos.is_empty() && !has_gradual {
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        format!("this `{rec_name}` pattern can never match `{s}`"),
                        pattern.span,
                    );
                }

                let mut all_irrefutable = true;
                let mut seen_fields = HashSet::new();
                for field in fields {
                    let fname = self.former.text(field.name.span);
                    let duplicate_source = !seen_fields.insert(fname.to_string());
                    if duplicate_source {
                        self.former.error(
                            codes::REDECLARE,
                            format!(
                                "nominal record pattern field `{fname}` is specified more than once"
                            ),
                            field.span,
                        );
                    }
                    let declared_field = declared_info
                        .as_ref()
                        .and_then(|i| i.fields.iter().find(|f| f.name == fname))
                        .cloned();
                    if declared_info.is_some() && declared_field.is_none() {
                        let known: Vec<&str> = declared_info
                            .as_ref()
                            .map(|i| i.fields.iter().map(|f| f.name.as_str()).collect())
                            .unwrap_or_default();
                        let hint = topaz_diag::suggest::did_you_mean(fname, known.iter().copied());
                        self.former.error(
                            codes::NO_FIELD,
                            format!("record `{rec_name}` has no field `{fname}`{hint}"),
                            field.span,
                        );
                    }
                    let mut field_types: Vec<Type> = candidate_infos
                        .iter()
                        .filter_map(|info| {
                            info.fields
                                .iter()
                                .find(|candidate| candidate.name == fname)
                                .map(|candidate| candidate.ty.clone())
                        })
                        .collect();
                    if has_gradual {
                        field_types.push(Type::Unknown);
                    }
                    let field_ty = if field_types.is_empty() {
                        declared_field
                            .map(|field| field.ty)
                            .unwrap_or(Type::Unknown)
                    } else {
                        Type::union(field_types)
                    };
                    match &field.pattern {
                        Some(sub) => {
                            let cov = self.bind_match_pattern_at(sub, &field_ty, false);
                            all_irrefutable &= cov.irrefutable;
                        }
                        None => {
                            // Shorthand `{ name }` binds the field name irrefutably.
                            if !duplicate_source {
                                self.bind_decl(fname.to_string(), field_ty, false, field.span);
                            }
                        }
                    }
                }
                if declared_info.is_some() && all_irrefutable {
                    if let Some(base) = head_base {
                        cov.nominal_records.insert(base);
                    }
                    if all_members_match {
                        cov.irrefutable = true;
                    }
                }
            }
            _ => unreachable!("pattern family changed after classification"),
        }
        cov
    }

    pub(super) fn bind_sequence_pattern(
        &mut self,
        pattern: &'a ast::Pattern,
        s: &Type,
        top_level: bool,
    ) -> Coverage {
        let mut cov = Coverage::default();
        match &pattern.kind {
            ast::PatternKind::List(elems) => {
                // Like the record arm: a list pattern matches an `Array`
                // scrutinee, or any array MEMBER of a union scrutinee.
                let arrays: Vec<&Type> = match s {
                    Type::Ctor(Ctor::Array, _) => vec![s],
                    Type::Union(ms) => ms
                        .iter()
                        .filter(|m| matches!(m, Type::Ctor(Ctor::Array, _)))
                        .collect(),
                    _ => Vec::new(),
                };
                // SPLIT opacity like the record arm: a RIGID member (a generic
                // `T` — Skolem/Foreign) yields an `ElemOf<T>` element PROJECTION,
                // which can neither discharge a concrete element type nor be used
                // as `T`; a GRADUAL member (Unknown/Var) stays `Unknown`. Either
                // suppresses "can never match" (an opaque member might be an array).
                let rigid_members: Vec<Type> = match s {
                    Type::Skolem { .. } | Type::Foreign { .. } => vec![s.clone()],
                    Type::Union(ms) => ms
                        .iter()
                        .filter(|m| matches!(m, Type::Skolem { .. } | Type::Foreign { .. }))
                        .cloned()
                        .collect(),
                    _ => Vec::new(),
                };
                let has_gradual = s.has_unknown();
                let any_opaque = !rigid_members.is_empty() || has_gradual;
                if arrays.is_empty() && !any_opaque && decidable_type(s) {
                    let display = s.clone();
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        format!("this pattern can never match `{display}`"),
                        pattern.span,
                    );
                }
                // Element type = union of the array members' elements + an
                // `ElemOf<T>` per rigid member (+ `Unknown` if any gradual member).
                // A `...rest` binds the array of those same elements — a matched
                // list pattern proves the rigid value WAS an array.
                let mut elem_tys: Vec<Type> = arrays
                    .iter()
                    .copied()
                    .filter_map(|a| match a {
                        Type::Ctor(Ctor::Array, inner) => Some(inner[0].clone()),
                        _ => None,
                    })
                    .collect();
                let mut rest_tys: Vec<Type> = arrays.iter().copied().cloned().collect();
                for rm in &rigid_members {
                    let elem_proj = self.project(format!("ElemOf<{rm}>"));
                    rest_tys.push(Type::Ctor(Ctor::Array, vec![elem_proj.clone()]));
                    elem_tys.push(elem_proj);
                }
                if has_gradual {
                    elem_tys.push(Type::Unknown);
                    rest_tys.push(Type::Unknown);
                }
                let elem_ty = if elem_tys.is_empty() {
                    Type::Unknown
                } else {
                    Type::union(elem_tys)
                };
                let rest_ty = if rest_tys.is_empty() {
                    Type::Unknown
                } else {
                    Type::union(rest_tys)
                };
                for elem in elems {
                    match elem {
                        ast::ListPatternElem::Pattern(p) => {
                            self.bind_match_pattern_at(p, &elem_ty, false);
                        }
                        ast::ListPatternElem::Rest(Some(p)) => {
                            self.bind_match_pattern_at(p, &rest_ty, false);
                        }
                        ast::ListPatternElem::Rest(None) => {}
                    }
                }
                if matches!(s, Type::Ctor(Ctor::Array, _))
                    && list_pattern_matches_every_array(elems)
                {
                    cov.irrefutable = true;
                }
            }
            ast::PatternKind::Or(alts) => {
                // §6 (v5.4) BINDING or-pattern AGREEMENT. Each alternative is bound
                // in its OWN scratch scope, so two alternatives binding the same name
                // (`A(x) | B(x)`) do NOT collide as a same-scope redeclaration; the
                // captured (name → type) maps are then reconciled and the agreed
                // names are bound ONCE into the real arm scope. (At `< V5_4` the
                // parser admits no bindings here, so every alternative's map is empty
                // and this degenerates to the old coverage-only union.)
                let mut alt_binds: Vec<HashMap<String, (Type, Span)>> = Vec::new();
                for alt in alts {
                    // An or-alternative stays at the SAME position as the or-pattern
                    // (a top-level `case Red | Blue =>` keeps the strict gating).
                    self.push_scope();
                    let sub = self.bind_match_pattern_at(alt, s, top_level);
                    cov.merge(sub);
                    // Capture this alternative's bindings (name → type), located at
                    // the alternative for the agreement diagnostics, then drop the
                    // scratch scope.
                    let captured: HashMap<String, (Type, Span)> = self
                        .scopes
                        .last()
                        .expect("scratch scope")
                        .bindings
                        .iter()
                        .map(|(n, t)| (n.clone(), (t.clone(), alt.span)))
                        .collect();
                    self.pop_scope();
                    alt_binds.push(captured);
                }
                // The agreed name set = the UNION of every alternative's names; any
                // name missing from SOME alternative is a TPZ5710 (the body could
                // reference it after that alternative matched, leaving it unbound).
                let mut all_names: Vec<String> = alt_binds
                    .iter()
                    .flat_map(|m| m.keys().cloned())
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect();
                all_names.sort();
                for name in &all_names {
                    let mut tys: Vec<Type> = Vec::new();
                    let mut missing = false;
                    for m in &alt_binds {
                        match m.get(name) {
                            Some((t, _)) => tys.push(t.clone()),
                            None => missing = true,
                        }
                    }
                    if missing {
                        // Locate the error at the FIRST alternative that DOES bind the
                        // name (it is the surprising one — the others lack it).
                        let span = alt_binds
                            .iter()
                            .find_map(|m| m.get(name).map(|(_, sp)| *sp))
                            .unwrap_or(pattern.span);
                        self.former.error(
                            codes::OR_PATTERN_NAMES,
                            format!(
                                "or-pattern alternatives must bind the same names — `{name}` is bound in one alternative but not another"
                            ),
                            span,
                        );
                        // Recover by binding `name` at the union of where it WAS
                        // bound, so the arm body still sees it (no cascade of
                        // unbound-name errors).
                        let span = alt_binds
                            .iter()
                            .find_map(|m| m.get(name).map(|(_, sp)| *sp))
                            .unwrap_or(pattern.span);
                        self.bind_decl(name.clone(), Type::union(tys), false, span);
                        continue;
                    }
                    // A name bound by EVERY alternative must have one connected
                    // static-overlap component. Check the whole component instead of
                    // folding in source order: `1 | 2 | int` and `int | 1 | 2`
                    // describe the same agreement because `int` connects the two
                    // literals. A gradual member connects the staged component. The
                    // arm body sees the normalized UNION of every alternative type.
                    let conflict = disconnected_overlap_pair(&tys);
                    let unified = Type::union(tys);
                    let span = alt_binds
                        .iter()
                        .find_map(|m| m.get(name).map(|(_, sp)| *sp))
                        .unwrap_or(pattern.span);
                    if let Some((a, b)) = conflict {
                        self.former.error(
                            codes::OR_PATTERN_TYPES,
                            format!(
                                "binding `{name}` has type `{a}` in one or-pattern alternative and `{b}` in another"
                            ),
                            span,
                        );
                    }
                    // Bind ONCE at the unified type into the real arm scope.
                    self.bind_decl(name.clone(), unified, false, span);
                }
            }
            _ => unreachable!("pattern family changed after classification"),
        }
        cov
    }

    pub(super) fn literal_from_span(&mut self, span: Span) -> Type {
        let text = self.former.text(span).replace('_', "");
        match text.parse::<i64>() {
            Ok(n) => Type::Literal(Lit::Int(n)),
            Err(_) => {
                self.former.error(
                    codes::MALFORMED_TYPE,
                    format!("integer literal `{text}` is out of range"),
                    span,
                );
                Type::Unknown
            }
        }
    }
}
