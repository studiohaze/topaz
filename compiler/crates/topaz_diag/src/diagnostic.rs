use crate::code::Code;
use crate::span::Span;

/// Diagnostic severity. v0.1 producers emit errors; warnings exist so
/// the model does not need to change when a non-fatal producer
/// appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// A span with an attached message. The primary label points at the
/// offending source; secondary labels add context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub span: Span,
    pub message: String,
}

impl Label {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}

/// A single diagnostic (CDR-001 §5).
///
/// Data-oriented by design (CDR-001 §5): rendering — plain text
/// today, JSON/LSP later — consumes this structure and never feeds
/// back into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: Code,
    pub severity: Severity,
    pub message: String,
    pub primary: Label,
    pub secondary: Vec<Label>,
    pub notes: Vec<String>,
}

impl Diagnostic {
    /// An error diagnostic with its primary label.
    pub fn error(code: Code, message: impl Into<String>, primary: Label) -> Self {
        Self {
            code,
            severity: Severity::Error,
            message: message.into(),
            primary,
            secondary: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// A warning diagnostic with its primary label. Non-fatal: a
    /// consumer renders it but does not fail on it (see [`has_errors`]).
    /// No producer emits one yet; the future determinism lint is the
    /// first (CDR-001 §5 — severity exists so the model need not change
    /// when a non-fatal producer appears).
    pub fn warning(code: Code, message: impl Into<String>, primary: Label) -> Self {
        Self {
            code,
            severity: Severity::Warning,
            message: message.into(),
            primary,
            secondary: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// Adds a secondary (context) label.
    pub fn with_secondary(mut self, span: Span, message: impl Into<String>) -> Self {
        self.secondary.push(Label::new(span, message));
        self
    }

    /// Adds a free-text note rendered after the labels.
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

/// True when any diagnostic in the slice is error-severity. This is the
/// single CLI admission policy (CDR-001 §5): a stream carrying only
/// warnings is rendered but is NOT a failure.
///
/// Resolver and checker passes use emptiness only for diagnostic streams that
/// cannot contain warnings; user-facing admission is always severity-aware.
pub fn has_errors(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| d.severity == Severity::Error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::{FileId, Span};

    fn label() -> Label {
        Label::new(Span::new(FileId(0), 0, 0), "")
    }

    #[test]
    fn warning_sets_warning_severity() {
        let d = Diagnostic::warning(Code::new("TPZ9001"), "w", label());
        assert_eq!(d.severity, Severity::Warning);
    }

    #[test]
    fn error_sets_error_severity() {
        let d = Diagnostic::error(Code::new("TPZ0001"), "e", label());
        assert_eq!(d.severity, Severity::Error);
    }

    #[test]
    fn has_errors_is_true_only_with_an_error() {
        let warn = Diagnostic::warning(Code::new("TPZ9001"), "w", label());
        let err = Diagnostic::error(Code::new("TPZ0001"), "e", label());
        assert!(!has_errors(&[]));
        assert!(!has_errors(std::slice::from_ref(&warn)));
        assert!(has_errors(std::slice::from_ref(&err)));
        assert!(has_errors(&[warn, err]));
    }
}
