//! The differential comparator (CDR-006 §3): ONE implementation of
//! expectation matching, called by every harness — the interpreter
//! corpus, the CLI checks, and both differential harnesses —
//! so "matches" can never mean two different things in two places.
//!
//! Comparison is over UTF-8 text with LF-normalized line endings.
//! Fault expectations are EXACT: code equality plus full-message
//! equality (an empty expected message pins only the code — used
//! while a corpus row migrates, never as a long-term posture).

/// Normalizes CRLF/CR line endings to LF.
pub fn normalize_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Exact text equality under LF normalization.
pub fn same_text(expected: &str, got: &str) -> bool {
    normalize_lf(expected) == normalize_lf(got)
}

/// Fault-message matching: EXACT under LF normalization; an empty
/// expectation pins only the code.
pub fn message_matches(expected: &str, got: &str) -> bool {
    expected.is_empty() || same_text(expected, got)
}

/// Splits a golden transcript sidecar into expected lines: the file
/// is LF-normalized, one trailing newline is tolerated, and an
/// empty file means an empty transcript.
pub fn transcript_lines(golden: &str) -> Vec<String> {
    let normalized = normalize_lf(golden);
    if normalized.is_empty() {
        return Vec::new();
    }
    normalized
        .strip_suffix('\n')
        .unwrap_or(&normalized)
        .split('\n')
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_and_exactness() {
        assert!(same_text("a\nb", "a\r\nb"));
        assert!(!same_text("a", "a "));
        assert!(message_matches("", "anything"));
        assert!(message_matches("division by zero", "division by zero"));
        assert!(!message_matches("division", "division by zero"));
        assert_eq!(transcript_lines("a\r\nb\r\n"), vec!["a", "b"]);
        assert_eq!(transcript_lines("a\n\n"), vec!["a", ""]);
        assert_eq!(transcript_lines(""), Vec::<String>::new());
    }
}
