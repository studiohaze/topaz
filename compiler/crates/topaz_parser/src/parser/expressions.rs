use super::*;

impl Parser<'_> {
    // ---- expressions --------------------------------------------------

    pub(super) fn expr(&mut self) -> PResult<Expr> {
        self.expr_bp(1)
    }

    /// An expression nested inside a grouping delimiter. Case-guard lambda and
    /// construct-body record-update restrictions apply only at the outer level,
    /// so both forms are re-allowed here.
    pub(super) fn expr_nested(&mut self) -> PResult<Expr> {
        let saved_lambda = std::mem::replace(&mut self.naked_lambda_ok, true);
        let result = self.expr_record_nested();
        self.naked_lambda_ok = saved_lambda;
        result
    }

    /// Re-allows record updates inside a delimiter while preserving any
    /// independent lambda restriction owned by the surrounding grammar.
    pub(super) fn expr_record_nested(&mut self) -> PResult<Expr> {
        let saved = std::mem::replace(&mut self.record_update_ok, true);
        let result = self.expr();
        self.record_update_ok = saved;
        result
    }

    /// Parses the expression immediately followed by a construct-owned block.
    /// Only the expression's outer level is protected: nested delimiters use
    /// `expr_nested` and retain ordinary postfix record updates.
    pub(super) fn expr_before_block(&mut self) -> PResult<Expr> {
        let saved = std::mem::replace(&mut self.record_update_ok, false);
        let result = self.expr();
        self.record_update_ok = saved;
        result
    }

    pub(super) fn expr_bp(&mut self, min_bp: u8) -> PResult<Expr> {
        let lo = self.cur_span().lo;
        let mut lhs = self.unary()?;
        loop {
            let kind = self.peek();
            let Some(bp) = binary_bp(kind) else { break };
            if bp < min_bp {
                break;
            }
            self.bump();
            lhs = match kind {
                TokenKind::PipeGt => {
                    let rhs = if self.eat(TokenKind::Dot) {
                        // `PipeRhs ::= Expression | "." Identifier`
                        // (SPEC §11): the pipe field sugar is NOT one
                        // of ADR-075's seven FieldName positions and
                        // stays identifier-only at every version.
                        PipeRhs::Field(self.ident("a field name")?)
                    } else {
                        PipeRhs::Expr(Rc::new(self.expr_bp(bp + 1)?))
                    };
                    Expr {
                        kind: ExprKind::Pipe {
                            lhs: Rc::new(lhs),
                            rhs: Rc::new(rhs),
                        },
                        span: self.span_from(lo),
                    }
                }
                TokenKind::GtGt => {
                    // Right-associative composition (§2 level 11).
                    let rhs = self.expr_bp(bp)?;
                    Expr {
                        kind: ExprKind::Compose {
                            lhs: Rc::new(lhs),
                            rhs: Rc::new(rhs),
                        },
                        span: self.span_from(lo),
                    }
                }
                TokenKind::DotDot | TokenKind::DotDotLt => {
                    // A range endpoint is REQUIRED. If a stopper (clause end,
                    // closer, separator, or `by`) follows `..`, the endpoint is
                    // missing — point at the `..` itself with a specific message
                    // and recover with a synthetic endpoint so `(1..)` / `[1..]`
                    // do not cascade. (A `{` is intentionally NOT a stopper: it can
                    // begin a block-expression endpoint, and disambiguating that
                    // from a `for` body is a separate concern.)
                    //
                    // CROSS-STATEMENT note: for `1..\nlet x = 2`, `..` is a binary
                    // trailing-continuation operator (SPEC §1a, like `+`/`*`/…), so
                    // it absorbs the newline and the two statements are not
                    // separated. The win is the PRIMARY: it points at the `..` with
                    // the range-endpoint hint instead of a vague "expected an
                    // expression" at `let`. A secondary "expected a statement
                    // separator" still follows (and, as with any operator's newline
                    // continuation, the trailing statement is not separately
                    // recovered) — operator uniformity, not a range-specific defect.
                    let dotdot = self.token_at(self.pos - 1).span;
                    // The endpoint is MISSING when the next token cannot begin an
                    // expression. `..` is a trailing-continuation operator, so it
                    // also swallows the newline before a following statement — hence
                    // EVERY keyword except the expression-starting ones
                    // (`if`/`match`/`for`/`concurrent` and the `true`/`false`/`null`
                    // literals) is a stopper, alongside separators, closers, `,`, and
                    // `=>`. Recover with a synthetic endpoint so nothing cascades.
                    let missing_endpoint = match self.peek() {
                        TokenKind::Sep
                        | TokenKind::Eof
                        | TokenKind::RParen
                        | TokenKind::RBrace
                        | TokenKind::RBracket
                        | TokenKind::Comma
                        | TokenKind::FatArrow => true,
                        TokenKind::Kw(k) => !matches!(
                            k,
                            Keyword::If
                                | Keyword::Match
                                | Keyword::For
                                | Keyword::Concurrent
                                | Keyword::True
                                | Keyword::False
                                | Keyword::Null
                        ),
                        _ => false,
                    };
                    let hi = if missing_endpoint {
                        self.error(
                            codes::UNEXPECTED_TOKEN,
                            dotdot,
                            "expected a range endpoint after `..`, e.g. `1..5`",
                        );
                        lhs.clone()
                    } else {
                        self.expr_bp(bp + 1)?
                    };
                    let step = if self.eat(TokenKind::Kw(Keyword::By)) {
                        Some(Rc::new(self.expr_bp(bp + 1)?))
                    } else {
                        None
                    };
                    Expr {
                        kind: ExprKind::Range {
                            lo: Rc::new(lhs),
                            hi: Rc::new(hi),
                            inclusive: kind == TokenKind::DotDot,
                            step,
                        },
                        span: self.span_from(lo),
                    }
                }
                _ => {
                    let op = binary_op(kind);
                    let rhs = self.expr_bp(bp + 1)?;
                    Expr {
                        kind: ExprKind::Binary {
                            op,
                            lhs: Rc::new(lhs),
                            rhs: Rc::new(rhs),
                        },
                        span: self.span_from(lo),
                    }
                }
            };
        }
        Ok(lhs)
    }

    pub(super) fn unary(&mut self) -> PResult<Expr> {
        let lo = self.cur_span().lo;
        // `~` is reserved (TPZ2013): Topaz is arithmetic-only, no bitwise. The
        // lexer keeps the token for recovery; reject it here and parse the
        // operand so later diagnostics still surface.
        if self.peek() == TokenKind::Tilde {
            self.error(
                codes::RESERVED_OPERATOR,
                self.cur_span(),
                "reserved operator `~`: Topaz has no bitwise operations",
            );
            self.bump();
            return self.unary();
        }
        let op = match self.peek() {
            TokenKind::Plus => Some(UnaryOp::Plus),
            TokenKind::Minus => Some(UnaryOp::Minus),
            TokenKind::Bang => Some(UnaryOp::Not),
            _ => None,
        };
        if let Some(op) = op {
            self.bump();
            let operand = self.unary()?;
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op,
                    operand: Rc::new(operand),
                },
                span: self.span_from(lo),
            });
        }
        self.power()
    }

    pub(super) fn power(&mut self) -> PResult<Expr> {
        let lo = self.cur_span().lo;
        let lhs = self.postfix()?;
        if self.eat(TokenKind::StarStar) {
            // `**` binds tighter than unary and is right-associative
            // (§2 level 2); the exponent may itself be unary.
            let rhs = self.unary()?;
            return Ok(Expr {
                kind: ExprKind::Binary {
                    op: BinaryOp::Pow,
                    lhs: Rc::new(lhs),
                    rhs: Rc::new(rhs),
                },
                span: self.span_from(lo),
            });
        }
        Ok(lhs)
    }

    pub(super) fn postfix(&mut self) -> PResult<Expr> {
        let lo = self.cur_span().lo;
        let mut e = self.atom()?;
        loop {
            match self.peek() {
                TokenKind::LParen => {
                    let args = self.call_args()?;
                    e = Expr {
                        kind: ExprKind::Call {
                            callee: Rc::new(e),
                            args,
                            type_args: Vec::new(),
                        },
                        span: self.span_from(lo),
                    };
                }
                TokenKind::LBracket => {
                    self.bump();
                    let index = self.expr_nested()?;
                    self.expect(TokenKind::RBracket, "`]`")?;
                    e = Expr {
                        kind: ExprKind::Index {
                            object: Rc::new(e),
                            index: Rc::new(index),
                        },
                        span: self.span_from(lo),
                    };
                }
                TokenKind::Dot => {
                    self.bump();
                    let field = self.field_name("a member name")?;
                    e = Expr {
                        kind: ExprKind::Member {
                            object: Rc::new(e),
                            field,
                        },
                        span: self.span_from(lo),
                    };
                }
                TokenKind::QuestionDot => {
                    self.bump();
                    let field = self.field_name("a member name")?;
                    e = Expr {
                        kind: ExprKind::OptionalAccess {
                            object: Rc::new(e),
                            field,
                        },
                        span: self.span_from(lo),
                    };
                }
                TokenKind::Question => {
                    self.bump();
                    e = Expr {
                        kind: ExprKind::Try(Rc::new(e)),
                        span: self.span_from(lo),
                    };
                }
                // Postfix record update (SPEC §5/§8): `{` directly
                // after a complete expression, opening `Identifier
                // ":"` — mirrors the layout normalizer's rule.
                TokenKind::LBrace
                    if self.record_update_ok
                        && self.is_field_name_token(self.peek_at(1))
                        && self.peek_at(2) == TokenKind::Colon =>
                {
                    let (spread, fields) = self.record_construct_fields()?;
                    e = Expr {
                        kind: ExprKind::RecordUpdate {
                            base: Rc::new(e),
                            spread,
                            fields,
                        },
                        span: self.span_from(lo),
                    };
                }
                // §3 (v5.4) NOMINAL spread-update `User { ...base, field: … }` — a
                // `{` opening with `...` (a leading spread) DIRECTLY after a bare
                // IDENTIFIER. The checker decides whether `R` is a declared record
                // (nominal spread-update) or rejects it. Gated `>= V5_4`; MVP accepts
                // exactly ONE leading spread (record_construct_fields rejects more).
                TokenKind::LBrace
                    if self.record_update_ok
                        && self.version >= LangVersion::V5_4
                        && self.peek_at(1) == TokenKind::Ellipsis
                        && matches!(e.kind, ExprKind::Ident) =>
                {
                    let (spread, fields) = self.record_construct_fields()?;
                    e = Expr {
                        kind: ExprKind::RecordUpdate {
                            base: Rc::new(e),
                            spread,
                            fields,
                        },
                        span: self.span_from(lo),
                    };
                }
                // §3 (v5.4) EMPTY nominal construction `R {}` — an empty brace
                // DIRECTLY after a bare IDENTIFIER (so it cannot steal a structural
                // block, which never sits in postfix position after an ident). The
                // checker decides whether `R` is a declared record (empty/all-default
                // construction) or an ordinary value (an empty, no-op update). Gated
                // `>= V5_4` so the v5.1–5.3 grammar is unchanged.
                TokenKind::LBrace
                    if self.record_update_ok
                        && self.version >= LangVersion::V5_4
                        && self.peek_at(1) == TokenKind::RBrace
                        && matches!(e.kind, ExprKind::Ident) =>
                {
                    self.bump(); // `{`
                    self.bump(); // `}`
                    e = Expr {
                        kind: ExprKind::RecordUpdate {
                            base: Rc::new(e),
                            spread: None,
                            fields: Vec::new(),
                        },
                        span: self.span_from(lo),
                    };
                }
                // §3 (v5.4) EXPLICIT call-site type arguments `f<T>(args)` /
                // `Map.new<K, V>()` / `Set.of<T>()`. Recognized only when the
                // callee is an `Ident`/`Member`, a STRICT speculative scan proves
                // a well-formed type list, and its closing `>` is IMMEDIATELY
                // ADJACENT to `(` — so `x < int > (y)` stays a comparison and the
                // `<` falls through unchanged. Gated `>= V5_4`; CHECK-only (the
                // type-args feed the checker and are type-erased at run/build).
                TokenKind::Lt
                    if self.version >= LangVersion::V5_4
                        && matches!(
                            e.kind,
                            ExprKind::Ident
                                | ExprKind::Member { .. }
                                | ExprKind::OptionalAccess { .. }
                        )
                        && self.looks_like_call_type_args() =>
                {
                    let type_args = self.type_args()?;
                    let args = self.call_args()?;
                    e = Expr {
                        kind: ExprKind::Call {
                            callee: Rc::new(e),
                            args,
                            type_args,
                        },
                        span: self.span_from(lo),
                    };
                }
                _ => break,
            }
        }
        Ok(e)
    }

    pub(super) fn atom(&mut self) -> PResult<Expr> {
        let lo = self.cur_span().lo;
        let kind = match self.peek() {
            TokenKind::Int => {
                self.bump();
                ExprKind::Int
            }
            TokenKind::Float => {
                self.bump();
                ExprKind::Float
            }
            TokenKind::Duration(_) => {
                // SPEC §15: duration literals exist only in the
                // `concurrent(timeout: ...)` clause, which consumes
                // them directly — they are not §1 literals.
                return Err(self
                    .error_here("a duration literal is valid only in `concurrent(timeout: ...)`"));
            }
            TokenKind::Kw(Keyword::True) => {
                self.bump();
                ExprKind::Bool(true)
            }
            TokenKind::Kw(Keyword::False) => {
                self.bump();
                ExprKind::Bool(false)
            }
            TokenKind::Kw(Keyword::Null) => {
                self.bump();
                ExprKind::Null
            }
            TokenKind::Underscore => {
                self.bump();
                ExprKind::Placeholder
            }
            TokenKind::Ident => {
                if self.naked_lambda_ok && self.peek_at(1) == TokenKind::FatArrow {
                    return self.single_param_lambda();
                }
                // `loop` is contextual, preserving the locked 21-keyword
                // v5.1-v5.3 surface and ordinary identifier uses in every mode.
                // Only the exact v5.4+ primary-expression head followed by its
                // optional label and required block selects the loop expression.
                // This bounded lookahead keeps `let loop = 1`, `loop + 1`, and
                // `loop(value)` on the ordinary identifier path.
                if self.version >= LangVersion::V5_4
                    && self.text(self.token_at(self.pos).span) == "loop"
                    && (self.peek_at(1) == TokenKind::LBrace
                        || (self.peek_at(1) == TokenKind::Label
                            && self.peek_at(2) == TokenKind::LBrace))
                {
                    return self.loop_expr();
                }
                // §6 (v5.4) CONTEXTUAL `set { … }` / `map { … }` collection literals.
                // `set`/`map` are NOT keywords — they stay ordinary identifiers
                // EVERYWHERE except this STRICT primary-position lookahead: the
                // identifier text is exactly `set`/`map` AND the very next token is
                // `{`. Outside this shape (`map(xs, f)` free fn, `let map = 1`,
                // `set + 1`) the identifier falls through unchanged. Gated `>= V5_4`.
                if self.version >= LangVersion::V5_4 && self.peek_at(1) == TokenKind::LBrace {
                    let head = self.text(self.token_at(self.pos).span);
                    if head == "set" {
                        return self.set_or_comprehension();
                    } else if head == "map" {
                        return self.map_or_comprehension();
                    }
                }
                self.bump();
                ExprKind::Ident
            }
            TokenKind::StringStart { .. } => ExprKind::String(Rc::new(self.string_lit()?)),
            TokenKind::LParen => return self.paren_unit_or_lambda(),
            TokenKind::LBracket => {
                // §6.4 (v5.4) ARRAY COMPREHENSION `[ for x in xs … => body ]` — a
                // leading `for` right after `[` selects the comprehension; otherwise
                // this is a plain array literal. Gated `>= V5_4` (an array literal can
                // never begin with the `for` keyword pre-5.4 either, so the gate only
                // affects the comprehension form).
                if self.version >= LangVersion::V5_4
                    && self.peek_at(1) == TokenKind::Kw(Keyword::For)
                {
                    return self.array_comprehension();
                }
                self.array_literal()?
            }
            TokenKind::LBrace => {
                if self.is_field_name_token(self.peek_at(1)) && self.peek_at(2) == TokenKind::Colon
                {
                    ExprKind::RecordLiteral {
                        fields: self.record_fields()?,
                    }
                } else {
                    ExprKind::Block(Rc::new(self.block()?))
                }
            }
            TokenKind::Kw(Keyword::If) => return self.if_expr(),
            TokenKind::Kw(Keyword::Match) => return self.match_expr(),
            TokenKind::Kw(Keyword::For) => return self.for_expr(),
            TokenKind::Kw(Keyword::Concurrent) => return self.concurrent_expr(),
            _ => return Err(self.error_here("expected an expression")),
        };
        Ok(Expr {
            kind,
            span: self.span_from(lo),
        })
    }

    pub(super) fn single_param_lambda(&mut self) -> PResult<Expr> {
        let lo = self.cur_span().lo;
        let name = self.binding_ident("a parameter name")?;
        let params = vec![LambdaParam {
            name,
            ty: None,
            span: name.span,
        }];
        self.expect(TokenKind::FatArrow, "`=>`")?;
        let body = self.expr()?;
        Ok(Expr {
            kind: ExprKind::Lambda {
                params,
                body: Rc::new(body),
            },
            span: self.span_from(lo),
        })
    }

    /// At `(`: a lambda parameter list (when the matching `)` is
    /// followed by `=>`), the unit literal `()`, or a parenthesized
    /// expression.
    pub(super) fn paren_unit_or_lambda(&mut self) -> PResult<Expr> {
        let lo = self.cur_span().lo;
        if self.naked_lambda_ok && self.closing_paren_is_followed_by(TokenKind::FatArrow) {
            return self.paren_lambda();
        }
        let lparen_pos = self.pos; // for the explicit-type-argument heuristic below
        self.bump(); // (
        if self.eat(TokenKind::RParen) {
            return Ok(Expr {
                kind: ExprKind::Unit,
                span: self.span_from(lo),
            });
        }
        let inner = self.expr_nested()?;
        if self.at(TokenKind::Comma) {
            // `(a, b)` is not Topaz syntax — parentheses group a SINGLE expression
            // (there are no tuples; a lambda parameter list took the `=> ` branch
            // above). Report ONCE and recover to the matching `)` so a comma list
            // does not abort into a multi-diagnostic cascade.
            let mut diag = Diagnostic::error(
                codes::UNEXPECTED_TOKEN,
                "parentheses group a single expression; `(a, b)` comma lists are not Topaz syntax",
                Label::new(self.cur_span(), ""),
            );
            // When the `(` directly follows a `callee<type-list>` window, the user
            // almost certainly attempted call-site type arguments (`Array.of<int>(…)`)
            // — which Topaz infers. Add that as a NOTE. This NEVER changes the parse
            // and cannot misfire on a valid comparison like `a < b > (c)` (which has
            // no comma in its parens, so it never reaches here). Full detection of
            // the parse-ok shapes (`Array.of<int>(1)`) belongs to the checker.
            if self.looks_like_explicit_type_args(lparen_pos) {
                diag = diag.with_note(
                    "Topaz infers type arguments — drop the explicit `<…>`, e.g. `Array.of(…)`",
                );
            }
            self.diagnostics.push(diag);
            self.recover_to_matching_rparen();
            return Ok(Expr {
                kind: ExprKind::Paren(Rc::new(inner)),
                span: self.span_from(lo),
            });
        }
        self.expect(TokenKind::RParen, "`)`")?;
        Ok(Expr {
            kind: ExprKind::Paren(Rc::new(inner)),
            span: self.span_from(lo),
        })
    }

    /// Whether the raw tokens immediately before the `(` at `lparen_pos` form a
    /// `callee < type-list > ` window — i.e. `Ident` `<` (`Ident`/`,`)+ `>` `(`.
    /// A purely SYNTACTIC heuristic used only to ATTACH A NOTE to an
    /// already-reported error; it never drives a parse decision.
    pub(super) fn looks_like_explicit_type_args(&self, lparen_pos: usize) -> bool {
        if lparen_pos == 0 || self.token_at(lparen_pos - 1).kind != TokenKind::Gt {
            return false;
        }
        // Walk back from the `>` to its matching `<`, allowing only a type-list of
        // identifiers and commas in between (no separators, no other operators).
        let mut i = lparen_pos - 1;
        let mut depth = 0i32;
        loop {
            match self.token_at(i).kind {
                TokenKind::Gt => depth += 1,
                TokenKind::Lt => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                TokenKind::Ident | TokenKind::Comma => {}
                _ => return false,
            }
            if i == 0 {
                return false;
            }
            i -= 1;
        }
        // The token before the `<` must be the callee name (`Array.of` ends in an
        // `Ident`; a bare `f` is an `Ident` too).
        i > 0 && self.token_at(i - 1).kind == TokenKind::Ident
    }

    /// Speculative, ALLOCATION-FREE forward scan at a `<` in postfix position
    /// (the callee is already an `Ident`/`Member`): whether the tokens from the
    /// cursor form an EXPLICIT call-site type-argument list `<T, U>(` — a
    /// comma-separated list of well-formed types whose closing `>` is
    /// IMMEDIATELY ADJACENT to a following `(` (no intervening byte). Adjacency
    /// is the disambiguator: `f<int>(y)` (adjacent) is a type-arg call, while
    /// `x < int > (y)` (a space before `(`) stays a comparison. The scan never
    /// mutates parser state; on `true` the caller commits via `type_args()`.
    ///
    /// Token-level type grammar (a STRICTER superset of the heuristic above,
    /// matching `primary_type`'s leaves): identifiers, `.` (qualified names),
    /// nested `<…>`/`>>` (angle depth), `(…)`/`->`/`...` (function types),
    /// `{…}` (record types), `|` (unions), and literal-type leaves. A token
    /// outside this set, or a `<` group that never closes adjacent to `(`,
    /// fails the scan (the `<` is then a comparison operator).
    pub(super) fn looks_like_call_type_args(&self) -> bool {
        debug_assert_eq!(self.peek(), TokenKind::Lt);
        debug_assert!(self.pending_gt.is_none());
        // `angle` = open `<` groups; `paren`/`brace` = nesting inside a type
        // (function-type parens, record-type braces) so a `>`/`(` there is not
        // the call's close. The scan starts AT the opening `<` (angle becomes 1).
        let mut angle = 0i32;
        let mut paren = 0i32;
        let mut brace = 0i32;
        let mut i = self.pos;
        // A non-empty list and a balanced, well-formed shape are required; an
        // empty `<>` is rejected (no zero type-argument call form).
        let mut saw_type_token = false;
        loop {
            let tok = self.token_at(i);
            match tok.kind {
                TokenKind::Lt => {
                    angle += 1;
                }
                TokenKind::Gt | TokenKind::GtGt => {
                    // `>>` closes two angle groups (CDR-001 §6 split); a single
                    // `>` closes one. Only meaningful at the type's top nesting
                    // (not inside a function-type paren / record brace).
                    if paren == 0 && brace == 0 {
                        angle -= if tok.kind == TokenKind::GtGt { 2 } else { 1 };
                        if angle <= 0 {
                            // The list closed. The byte after this close token
                            // must be `(` with NO gap (adjacency). For `>>` the
                            // adjacent half is its second `>`, whose hi is the
                            // `>>` token's hi — so the same span test holds.
                            let next = self.token_at(i + 1);
                            // `angle < 0` means a `>>` overshot a single open
                            // group: `a < b >> (` is not a type-arg list.
                            return angle == 0
                                && saw_type_token
                                && next.kind == TokenKind::LParen
                                && tok.span.hi == next.span.lo;
                        }
                        saw_type_token = true;
                    } else if tok.kind == TokenKind::Gt {
                        // A `>` inside a function-type paren / record brace is not
                        // a type-list separator and not a valid type token here.
                        return false;
                    } else {
                        return false;
                    }
                }
                TokenKind::LParen => {
                    paren += 1;
                    saw_type_token = true;
                }
                TokenKind::RParen => {
                    paren -= 1;
                    if paren < 0 {
                        return false;
                    }
                }
                TokenKind::LBrace => {
                    brace += 1;
                    saw_type_token = true;
                }
                TokenKind::RBrace => {
                    brace -= 1;
                    if brace < 0 {
                        return false;
                    }
                }
                // Valid type leaves and connectives.
                TokenKind::Ident
                | TokenKind::Dot
                | TokenKind::ThinArrow
                | TokenKind::Ellipsis
                | TokenKind::Pipe
                | TokenKind::Colon
                | TokenKind::Int
                | TokenKind::Float
                | TokenKind::Kw(Keyword::True)
                | TokenKind::Kw(Keyword::False)
                | TokenKind::Kw(Keyword::Null)
                | TokenKind::StringStart { .. }
                | TokenKind::StringEnd
                | TokenKind::StringText => {
                    saw_type_token = true;
                }
                // A comma is a list separator; it carries no type token of its own.
                TokenKind::Comma => {}
                // Anything else (operators, `=`, `;`, EOF, …) cannot appear in a
                // type-argument list — this is a comparison, not type-args.
                _ => return false,
            }
            i += 1;
            // A runaway scan (unbalanced, never closes) bails at EOF.
            if tok.kind == TokenKind::Eof {
                return false;
            }
        }
    }

    /// Whether the `)` matching the `(` at the cursor is immediately
    /// followed by `kind` (token-tree scan, no allocation).
    pub(super) fn closing_paren_is_followed_by(&self, kind: TokenKind) -> bool {
        debug_assert!(self.pending_gt.is_none());
        let mut depth = 0usize;
        let mut i = self.pos;
        while i < self.tokens.len() {
            let k = self.tokens[i].kind;
            if Self::is_opener(k) {
                depth += 1;
            } else if Self::is_closer(k) {
                depth -= 1;
                if depth == 0 {
                    return self.token_at(i + 1).kind == kind;
                }
            }
            i += 1;
        }
        false
    }

    pub(super) fn paren_lambda(&mut self) -> PResult<Expr> {
        let lo = self.cur_span().lo;
        self.bump(); // (
        let mut params = Vec::new();
        while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
            let plo = self.cur_span().lo;
            let name = self.binding_ident("a parameter name")?;
            let ty = if self.eat(TokenKind::Colon) {
                Some(self.type_()?)
            } else {
                None
            };
            params.push(LambdaParam {
                name,
                ty,
                span: self.span_from(plo),
            });
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RParen, "`)`")?;
        self.expect(TokenKind::FatArrow, "`=>`")?;
        let body = self.expr()?;
        Ok(Expr {
            kind: ExprKind::Lambda {
                params,
                body: Rc::new(body),
            },
            span: self.span_from(lo),
        })
    }

    pub(super) fn array_literal(&mut self) -> PResult<ExprKind> {
        self.bump(); // [
        let mut elements = Vec::new();
        while !self.at(TokenKind::RBracket) && !self.at(TokenKind::Eof) {
            if self.eat(TokenKind::Ellipsis) {
                elements.push(ArrayElement::Spread(self.expr_nested()?));
            } else {
                elements.push(ArrayElement::Expr(self.expr_nested()?));
            }
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RBracket, "`]`")?;
        Ok(ExprKind::Array(elements))
    }

    /// §6 (v5.4) `set { e, e, … }` — a SET literal. The leading `set` identifier
    /// and the following `{` were proven by the caller's strict primary-position
    /// lookahead. Comma-separated expressions, a trailing comma allowed, and an
    /// empty `set {}` permitted (it demands an expected type in the checker, like
    /// `[]`). Returns a full `Expr` (its span covers `set { … }`).
    pub(super) fn set_literal(&mut self) -> PResult<Expr> {
        let lo = self.cur_span().lo;
        self.bump(); // `set`
        self.expect(TokenKind::LBrace, "`{`")?;
        let mut elements = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            elements.push(self.expr_nested()?);
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RBrace, "`}`")?;
        Ok(Expr {
            kind: ExprKind::SetLiteral(elements),
            span: self.span_from(lo),
        })
    }

    /// §6 (v5.4) `map { k: v, … }` — a MAP literal. The leading `map` identifier
    /// and the following `{` were proven by the caller. Comma-separated
    /// `key: value` entries (each a general expression key and value), a trailing
    /// comma allowed, and an empty `map {}` permitted (demands an expected type).
    pub(super) fn map_literal(&mut self) -> PResult<Expr> {
        let lo = self.cur_span().lo;
        self.bump(); // `map`
        self.expect(TokenKind::LBrace, "`{`")?;
        let mut entries = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            let key = self.expr_nested()?;
            self.expect(TokenKind::Colon, "`:`")?;
            let value = self.expr_nested()?;
            entries.push((key, value));
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RBrace, "`}`")?;
        Ok(Expr {
            kind: ExprKind::MapLiteral(entries),
            span: self.span_from(lo),
        })
    }

    /// §6 (v5.4) `set { … }`: a SET COMPREHENSION when a `for` clause leads the
    /// braces, otherwise the `set { e, … }` LITERAL. The leading `set` + `{` were
    /// proven by the caller. (Both forms gated `>= V5_4` at the call site.)
    pub(super) fn set_or_comprehension(&mut self) -> PResult<Expr> {
        // `set` then `{` then `for` → comprehension; the caller proved `set {`.
        if self.peek_at(2) == TokenKind::Kw(Keyword::For) {
            return self.set_comprehension();
        }
        self.set_literal()
    }

    /// §6 (v5.4) `map { … }`: a MAP COMPREHENSION when a `for` clause leads the
    /// braces, otherwise the `map { k: v, … }` LITERAL.
    pub(super) fn map_or_comprehension(&mut self) -> PResult<Expr> {
        if self.peek_at(2) == TokenKind::Kw(Keyword::For) {
            return self.map_comprehension();
        }
        self.map_literal()
    }

    /// §6.4 (v5.4) ARRAY COMPREHENSION `[ <clauses> => body ]`. The caller proved
    /// the leading `[` `for`.
    pub(super) fn array_comprehension(&mut self) -> PResult<Expr> {
        let lo = self.cur_span().lo;
        self.bump(); // `[`
        let clauses = self.comprehension_clauses()?;
        let body = self.expr_nested()?;
        let body = Rc::new(CompBody::Elem(Rc::new(body)));
        self.expect(TokenKind::RBracket, "`]`")?;
        Ok(Expr {
            kind: ExprKind::Comprehension {
                kind: CompKind::Array,
                clauses,
                body,
            },
            span: self.span_from(lo),
        })
    }

    /// §6.4 (v5.4) SET COMPREHENSION `set { <clauses> => body }`.
    pub(super) fn set_comprehension(&mut self) -> PResult<Expr> {
        let lo = self.cur_span().lo;
        self.bump(); // `set`
        self.expect(TokenKind::LBrace, "`{`")?;
        let clauses = self.comprehension_clauses()?;
        let body = self.expr_nested()?;
        let body = Rc::new(CompBody::Elem(Rc::new(body)));
        self.expect(TokenKind::RBrace, "`}`")?;
        Ok(Expr {
            kind: ExprKind::Comprehension {
                kind: CompKind::Set,
                clauses,
                body,
            },
            span: self.span_from(lo),
        })
    }

    /// §6.4 (v5.4) MAP COMPREHENSION `map { <clauses> => key: value }`. The body
    /// MUST be a `key: value` entry (a missing `:` is a parse error here; the
    /// checker reports the shape mismatch TPZ5611 when relevant).
    pub(super) fn map_comprehension(&mut self) -> PResult<Expr> {
        let lo = self.cur_span().lo;
        self.bump(); // `map`
        self.expect(TokenKind::LBrace, "`{`")?;
        let clauses = self.comprehension_clauses()?;
        let key = self.expr_nested()?;
        self.expect(
            TokenKind::Colon,
            "`:` (a map comprehension body is `key: value`)",
        )?;
        let value = self.expr_nested()?;
        let body = Rc::new(CompBody::Entry {
            key: Rc::new(key),
            value: Rc::new(value),
        });
        self.expect(TokenKind::RBrace, "`}`")?;
        Ok(Expr {
            kind: ExprKind::Comprehension {
                kind: CompKind::Map,
                clauses,
                body,
            },
            span: self.span_from(lo),
        })
    }

    /// §6.4 (v5.4) the leading CLAUSE LIST of a comprehension: one or more
    /// `for <pattern> in <expr>` / `if <cond>` clauses, in source order, terminated
    /// by the `=>` that introduces the body. Clauses nest left-to-right (a `for`
    /// pattern binds in the clauses to its right and in the body). At least one
    /// `for` is required (the caller proved a leading `for`). The `=>` is consumed.
    pub(super) fn comprehension_clauses(&mut self) -> PResult<Vec<CompClause>> {
        let mut clauses = Vec::new();
        loop {
            if self.eat(TokenKind::Kw(Keyword::For)) {
                let pattern = self.pattern()?;
                self.expect(TokenKind::Kw(Keyword::In), "`in`")?;
                // A naked lambda at the iter's top level would swallow the `=>` that
                // ends the clause list (`for x in xs => body` must NOT parse `xs => …`
                // as a lambda) — disable it, exactly as a `case` guard does. Grouping
                // delimiters re-allow lambdas inside the iter.
                let saved = std::mem::replace(&mut self.naked_lambda_ok, false);
                let iter = self.expr();
                self.naked_lambda_ok = saved;
                let iter = iter?;
                clauses.push(CompClause::For {
                    pattern,
                    iter: Rc::new(iter),
                });
            } else if self.eat(TokenKind::Kw(Keyword::If)) {
                let saved = std::mem::replace(&mut self.naked_lambda_ok, false);
                let cond = self.expr();
                self.naked_lambda_ok = saved;
                clauses.push(CompClause::If(Rc::new(cond?)));
            } else {
                break;
            }
        }
        self.expect(
            TokenKind::FatArrow,
            "`=>` to introduce the comprehension body",
        )?;
        Ok(clauses)
    }

    pub(super) fn record_fields(&mut self) -> PResult<Vec<FieldInit>> {
        self.expect(TokenKind::LBrace, "`{`")?;
        let mut fields = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            let lo = self.cur_span().lo;
            let name = self.field_name("a field name")?;
            self.expect(TokenKind::Colon, "`:`")?;
            let value = self.expr_nested()?;
            fields.push(FieldInit {
                name,
                value: Rc::new(value),
                span: self.span_from(lo),
            });
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RBrace, "`}`")?;
        Ok(fields)
    }

    /// Parse a `{ ...spread?, field: value, … }` brace for record CONSTRUCTION /
    /// UPDATE: an OPTIONAL single LEADING spread (`...expr`), then zero or more
    /// `field: value` inits. Returns `(spread, fields)`. MVP rejects a SECOND
    /// spread and a NON-LEADING spread (deferred — see `RecordUpdate` AST doc).
    pub(super) fn record_construct_fields(
        &mut self,
    ) -> PResult<(Option<Rc<Expr>>, Vec<FieldInit>)> {
        self.expect(TokenKind::LBrace, "`{`")?;
        let mut spread: Option<Rc<Expr>> = None;
        if self.eat(TokenKind::Ellipsis) {
            spread = Some(Rc::new(self.expr_nested()?));
            // A leading spread MUST be followed by `,` (more fields) or `}` (end).
            if !self.at(TokenKind::RBrace) && !self.eat(TokenKind::Comma) {
                self.error(
                    codes::UNEXPECTED_TOKEN,
                    self.cur_span(),
                    "expected `,` or `}`",
                );
                self.recover_to_matching_rbrace();
            }
        }
        let mut fields = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Ellipsis) {
                // MVP: only a single LEADING spread is allowed.
                self.error(
                    codes::UNEXPECTED_TOKEN,
                    self.cur_span(),
                    "a record spread `...` must be the FIRST element and appear at most once",
                );
                self.recover_to_matching_rbrace();
                break;
            }
            let lo = self.cur_span().lo;
            let name = self.field_name("a field name")?;
            self.expect(TokenKind::Colon, "`:`")?;
            let value = self.expr_nested()?;
            fields.push(FieldInit {
                name,
                value: Rc::new(value),
                span: self.span_from(lo),
            });
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RBrace, "`}`")?;
        Ok((spread, fields))
    }

    pub(super) fn call_args(&mut self) -> PResult<Vec<CallArg>> {
        self.expect(TokenKind::LParen, "`(`")?;
        let mut args = Vec::new();
        while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
            if self.eat(TokenKind::Ellipsis) {
                args.push(CallArg::Spread(self.expr_nested()?));
            } else if self.at(TokenKind::Ident) && self.peek_at(1) == TokenKind::Colon {
                let name = self.ident("an argument name")?;
                self.bump(); // :
                let value = self.expr_nested()?;
                args.push(CallArg::Named { name, value });
            } else {
                args.push(CallArg::Positional(self.expr_nested()?));
            }
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RParen, "`)`")?;
        Ok(args)
    }

    pub(super) fn if_expr(&mut self) -> PResult<Expr> {
        let lo = self.cur_span().lo;
        self.bump(); // if
        // v5.4+ `if let <pattern> = <expr> { … } else { … }` is desugared
        // in the parser to a `match` so every later stage receives the same AST.
        // Backend agreement is separately fixture-measured, not inferred from the
        // lowering. At older editions `let` after `if` falls through and is reported
        // as "expected an expression" by `self.expr()`.
        if self.version >= LangVersion::V5_4 && self.at(TokenKind::Kw(Keyword::Let)) {
            return self.if_let(lo);
        }
        let cond = self.expr_before_block()?;
        let then_block = self.block()?;
        let else_branch = if self.eat(TokenKind::Kw(Keyword::Else)) {
            if self.at(TokenKind::Kw(Keyword::If)) {
                Some(Rc::new(self.if_expr()?))
            } else {
                let blo = self.cur_span().lo;
                let block = self.block()?;
                Some(Rc::new(Expr {
                    kind: ExprKind::Block(Rc::new(block)),
                    span: self.span_from(blo),
                }))
            }
        } else {
            None
        };
        Ok(Expr {
            kind: ExprKind::If {
                cond: Rc::new(cond),
                then_block: Rc::new(then_block),
                else_branch,
            },
            span: self.span_from(lo),
        })
    }

    /// v5.4 `if let <pattern> = <scrutinee> { then } else { else }` — DESUGARED to
    /// `match <scrutinee> { case <pattern> => then\n case _ => else }`. The `else`
    /// arm is the explicit `else` block when present, else `()` (mirroring a plain
    /// `if` with no `else`, which yields Unit). Because this is a real `match`, the
    /// pattern MAY be refutable (the inverse of the destructuring-`let` rule), the
    /// binding scope is confined to the `then` arm. Compiled agreement is pinned by
    /// the differential fixtures rather than assumed from this lowering.
    /// `self` has already consumed `if`; the caller passes its `lo`.
    pub(super) fn if_let(&mut self, lo: u32) -> PResult<Expr> {
        self.bump(); // let
        let pattern = self.pattern()?;
        self.expect(TokenKind::Eq, "`=`")?;
        let scrutinee = self.expr_before_block()?;
        let then_block = self.block()?;
        let then_expr = self.block_as_expr(then_block);
        let else_expr = if self.eat(TokenKind::Kw(Keyword::Else)) {
            // `else if` chains: the else arm is itself an `if`/`if let` expression.
            if self.at(TokenKind::Kw(Keyword::If)) {
                self.if_expr()?
            } else {
                let blo = self.cur_span().lo;
                let block = self.block()?;
                self.block_as_expr_at(block, blo)
            }
        } else {
            self.unit_expr()
        };
        let span = self.span_from(lo);
        let cases = vec![
            CaseClause {
                pattern,
                guard: None,
                body: CaseArmBody::Expr(then_expr),
                span,
            },
            CaseClause {
                pattern: Pattern {
                    kind: PatternKind::Wildcard,
                    span,
                },
                guard: None,
                body: CaseArmBody::Expr(else_expr),
                span,
            },
        ];
        Ok(Expr {
            kind: ExprKind::Match {
                scrutinee: Rc::new(scrutinee),
                cases,
            },
            span,
        })
    }

    /// v5.4 `while let <pattern> = <scrutinee> { body }` — DESUGARED to
    /// `while true { match <scrutinee> { case <pattern> => body\n case _ => break } }`
    /// so the loop drains while the pattern matches and exits on the first miss. The
    /// pattern MAY be refutable (it is a `match` arm); the binding scope is confined
    /// to the body arm. `self` has already consumed `while`; the caller passes its `lo`.
    pub(super) fn while_let(&mut self, lo: u32) -> PResult<StmtKind> {
        self.bump(); // let
        let pattern = self.pattern()?;
        self.expect(TokenKind::Eq, "`=`")?;
        let scrutinee = self.expr_before_block()?;
        let body_block = self.block()?;
        let body_expr = self.block_as_expr(body_block);
        let span = self.span_from(lo);
        // The miss arm is a `break` arm (`CaseArmBody::Return` is the only divergent
        // arm form; a `break` is expressed as a `break` statement inside a block arm).
        let break_block = Block {
            stmts: vec![Stmt {
                kind: StmtKind::Break {
                    label: None,
                    value: None,
                },
                span,
            }],
            tail: None,
            span,
        };
        let break_expr = self.block_as_expr(break_block);
        let cases = vec![
            CaseClause {
                pattern,
                guard: None,
                body: CaseArmBody::Expr(body_expr),
                span,
            },
            CaseClause {
                pattern: Pattern {
                    kind: PatternKind::Wildcard,
                    span,
                },
                guard: None,
                body: CaseArmBody::Expr(break_expr),
                span,
            },
        ];
        let match_expr = Expr {
            kind: ExprKind::Match {
                scrutinee: Rc::new(scrutinee),
                cases,
            },
            span,
        };
        // `while true { <match> }`
        let loop_body = Block {
            stmts: Vec::new(),
            tail: Some(Rc::new(match_expr)),
            span,
        };
        Ok(StmtKind::While {
            cond: Rc::new(Expr {
                kind: ExprKind::Bool(true),
                span,
            }),
            body: Rc::new(loop_body),
        })
    }

    /// Wraps a parsed `Block` as a `Block` EXPRESSION over its own span — the form
    /// a `match` arm body takes. Used by the `if let` / `while let` desugars.
    pub(super) fn block_as_expr(&self, block: Block) -> Expr {
        let span = block.span;
        Expr {
            kind: ExprKind::Block(Rc::new(block)),
            span,
        }
    }

    /// `block_as_expr` with an explicit `lo` for the span (so an `else` block's
    /// expression span starts at the block, matching the plain-`if` else lowering).
    pub(super) fn block_as_expr_at(&self, block: Block, lo: u32) -> Expr {
        Expr {
            kind: ExprKind::Block(Rc::new(block)),
            span: self.span_from(lo),
        }
    }

    /// A `()` expression at the current span — the implicit else of an else-less
    /// `if let` (mirroring a plain `if` with no `else`, which yields Unit).
    pub(super) fn unit_expr(&self) -> Expr {
        Expr {
            kind: ExprKind::Unit,
            span: self.cur_span(),
        }
    }

    pub(super) fn match_expr(&mut self) -> PResult<Expr> {
        let lo = self.cur_span().lo;
        self.bump(); // match
        let scrutinee = self.expr_before_block()?;
        let open = self.expect(TokenKind::LBrace, "`{`")?;
        let mut cases = Vec::new();
        self.skip_seps();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            let before = self.pos;
            match self.case_clause() {
                Ok(case) => {
                    cases.push(case);
                    self.item_boundary();
                }
                Err(Abort) => self.synchronize(),
            }
            if self.pos == before && !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
                let span = self.cur_span();
                self.error(codes::UNEXPECTED_TOKEN, span, "expected `case`");
                self.bump();
            }
            self.skip_seps();
        }
        self.expect(TokenKind::RBrace, "`}`")?;
        if cases.is_empty() {
            self.error(
                codes::UNEXPECTED_TOKEN,
                Span::new(self.file, open.span.lo, self.last_hi),
                "`match` requires at least one `case`",
            );
        }
        Ok(Expr {
            kind: ExprKind::Match {
                scrutinee: Rc::new(scrutinee),
                cases,
            },
            span: self.span_from(lo),
        })
    }

    pub(super) fn case_clause(&mut self) -> PResult<CaseClause> {
        let lo = self.cur_span().lo;
        self.expect(TokenKind::Kw(Keyword::Case), "`case`")?;
        let pattern = self.pattern()?;
        let guard = if self.eat(TokenKind::Kw(Keyword::If)) {
            // A naked lambda at the guard's top level would swallow
            // the case arrow; grouping delimiters re-allow lambdas.
            let saved = std::mem::replace(&mut self.naked_lambda_ok, false);
            let guard = self.expr_record_nested();
            self.naked_lambda_ok = saved;
            Some(guard?)
        } else {
            None
        };
        self.expect(TokenKind::FatArrow, "`=>`")?;
        // `CaseArmBody ::= Expression | ReturnStmt` (v5.2, ADR-074).
        // At V5_1 a `return` arm falls through to the expression
        // path and keeps its v0.1 diagnostic.
        let body =
            if self.version >= LangVersion::V5_2 && self.peek() == TokenKind::Kw(Keyword::Return) {
                let arm_lo = self.cur_span().lo;
                self.bump();
                let value = if matches!(
                    self.peek(),
                    TokenKind::Sep | TokenKind::RBrace | TokenKind::Eof
                ) {
                    None
                } else {
                    Some(self.expr_nested()?)
                };
                CaseArmBody::Return {
                    value,
                    span: self.span_from(arm_lo),
                }
            } else {
                CaseArmBody::Expr(self.expr_nested()?)
            };
        Ok(CaseClause {
            pattern,
            guard,
            body,
            span: self.span_from(lo),
        })
    }

    pub(super) fn for_expr(&mut self) -> PResult<Expr> {
        let lo = self.cur_span().lo;
        self.bump(); // for
        let pattern = self.pattern()?;
        self.expect(TokenKind::Kw(Keyword::In), "`in`")?;
        let iter = self.expr_before_block()?;
        let body = self.block()?;
        Ok(Expr {
            kind: ExprKind::For {
                pattern: Rc::new(pattern),
                iter: Rc::new(iter),
                body: Rc::new(body),
            },
            span: self.span_from(lo),
        })
    }

    /// `loop ('label)? { body }` is the infinite-loop
    /// expression. The caller has proved the v5.4+ contextual head; in older
    /// modes and outside this shape `loop` remains an ordinary identifier.
    pub(super) fn loop_expr(&mut self) -> PResult<Expr> {
        let lo = self.cur_span().lo;
        self.bump(); // loop
        let label = self.opt_loop_label();
        let body = self.block()?;
        Ok(Expr {
            kind: ExprKind::Loop {
                label,
                body: Rc::new(body),
            },
            span: self.span_from(lo),
        })
    }

    pub(super) fn concurrent_expr(&mut self) -> PResult<Expr> {
        let lo = self.cur_span().lo;
        self.bump(); // concurrent
        let timeout = if self.eat(TokenKind::LParen) {
            let key = self.ident("`timeout`")?;
            if self.text(key.span) != "timeout" {
                self.error(
                    codes::CONCURRENT_FORM,
                    key.span,
                    "`concurrent` takes only a `timeout:` argument",
                );
            }
            // RECOVER instead of aborting: a missing `:` or a non-duration value
            // must yield ONE clear diagnostic, not a cascade from unwinding the
            // half-parsed `timeout(...)` clause back through the caller.
            if !self.eat(TokenKind::Colon) {
                self.error(
                    codes::UNEXPECTED_TOKEN,
                    self.cur_span(),
                    "expected `:` after `timeout`",
                );
            }
            let tlo = self.cur_span().lo;
            let unit = match self.peek() {
                TokenKind::Duration(unit) => {
                    self.bump();
                    unit
                }
                _ => {
                    self.error(
                        codes::UNEXPECTED_TOKEN,
                        self.cur_span(),
                        "expected a duration literal, e.g. `3s`",
                    );
                    // Skip the malformed value up to the clause's own `)` at depth
                    // 0, honoring nested delimiters, then assume seconds. A
                    // DELIMITER-AWARE skip (not a single `bump`) is essential: a
                    // value like `(3)`, `{ a: f() }`, or `3 + 4` would otherwise
                    // desync the recovery and cascade. The `expect(RParen)` below
                    // consumes the real close.
                    let mut depth = 0u32;
                    loop {
                        let k = self.peek();
                        if k == TokenKind::Eof {
                            break;
                        }
                        if depth == 0 && k == TokenKind::RParen {
                            break;
                        }
                        if Self::is_opener(k) {
                            depth += 1;
                        } else if Self::is_closer(k) {
                            if depth == 0 {
                                break;
                            }
                            depth -= 1;
                        }
                        self.bump();
                    }
                    topaz_syntax::DurationUnit::S
                }
            };
            let timeout = Expr {
                kind: ExprKind::Duration(unit),
                span: self.span_from(tlo),
            };
            self.expect(TokenKind::RParen, "`)`")?;
            Some(Rc::new(timeout))
        } else {
            None
        };
        self.expect(TokenKind::LBrace, "`{`")?;
        let mut arms = Vec::new();
        self.skip_seps();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            let before = self.pos;
            match self.concurrent_arm() {
                Ok(arm) => {
                    arms.push(arm);
                    self.item_boundary();
                }
                Err(Abort) => self.synchronize(),
            }
            if self.pos == before && !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
                let span = self.cur_span();
                self.error(codes::UNEXPECTED_TOKEN, span, "expected a concurrent arm");
                self.bump();
            }
            self.skip_seps();
        }
        self.expect(TokenKind::RBrace, "`}`")?;
        if arms.is_empty() {
            self.error(
                codes::UNEXPECTED_TOKEN,
                self.span_from(lo),
                "`concurrent` requires at least one arm",
            );
        }
        let else_block = if self.at(TokenKind::Kw(Keyword::Else)) {
            if timeout.is_none() {
                let span = self.cur_span();
                self.error(
                    codes::CONCURRENT_FORM,
                    span,
                    "`else` requires the `concurrent(timeout: ...)` form",
                );
            }
            self.bump();
            Some(self.block()?)
        } else {
            if timeout.is_some() {
                let span = self.span_from(lo);
                self.error(
                    codes::CONCURRENT_FORM,
                    span,
                    "the `concurrent(timeout: ...)` form requires an `else` block",
                );
            }
            None
        };
        Ok(Expr {
            kind: ExprKind::Concurrent {
                timeout,
                arms,
                else_block: else_block.map(Rc::new),
            },
            span: self.span_from(lo),
        })
    }

    pub(super) fn concurrent_arm(&mut self) -> PResult<ConcurrentArm> {
        let lo = self.cur_span().lo;
        let name = self.ident("an arm name")?;
        self.expect(TokenKind::Colon, "`:`")?;
        let value = self.expr_nested()?;
        Ok(ConcurrentArm {
            name,
            value: Rc::new(value),
            span: self.span_from(lo),
        })
    }

    pub(super) fn string_lit(&mut self) -> PResult<StringLit> {
        let start = self.bump();
        let TokenKind::StringStart { tagged, multiline } = start.kind else {
            unreachable!("string_lit entered off a StringStart");
        };
        let delim_len = if multiline { 3 } else { 1 };
        let tag = if tagged {
            let tag_span = Span::new(self.file, start.span.lo, start.span.hi - delim_len);
            let text = self.text(tag_span);
            if !matches!(text, "p" | "r" | "sh" | "sql") {
                self.error(
                    codes::UNKNOWN_TEMPLATE_TAG,
                    tag_span,
                    &format!(
                        "unknown template tag `{text}`; the registry is `p`, `r`, `sh`, `sql`"
                    ),
                );
            }
            Some(tag_span)
        } else {
            None
        };
        let mut parts = Vec::new();
        let hi = loop {
            match self.peek() {
                TokenKind::StringText => {
                    parts.push(StringPart::Text(self.bump().span));
                }
                TokenKind::InterpolationStart => {
                    self.bump();
                    if self.at(TokenKind::InterpolationEnd) {
                        self.error_here("expected an expression");
                        self.bump();
                    } else {
                        let e = self.expr_nested()?;
                        self.expect(TokenKind::InterpolationEnd, "`}`")?;
                        parts.push(StringPart::Interpolation(e));
                    }
                }
                TokenKind::StringEnd => break self.bump().span.hi,
                _ => return Err(self.error_here("expected string content")),
            }
        };
        Ok(StringLit {
            tag,
            multiline,
            parts,
            span: Span::new(self.file, start.span.lo, hi),
        })
    }
}
/// Binding power for the §2 binary table, levels 12 (loosest, `|>`)
/// through 4 (`*`); higher binds tighter. `**`, unary, and postfix
/// are handled structurally.
fn binary_bp(kind: TokenKind) -> Option<u8> {
    Some(match kind {
        TokenKind::PipeGt => 1,
        TokenKind::GtGt => 2,
        TokenKind::QuestionQuestion => 3,
        TokenKind::OrOr => 4,
        TokenKind::AndAnd => 5,
        TokenKind::Lt
        | TokenKind::Le
        | TokenKind::Gt
        | TokenKind::Ge
        | TokenKind::EqEq
        | TokenKind::Ne
        | TokenKind::Kw(Keyword::In) => 6,
        TokenKind::DotDot | TokenKind::DotDotLt => RANGE_BP,
        TokenKind::Plus | TokenKind::Minus => 8,
        TokenKind::Star | TokenKind::Slash | TokenKind::Percent => 9,
        _ => return None,
    })
}

fn binary_op(kind: TokenKind) -> BinaryOp {
    match kind {
        TokenKind::QuestionQuestion => BinaryOp::Coalesce,
        TokenKind::OrOr => BinaryOp::Or,
        TokenKind::AndAnd => BinaryOp::And,
        TokenKind::Lt => BinaryOp::Lt,
        TokenKind::Le => BinaryOp::Le,
        TokenKind::Gt => BinaryOp::Gt,
        TokenKind::Ge => BinaryOp::Ge,
        TokenKind::EqEq => BinaryOp::Eq,
        TokenKind::Ne => BinaryOp::Ne,
        TokenKind::Kw(Keyword::In) => BinaryOp::In,
        TokenKind::Plus => BinaryOp::Add,
        TokenKind::Minus => BinaryOp::Sub,
        TokenKind::Star => BinaryOp::Mul,
        TokenKind::Slash => BinaryOp::Div,
        TokenKind::Percent => BinaryOp::Rem,
        _ => unreachable!("not a plain binary operator: {kind:?}"),
    }
}
