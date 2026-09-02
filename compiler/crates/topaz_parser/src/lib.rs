//! Recursive-descent parser and panic-mode recovery for Topaz.
//!
//! Parses the full SPEC grammar of the selected language version —
//! v5.1 (frozen) or the locked v5.2 (profiles are a checker-era
//! concern, CDR-001 §6) — over the normalized token stream produced by
//! [`topaz_lexer`]. [`parse`] runs the whole front end — raw lexing,
//! template lexing, layout normalization, parsing — and aggregates
//! every diagnostic; parse-ok means zero diagnostics with the full
//! file consumed. The parse-corpus harness lives in this crate's
//! integration tests.

pub mod codes;
mod parser;

pub use parser::{
    LayoutTokenUnit, ParseOptions, ParseOutput, ParsedUnit, RawTokenUnit, StagedParseOutput, parse,
    parse_staged, parse_with_options,
};
