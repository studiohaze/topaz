//! Source spans, source maps and diagnostics for Topaz.
//!
//! Lines and Unicode scalar columns are 1-based. Byte offsets refer to UTF-8
//! source. Caret alignment does not account for the display width of wide glyphs.
//! Plain-text and JSON renderers consume the same diagnostic data.

mod code;
mod diagnostic;
mod explain;
mod import_chain;
mod render;
mod source_map;
mod span;
pub mod suggest;

pub use code::{Code, extern_codes, guard_codes};
pub use diagnostic::{Diagnostic, Label, Severity, has_errors};
pub use explain::{
    DiagnosticExplanation, ExplainExamples, explain_code, is_explain_code_shape, render_explain,
    render_explain_json,
};
pub use import_chain::render_import_chain;
pub use render::{render, render_json};
pub use source_map::{LineCol, MAX_SOURCE_LEN, SourceFile, SourceMap, SourceMapError};
pub use span::{FileId, Span};
