//! Identifier character classification (SPEC §1) against the
//! generated, pinned Unicode tables.
//!
//! Emoji identifier classification is by Unicode scalar property
//! `Emoji=True`, after the ASCII exclusion (CDR-001 §3). Emoji ZWJ
//! sequences are **not** single identifier atoms: U+200D is not an
//! identifier character, so it terminates the identifier and is
//! diagnosed as an unknown character unless a future language ADR
//! admits emoji sequences.

use crate::unicode_tables::{EMOJI, LETTER, NUMBER, contains};

/// `IdentifierStart ::= UnicodeLetter | "_" | Emoji`
pub fn is_identifier_start(ch: char) -> bool {
    ch == '_' || contains(LETTER, ch) || contains(EMOJI, ch)
}

/// `IdentifierContinue ::= IdentifierStart | UnicodeNumber`
pub fn is_identifier_continue(ch: char) -> bool {
    is_identifier_start(ch) || contains(NUMBER, ch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unicode_version::UNICODE_VERSION;

    #[test]
    fn version_is_pinned() {
        assert_eq!(UNICODE_VERSION, "16.0.0");
    }

    #[test]
    fn letters_across_scripts_start_identifiers() {
        for ch in ['a', 'Z', '한', '글', 'п', 'я', 'é', '中'] {
            assert!(is_identifier_start(ch), "{ch:?} should start an identifier");
        }
    }

    #[test]
    fn emoji_start_identifiers_but_ascii_emoji_overlap_does_not() {
        assert!(is_identifier_start('😀'));
        assert!(is_identifier_start('🚀'));
        // `#`, `*`, and digits carry the UCD Emoji property but are
        // grammar characters; the generator excludes ASCII.
        assert!(!is_identifier_start('#'));
        assert!(!is_identifier_start('*'));
        assert!(!is_identifier_start('0'));
    }

    #[test]
    fn digits_continue_but_do_not_start() {
        assert!(!is_identifier_start('7'));
        assert!(is_identifier_continue('7'));
        // Non-ASCII digits count as UnicodeNumber.
        assert!(is_identifier_continue('٣')); // ARABIC-INDIC DIGIT THREE
    }

    #[test]
    fn operators_and_whitespace_are_not_identifier_chars() {
        for ch in ['+', '|', '.', ' ', '\n', '"', '{', '}'] {
            assert!(!is_identifier_continue(ch), "{ch:?}");
        }
    }
}
