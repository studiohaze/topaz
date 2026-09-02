use super::*;

#[test]
fn emit_error_displays_a_human_reason() {
    // The `topaz emit` / `build` CLI renders the reason a program cannot be
    // lowered yet via `Display` (not the raw `Debug`).
    assert_eq!(
        EmitError::unsupported("typed pattern type").to_string(),
        "unsupported: typed pattern type"
    );
    assert_eq!(
        EmitError::no_entry().to_string(),
        "the unit has no entry module"
    );
    assert_eq!(
        EmitError::malformed_literal("int literal").to_string(),
        "malformed literal: int literal"
    );
}

#[test]
fn unsupported_renders_a_located_tpz6001_with_a_remedy() {
    // The emitter OWNS the coverage-gap diagnostic: code TPZ6001, the offending
    // span, the construct in the message, and a "still runs under `topaz run`"
    // remedy note. The CLI only renders it (CDR-001 §5; `topaz_emit::codes`).
    let span = Span::new(topaz_diag::FileId(0), 4, 7);
    let diag = EmitError::unsupported("free identifier")
        .at(span)
        .diagnostic()
        .expect("an unsupported construct yields a diagnostic");
    assert_eq!(diag.code.as_str(), "TPZ6001");
    assert_eq!(diag.primary.span, span);
    assert!(
        diag.message.contains("free identifier"),
        "got: {}",
        diag.message
    );
    assert!(
        diag.notes.iter().any(|n| n.contains("topaz run")),
        "want a remedy note pointing at `topaz run`: {:?}",
        diag.notes
    );
}

#[test]
fn at_is_first_wins_so_the_innermost_span_survives() {
    // As an error unwinds, every enclosing boundary calls `.at`; the FIRST
    // (innermost, tightest) span must survive so the caret lands on the
    // offending node, not its coarse enclosure.
    let inner = Span::new(topaz_diag::FileId(0), 10, 14);
    let outer = Span::new(topaz_diag::FileId(0), 0, 40);
    let located = EmitError::unsupported("x").at(inner).at(outer);
    assert_eq!(located.span, Some(inner));
}

#[test]
fn internal_or_unlocated_errors_have_no_user_diagnostic() {
    // `NoEntry` is an internal defect (a checked program always has an entry),
    // and an unlocated error cannot point anywhere — neither yields a
    // diagnostic, so the CLI falls back to a plain message instead.
    assert!(EmitError::no_entry().diagnostic().is_none());
    assert!(EmitError::unsupported("x").diagnostic().is_none());
    assert!(EmitError::malformed_literal("int").diagnostic().is_none());
}
