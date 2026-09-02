//! Raw lexer, template lexer, and layout normalizer for Topaz
//! (CDR-001 §4).
//!
//! Owns raw lexing, the string/template token-tree lexer, the
//! syntax-aware layout normalizer, and the generated Unicode
//! identifier tables (`unicode_tables`, produced by
//! `tools/unicode-gen` from the pinned UCD version in
//! [`UNICODE_VERSION`]).

pub mod codes;
mod layout;
mod raw;
mod template;
mod unicode;
#[rustfmt::skip]
mod unicode_tables;
mod unicode_version;

pub use layout::{LayoutOptions, LayoutOutput, normalize, normalize_with_options};
pub use raw::{LexOutput, lex};
pub use unicode::{is_identifier_continue, is_identifier_start};
pub use unicode_version::UNICODE_VERSION;
