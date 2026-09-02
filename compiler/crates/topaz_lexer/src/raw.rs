//! The raw lexer (CDR-001 §4): longest-match tokenization of
//! everything outside string/template content, which is owned by the
//! template lexer (`crate::template`) through a shared mode stack.
//!
//! Physical newlines are emitted as [`TokenKind::Newline`] for the
//! layout normalizer; the parser never sees them.

use topaz_diag::{Code, Diagnostic, FileId, Label, Span};
use topaz_syntax::{DurationUnit, Keyword, Token, TokenKind};

use crate::codes;
use crate::template::Mode;
use crate::unicode::{is_identifier_continue, is_identifier_start};

/// Result of lexing one source file.
#[derive(Debug)]
pub struct LexOutput {
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Lexes `src` (the full text of `file`) into tokens plus
/// diagnostics. Never panics on malformed input: unknown characters
/// produce diagnostics and lexing continues.
pub fn lex(file: FileId, src: &str) -> LexOutput {
    Lexer {
        src,
        file,
        pos: 0,
        modes: Vec::new(),
        tokens: Vec::new(),
        diagnostics: Vec::new(),
    }
    .run()
}

pub(crate) struct Lexer<'src> {
    pub(crate) src: &'src str,
    pub(crate) file: FileId,
    pub(crate) pos: usize,
    /// String/template mode stack (`crate::template`); empty while
    /// lexing ordinary code.
    pub(crate) modes: Vec<Mode>,
    pub(crate) tokens: Vec<Token>,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

impl Lexer<'_> {
    fn run(mut self) -> LexOutput {
        // Every step consumes input or, at end of input, pops a mode,
        // so the loop terminates.
        while self.pos < self.src.len() || !self.modes.is_empty() {
            match self.modes.last() {
                None => {
                    let ch = self.peek().expect("guarded by the loop condition");
                    self.code_token(ch);
                }
                Some(Mode::Text { .. }) => self.string_text(),
                Some(Mode::Interp { .. }) => self.interpolation_token(),
            }
        }
        let end = self.src.len() as u32;
        self.tokens
            .push(Token::new(TokenKind::Eof, Span::new(self.file, end, end)));
        LexOutput {
            tokens: self.tokens,
            diagnostics: self.diagnostics,
        }
    }

    /// Lexes one token of ordinary code. Interpolation bodies reuse
    /// this after intercepting braces and single-line line breaks.
    pub(crate) fn code_token(&mut self, ch: char) {
        let start = self.pos;
        match ch {
            ' ' | '\t' | '\r' => {
                // A lone `\r` is whitespace; `\r\n` emits Newline
                // when the `\n` is reached.
                self.bump(ch);
            }
            '\n' => {
                self.bump(ch);
                // Fold a preceding `\r` into the Newline span.
                let lo = if start > 0 && self.src.as_bytes()[start - 1] == b'\r' {
                    start - 1
                } else {
                    start
                };
                self.push_at(TokenKind::Newline, lo);
            }
            '/' => self.slash(start),
            '0'..='9' => self.number(start),
            '"' => self.string_start(start, false),
            '\'' => self.label(start),
            _ if is_identifier_start(ch) => self.identifier(start),
            _ => self.operator(ch, start),
        }
    }

    // ---- cursor ---------------------------------------------------

    pub(crate) fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    pub(crate) fn bump(&mut self, ch: char) {
        self.pos += ch.len_utf8();
    }

    /// Consumes `text` if the source continues with it.
    pub(crate) fn eat(&mut self, text: &str) -> bool {
        if self.src[self.pos..].starts_with(text) {
            self.pos += text.len();
            true
        } else {
            false
        }
    }

    pub(crate) fn push_at(&mut self, kind: TokenKind, lo: usize) {
        self.tokens.push(Token::new(
            kind,
            Span::new(self.file, lo as u32, self.pos as u32),
        ));
    }

    pub(crate) fn error_at(&mut self, code: Code, message: &str, lo: usize) {
        let span = Span::new(self.file, lo as u32, self.pos as u32);
        self.diagnostics
            .push(Diagnostic::error(code, message, Label::new(span, "")));
    }

    // ---- productions ----------------------------------------------

    fn slash(&mut self, start: usize) {
        if self.eat("//") {
            while let Some(ch) = self.peek() {
                if ch == '\n' {
                    break; // the Newline token still fires
                }
                self.bump(ch);
            }
        } else if self.eat("/*") {
            // Block comments do not nest (SPEC §1). Newlines inside
            // are part of the comment, not layout.
            loop {
                if self.eat("*/") {
                    break;
                }
                match self.peek() {
                    Some(ch) => self.bump(ch),
                    None => {
                        self.error_at(
                            codes::UNTERMINATED_BLOCK_COMMENT,
                            "unterminated block comment",
                            start,
                        );
                        break;
                    }
                }
            }
        } else if self.eat("/=") {
            self.push_at(TokenKind::SlashEq, start);
        } else {
            self.eat("/");
            self.push_at(TokenKind::Slash, start);
        }
    }

    fn number(&mut self, start: usize) {
        self.digits();
        // Float: a dot followed by a digit (longest match keeps
        // `1..5` as Int DotDot Int and `3.` as Int Dot).
        let mut kind = TokenKind::Int;
        let rest = &self.src[self.pos..];
        if let Some(after_dot) = rest.strip_prefix('.')
            && after_dot.starts_with(|c: char| c.is_ascii_digit())
        {
            self.pos += 1;
            self.digits();
            kind = TokenKind::Float;
        }
        // Duration adjacency (SPEC §15): an *integer* immediately
        // followed by a registry unit, where the unit run is the whole
        // identifier-continue run after the digits.
        if kind == TokenKind::Int {
            let unit_start = self.pos;
            let run: String = self.src[self.pos..]
                .chars()
                .take_while(|&c| is_identifier_continue(c))
                .collect();
            if let Some(unit) = DurationUnit::lookup(&run) {
                self.pos = unit_start + run.len();
                self.push_at(TokenKind::Duration(unit), start);
                return;
            }
        }
        self.push_at(kind, start);
    }

    fn digits(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                self.bump(ch);
            } else {
                break;
            }
        }
    }

    fn identifier(&mut self, start: usize) {
        while let Some(ch) = self.peek() {
            if is_identifier_continue(ch) {
                self.bump(ch);
            } else {
                break;
            }
        }
        let text = &self.src[start..self.pos];
        let kind = if text == "_" {
            TokenKind::Underscore
        } else if let Some(kw) = Keyword::lookup(text) {
            TokenKind::Kw(kw)
        } else {
            // Tagged-template adjacency (SPEC §1): an identifier
            // immediately followed by a string delimiter is a tag
            // candidate. The registry check is parser-side (SPEC §16);
            // keywords and `_` are not identifier-like tags.
            if self.peek() == Some('"') {
                self.string_start(start, true);
                return;
            }
            TokenKind::Ident
        };
        self.push_at(kind, start);
    }

    /// A `'name` loop label — lifetime-style. The leading `'` is at
    /// `start` and already peeked; a valid label is `'` immediately followed by
    /// an identifier-start, then an identifier-continue run. A lone `'` (or `'`
    /// before a non-identifier char) is an UNKNOWN_CHAR error — Topaz has no
    /// character literals, so the apostrophe is otherwise meaningless.
    fn label(&mut self, start: usize) {
        self.bump('\''); // consume the leading apostrophe
        match self.peek() {
            Some(ch) if is_identifier_start(ch) => {
                self.bump(ch);
                while let Some(ch) = self.peek() {
                    if is_identifier_continue(ch) {
                        self.bump(ch);
                    } else {
                        break;
                    }
                }
                self.push_at(TokenKind::Label, start);
            }
            _ => {
                self.error_at(
                    codes::UNKNOWN_CHAR,
                    "a `'` begins a loop label `'name`; a bare `'` is not valid",
                    start,
                );
            }
        }
    }

    /// Longest-match operator/punctuation lexing (SPEC §1).
    fn operator(&mut self, ch: char, start: usize) {
        let kind = if self.eat("??=") {
            TokenKind::QuestionQuestionEq
        } else if self.eat("??") {
            TokenKind::QuestionQuestion
        } else if self.eat("?.") {
            TokenKind::QuestionDot
        } else if self.eat("?") {
            TokenKind::Question
        } else if self.eat("...") {
            TokenKind::Ellipsis
        } else if self.eat("..<") {
            TokenKind::DotDotLt
        } else if self.eat("..") {
            TokenKind::DotDot
        } else if self.eat(".") {
            TokenKind::Dot
        } else if self.eat("==") {
            TokenKind::EqEq
        } else if self.eat("=>") {
            TokenKind::FatArrow
        } else if self.eat("=") {
            TokenKind::Eq
        } else if self.eat("+=") {
            TokenKind::PlusEq
        } else if self.eat("+") {
            TokenKind::Plus
        } else if self.eat("->") {
            TokenKind::ThinArrow
        } else if self.eat("-=") {
            TokenKind::MinusEq
        } else if self.eat("-") {
            TokenKind::Minus
        } else if self.eat("**") {
            TokenKind::StarStar
        } else if self.eat("*=") {
            TokenKind::StarEq
        } else if self.eat("*") {
            TokenKind::Star
        } else if self.eat("%=") {
            TokenKind::PercentEq
        } else if self.eat("%") {
            TokenKind::Percent
        } else if self.eat("<=") {
            TokenKind::Le
        } else if self.eat("<") {
            TokenKind::Lt
        } else if self.eat(">>") {
            TokenKind::GtGt
        } else if self.eat(">=") {
            TokenKind::Ge
        } else if self.eat(">") {
            TokenKind::Gt
        } else if self.eat("!=") {
            TokenKind::Ne
        } else if self.eat("!") {
            TokenKind::Bang
        } else if self.eat("&&") {
            TokenKind::AndAnd
        } else if self.eat("|>") {
            TokenKind::PipeGt
        } else if self.eat("||") {
            TokenKind::OrOr
        } else if self.eat("|") {
            TokenKind::Pipe
        } else if self.eat("~") {
            TokenKind::Tilde
        } else if self.eat("(") {
            TokenKind::LParen
        } else if self.eat(")") {
            TokenKind::RParen
        } else if self.eat("[") {
            TokenKind::LBracket
        } else if self.eat("]") {
            TokenKind::RBracket
        } else if self.eat("{") {
            TokenKind::LBrace
        } else if self.eat("}") {
            TokenKind::RBrace
        } else if self.eat(",") {
            TokenKind::Comma
        } else if self.eat(":") {
            TokenKind::Colon
        } else if self.eat(";") {
            TokenKind::Semicolon
        } else {
            self.bump(ch);
            self.error_at(
                codes::UNKNOWN_CHAR,
                &format!("unknown character `{ch}`"),
                start,
            );
            return;
        };
        self.push_at(kind, start);
    }
}
