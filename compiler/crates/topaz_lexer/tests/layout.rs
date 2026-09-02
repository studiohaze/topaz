//! Layout-normalizer integration tests (SPEC §1a, CDR-001 §4/§7):
//! newline significance, the Sep token, and brace classification.

use topaz_diag::FileId;
use topaz_lexer::{lex, normalize};
use topaz_syntax::{DurationUnit, Keyword, TokenKind};

use Keyword as Kw;
use TokenKind as K;

/// Lexes, normalizes, and returns kinds without the trailing Eof;
/// asserts that neither pass diagnosed anything.
fn kinds(src: &str) -> Vec<TokenKind> {
    let lexed = lex(FileId(0), src);
    assert!(
        lexed.diagnostics.is_empty(),
        "unexpected lex diagnostics for {src:?}: {:?}",
        lexed.diagnostics
    );
    let out = normalize(&lexed.tokens);
    assert!(
        out.diagnostics.is_empty(),
        "unexpected layout diagnostics for {src:?}: {:?}",
        out.diagnostics
    );
    let mut kinds: Vec<TokenKind> = out.tokens.into_iter().map(|t| t.kind).collect();
    assert_eq!(kinds.pop(), Some(TokenKind::Eof));
    kinds
}

fn sep_count(src: &str) -> usize {
    kinds(src).iter().filter(|&&k| k == K::Sep).count()
}

#[test]
fn top_level_items_separate_at_newlines() {
    assert_eq!(
        kinds("let a = 1\nlet b = 2"),
        [
            K::Kw(Kw::Let),
            K::Ident,
            K::Eq,
            K::Int,
            K::Sep,
            K::Kw(Kw::Let),
            K::Ident,
            K::Eq,
            K::Int,
        ]
    );
    // Leading and trailing newlines are empty separators.
    assert_eq!(kinds("\nlet a = 1\n"), kinds("let a = 1"));
}

#[test]
fn blank_lines_collapse_into_one_sep() {
    assert_eq!(kinds("a\n\n\nb"), [K::Ident, K::Sep, K::Ident]);
}

#[test]
fn semicolon_is_an_explicit_sep() {
    assert_eq!(kinds("a; b"), [K::Ident, K::Sep, K::Ident]);
    // A newline after `;` is an empty separator.
    assert_eq!(kinds("a;\nb"), [K::Ident, K::Sep, K::Ident]);
}

#[test]
fn trailing_continuation_tokens_absorb_the_newline() {
    assert_eq!(
        kinds("let x =\n5"),
        [K::Kw(Kw::Let), K::Ident, K::Eq, K::Int]
    );
    assert_eq!(kinds("a &&\nb"), [K::Ident, K::AndAnd, K::Ident]);
    assert_eq!(kinds("x +\n1"), [K::Ident, K::Plus, K::Int]);
}

#[test]
fn leading_continuation_tokens_absorb_the_newline() {
    assert_eq!(
        kinds("value\n  |> f\n  |> g"),
        [K::Ident, K::PipeGt, K::Ident, K::PipeGt, K::Ident]
    );
    assert_eq!(kinds("x\n  .field"), [K::Ident, K::Dot, K::Ident]);
    assert_eq!(kinds("x\n  ?.field"), [K::Ident, K::QuestionDot, K::Ident]);
    // `+` is trailing-only: a leading `+` starts a new item.
    assert_eq!(kinds("a\n+ b"), [K::Ident, K::Sep, K::Plus, K::Ident]);
}

#[test]
fn else_continues_the_if_after_a_block() {
    assert_eq!(
        kinds("if a { 1 }\nelse { 2 }"),
        [
            K::Kw(Kw::If),
            K::Ident,
            K::LBrace,
            K::Int,
            K::RBrace,
            K::Kw(Kw::Else),
            K::LBrace,
            K::Int,
            K::RBrace,
        ]
    );
}

#[test]
fn match_cases_separate_as_items() {
    assert_eq!(
        kinds("match x {\n    case 1 => a\n    case _ => b\n}"),
        [
            K::Kw(Kw::Match),
            K::Ident,
            K::LBrace,
            K::Kw(Kw::Case),
            K::Int,
            K::FatArrow,
            K::Ident,
            K::Sep,
            K::Kw(Kw::Case),
            K::Underscore,
            K::FatArrow,
            K::Ident,
            K::RBrace,
        ]
    );
}

#[test]
fn concurrent_arms_are_items_not_record_fields() {
    // The body brace is construct-determined: without it the
    // `Identifier ":"` arms would read as a record literal.
    assert_eq!(
        kinds("concurrent {\n    a: f(x)\n    b: g(y)\n}"),
        [
            K::Kw(Kw::Concurrent),
            K::LBrace,
            K::Ident,
            K::Colon,
            K::Ident,
            K::LParen,
            K::Ident,
            K::RParen,
            K::Sep,
            K::Ident,
            K::Colon,
            K::Ident,
            K::LParen,
            K::Ident,
            K::RParen,
            K::RBrace,
        ]
    );
}

#[test]
fn concurrent_timeout_form_with_else() {
    assert_eq!(
        kinds("concurrent(timeout: 3s) {\n    a: f()\n}\nelse { 0 }"),
        [
            K::Kw(Kw::Concurrent),
            K::LParen,
            K::Ident,
            K::Colon,
            K::Duration(DurationUnit::S),
            K::RParen,
            K::LBrace,
            K::Ident,
            K::Colon,
            K::Ident,
            K::LParen,
            K::RParen,
            K::RBrace,
            K::Kw(Kw::Else),
            K::LBrace,
            K::Int,
            K::RBrace,
        ]
    );
}

#[test]
fn record_literal_braces_are_continuation() {
    assert_eq!(
        kinds("let p = {\n    x: 1,\n    y: 2\n}"),
        [
            K::Kw(Kw::Let),
            K::Ident,
            K::Eq,
            K::LBrace,
            K::Ident,
            K::Colon,
            K::Int,
            K::Comma,
            K::Ident,
            K::Colon,
            K::Int,
            K::RBrace,
        ]
    );
}

#[test]
fn block_braces_are_separator() {
    assert_eq!(
        kinds("{\n    f()\n    g()\n}"),
        [
            K::LBrace,
            K::Ident,
            K::LParen,
            K::RParen,
            K::Sep,
            K::Ident,
            K::LParen,
            K::RParen,
            K::RBrace,
        ]
    );
    // `{}` is an empty block, not an empty record.
    assert_eq!(
        kinds("let u = {}"),
        [K::Kw(Kw::Let), K::Ident, K::Eq, K::LBrace, K::RBrace]
    );
}

#[test]
fn record_update_vs_following_block() {
    // Same item, no separator: a record update.
    assert_eq!(
        kinds("let q = p { x: 3 }"),
        [
            K::Kw(Kw::Let),
            K::Ident,
            K::Eq,
            K::Ident,
            K::LBrace,
            K::Ident,
            K::Colon,
            K::Int,
            K::RBrace,
        ]
    );
    // Across a significant newline: a new item that opens a block.
    assert_eq!(
        kinds("q\n{ f() }"),
        [
            K::Ident,
            K::Sep,
            K::LBrace,
            K::Ident,
            K::LParen,
            K::RParen,
            K::RBrace,
        ]
    );
}

#[test]
fn record_pattern_braces_are_continuation() {
    // The colon-less shorthand has no `Identifier ":"` to look at:
    // pattern position decides.
    assert_eq!(
        kinds("let { x, y } = p"),
        [
            K::Kw(Kw::Let),
            K::LBrace,
            K::Ident,
            K::Comma,
            K::Ident,
            K::RBrace,
            K::Eq,
            K::Ident,
        ]
    );
    // Multiline record pattern in a case, with a nested subpattern.
    assert_eq!(
        sep_count("match p {\n    case {\n        x,\n        y\n    } => x\n}"),
        0
    );
    assert_eq!(sep_count("match p {\n    case Some({ x }) => x\n}"), 0);
}

#[test]
fn for_pattern_then_separator_body() {
    assert_eq!(
        kinds("for { a, b } in pairs {\n    f(a)\n    g(b)\n}"),
        [
            K::Kw(Kw::For),
            K::LBrace,
            K::Ident,
            K::Comma,
            K::Ident,
            K::RBrace,
            K::Kw(Kw::In),
            K::Ident,
            K::LBrace,
            K::Ident,
            K::LParen,
            K::Ident,
            K::RParen,
            K::Sep,
            K::Ident,
            K::LParen,
            K::Ident,
            K::RParen,
            K::RBrace,
        ]
    );
}

#[test]
fn record_type_braces_are_continuation() {
    assert_eq!(sep_count("type P = {\n    x: int,\n    y: int\n}"), 0);
    // A record return type, then a body block, then a record literal.
    assert_eq!(
        kinds("function f() -> { x: int } {\n    return { x: 1 }\n}"),
        [
            K::Kw(Kw::Function),
            K::Ident,
            K::LParen,
            K::RParen,
            K::ThinArrow,
            K::LBrace,
            K::Ident,
            K::Colon,
            K::Ident,
            K::RBrace,
            K::LBrace,
            K::Kw(Kw::Return),
            K::LBrace,
            K::Ident,
            K::Colon,
            K::Int,
            K::RBrace,
            K::RBrace,
        ]
    );
}

#[test]
fn lambda_bodies_classify_by_lookahead() {
    assert_eq!(sep_count("let f = (x) => {\n    g(x)\n    h(x)\n}"), 1);
    assert_eq!(sep_count("let f = (x) => { y: x }"), 0);
}

#[test]
fn defer_block_separates_statements() {
    assert_eq!(sep_count("defer {\n    a()\n    b()\n}"), 1);
}

#[test]
fn continuation_frames_drop_newlines() {
    assert_eq!(
        kinds("f(\n    a,\n    b\n)"),
        [K::Ident, K::LParen, K::Ident, K::Comma, K::Ident, K::RParen]
    );
    assert_eq!(
        kinds("[\n    1,\n    2\n]"),
        [K::LBracket, K::Int, K::Comma, K::Int, K::RBracket]
    );
}

#[test]
fn template_interpolation_is_continuation() {
    assert_eq!(
        kinds("\"\"\"{x\n|> f}\"\"\""),
        [
            K::StringStart {
                tagged: false,
                multiline: true
            },
            K::InterpolationStart,
            K::Ident,
            K::PipeGt,
            K::Ident,
            K::InterpolationEnd,
            K::StringEnd,
        ]
    );
    // A block nested inside an interpolation still separates.
    assert_eq!(sep_count("\"\"\"{ {\na()\nb()\n} }\"\"\""), 1);
}

#[test]
fn semicolon_in_a_delimiter_list_diagnoses() {
    let lexed = lex(FileId(0), "f(a; b)");
    assert!(lexed.diagnostics.is_empty());
    let out = normalize(&lexed.tokens);
    assert_eq!(out.diagnostics.len(), 1);
    assert_eq!(out.diagnostics[0].code.as_str(), "TPZ1001");
    let kinds: Vec<_> = out.tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        [K::Ident, K::LParen, K::Ident, K::Ident, K::RParen, K::Eof]
    );
}

#[test]
fn sep_spans_point_at_their_source() {
    let lexed = lex(FileId(0), "a\nb");
    let out = normalize(&lexed.tokens);
    let sep = out.tokens.iter().find(|t| t.kind == K::Sep).expect("sep");
    assert_eq!((sep.span.lo, sep.span.hi), (1, 2)); // the newline

    let lexed = lex(FileId(0), "a; b");
    let out = normalize(&lexed.tokens);
    let sep = out.tokens.iter().find(|t| t.kind == K::Sep).expect("sep");
    assert_eq!((sep.span.lo, sep.span.hi), (1, 2)); // the semicolon
}

#[test]
fn no_newline_tokens_survive_normalization() {
    for src in [
        "a\nb",
        "f(\na\n)",
        "let p = {\n x: 1\n}",
        "\"\"\"{x\n|> f}\"\"\"",
        "match x {\n case _ => 1\n}",
    ] {
        let out = normalize(&lex(FileId(0), src).tokens);
        assert!(
            out.tokens.iter().all(|t| t.kind != K::Newline),
            "newline leaked for {src:?}"
        );
    }
}
