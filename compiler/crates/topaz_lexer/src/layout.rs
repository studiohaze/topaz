//! The layout normalizer (CDR-001 §4, SPEC §1a): a syntax-aware token
//! pass that resolves newline significance. It consumes the raw token
//! stream and emits parser-facing tokens in which physical newlines
//! are gone — a significant newline or an explicit `;` becomes a
//! [`TokenKind::Sep`], everything else passes through unchanged.
//!
//! A frame stack tracks the layout mode (ADR-049): separator mode for
//! the program top level, blocks, `match` bodies, and `concurrent`
//! bodies; continuation mode for every delimiter context holding a
//! list — parens, brackets, record braces, template interpolations.
//!
//! Brace classification (SPEC §5/§8):
//! - a `{` inside a pattern region (`let`/`const` until `:`/`=`,
//!   `case` until guard `if`/`=>`, `for` until `in`) is a record
//!   pattern — continuation, with no lookahead (record patterns admit
//!   the colon-less `{ x, y }` shorthand);
//! - the `{` awaited by a `concurrent` header is its body — separator,
//!   with no lookahead (arms are `Identifier ":"` and must not read as
//!   record fields);
//! - every other `{` uses the expression-position lookahead:
//!   `Identifier ":"` next means a record form (literal, constant,
//!   update, or type — all colon-first by grammar) — continuation;
//!   anything else opens a block — separator. Construct bodies
//!   (`if`/`else`/`while`/`for`/`match`/`function`/`defer`) resolve
//!   to separator through this rule because no statement can begin
//!   `Identifier ":"`. Expression positions such as a lambda or case
//!   body after `=>` classify naturally: a record literal there is a
//!   record, a block is a block.

use topaz_diag::{Diagnostic, Label, Span};
use topaz_syntax::{Keyword, LangVersion, Token, TokenKind};

use crate::codes;

/// Result of normalizing one raw token stream.
#[derive(Debug)]
pub struct LayoutOutput {
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Options for layout normalization (CDR-002 §1): layout
/// classification is version-bearing in v5.2 (`FieldName ":"`
/// lookahead, pattern-`|` continuation regions, import-list
/// continuation braces), so the language version enters here, not
/// only in the parser.
#[derive(Debug, Clone, Copy, Default)]
pub struct LayoutOptions {
    pub language_version: LangVersion,
}

/// Normalizes a raw token stream (the output of [`crate::lex`]) into
/// the parser-facing stream: `Newline` tokens are resolved into `Sep`
/// or dropped, `;` becomes `Sep` in separator mode and a diagnostic in
/// continuation mode. v5.1 convenience for
/// [`normalize_with_options`].
pub fn normalize(input: &[Token]) -> LayoutOutput {
    // V5_1 never reads the source text (no textual head words).
    normalize_with_options(input, "", LayoutOptions::default())
}

/// Versioned normalization entry (CDR-002 §1). `src` is the source
/// text the tokens were lexed from; it is read only at V5_2, where
/// module-head recognition is textual (`import` is an identifier,
/// not a keyword — SPEC v5.2 §17).
pub fn normalize_with_options(input: &[Token], src: &str, options: LayoutOptions) -> LayoutOutput {
    Normalizer {
        input,
        src,
        version: options.language_version,
        module_item: false,
        out: Vec::with_capacity(input.len()),
        diagnostics: Vec::new(),
        frames: vec![Frame {
            mode: Mode::Separator,
            opener: Opener::Program,
            has_item: false,
        }],
        pending: None,
        prev: None,
        pattern: None,
        concurrent: Vec::new(),
    }
    .run()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Separator,
    Continuation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Opener {
    Program,
    Paren,
    Bracket,
    Brace,
    Interpolation,
}

#[derive(Debug)]
struct Frame {
    mode: Mode,
    opener: Opener,
    /// Whether any item content has appeared in this frame — a newline
    /// right after an opening separator delimiter is an ignored empty
    /// separator (SPEC §1a).
    has_item: bool,
}

/// Pattern-region kinds and the token that ends each region (checked
/// only at the region's own frame depth; nested delimiter contents are
/// part of the pattern).
#[derive(Debug, Clone, Copy)]
enum PatternKind {
    /// `let p`/`const p` until the type annotation `:` or the `=`.
    Binding,
    /// `case p` until the guard `if` or the `=>`.
    Case,
    /// `for p` until the `in`.
    For,
}

impl PatternKind {
    fn terminates(self, kind: TokenKind) -> bool {
        match self {
            PatternKind::Binding => matches!(kind, TokenKind::Eq | TokenKind::Colon),
            PatternKind::Case => {
                matches!(kind, TokenKind::FatArrow | TokenKind::Kw(Keyword::If))
            }
            PatternKind::For => kind == TokenKind::Kw(Keyword::In),
        }
    }
}

struct Normalizer<'a> {
    input: &'a [Token],
    src: &'a str,
    /// Session language version (CDR-002 §1): gates the v5.2 layout
    /// behavior; at `V5_1` the normalizer is the v0.1 normalizer.
    version: LangVersion,
    /// Whether the current top-level item began with the module head
    /// word `import` (V5_2): its selection-list brace is a
    /// continuation-mode `ImportList` (SPEC v5.2 §1a/§17), not a
    /// record/block candidate. Cleared at each item separator.
    module_item: bool,
    out: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
    frames: Vec<Frame>,
    /// Span of the first separator-worthy newline awaiting the next
    /// significant token's decision (SPEC §1a rule 3 needs that
    /// token).
    pending: Option<Span>,
    /// Kind of the previous significant token (SPEC §1a rule 2).
    prev: Option<TokenKind>,
    /// Active pattern region: (frame depth, kind).
    pattern: Option<(usize, PatternKind)>,
    /// Frame depths of `concurrent` headers awaiting their body `{`.
    concurrent: Vec<usize>,
}

impl Normalizer<'_> {
    fn run(mut self) -> LayoutOutput {
        for i in 0..self.input.len() {
            let tok = self.input[i];
            match tok.kind {
                TokenKind::Newline => self.newline(tok),
                TokenKind::Semicolon => self.semicolon(tok),
                _ => self.significant(tok, i),
            }
        }
        LayoutOutput {
            tokens: self.out,
            diagnostics: self.diagnostics,
        }
    }

    fn top(&mut self) -> &mut Frame {
        self.frames
            .last_mut()
            .expect("program frame is never popped")
    }

    fn text(&self, span: Span) -> &str {
        &self.src[span.lo as usize..span.hi as usize]
    }

    /// Whether a `|` is a layout-continuation token here: only inside
    /// an active pattern region at its own frame depth, and only at
    /// v5.2 (SPEC v5.2 §1a, ADR-073). Expression-position `|`/`||`
    /// layout is unchanged.
    fn pipe_continues(&self) -> bool {
        self.version >= LangVersion::V5_2
            && self
                .pattern
                .is_some_and(|(depth, _)| depth == self.frames.len())
    }

    /// Whether the current keyword token is a v5.2+ `FieldName`, not
    /// a layout construct head. Keyword members are selected by `.` or
    /// `?.`; keyword record fields are followed by `:` after optional
    /// physical newlines inside their continuation-mode brace.
    fn keyword_has_field_role(&self, i: usize) -> bool {
        if self.version < LangVersion::V5_2 {
            return false;
        }
        if matches!(self.prev, Some(TokenKind::Dot | TokenKind::QuestionDot)) {
            return true;
        }
        self.input[i + 1..]
            .iter()
            .find(|token| token.kind != TokenKind::Newline)
            .is_some_and(|token| token.kind == TokenKind::Colon)
    }

    fn newline(&mut self, tok: Token) {
        let frame = self.frames.last().expect("program frame");
        // Insignificant in continuation mode; an empty separator right
        // after an opening delimiter or right after a `Sep` is ignored.
        if frame.mode != Mode::Separator || !frame.has_item {
            return;
        }
        if self.out.last().is_some_and(|t| t.kind == TokenKind::Sep) {
            return;
        }
        // Rule 2: a trailing-continuation token absorbs the newline.
        if self
            .prev
            .is_some_and(|kind| is_trailing_continuation(kind, self.version))
            || (self.prev == Some(TokenKind::Pipe) && self.pipe_continues())
        {
            return;
        }
        if self.pending.is_none() {
            self.pending = Some(tok.span);
        }
    }

    fn semicolon(&mut self, tok: Token) {
        if self.frames.last().expect("program frame").mode == Mode::Continuation {
            self.diagnostics.push(Diagnostic::error(
                codes::SEMICOLON_IN_DELIMITER_LIST,
                "`;` does not separate items inside a delimiter list; use `,`",
                Label::new(tok.span, ""),
            ));
        } else {
            self.pending = None;
            self.emit_sep(tok.span);
        }
        self.prev = Some(TokenKind::Semicolon);
    }

    fn significant(&mut self, tok: Token, i: usize) {
        // Resolve a pending newline: rule 3 (leading continuation) and
        // the empty separator before a closing delimiter or Eof.
        if let Some(span) = self.pending.take() {
            let absorb = is_leading_continuation(tok.kind, self.version)
                || (tok.kind == TokenKind::Pipe && self.pipe_continues())
                || is_closer(tok.kind)
                || tok.kind == TokenKind::Eof;
            if !absorb {
                self.emit_sep(span);
            }
        }
        // v5.2 import items (SPEC v5.2 §17). Same bounded decision as
        // the parser's ADR-076 follow table: the head identifier
        // `import` commits to the module reading only when its
        // no-`Sep` follow token is an identifier (the module path
        // head). Base-owned shapes (`import = ...`, `import(...)`,
        // bare `import`) keep their v0.1 layout. Detected here so
        // `classify_brace` can give the selection list its
        // continuation mode.
        if self.version >= LangVersion::V5_2
            && self.frames.len() == 1
            && tok.kind == TokenKind::Ident
            && (self.out.is_empty() || self.out.last().is_some_and(|t| t.kind == TokenKind::Sep))
            && self.text(tok.span) == "import"
            && self
                .input
                .get(i + 1)
                .is_some_and(|t| t.kind == TokenKind::Ident)
        {
            self.module_item = true;
        }
        // Pattern-region terminator, only at the region's own depth.
        if let Some((depth, kind)) = self.pattern
            && depth == self.frames.len()
            && kind.terminates(tok.kind)
        {
            self.pattern = None;
        }
        let keyword_has_field_role =
            matches!(tok.kind, TokenKind::Kw(_)) && self.keyword_has_field_role(i);
        match tok.kind {
            TokenKind::LParen | TokenKind::LBracket | TokenKind::InterpolationStart => {
                self.top().has_item = true;
                self.frames.push(Frame {
                    mode: Mode::Continuation,
                    opener: match tok.kind {
                        TokenKind::LParen => Opener::Paren,
                        TokenKind::LBracket => Opener::Bracket,
                        _ => Opener::Interpolation,
                    },
                    has_item: false,
                });
            }
            TokenKind::LBrace => {
                self.top().has_item = true;
                let mode = self.classify_brace(i);
                self.frames.push(Frame {
                    mode,
                    opener: Opener::Brace,
                    has_item: false,
                });
            }
            TokenKind::RParen
            | TokenKind::RBracket
            | TokenKind::RBrace
            | TokenKind::InterpolationEnd => {
                let expected = match tok.kind {
                    TokenKind::RParen => Opener::Paren,
                    TokenKind::RBracket => Opener::Bracket,
                    TokenKind::RBrace => Opener::Brace,
                    _ => Opener::Interpolation,
                };
                // Pop only on a match; delimiter errors are the
                // parser's to report.
                if self.frames.last().expect("program frame").opener == expected {
                    self.frames.pop();
                    let depth = self.frames.len();
                    self.concurrent.retain(|&d| d <= depth);
                    if self.pattern.is_some_and(|(d, _)| d > depth) {
                        self.pattern = None;
                    }
                }
                self.top().has_item = true;
            }
            TokenKind::Kw(Keyword::Concurrent) if !keyword_has_field_role => {
                self.concurrent.push(self.frames.len());
                self.top().has_item = true;
            }
            TokenKind::Kw(Keyword::Let | Keyword::Const) if !keyword_has_field_role => {
                self.pattern = Some((self.frames.len(), PatternKind::Binding));
                self.top().has_item = true;
            }
            TokenKind::Kw(Keyword::Case) if !keyword_has_field_role => {
                self.pattern = Some((self.frames.len(), PatternKind::Case));
                self.top().has_item = true;
            }
            TokenKind::Kw(Keyword::For) if !keyword_has_field_role => {
                self.pattern = Some((self.frames.len(), PatternKind::For));
                self.top().has_item = true;
            }
            _ => self.top().has_item = true,
        }
        self.out.push(tok);
        self.prev = Some(tok.kind);
    }

    /// Emits a `Sep` and drops construct state that cannot span a
    /// separator (a stale pattern region or `concurrent` header marker
    /// only survives this point in malformed code).
    fn emit_sep(&mut self, span: Span) {
        self.module_item = false;
        self.out.push(Token::new(TokenKind::Sep, span));
        self.pattern = None;
        let depth = self.frames.len();
        self.concurrent.retain(|&d| d != depth);
    }

    /// Mode for the brace at input index `i` (see the module docs for
    /// the classification rules).
    fn classify_brace(&mut self, i: usize) -> Mode {
        if self.module_item {
            return Mode::Continuation; // ImportList (SPEC v5.2 §1a)
        }
        if self.pattern.is_some() {
            return Mode::Continuation; // record pattern
        }
        if self.concurrent.last() == Some(&self.frames.len()) {
            self.concurrent.pop();
            return Mode::Separator; // concurrent body
        }
        // Expression-position lookahead (SPEC §5): `FieldName ":"`
        // opens a record form (`Identifier ":"` at V5_1; keyword
        // field names join at V5_2 — SPEC v5.2 §3, ADR-075).
        // Newlines are skipped — they are insignificant inside the
        // record the lookahead would commit to.
        //
        // Construct-first classification (SPEC §5: braces after
        // `if`/`else`/`for`/`while`/`function` are blocks before any
        // record lookahead) is parser-owned: a construct-body brace
        // whose first tokens are `FieldName ":"` is invalid in both
        // readings, so this lookahead's classification only affects
        // newline significance inside an already-invalid region —
        // the v0.1 boundary, unchanged by v5.2.
        let v52 = self.version >= LangVersion::V5_2;
        let mut j = i + 1;
        let mut next_significant = || {
            while let Some(tok) = self.input.get(j) {
                j += 1;
                if tok.kind != TokenKind::Newline {
                    return Some(tok.kind);
                }
            }
            None
        };
        let opens_field = next_significant()
            .is_some_and(|k| k == TokenKind::Ident || (v52 && matches!(k, TokenKind::Kw(_))));
        if opens_field && next_significant() == Some(TokenKind::Colon) {
            Mode::Continuation
        } else {
            Mode::Separator
        }
    }
}

fn is_closer(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace | TokenKind::InterpolationEnd
    )
}

enum LineContinuation {
    Leading,
    Trailing,
    Both,
}

impl LineContinuation {
    fn leads(self) -> bool {
        matches!(self, Self::Leading | Self::Both)
    }

    fn trails(self) -> bool {
        matches!(self, Self::Trailing | Self::Both)
    }
}

/// SPEC §1a line-continuation authority. Most operators absorb a
/// newline on either side; the deliberately asymmetric groups are
/// declared once beside that common denominator.
fn line_continuation(kind: TokenKind, version: LangVersion) -> Option<LineContinuation> {
    match kind {
        // ERR-003 (2026-06-13): v5.2+ makes `>` and `>>` leading-only
        // so a generic-final line does not absorb the next item. The
        // frozen v5.1 keeps both directions.
        TokenKind::Gt | TokenKind::GtGt if version == LangVersion::V5_1 => {
            Some(LineContinuation::Both)
        }
        TokenKind::Gt | TokenKind::GtGt | TokenKind::Kw(Keyword::Else) => {
            Some(LineContinuation::Leading)
        }
        // Unary-capable operators cannot lead continuation. `~` is
        // reserved (TPZ2013) but remains trailing for lexer recovery.
        TokenKind::Plus
        | TokenKind::Minus
        | TokenKind::Bang
        | TokenKind::Tilde
        | TokenKind::Comma
        | TokenKind::FatArrow
        | TokenKind::LParen
        | TokenKind::LBracket => Some(LineContinuation::Trailing),
        TokenKind::StarStar
        | TokenKind::Star
        | TokenKind::Slash
        | TokenKind::Percent
        | TokenKind::DotDot
        | TokenKind::DotDotLt
        | TokenKind::Lt
        | TokenKind::Le
        | TokenKind::Ge
        | TokenKind::EqEq
        | TokenKind::Ne
        | TokenKind::Kw(Keyword::In)
        | TokenKind::AndAnd
        | TokenKind::OrOr
        | TokenKind::QuestionQuestion
        | TokenKind::PipeGt
        | TokenKind::Eq
        | TokenKind::PlusEq
        | TokenKind::MinusEq
        | TokenKind::StarEq
        | TokenKind::SlashEq
        | TokenKind::PercentEq
        | TokenKind::QuestionQuestionEq
        | TokenKind::Dot
        | TokenKind::QuestionDot
        | TokenKind::Kw(Keyword::By) => Some(LineContinuation::Both),
        _ => None,
    }
}

fn is_trailing_continuation(kind: TokenKind, version: LangVersion) -> bool {
    line_continuation(kind, version).is_some_and(LineContinuation::trails)
}

fn is_leading_continuation(kind: TokenKind, version: LangVersion) -> bool {
    line_continuation(kind, version).is_some_and(LineContinuation::leads)
}
