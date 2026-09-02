//! Lexer (TPZ0xxx) and layout (TPZ1xxx) diagnostic codes (CDR-001
//! §5).
//!
//! These codes are **stable**: each is pinned by a fixture in
//! `corpus/v5.1/invalid/` that asserts it as the primary diagnostic.
//! Renumbering or removal is a breaking change to downstream
//! consumers and requires a design-record decision.

use topaz_diag::Code;

/// A character that begins no token.
pub const UNKNOWN_CHAR: Code = Code::new("TPZ0001");
/// `/*` without a closing `*/`.
pub const UNTERMINATED_BLOCK_COMMENT: Code = Code::new("TPZ0002");
/// A string literal that is not closed before a line break
/// (single-line form) or the end of the file.
pub const UNTERMINATED_STRING: Code = Code::new("TPZ0003");
/// A `\` escape outside the SPEC §1 escape set.
pub const INVALID_ESCAPE: Code = Code::new("TPZ0004");
/// An unescaped `}` in single-line string text (SPEC §1 requires `\}`).
pub const STRAY_BRACE_IN_STRING: Code = Code::new("TPZ0005");
/// A non-blank multiline-string content line that does not begin with
/// the closing indent (SPEC §1 indentation stripping).
pub const TEMPLATE_INDENT: Code = Code::new("TPZ0006");

/// `;` inside a continuation-mode delimiter context (SPEC §1a:
/// delimiter lists separate with `,`).
pub const SEMICOLON_IN_DELIMITER_LIST: Code = Code::new("TPZ1001");
