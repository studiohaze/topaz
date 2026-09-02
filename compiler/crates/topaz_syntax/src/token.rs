use topaz_diag::Span;

macro_rules! keyword_registry {
    (
        $(#[$enum_meta:meta])*
        pub enum Keyword {
            $($variant:ident => $spelling:literal),+ $(,)?
        }
    ) => {
        $(#[$enum_meta])*
        pub enum Keyword {
            $($variant),+
        }

        impl Keyword {
            /// Every reserved word in declaration order.
            #[cfg(test)]
            pub(crate) fn all() -> impl ExactSizeIterator<Item = Self> {
                [$(Self::$variant),+].into_iter()
            }

            /// Keyword for an identifier spelling, if reserved.
            pub fn lookup(text: &str) -> Option<Self> {
                match text {
                    $($spelling => Some(Self::$variant),)+
                    _ => None,
                }
            }

            /// Canonical source spelling.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $spelling),+
                }
            }
        }
    };
}

keyword_registry! {
    /// The 21 reserved words of Topaz (SPEC §1): v5.0's 18 plus
    /// `while` / `break` / `continue` (v5.1). Duration units and template
    /// tags are registry entries, not keywords; `loop` and
    /// `import`/`export`/`use` are deliberately absent (they lex as ordinary
    /// identifiers and are recognized contextually by the parser).
    #[repr(u8)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum Keyword {
        Let => "let",
        Mut => "mut",
        Const => "const",
        Type => "type",
        Function => "function",
        Return => "return",
        If => "if",
        Else => "else",
        Match => "match",
        Case => "case",
        For => "for",
        In => "in",
        By => "by",
        While => "while",
        Break => "break",
        Continue => "continue",
        Defer => "defer",
        Concurrent => "concurrent",
        True => "true",
        False => "false",
        Null => "null",
    }
}

/// Unit of a `DurationLiteral` (SPEC §15): `ms`, `s`, or `m`,
/// lexically adjacent to its integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DurationUnit {
    Ms,
    S,
    M,
}

impl DurationUnit {
    pub fn lookup(text: &str) -> Option<DurationUnit> {
        Some(match text {
            "ms" => DurationUnit::Ms,
            "s" => DurationUnit::S,
            "m" => DurationUnit::M,
            _ => return None,
        })
    }

    /// Convert a duration magnitude to the §15 runtime millisecond unit.
    /// Overflow is not a duration value; callers surface the static or dynamic
    /// boundary appropriate to their phase.
    pub const fn checked_milliseconds(self, magnitude: u64) -> Option<u64> {
        match self {
            Self::Ms => Some(magnitude),
            Self::S => magnitude.checked_mul(1_000),
            Self::M => magnitude.checked_mul(60_000),
        }
    }
}

/// Parse an exact §15 duration literal and convert it to the runtime
/// millisecond unit. Keeping lexical parsing here lets the AST, checked HIR,
/// interpreter, and backends share overflow behavior without translating
/// their phase-owned `DurationUnit` enums.
pub fn parse_duration_milliseconds(literal: &str) -> Option<u64> {
    let (magnitude, unit) = if let Some(magnitude) = literal.strip_suffix("ms") {
        (magnitude, DurationUnit::Ms)
    } else if let Some(magnitude) = literal.strip_suffix('s') {
        (magnitude, DurationUnit::S)
    } else if let Some(magnitude) = literal.strip_suffix('m') {
        (magnitude, DurationUnit::M)
    } else {
        return None;
    };
    if magnitude.is_empty() || !magnitude.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    unit.checked_milliseconds(magnitude.parse().ok()?)
}

/// Token vocabulary produced by the raw and template lexers.
///
/// Token text is not owned here — it is recovered through the token's
/// span (CDR-001 §4). Layout `Sep` lands with the layout normalizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    // Names and literals
    Kw(Keyword),
    Ident,
    /// The reserved single `_` (ADR-050) — not a bindable identifier.
    Underscore,
    Int,
    Float,
    Duration(DurationUnit),
    /// A `'name` loop label — lifetime-style. The token span covers
    /// the leading `'` plus the identifier; the label NAME is the span minus
    /// that apostrophe. Only the parser interprets it (in `loop`/`break`/
    /// `continue` positions); elsewhere it is a syntax error.
    Label,

    // Strings and templates — token-tree model (CDR-001 §4): a
    // literal lexes as StringStart, then StringText and interpolation
    // runs, then StringEnd. Interpolation bodies are ordinary tokens.
    /// Opening delimiter of a string or template literal. When
    /// `tagged`, the span also covers the adjacent tag candidate;
    /// registry validation is parser-side (SPEC §16).
    StringStart {
        tagged: bool,
        multiline: bool,
    },
    /// A maximal run of raw string text, escape sequences included.
    /// Cooking (escape resolution, leading-newline absorption,
    /// indentation stripping) happens at lowering.
    StringText,
    /// `{` opening an interpolation inside string text.
    InterpolationStart,
    /// `}` closing an interpolation, back into string text.
    InterpolationEnd,
    /// Closing delimiter of a string or template literal. Zero-width
    /// when synthesized for an unterminated literal.
    StringEnd,

    // Grouping
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,

    // Operators and punctuation (longest-match, SPEC §1/§2)
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    StarStar,
    DotDot,
    DotDotLt,
    Ellipsis,
    Dot,
    QuestionDot,
    Question,
    QuestionQuestion,
    QuestionQuestionEq,
    Lt,
    Le,
    Gt,
    Ge,
    GtGt,
    EqEq,
    Ne,
    Eq,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    Bang,
    Tilde,
    AndAnd,
    OrOr,
    Pipe,
    PipeGt,
    FatArrow,
    ThinArrow,
    Comma,
    Colon,
    /// `;` — an explicit item separator (SPEC §1a). The layout
    /// normalizer turns it into [`TokenKind::Sep`].
    Semicolon,

    // Layout
    /// A statement/item separator synthesized by the layout
    /// normalizer from a significant newline or an explicit `;`
    /// (SPEC §1a); its span is the newline or semicolon it stands
    /// for.
    Sep,
    /// A physical line terminator (`\n` or `\r\n`). The layout
    /// normalizer consumes these; the parser never sees them.
    Newline,
    /// End of file.
    Eof,
}

/// One lexed token: kind plus source span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_table_has_exactly_21_entries() {
        assert_eq!(Keyword::all().len(), 21);
        for (ordinal, keyword) in Keyword::all().enumerate() {
            assert_eq!(keyword as usize, ordinal);
            assert_eq!(Keyword::lookup(keyword.as_str()), Some(keyword));
        }
        assert_eq!(Keyword::lookup("loop"), None, "loop is contextual");
    }

    #[test]
    fn module_words_are_not_keywords() {
        // ADR-071: forbidden by grammar, not by reservation.
        assert_eq!(Keyword::lookup("import"), None);
        assert_eq!(Keyword::lookup("export"), None);
        assert_eq!(Keyword::lookup("use"), None);
        // Registry entries are not keywords either.
        assert_eq!(Keyword::lookup("ms"), None);
        assert_eq!(Keyword::lookup("sql"), None);
    }

    #[test]
    fn duration_units_reject_millisecond_overflow() {
        assert_eq!(
            DurationUnit::Ms.checked_milliseconds(u64::MAX),
            Some(u64::MAX)
        );
        assert_eq!(
            DurationUnit::S.checked_milliseconds(u64::MAX / 1_000),
            Some((u64::MAX / 1_000) * 1_000)
        );
        assert_eq!(
            DurationUnit::S.checked_milliseconds(u64::MAX / 1_000 + 1),
            None
        );
        assert_eq!(
            DurationUnit::M.checked_milliseconds(u64::MAX / 60_000),
            Some((u64::MAX / 60_000) * 60_000)
        );
        assert_eq!(
            DurationUnit::M.checked_milliseconds(u64::MAX / 60_000 + 1),
            None
        );
        assert_eq!(parse_duration_milliseconds("3s"), Some(3_000));
        assert_eq!(parse_duration_milliseconds("99999999999999999m"), None);
        assert_eq!(parse_duration_milliseconds("3m"), Some(180_000));
        assert_eq!(parse_duration_milliseconds("3"), None);
    }
}
