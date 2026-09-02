//! The recursive-descent parser (CDR-001 §6): the full SPEC grammar
//! of the selected version (v5.1 frozen / v5.2 locked, CDR-002)
//! over the normalized token stream, expression parsing by precedence
//! climbing over the frozen §2 operator table, panic-mode recovery
//! synchronizing on `Sep` and closing delimiters.
//!
//! `GtGt` splitting is type-context-only: when the type parser
//! expects a closing `>` and sees `>>`, it consumes one synthetic `>`
//! and leaves one pending; expression parsing always preserves `>>`
//! as the composition operator.

use std::collections::HashSet;
use std::rc::Rc;
use topaz_diag::{Diagnostic, FileId, Label, Span};
use topaz_lexer::{LayoutOptions, lex, normalize_with_options};
use topaz_syntax::ast::*;
use topaz_syntax::{Keyword, LangVersion, Token, TokenKind};

use crate::codes;

mod core;
mod declarations_modules;
mod expressions;
mod recovery;
mod types_patterns;

/// Result of parsing one source file: the `Program` plus every
/// diagnostic from lexing, layout normalization, and parsing.
/// parse-ok (CDR-001 §7) means `diagnostics` is empty.
#[derive(Debug)]
pub struct ParseOutput {
    pub program: Program,
    pub diagnostics: Vec<Diagnostic>,
}
/// Raw lexer result retained as an explicit compiler stage.
#[derive(Debug)]
pub struct RawTokenUnit {
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Parser-facing layout token stream retained as an explicit compiler stage.
#[derive(Debug)]
pub struct LayoutTokenUnit {
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Parser-only result over a supplied layout token stream.
#[derive(Debug)]
pub struct ParsedUnit {
    pub program: Program,
    pub diagnostics: Vec<Diagnostic>,
}

/// One-pass front-end stages for a single source.
#[derive(Debug)]
pub struct StagedParseOutput {
    pub raw: RawTokenUnit,
    pub layout: LayoutTokenUnit,
    pub parsed: ParsedUnit,
}

/// Lexes, normalizes, and parses `src` (the full text of `file`).
///
/// Precondition: `src` must fit the source-map loader bound
/// (`topaz_diag::MAX_SOURCE_LEN`, byte offsets are `u32`). Sources
/// loaded through `SourceMap::add_file` satisfy this by construction;
/// the CLI preflights file sizes at its loader boundary.
pub fn parse(file: FileId, src: &str) -> ParseOutput {
    parse_with_options(file, src, ParseOptions::default())
}

/// Options for a parse session (CDR-002 §1).
#[derive(Debug, Clone, Copy, Default)]
pub struct ParseOptions {
    pub language_version: LangVersion,
}

/// Versioned parse entry (CDR-002 §1): lexes, normalizes, and parses
/// `src` under the selected language version. Same precondition as
/// [`parse`].
pub fn parse_with_options(file: FileId, src: &str, options: ParseOptions) -> ParseOutput {
    let staged = parse_staged(file, src, options);
    let mut diagnostics = staged.raw.diagnostics;
    diagnostics.extend(staged.layout.diagnostics);
    diagnostics.extend(staged.parsed.diagnostics);
    ParseOutput {
        program: staged.parsed.program,
        diagnostics,
    }
}

/// Lex, normalize layout, and parse exactly once while retaining each stage.
pub fn parse_staged(file: FileId, src: &str, options: ParseOptions) -> StagedParseOutput {
    let lexed = lex(file, src);
    let layout = normalize_with_options(
        &lexed.tokens,
        src,
        LayoutOptions {
            language_version: options.language_version,
        },
    );
    let parsed = parse_layout_tokens(file, src, &layout.tokens, options);
    StagedParseOutput {
        raw: RawTokenUnit {
            tokens: lexed.tokens,
            diagnostics: lexed.diagnostics,
        },
        layout: LayoutTokenUnit {
            tokens: layout.tokens,
            diagnostics: layout.diagnostics,
        },
        parsed,
    }
}

/// Parse one already normalized token stream without lexing or layout work.
fn parse_layout_tokens(
    file: FileId,
    src: &str,
    tokens: &[Token],
    options: ParseOptions,
) -> ParsedUnit {
    let mut parser = Parser {
        tokens,
        src,
        file,
        pos: 0,
        last_hi: 0,
        pending_gt: None,
        naked_lambda_ok: true,
        record_update_ok: true,
        version: options.language_version,
        diagnostics: Vec::new(),
    };
    let program = parser.program();
    ParsedUnit {
        program,
        diagnostics: parser.diagnostics,
    }
}

/// Module-head classification (ADR-076 follow table).
#[derive(Debug, Clone, Copy)]
enum ModuleHead {
    Import,
    Export,
    ExportList,
    ExportImport,
    ReservedPath,
    ReservedUse,
}

/// Marker for panic-mode unwinding; the diagnostic is already pushed.
struct Abort;

type PResult<T> = Result<T, Abort>;

/// Range binding power (§2 level 6); range-pattern endpoints parse
/// one level tighter.
const RANGE_BP: u8 = 7;

struct Parser<'a> {
    tokens: &'a [Token],
    src: &'a str,
    file: FileId,
    pos: usize,
    /// End of the last consumed token, for node spans.
    last_hi: u32,
    /// The pending synthetic `>` left by splitting a `>>` in type
    /// context (CDR-001 §6); it is the next token while set.
    pending_gt: Option<Span>,
    /// Whether a lambda may begin at the top level of the expression
    /// being parsed. False only inside a `case` guard, where a naked
    /// `param =>` / `(params) =>` would otherwise swallow the case
    /// arrow (`Guard ::= "if" Expression` is ambiguous against
    /// `CaseClause`); any grouping delimiter re-allows lambdas.
    naked_lambda_ok: bool,
    /// Whether a postfix record update may start at the current expression
    /// level. Construct-bound expressions (`if cond { ... }`, etc.) disable it
    /// only at their outer level so the following brace remains the construct
    /// body; grouping delimiters re-enable it for nested expressions.
    record_update_ok: bool,
    /// Session language version (CDR-002 §1): gates the v5.2 syntax
    /// additions; at `V5_1` the parser is the v0.1 parser.
    version: LangVersion,
    diagnostics: Vec<Diagnostic>,
}
