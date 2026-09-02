//! Token kinds and AST definitions for the Topaz language.
//!
//! Types only — no lexing or parsing logic lives here (CDR-001 §3).
//! Tokens and the statement/expression/pattern/type AST nodes carry
//! [`topaz_diag`] spans; supporting nodes recover their extent from
//! their children (see `ast`).

pub mod ast;
mod token;
mod version;

pub use token::{DurationUnit, Keyword, Token, TokenKind, parse_duration_milliseconds};
pub use version::LangVersion;
