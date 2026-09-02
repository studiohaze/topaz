//! The template lexer (CDR-001 §4): string and template literals as
//! token trees — `StringStart`, `StringText` and interpolation runs,
//! `StringEnd` — sharing the raw scanner through a mode stack.
//!
//! Interpolation bodies are ordinary tokens (nested strings included)
//! with brace-depth tracking, so interpolation expressions go through
//! the normal expression parser. Physical newlines inside multiline
//! interpolations are emitted as `Newline` tokens; the layout
//! normalizer treats the region between `InterpolationStart` and
//! `InterpolationEnd` as continuation mode (SPEC §1a).
//!
//! Text runs are raw source: escape sequences are validated here but
//! resolved at lowering, and multiline indentation is validated here
//! (SPEC §1) while the stripping itself happens at lowering.

use topaz_diag::{Diagnostic, Label, Span};
use topaz_syntax::TokenKind;

use crate::codes;
use crate::raw::Lexer;

/// One string/template lexing context on the mode stack.
#[derive(Debug)]
pub(crate) enum Mode {
    /// Inside string text. `quote` is the offset of the opening
    /// delimiter's first `"`, for unterminated-string labels. An open
    /// multiline string owns its indentation-validation state here.
    Text {
        quote: usize,
        multiline: Option<MultilineString>,
    },
    /// Inside a `{...}` interpolation; `depth` counts open braces
    /// that belong to the expression, not to the template. The
    /// enclosing `Text` mode owns whether the string is multiline.
    Interp { depth: u32 },
}

/// The nearest interpolation whose enclosing string forbids physical
/// line breaks. The stack position and opening quote are observed
/// together so recovery does not have to rediscover the outer text mode.
#[derive(Debug, Clone, Copy)]
struct SingleLineInterpolation {
    mode: usize,
    quote: usize,
}

/// Validation state for one open multiline string.
#[derive(Debug, Default)]
pub(crate) struct MultilineString {
    /// Offsets of content lines that begin in string text. A line
    /// that begins inside an interpolation is expression code and is
    /// not subject to the indent check.
    line_starts: Vec<usize>,
}

impl Lexer<'_> {
    /// Opens a string literal at the current position. `token_lo`
    /// extends left over the tag candidate when `tagged` (SPEC §1
    /// adjacency); the tag registry is validated parser-side.
    pub(crate) fn string_start(&mut self, token_lo: usize, tagged: bool) {
        let quote = self.pos;
        let multiline = self.eat("\"\"\"");
        if !multiline {
            self.eat("\"");
        }
        self.push_at(TokenKind::StringStart { tagged, multiline }, token_lo);
        let multiline = if multiline {
            let mut meta = MultilineString::default();
            // SPEC §1: content begins immediately after the opening
            // delimiter unless the delimiter is immediately followed
            // by a line terminator — same-line content is the first
            // content line and is subject to the indent check.
            let rest = &self.src[self.pos..];
            if !rest.is_empty() && !rest.starts_with('\n') && !rest.starts_with("\r\n") {
                meta.line_starts.push(self.pos);
            }
            Some(meta)
        } else {
            None
        };
        self.modes.push(Mode::Text { quote, multiline });
    }

    /// Lexes string text up to the next structural point: an
    /// interpolation start, the closing delimiter, or an unterminated
    /// break.
    pub(crate) fn string_text(&mut self) {
        let Some(Mode::Text { multiline, quote }) = self.modes.last() else {
            unreachable!("string_text outside Text mode");
        };
        let multiline = multiline.is_some();
        let quote = *quote;
        let single_line_interpolation = if multiline {
            self.single_line_interpolation_ancestor()
        } else {
            None
        };
        let run_start = self.pos;
        loop {
            let Some(ch) = self.peek() else {
                self.flush_text(run_start);
                self.unterminated(multiline, quote, "unterminated string literal");
                return;
            };
            match ch {
                '"' if multiline => {
                    // The first unescaped `"""` closes the literal
                    // (SPEC §1); lone `"` and `""` are raw text.
                    if self.src[self.pos..].starts_with("\"\"\"") {
                        self.flush_text(run_start);
                        let delim = self.pos;
                        self.pos += 3;
                        self.push_at(TokenKind::StringEnd, delim);
                        let Some(Mode::Text {
                            multiline: Some(meta),
                            ..
                        }) = self.modes.pop()
                        else {
                            unreachable!("multiline text mode changed before removal");
                        };
                        self.check_indent(&meta, delim);
                        return;
                    }
                    self.bump(ch);
                }
                '"' => {
                    self.flush_text(run_start);
                    let delim = self.pos;
                    self.bump(ch);
                    self.push_at(TokenKind::StringEnd, delim);
                    self.modes.pop();
                    return;
                }
                '{' => {
                    self.flush_text(run_start);
                    let brace = self.pos;
                    self.bump(ch);
                    self.push_at(TokenKind::InterpolationStart, brace);
                    self.modes.push(Mode::Interp { depth: 0 });
                    return;
                }
                '\\' => self.escape(),
                '\n' if multiline => {
                    if let Some(interpolation) = single_line_interpolation {
                        self.flush_text(run_start);
                        self.close_single_line_interpolation_at_line_break(interpolation);
                        return;
                    }
                    self.bump(ch);
                    let Some(Mode::Text {
                        multiline: Some(meta),
                        ..
                    }) = self.modes.last_mut()
                    else {
                        unreachable!("multiline text mode changed while scanning");
                    };
                    meta.line_starts.push(self.pos);
                }
                '\r' if multiline && self.src[self.pos..].starts_with("\r\n") => {
                    if let Some(interpolation) = single_line_interpolation {
                        self.flush_text(run_start);
                        self.close_single_line_interpolation_at_line_break(interpolation);
                        return;
                    }
                    self.bump(ch);
                }
                '\n' if !multiline => {
                    self.flush_text(run_start);
                    self.unterminated(
                        multiline,
                        quote,
                        "single-line string literal is not closed before the line break",
                    );
                    return; // the line break lexes as a Newline token
                }
                '\r' if !multiline && self.src[self.pos..].starts_with("\r\n") => {
                    self.flush_text(run_start);
                    self.unterminated(
                        multiline,
                        quote,
                        "single-line string literal is not closed before the line break",
                    );
                    return;
                }
                '}' if !multiline => {
                    // SPEC §1: single-line text may not contain a bare
                    // `}` (multiline raw text may).
                    let brace = self.pos;
                    self.bump(ch);
                    self.error_at(
                        codes::STRAY_BRACE_IN_STRING,
                        "unescaped `}` in a string literal; write `\\}`",
                        brace,
                    );
                }
                _ => self.bump(ch),
            }
        }
    }

    /// Lexes one token inside `{...}` interpolation: ordinary code
    /// tokens, with brace depth deciding which `}` closes the
    /// interpolation.
    pub(crate) fn interpolation_token(&mut self) {
        let interpolation = self.modes.len() - 1;
        let [.., Mode::Text { multiline, quote }, Mode::Interp { depth }] = self.modes.as_slice()
        else {
            unreachable!("interpolation_token outside Interp mode");
        };
        let multiline = multiline.is_some();
        let quote = *quote;
        let depth = *depth;
        let Some(ch) = self.peek() else {
            // End of input: balance the tree; the enclosing text mode
            // reports the unterminated string.
            self.push_at(TokenKind::InterpolationEnd, self.pos);
            self.modes.pop();
            return;
        };
        if !multiline && (ch == '\n' || (ch == '\r' && self.src[self.pos..].starts_with("\r\n"))) {
            self.close_single_line_interpolation_at_line_break(SingleLineInterpolation {
                mode: interpolation,
                quote,
            });
            return;
        }
        let start = self.pos;
        match ch {
            '{' => {
                self.bump(ch);
                self.push_at(TokenKind::LBrace, start);
                if let Some(Mode::Interp { depth, .. }) = self.modes.last_mut() {
                    *depth += 1;
                }
            }
            '}' => {
                self.bump(ch);
                if depth == 0 {
                    self.push_at(TokenKind::InterpolationEnd, start);
                    self.modes.pop();
                } else {
                    self.push_at(TokenKind::RBrace, start);
                    if let Some(Mode::Interp { depth, .. }) = self.modes.last_mut() {
                        *depth -= 1;
                    }
                }
            }
            _ => self.code_token(ch),
        }
    }

    /// SPEC §1: a single-line string literal does not contain
    /// unescaped newlines — its interpolations included. Closes the
    /// interpolation and the string with synthetic tokens; the line
    /// break itself lexes as a Newline token.
    /// A single-line interpolation may be the active mode or an ancestor
    /// of nested strings. Balance every nested mode at a physical line
    /// break, then close and report the enclosing single-line string.
    fn close_single_line_interpolation_at_line_break(
        &mut self,
        interpolation: SingleLineInterpolation,
    ) {
        while self.modes.len() > interpolation.mode + 1 {
            match self.modes.pop().expect("nested string mode") {
                Mode::Text { .. } => {
                    self.push_at(TokenKind::StringEnd, self.pos);
                }
                Mode::Interp { .. } => {
                    self.push_at(TokenKind::InterpolationEnd, self.pos);
                }
            }
        }

        let Some(Mode::Interp { .. }) = self.modes.pop() else {
            unreachable!("single-line interpolation mode changed before recovery");
        };
        self.push_at(TokenKind::InterpolationEnd, self.pos);
        self.unterminated(
            false,
            interpolation.quote,
            "string interpolation in a single-line string cannot contain a line break",
        );
    }

    fn single_line_interpolation_ancestor(&self) -> Option<SingleLineInterpolation> {
        self.modes
            .windows(2)
            .enumerate()
            .rev()
            .find_map(|(text, pair)| match pair {
                [
                    Mode::Text {
                        quote,
                        multiline: None,
                    },
                    Mode::Interp { .. },
                ] => Some(SingleLineInterpolation {
                    mode: text + 1,
                    quote: *quote,
                }),
                _ => None,
            })
    }

    /// Validates one `\` escape against the SPEC §1 escape set.
    fn escape(&mut self) {
        let esc = self.pos;
        self.bump('\\');
        match self.peek() {
            Some(ch @ ('n' | 't' | 'r' | '\\' | '"' | '{' | '}')) => self.bump(ch),
            Some('\n' | '\r') | None => {
                // Not consumed: the line break (or end of input) is
                // handled by the text loop.
                self.error_at(
                    codes::INVALID_ESCAPE,
                    "invalid escape: `\\` before the end of the line",
                    esc,
                );
            }
            Some(ch) => {
                self.bump(ch);
                self.error_at(
                    codes::INVALID_ESCAPE,
                    &format!("invalid escape `\\{ch}` in a string literal"),
                    esc,
                );
            }
        }
    }

    /// Emits a `StringText` token for `run_start..pos` if non-empty.
    fn flush_text(&mut self, run_start: usize) {
        if self.pos > run_start {
            self.push_at(TokenKind::StringText, run_start);
        }
    }

    /// Emits the unterminated-string diagnostic (label on the opening
    /// delimiter) and a zero-width synthetic `StringEnd` so the token
    /// tree stays balanced.
    fn unterminated(&mut self, multiline: bool, quote: usize, message: &str) {
        let delim_len = if multiline { 3 } else { 1 };
        let span = Span::new(self.file, quote as u32, (quote + delim_len) as u32);
        self.diagnostics.push(Diagnostic::error(
            codes::UNTERMINATED_STRING,
            message,
            Label::new(span, "string starts here"),
        ));
        self.push_at(TokenKind::StringEnd, self.pos);
        self.modes.pop();
    }

    /// SPEC §1 indentation validation for a closed multiline string:
    /// if the closing delimiter is preceded on its line only by spaces
    /// and tabs, that exact prefix is the closing indent, and every
    /// non-blank content line must begin with it. The diagnostic span
    /// is the offending line; stripping happens at lowering.
    fn check_indent(&mut self, meta: &MultilineString, delim: usize) {
        let src = self.src;
        let close_line = src[..delim].rfind('\n').map_or(0, |i| i + 1);
        let indent = &src[close_line..delim];
        if indent.is_empty() || !indent.chars().all(|c| c == ' ' || c == '\t') {
            // Empty closing indent: nothing to strip, nothing to check.
            return;
        }
        for &line in &meta.line_starts {
            if line >= close_line {
                break; // the closing-delimiter line is not content
            }
            let end = src[line..].find('\n').map_or(src.len(), |i| line + i);
            let raw = &src[line..end];
            let text = raw.strip_suffix('\r').unwrap_or(raw);
            if text.chars().all(|c| c == ' ' || c == '\t') {
                continue; // blank lines are exempt
            }
            if !text.starts_with(indent) {
                let span = Span::new(self.file, line as u32, (line + text.len()) as u32);
                self.diagnostics.push(Diagnostic::error(
                    codes::TEMPLATE_INDENT,
                    "line does not begin with the closing indent of its multiline string",
                    Label::new(
                        span,
                        "must start with the whitespace prefix of the closing `\"\"\"`",
                    ),
                ));
            }
        }
    }
}
