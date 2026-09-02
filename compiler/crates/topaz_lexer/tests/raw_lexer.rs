//! Raw-lexer integration tests over the public API.

use topaz_diag::FileId;
use topaz_lexer::lex;
use topaz_syntax::{DurationUnit, Keyword, TokenKind};

/// Lexes and returns kinds without the trailing Eof; asserts no
/// diagnostics.
fn kinds(src: &str) -> Vec<TokenKind> {
    let out = lex(FileId(0), src);
    assert!(
        out.diagnostics.is_empty(),
        "unexpected diagnostics for {src:?}: {:?}",
        out.diagnostics
    );
    let mut kinds: Vec<TokenKind> = out.tokens.into_iter().map(|t| t.kind).collect();
    assert_eq!(kinds.pop(), Some(TokenKind::Eof));
    kinds
}

use TokenKind as K;

#[test]
fn keywords_and_identifiers() {
    assert_eq!(
        kinds("let mut while break continue"),
        [
            K::Kw(Keyword::Let),
            K::Kw(Keyword::Mut),
            K::Kw(Keyword::While),
            K::Kw(Keyword::Break),
            K::Kw(Keyword::Continue),
        ]
    );
    // Keyword spelling is exact: prefixes/extensions are identifiers.
    assert_eq!(kinds("letx iffy in1"), [K::Ident, K::Ident, K::Ident]);
    // ADR-071: module words are ordinary identifiers.
    assert_eq!(kinds("import export use"), [K::Ident, K::Ident, K::Ident]);
}

#[test]
fn unicode_identifiers() {
    assert_eq!(kinds("한글변수"), [K::Ident]);
    assert_eq!(kinds("переменная3"), [K::Ident]);
    assert_eq!(kinds("😀"), [K::Ident]);
    assert_eq!(kinds("x😀2"), [K::Ident]);
    // Mixed-script statement.
    assert_eq!(
        kinds("let 사용자 = пользователь"),
        [K::Kw(Keyword::Let), K::Ident, K::Eq, K::Ident]
    );
}

#[test]
fn underscore_is_reserved_but_prefixed_names_are_identifiers() {
    assert_eq!(kinds("_"), [K::Underscore]);
    assert_eq!(kinds("_internal _1"), [K::Ident, K::Ident]);
}

#[test]
fn numbers_and_ranges() {
    assert_eq!(kinds("42"), [K::Int]);
    assert_eq!(kinds("3.14"), [K::Float]);
    // Longest match must not steal the range dots (SPEC §1).
    assert_eq!(kinds("1..5"), [K::Int, K::DotDot, K::Int]);
    assert_eq!(kinds("1..<5"), [K::Int, K::DotDotLt, K::Int]);
    // A trailing dot is member access, not a float.
    assert_eq!(kinds("3."), [K::Int, K::Dot]);
}

#[test]
fn duration_adjacency() {
    assert_eq!(kinds("3s"), [K::Duration(DurationUnit::S)]);
    assert_eq!(kinds("250ms"), [K::Duration(DurationUnit::Ms)]);
    assert_eq!(kinds("1m"), [K::Duration(DurationUnit::M)]);
    // With whitespace they are separate tokens (`3 s` is a parse
    // error later, not a lex error).
    assert_eq!(kinds("3 s"), [K::Int, K::Ident]);
    // The unit must be the whole adjacent run.
    assert_eq!(kinds("3sec"), [K::Int, K::Ident]);
    assert_eq!(kinds("3m2"), [K::Int, K::Ident]);
    // Durations are integer-only (SPEC §15).
    assert_eq!(kinds("3.5s"), [K::Float, K::Ident]);
}

#[test]
fn longest_match_operator_families() {
    assert_eq!(kinds("??="), [K::QuestionQuestionEq]);
    assert_eq!(kinds("a ?? b"), [K::Ident, K::QuestionQuestion, K::Ident]);
    assert_eq!(kinds("x?.y"), [K::Ident, K::QuestionDot, K::Ident]);
    assert_eq!(kinds("x?"), [K::Ident, K::Question]);
    assert_eq!(kinds("..."), [K::Ellipsis]);
    assert_eq!(kinds("|> || |"), [K::PipeGt, K::OrOr, K::Pipe]);
    assert_eq!(kinds(">> >= >"), [K::GtGt, K::Ge, K::Gt]);
    assert_eq!(kinds("== => ="), [K::EqEq, K::FatArrow, K::Eq]);
    assert_eq!(kinds("-> -= -"), [K::ThinArrow, K::MinusEq, K::Minus]);
    assert_eq!(kinds("** *= *"), [K::StarStar, K::StarEq, K::Star]);
    assert_eq!(kinds("+= /= %="), [K::PlusEq, K::SlashEq, K::PercentEq]);
    assert_eq!(kinds("!= !"), [K::Ne, K::Bang]);
    assert_eq!(kinds("&&"), [K::AndAnd]);
    assert_eq!(kinds("<= < ~"), [K::Le, K::Lt, K::Tilde]);
    // `**=` is not a token (SPEC §2 constraints): it lexes apart.
    assert_eq!(kinds("**="), [K::StarStar, K::Eq]);
}

#[test]
fn grouping_and_punctuation() {
    assert_eq!(
        kinds("( ) [ ] { } , : ;"),
        [
            K::LParen,
            K::RParen,
            K::LBracket,
            K::RBracket,
            K::LBrace,
            K::RBrace,
            K::Comma,
            K::Colon,
            K::Semicolon,
        ]
    );
}

#[test]
fn newlines_are_tokens_for_the_layout_pass() {
    assert_eq!(kinds("a\nb"), [K::Ident, K::Newline, K::Ident]);
    assert_eq!(kinds("a\r\nb"), [K::Ident, K::Newline, K::Ident]);
    assert_eq!(
        kinds("a\n\nb"),
        [K::Ident, K::Newline, K::Newline, K::Ident]
    );
}

#[test]
fn comments() {
    assert_eq!(kinds("x // trailing\ny"), [K::Ident, K::Newline, K::Ident]);
    // Newlines inside a block comment are comment content, not layout.
    assert_eq!(kinds("a /* b\nc */ d"), [K::Ident, K::Ident]);
    assert_eq!(
        kinds("a /* no nesting /* still one */ b"),
        [K::Ident, K::Ident]
    );
}

#[test]
fn unterminated_block_comment_diagnoses_and_continues() {
    let out = lex(FileId(0), "x /* open");
    assert_eq!(out.diagnostics.len(), 1);
    // The diagnostic code is stable and pinned by the negative corpus.
    assert_eq!(out.diagnostics[0].code.as_str(), "TPZ0002");
    let kinds: Vec<_> = out.tokens.iter().map(|t| t.kind).collect();
    assert_eq!(kinds, [K::Ident, K::Eof]);
}

#[test]
fn zwj_breaks_emoji_identifiers() {
    // U+200D is not an identifier character: an emoji ZWJ sequence is
    // not a single identifier atom. The joiner is diagnosed as an
    // unknown character and lexing continues.
    let out = lex(FileId(0), "\u{1F44D}\u{200D}\u{1F44D}");
    assert_eq!(out.diagnostics.len(), 1);
    assert_eq!(out.diagnostics[0].code.as_str(), "TPZ0001");
    let kinds: Vec<_> = out.tokens.iter().map(|t| t.kind).collect();
    assert_eq!(kinds, [K::Ident, K::Ident, K::Eof]);
}

#[test]
fn unknown_character_diagnoses_and_continues() {
    let out = lex(FileId(0), "a @ b");
    assert_eq!(out.diagnostics.len(), 1);
    assert_eq!(out.diagnostics[0].code.as_str(), "TPZ0001");
    let kinds: Vec<_> = out.tokens.iter().map(|t| t.kind).collect();
    assert_eq!(kinds, [K::Ident, K::Ident, K::Eof]);
}

#[test]
fn spans_cover_exact_source_bytes() {
    let src = "let 한글 = 12";
    let out = lex(FileId(0), src);
    let texts: Vec<&str> = out
        .tokens
        .iter()
        .filter(|t| t.kind != K::Eof)
        .map(|t| &src[t.span.lo as usize..t.span.hi as usize])
        .collect();
    assert_eq!(texts, ["let", "한글", "=", "12"]);
}

#[test]
fn statement_shaped_smoke() {
    assert_eq!(
        kinds("let total = reduce(xs, 0, (acc, x) => acc + x)"),
        [
            K::Kw(Keyword::Let),
            K::Ident,
            K::Eq,
            K::Ident,
            K::LParen,
            K::Ident,
            K::Comma,
            K::Int,
            K::Comma,
            K::LParen,
            K::Ident,
            K::Comma,
            K::Ident,
            K::RParen,
            K::FatArrow,
            K::Ident,
            K::Plus,
            K::Ident,
            K::RParen,
        ]
    );
    assert_eq!(
        kinds("concurrent(timeout: 3s)"),
        [
            K::Kw(Keyword::Concurrent),
            K::LParen,
            K::Ident,
            K::Colon,
            K::Duration(DurationUnit::S),
            K::RParen,
        ]
    );
}
