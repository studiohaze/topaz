//! Token kinds and AST definitions for the Topaz language.
//!
//! Types only — no lexing or parsing logic lives here (CDR-001 §3).
//! Tokens and the statement/expression/pattern/type AST nodes carry
//! [`topaz_diag`] spans; supporting nodes recover their extent from
//! their children (see `ast`).

pub mod ast;
mod literal;
pub use literal::{
    CHECKED_LITERAL_ENCODING, CookedRun, CookedText, IrLiteral, IrLiteralEncoding,
    LEGACY_LITERAL_ENCODING, LITERAL_TABLE_ENCODING, LITERAL_TABLE_MARKER, LiteralRender,
    NumericError, STANDARD_RENDER_DEPTH, STANDARD_RENDER_FUEL, StringLayout, cook_string_literal,
    cooked_source_map, decimal_literal_parts, decode_float_literal, decode_integer_literal,
    decode_ir_literal, decode_string_escapes, enter_standard_render, integer_literal_decimal,
    integer_literal_parts, render_aggregate_into, render_float, render_function_name_into,
    signed_integer_literal, signed_integer_literal_for_version, string_layout, valid_digit_run,
};
mod token;
mod version;

pub mod integer_bits;
pub mod length;
pub mod standard_api;
pub use length::{Length, LengthUnit};
pub use token::{DurationUnit, Keyword, Token, TokenKind, parse_duration_milliseconds};
pub use version::LangVersion;
