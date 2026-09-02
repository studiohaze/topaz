use super::*;

impl Parser<'_> {
    // ---- cursor -----------------------------------------------------

    pub(super) fn token_at(&self, pos: usize) -> Token {
        *self
            .tokens
            .get(pos)
            .or_else(|| self.tokens.last())
            .expect("token stream ends with Eof")
    }

    pub(super) fn peek(&self) -> TokenKind {
        match self.pending_gt {
            Some(_) => TokenKind::Gt,
            None => self.token_at(self.pos).kind,
        }
    }

    /// Lookahead past the current token; `pending_gt` occupies slot 0.
    pub(super) fn peek_at(&self, n: usize) -> TokenKind {
        let offset = match self.pending_gt {
            Some(_) if n == 0 => return TokenKind::Gt,
            Some(_) => n - 1,
            None => n,
        };
        self.token_at(self.pos + offset).kind
    }

    pub(super) fn cur_span(&self) -> Span {
        match self.pending_gt {
            Some(span) => span,
            None => self.token_at(self.pos).span,
        }
    }

    pub(super) fn bump(&mut self) -> Token {
        if let Some(span) = self.pending_gt.take() {
            self.last_hi = span.hi;
            return Token::new(TokenKind::Gt, span);
        }
        let tok = self.token_at(self.pos);
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        self.last_hi = tok.span.hi;
        tok
    }

    pub(super) fn at(&self, kind: TokenKind) -> bool {
        self.peek() == kind
    }

    pub(super) fn eat(&mut self, kind: TokenKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub(super) fn span_from(&self, lo: u32) -> Span {
        Span::new(self.file, lo, self.last_hi.max(lo))
    }

    pub(super) fn text(&self, span: Span) -> &str {
        &self.src[span.lo as usize..span.hi as usize]
    }

    // ---- diagnostics and recovery -----------------------------------

    pub(super) fn error(&mut self, code: topaz_diag::Code, span: Span, message: &str) {
        self.diagnostics
            .push(Diagnostic::error(code, message, Label::new(span, "")));
    }

    pub(super) fn error_here(&mut self, message: &str) -> Abort {
        self.error(codes::UNEXPECTED_TOKEN, self.cur_span(), message);
        Abort
    }

    pub(super) fn expect(&mut self, kind: TokenKind, what: &str) -> PResult<Token> {
        if self.at(kind) {
            Ok(self.bump())
        } else {
            Err(self.error_here(&format!("expected {what}")))
        }
    }

    pub(super) fn ident(&mut self, what: &str) -> PResult<Ident> {
        let tok = self.expect(TokenKind::Ident, what)?;
        Ok(Ident { span: tok.span })
    }

    /// A binding-position identifier. `None` is the §22.1 Option
    /// constructor — never an ordinary variable — so no binding
    /// position may take it (§6 makes bare `None` a constructor
    /// pattern).
    pub(super) fn binding_ident(&mut self, what: &str) -> PResult<Ident> {
        let name = self.ident(what)?;
        if self.text(name.span) == "None" {
            self.error(
                codes::RESERVED_BINDING_NAME,
                name.span,
                "`None` is the Option constructor (§22.1) and cannot be a binding name",
            );
        }
        Ok(name)
    }

    /// Consume an optional `'name` loop label, returning its name as an
    /// `Ident` whose span EXCLUDES the leading `'` (so `self.text(label.span)` is
    /// the bare name, matching `loop 'name` and `break 'name`). Returns `None`
    /// when the next token is not a `Label`. Labels lex only at `>= V5_4` (the `'`
    /// is an UNKNOWN_CHAR earlier), so no explicit version gate is needed here.
    pub(super) fn opt_loop_label(&mut self) -> Option<Ident> {
        if self.at(TokenKind::Label) {
            let tok = self.bump();
            // The `Label` span is `'name`; the name is the span minus the leading
            // apostrophe (one byte — `'` is ASCII).
            let name_span = Span::new(self.file, tok.span.lo + 1, tok.span.hi);
            Some(Ident { span: name_span })
        } else {
            None
        }
    }

    /// `FieldName ::= Identifier | Keyword` (SPEC v5.2 §3, ADR-075).
    /// Keyword field names exist only in the seven §8 positions, all
    /// of which call this helper; at V5_1 a field name is an
    /// identifier, exactly as in v0.1.
    pub(super) fn field_name(&mut self, what: &str) -> PResult<Ident> {
        if self.version >= LangVersion::V5_2 && matches!(self.peek(), TokenKind::Kw(_)) {
            let tok = self.bump();
            return Ok(Ident { span: tok.span });
        }
        self.ident(what)
    }

    /// Whether `kind` can begin a field name in the `FieldName ":"`
    /// brace lookaheads (SPEC v5.2 §5; identifier-only at V5_1).
    pub(super) fn is_field_name_token(&self, kind: TokenKind) -> bool {
        kind == TokenKind::Ident
            || (self.version >= LangVersion::V5_2 && matches!(kind, TokenKind::Kw(_)))
    }

    pub(super) fn is_opener(kind: TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::LParen
                | TokenKind::LBracket
                | TokenKind::LBrace
                | TokenKind::InterpolationStart
                | TokenKind::StringStart { .. }
        )
    }

    pub(super) fn is_closer(kind: TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::RParen
                | TokenKind::RBracket
                | TokenKind::RBrace
                | TokenKind::InterpolationEnd
                | TokenKind::StringEnd
        )
    }
}
