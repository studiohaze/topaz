//! Source spans, source maps, and diagnostics for the Topaz compiler.
//!
//! This crate owns [`FileId`], [`Span`], [`SourceFile`], [`SourceMap`],
//! [`Diagnostic`], [`Label`], and [`Severity`] (CDR-001 §3, §5). It has
//! no dependencies; every other crate in the workspace depends on it.
//!
//! Rendering rules for v0.1 (CDR-001 §5): line numbers are 1-based;
//! columns are 1-based Unicode **scalar** counts computed from the
//! UTF-8 source at render time (not grapheme/display-width); caret
//! alignment over wide glyphs is best-effort until a future
//! diagnostics CDR. The diagnostic model is data-oriented — the
//! plain-text renderer in [`render`] is one consumer, not the model.

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
