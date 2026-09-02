use super::*;

/// First name-binding site inside a pattern, if any (SPEC v5.2 §6:
/// or-pattern alternatives must bind no names; `_` is not a binding).
fn first_binding(pattern: &Pattern) -> Option<Span> {
    match &pattern.kind {
        PatternKind::Wildcard | PatternKind::Literal(_) | PatternKind::Range { .. } => None,
        PatternKind::Binding(name) => Some(name.span),
        PatternKind::Typed { name, .. } => Some(name.span),
        PatternKind::Or(alts) => alts.iter().find_map(first_binding),
        PatternKind::Constructor { args, .. } => args.iter().find_map(first_binding),
        PatternKind::List(elems) => elems.iter().find_map(|e| match e {
            ListPatternElem::Pattern(p) => first_binding(p),
            ListPatternElem::Rest(Some(p)) => first_binding(p),
            ListPatternElem::Rest(None) => None,
        }),
        PatternKind::Record(fields) | PatternKind::NominalRecord { fields, .. } => {
            fields.iter().find_map(|f| match &f.pattern {
                Some(p) => first_binding(p),
                None => Some(f.name.span), // shorthand binds the field name
            })
        }
    }
}
impl Parser<'_> {
    // ---- patterns -------------------------------------------------------

    /// `Pattern ::= OrPattern` (SPEC v5.2 §6, ADR-073). At V5_1 a
    /// pattern is exactly one primary pattern (the v0.1 grammar);
    /// the `|` loop exists only at V5_2. Type-union `|` binds inside
    /// `TypePattern` first because the type parser consumes unions
    /// before this loop ever sees a `|`; `||` is never a separator.
    pub(super) fn pattern(&mut self) -> PResult<Pattern> {
        let lo = self.cur_span().lo;
        let first = self.primary_pattern()?;
        if self.version < LangVersion::V5_2 || !self.at(TokenKind::Pipe) {
            return Ok(first);
        }
        let mut alternatives = vec![first];
        while self.eat(TokenKind::Pipe) {
            alternatives.push(self.primary_pattern()?);
        }
        // (v5.4 §6) BINDING or-patterns: an alternative MAY bind names, so long as
        // every alternative binds the SAME names at unifying types (the checker
        // enforces agreement — TPZ5710/TPZ5711). At v5.1–5.3 an or-pattern
        // alternative must bind NO names (the v5.2 grammar; `_` is allowed), so the
        // rejection is gated `< V5_4`.
        if self.version < LangVersion::V5_4 {
            for alt in &alternatives {
                if let Some(span) = first_binding(alt) {
                    self.error(
                        codes::OR_PATTERN_BINDING,
                        span,
                        "an or-pattern alternative must not bind names (`_` is allowed)",
                    );
                    break;
                }
            }
        }
        Ok(Pattern {
            kind: PatternKind::Or(alternatives),
            span: self.span_from(lo),
        })
    }

    pub(super) fn primary_pattern(&mut self) -> PResult<Pattern> {
        let lo = self.cur_span().lo;
        let kind = match self.peek() {
            TokenKind::Underscore => {
                self.bump();
                PatternKind::Wildcard
            }
            TokenKind::LBracket => self.list_pattern()?,
            TokenKind::LBrace => self.record_pattern()?,
            TokenKind::Ident => match self.peek_at(1) {
                TokenKind::LParen => {
                    let name = self.ident("a constructor name")?;
                    self.bump(); // (
                    let mut args = Vec::new();
                    while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
                        args.push(self.pattern()?);
                        if !self.eat(TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(TokenKind::RParen, "`)`")?;
                    PatternKind::Constructor { name, args }
                }
                TokenKind::Colon => {
                    let name = self.binding_ident("a binding name")?;
                    self.bump(); // :
                    let ty = self.type_()?;
                    PatternKind::Typed { name, ty }
                }
                // v5.4 NOMINAL record pattern `Name { field, … }` — an identifier
                // directly followed by `{`. (Gated only by being v5.4-recognized in
                // the checker; here it parses whenever the head ident precedes a
                // brace, which structural patterns never do.)
                TokenKind::LBrace if self.version >= LangVersion::V5_4 => {
                    let name = self.ident("a record name")?;
                    let fields = match self.record_pattern()? {
                        PatternKind::Record(fields) => fields,
                        _ => unreachable!("record_pattern yields a Record"),
                    };
                    PatternKind::NominalRecord { name, fields }
                }
                // A const-expression endpoint: identifier-led range
                // patterns like `MIN..0` or `BASE + 1..LIMIT`.
                TokenKind::DotDot
                | TokenKind::DotDotLt
                | TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::Star
                | TokenKind::Slash
                | TokenKind::Percent
                | TokenKind::StarStar => return self.const_pattern(),
                _ => {
                    let name = self.ident("a binding name")?;
                    // §6/§22.1: `None` is the polymorphic Option
                    // constructor, not an ordinary variable — bare
                    // `None` is a zero-argument constructor pattern,
                    // never a binding.
                    if self.text(name.span) == "None" {
                        PatternKind::Constructor {
                            name,
                            args: Vec::new(),
                        }
                    } else {
                        PatternKind::Binding(name)
                    }
                }
            },
            _ => return self.const_pattern(),
        };
        Ok(Pattern {
            kind,
            span: self.span_from(lo),
        })
    }

    /// A literal or range pattern: a const expression, which is a
    /// `LiteralPattern` on its own only when it is a plain literal,
    /// and a `RangePattern` endpoint when `..`/`..<` follows.
    pub(super) fn const_pattern(&mut self) -> PResult<Pattern> {
        let lo = self.cur_span().lo;
        let first = self.const_endpoint()?;
        if matches!(self.peek(), TokenKind::DotDot | TokenKind::DotDotLt) {
            let inclusive = self.bump().kind == TokenKind::DotDot;
            let hi = self.const_endpoint()?;
            return Ok(Pattern {
                kind: PatternKind::Range {
                    lo: Rc::new(first),
                    hi: Rc::new(hi),
                    inclusive,
                },
                span: self.span_from(lo),
            });
        }
        if matches!(
            first.kind,
            ExprKind::Int
                | ExprKind::Float
                | ExprKind::String(_)
                | ExprKind::Bool(_)
                | ExprKind::Null
                | ExprKind::Unit
        ) {
            return Ok(Pattern {
                span: first.span,
                kind: PatternKind::Literal(Rc::new(first)),
            });
        }
        self.error(codes::UNEXPECTED_TOKEN, first.span, "expected a pattern");
        Err(Abort)
    }

    /// A range-pattern endpoint: a const expression parsed above the
    /// range level so `..`/`..<` stays with the pattern.
    pub(super) fn const_endpoint(&mut self) -> PResult<Expr> {
        self.expr_bp(RANGE_BP + 1)
    }

    pub(super) fn list_pattern(&mut self) -> PResult<PatternKind> {
        self.bump(); // [
        let mut elems = Vec::new();
        while !self.at(TokenKind::RBracket) && !self.at(TokenKind::Eof) {
            if self.eat(TokenKind::DotDot) {
                let binding = if self.at(TokenKind::Comma) || self.at(TokenKind::RBracket) {
                    None
                } else {
                    Some(self.pattern()?)
                };
                elems.push(ListPatternElem::Rest(binding));
            } else {
                elems.push(ListPatternElem::Pattern(self.pattern()?));
            }
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RBracket, "`]`")?;
        Ok(PatternKind::List(elems))
    }

    pub(super) fn record_pattern(&mut self) -> PResult<PatternKind> {
        let open = self.bump(); // {
        let mut fields = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            let lo = self.cur_span().lo;
            // `RecordPatternField ::= FieldName ":" Pattern | Identifier`
            // (SPEC v5.2 §6): an explicit field may be keyword-named at
            // V5_2; the shorthand stays identifier-only (a keyword
            // shorthand would bind a keyword) and a bare keyword falls
            // through to `ident` and its v0.1 diagnostic.
            let name = if self.version >= LangVersion::V5_2
                && matches!(self.peek(), TokenKind::Kw(_))
                && self.peek_at(1) == TokenKind::Colon
            {
                let tok = self.bump();
                Ident { span: tok.span }
            } else {
                self.ident("a field name")?
            };
            let pattern = if self.eat(TokenKind::Colon) {
                Some(self.pattern()?)
            } else {
                // Shorthand binds the field name (§6) — and `None`
                // cannot be a binding name (§22.1).
                if self.text(name.span) == "None" {
                    self.error(
                        codes::RESERVED_BINDING_NAME,
                        name.span,
                        "`None` is the Option constructor (§22.1) and cannot be a binding name",
                    );
                }
                None
            };
            fields.push(RecordPatternField {
                name,
                pattern,
                span: self.span_from(lo),
            });
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        let close = self.expect(TokenKind::RBrace, "`}`")?;
        if fields.is_empty() {
            // SPEC §6: RecordPattern takes at least one field.
            self.error(
                codes::UNEXPECTED_TOKEN,
                Span::new(self.file, open.span.lo, close.span.hi),
                "a record pattern requires at least one field",
            );
        }
        Ok(PatternKind::Record(fields))
    }

    // ---- types ------------------------------------------------------------

    pub(super) fn type_(&mut self) -> PResult<Type> {
        let lo = self.cur_span().lo;
        let first = self.primary_type()?;
        if !self.at(TokenKind::Pipe) {
            return Ok(first);
        }
        let mut members = vec![first];
        while self.eat(TokenKind::Pipe) {
            members.push(self.primary_type()?);
        }
        Ok(Type {
            kind: TypeKind::Union(members),
            span: self.span_from(lo),
        })
    }

    pub(super) fn primary_type(&mut self) -> PResult<Type> {
        let lo = self.cur_span().lo;
        let kind = match self.peek() {
            TokenKind::Ident => {
                let name = self.ident("a type name")?;
                // `QualifiedNamedType ::= Identifier "." Identifier
                // TypeArgs?` (SPEC v5.2 §3): v5.2 only; v5.1 type
                // position has no `.` and keeps its v0.1 diagnostic.
                if self.version >= LangVersion::V5_2 && self.at(TokenKind::Dot) {
                    self.bump(); // .
                    let member = self.ident("an exported type name")?;
                    let args = if self.at(TokenKind::Lt) {
                        self.type_reference_args()?
                    } else {
                        Vec::new()
                    };
                    TypeKind::Qualified {
                        ns: name,
                        name: member,
                        args,
                    }
                } else {
                    let args = if self.at(TokenKind::Lt) {
                        self.type_reference_args()?
                    } else {
                        Vec::new()
                    };
                    TypeKind::Named { name, args }
                }
            }
            TokenKind::LBrace => {
                let open = self.bump();
                let mut fields = Vec::new();
                while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
                    let flo = self.cur_span().lo;
                    let name = self.field_name("a field name")?;
                    self.expect(TokenKind::Colon, "`:`")?;
                    let ty = self.type_()?;
                    fields.push(FieldType {
                        name,
                        ty,
                        span: self.span_from(flo),
                    });
                    if !self.eat(TokenKind::Comma) {
                        break;
                    }
                }
                let close = self.expect(TokenKind::RBrace, "`}`")?;
                if fields.is_empty() {
                    // SPEC §3: RecordType takes at least one field.
                    self.error(
                        codes::UNEXPECTED_TOKEN,
                        Span::new(self.file, open.span.lo, close.span.hi),
                        "a record type requires at least one field",
                    );
                }
                TypeKind::Record(fields)
            }
            TokenKind::LParen => return self.paren_type(),
            TokenKind::Int
            | TokenKind::Float
            | TokenKind::Kw(Keyword::True)
            | TokenKind::Kw(Keyword::False)
            | TokenKind::Kw(Keyword::Null) => {
                self.bump();
                TypeKind::Literal
            }
            TokenKind::StringStart { .. } => {
                let lit = self.string_lit()?;
                if lit
                    .parts
                    .iter()
                    .any(|p| matches!(p, StringPart::Interpolation(_)))
                {
                    self.error(
                        codes::UNEXPECTED_TOKEN,
                        lit.span,
                        "a literal type cannot contain interpolation",
                    );
                }
                TypeKind::Literal
            }
            _ => return Err(self.error_here("expected a type")),
        };
        Ok(Type {
            kind,
            span: self.span_from(lo),
        })
    }

    /// At `(` in type position: the unit type `()`, a function type
    /// `(params) -> Ret`, or a parenthesized type.
    pub(super) fn paren_type(&mut self) -> PResult<Type> {
        let lo = self.cur_span().lo;
        if self.closing_paren_is_followed_by(TokenKind::ThinArrow) {
            self.bump(); // (
            let mut params = Vec::new();
            while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
                let variadic = self.eat(TokenKind::Ellipsis);
                let ty = self.type_()?;
                params.push(FunctionTypeParam { ty, variadic });
                if !self.eat(TokenKind::Comma) {
                    break;
                }
            }
            self.expect(TokenKind::RParen, "`)`")?;
            self.expect(TokenKind::ThinArrow, "`->`")?;
            let ret = self.type_()?;
            return Ok(Type {
                kind: TypeKind::Function {
                    params,
                    ret: Box::new(ret),
                },
                span: self.span_from(lo),
            });
        }
        self.bump(); // (
        if self.eat(TokenKind::RParen) {
            return Ok(Type {
                kind: TypeKind::Unit,
                span: self.span_from(lo),
            });
        }
        let inner = self.type_()?;
        self.expect(TokenKind::RParen, "`)`")?;
        Ok(Type {
            kind: inner.kind,
            span: self.span_from(lo),
        })
    }

    pub(super) fn type_args(&mut self) -> PResult<Vec<Type>> {
        self.bump(); // <
        let mut args = vec![self.type_()?];
        while self.eat(TokenKind::Comma) {
            args.push(self.type_()?);
        }
        self.expect_close_angle()?;
        Ok(args)
    }

    pub(super) fn type_reference_args(&mut self) -> PResult<Vec<Rc<Type>>> {
        self.type_args()
            .map(|args| args.into_iter().map(Rc::new).collect())
    }

    /// Expects the `>` that closes a type-argument or type-parameter
    /// list, splitting a `>>` into two `>` when needed (CDR-001 §6).
    pub(super) fn expect_close_angle(&mut self) -> PResult<()> {
        match self.peek() {
            TokenKind::Gt => {
                self.bump();
                Ok(())
            }
            TokenKind::GtGt => {
                let tok = self.bump();
                // One `>` consumed now; one stays pending for the
                // enclosing list.
                self.pending_gt = Some(Span::new(self.file, tok.span.lo + 1, tok.span.hi));
                self.last_hi = tok.span.lo + 1;
                Ok(())
            }
            _ => Err(self.error_here("expected `>`")),
        }
    }
}
