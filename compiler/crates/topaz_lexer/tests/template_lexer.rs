//! Template-lexer integration tests: strings and tagged templates as
//! token trees (CDR-001 §4).

use topaz_diag::FileId;
use topaz_lexer::lex;
use topaz_syntax::{Keyword, TokenKind};

use TokenKind as K;

const PLAIN: K = K::StringStart {
    tagged: false,
    multiline: false,
};
const TAGGED: K = K::StringStart {
    tagged: true,
    multiline: false,
};
const PLAIN_ML: K = K::StringStart {
    tagged: false,
    multiline: true,
};
const TAGGED_ML: K = K::StringStart {
    tagged: true,
    multiline: true,
};

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

/// Kinds without the trailing Eof, plus diagnostic code strings.
fn kinds_and_codes(src: &str) -> (Vec<TokenKind>, Vec<&'static str>) {
    let out = lex(FileId(0), src);
    let mut kinds: Vec<TokenKind> = out.tokens.into_iter().map(|t| t.kind).collect();
    assert_eq!(kinds.pop(), Some(TokenKind::Eof));
    let codes = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    (kinds, codes)
}

#[test]
fn plain_string_is_a_token_tree() {
    assert_eq!(kinds(r#""hello""#), [PLAIN, K::StringText, K::StringEnd]);
    assert_eq!(kinds(r#""""#), [PLAIN, K::StringEnd]);
}

#[test]
fn escapes_stay_inside_one_text_run() {
    assert_eq!(
        kinds(r#""a\nb\tc\rd\\e\"f\{g\}h""#),
        [PLAIN, K::StringText, K::StringEnd]
    );
}

#[test]
fn invalid_escape_diagnoses_and_continues() {
    let (kinds, codes) = kinds_and_codes(r#""a\qb""#);
    assert_eq!(kinds, [PLAIN, K::StringText, K::StringEnd]);
    assert_eq!(codes, ["TPZ0004"]);
}

#[test]
fn interpolation_lexes_ordinary_tokens() {
    assert_eq!(
        kinds(r#""hi {name}!""#),
        [
            PLAIN,
            K::StringText,
            K::InterpolationStart,
            K::Ident,
            K::InterpolationEnd,
            K::StringText,
            K::StringEnd,
        ]
    );
}

#[test]
fn interpolation_tracks_nested_braces() {
    // A record literal's braces belong to the expression; only the
    // depth-zero `}` closes the interpolation.
    assert_eq!(
        kinds(r#""p: {Point { x: 1 }}""#),
        [
            PLAIN,
            K::StringText,
            K::InterpolationStart,
            K::Ident,
            K::LBrace,
            K::Ident,
            K::Colon,
            K::Int,
            K::RBrace,
            K::InterpolationEnd,
            K::StringEnd,
        ]
    );
}

#[test]
fn nested_string_inside_interpolation() {
    assert_eq!(
        kinds(r#""a {b ?? "z"} c""#),
        [
            PLAIN,
            K::StringText,
            K::InterpolationStart,
            K::Ident,
            K::QuestionQuestion,
            PLAIN,
            K::StringText,
            K::StringEnd,
            K::InterpolationEnd,
            K::StringText,
            K::StringEnd,
        ]
    );
}

#[test]
fn tagged_template_adjacency() {
    assert_eq!(
        kinds(r#"sql"SELECT""#),
        [TAGGED, K::StringText, K::StringEnd]
    );
    // Whitespace breaks adjacency: separate tokens, parser rejects.
    assert_eq!(
        kinds(r#"p "x""#),
        [K::Ident, PLAIN, K::StringText, K::StringEnd]
    );
    // Adjacency is lexical for any identifier; the registry check is
    // parser-side (SPEC §16).
    assert_eq!(kinds(r#"foo"x""#), [TAGGED, K::StringText, K::StringEnd]);
    // Keywords and `_` are not identifier-like tags.
    assert_eq!(
        kinds(r#"else"x""#),
        [K::Kw(Keyword::Else), PLAIN, K::StringText, K::StringEnd]
    );
    assert_eq!(
        kinds(r#"_"x""#),
        [K::Underscore, PLAIN, K::StringText, K::StringEnd]
    );
}

#[test]
fn multiline_strings_and_inner_quotes() {
    assert_eq!(
        kinds("\"\"\"\nhello\n\"\"\""),
        [PLAIN_ML, K::StringText, K::StringEnd]
    );
    // Lone `"` and `""` are raw text in the multiline form.
    assert_eq!(
        kinds(r#""""a "q" b""""#),
        [PLAIN_ML, K::StringText, K::StringEnd]
    );
    // Bare `}` is raw text in the multiline form (SPEC §1).
    assert_eq!(
        kinds(r#""""a}b""""#),
        [PLAIN_ML, K::StringText, K::StringEnd]
    );
}

#[test]
fn multiline_tagged_template() {
    assert_eq!(
        kinds("sql\"\"\"\n    SELECT name\n    \"\"\""),
        [TAGGED_ML, K::StringText, K::StringEnd]
    );
}

#[test]
fn multiline_indent_matches_closing_delimiter() {
    // All content lines carry the 4-space closing indent: no
    // diagnostics. Blank lines are exempt.
    kinds("let s = \"\"\"\n    line1\n\n    line2\n    \"\"\"");
}

#[test]
fn multiline_indent_violation_points_at_the_line() {
    let src = "let s = \"\"\"\n    line1\n  line2\n    \"\"\"";
    let out = lex(FileId(0), src);
    assert_eq!(out.diagnostics.len(), 1);
    assert_eq!(out.diagnostics[0].code.as_str(), "TPZ0006");
    let span = out.diagnostics[0].primary.span;
    assert_eq!(&src[span.lo as usize..span.hi as usize], "  line2");
}

#[test]
fn same_line_first_content_is_indent_checked() {
    // SPEC §1: with no line terminator after the opening delimiter,
    // content begins immediately — that first content line must also
    // carry the closing indent.
    let src = "let s = \"\"\"x\n    \"\"\"";
    let out = lex(FileId(0), src);
    assert_eq!(out.diagnostics.len(), 1);
    assert_eq!(out.diagnostics[0].code.as_str(), "TPZ0006");
    let span = out.diagnostics[0].primary.span;
    assert_eq!(&src[span.lo as usize..span.hi as usize], "x");

    // Same-line content that does carry the closing indent passes.
    kinds("let s = \"\"\"    x\n    y\n    \"\"\"");
}

#[test]
fn multiline_indent_is_an_exact_whitespace_match() {
    // Tabs are literal: a tab does not satisfy a space indent.
    let (_, codes) = kinds_and_codes("\"\"\"\n\tline\n    \"\"\"");
    assert_eq!(codes, ["TPZ0006"]);
}

#[test]
fn closing_indent_is_empty_when_text_precedes_the_delimiter() {
    // Non-whitespace before the closing delimiter: no stripping and
    // no indent check (SPEC §1).
    assert_eq!(
        kinds("\"\"\"x\n  y\nz\"\"\""),
        [PLAIN_ML, K::StringText, K::StringEnd]
    );
}

#[test]
fn unterminated_single_line_at_line_break() {
    let (kinds, codes) = kinds_and_codes("\"abc\ndef");
    assert_eq!(
        kinds,
        [PLAIN, K::StringText, K::StringEnd, K::Newline, K::Ident]
    );
    assert_eq!(codes, ["TPZ0003"]);
}

#[test]
fn unterminated_at_end_of_input() {
    let (kinds, codes) = kinds_and_codes("\"abc");
    assert_eq!(kinds, [PLAIN, K::StringText, K::StringEnd]);
    assert_eq!(codes, ["TPZ0003"]);

    let (kinds, codes) = kinds_and_codes("\"\"\"abc");
    assert_eq!(kinds, [PLAIN_ML, K::StringText, K::StringEnd]);
    assert_eq!(codes, ["TPZ0003"]);
}

#[test]
fn line_break_inside_single_line_interpolation() {
    // SPEC §1: a single-line literal contains no unescaped newlines,
    // its interpolations included. The tree stays balanced and the
    // line break lexes as layout.
    let (kinds, codes) = kinds_and_codes("\"a{x\ny");
    assert_eq!(
        kinds,
        [
            PLAIN,
            K::StringText,
            K::InterpolationStart,
            K::Ident,
            K::InterpolationEnd,
            K::StringEnd,
            K::Newline,
            K::Ident,
        ]
    );
    assert_eq!(codes, ["TPZ0003"]);
}

#[test]
fn multiline_literal_cannot_hide_a_single_line_interpolation_break() {
    let src = concat!("\"outer {\"\"\"inner", "\n");
    let out = lex(FileId(0), src);
    let kinds: Vec<TokenKind> = out.tokens.into_iter().map(|token| token.kind).collect();
    assert_eq!(
        kinds,
        [
            PLAIN,
            K::StringText,
            K::InterpolationStart,
            PLAIN_ML,
            K::StringText,
            K::StringEnd,
            K::InterpolationEnd,
            K::StringEnd,
            K::Newline,
            K::Eof,
        ]
    );
    assert_eq!(
        out.diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        ["TPZ0003"]
    );
    assert_eq!(out.diagnostics[0].primary.span.lo, 0);
    assert_eq!(out.diagnostics[0].primary.span.hi, 1);
}

#[test]
fn end_of_input_inside_interpolation_balances_the_tree() {
    let (kinds, codes) = kinds_and_codes("\"a{x");
    assert_eq!(
        kinds,
        [
            PLAIN,
            K::StringText,
            K::InterpolationStart,
            K::Ident,
            K::InterpolationEnd,
            K::StringEnd,
        ]
    );
    assert_eq!(codes, ["TPZ0003"]);
}

#[test]
fn stray_brace_in_single_line_text() {
    let (kinds, codes) = kinds_and_codes(r#""a}b""#);
    assert_eq!(kinds, [PLAIN, K::StringText, K::StringEnd]);
    assert_eq!(codes, ["TPZ0005"]);
}

#[test]
fn newline_tokens_survive_inside_multiline_interpolation() {
    // The layout normalizer treats interpolations as continuation
    // mode (SPEC §1a); the raw material is the Newline token.
    assert_eq!(
        kinds("\"\"\"{x\n|> f}\"\"\""),
        [
            PLAIN_ML,
            K::InterpolationStart,
            K::Ident,
            K::Newline,
            K::PipeGt,
            K::Ident,
            K::InterpolationEnd,
            K::StringEnd,
        ]
    );
}

#[test]
fn lines_starting_inside_interpolation_are_not_indent_checked() {
    // The `} b` line begins inside the interpolation (expression
    // code), so the indent rule does not apply to it.
    assert_eq!(
        kinds("\"\"\"\n    a {x\n} b\n    \"\"\""),
        [
            PLAIN_ML,
            K::StringText,
            K::InterpolationStart,
            K::Ident,
            K::Newline,
            K::InterpolationEnd,
            K::StringText,
            K::StringEnd,
        ]
    );
}

#[test]
fn spans_reassemble_the_source() {
    let src = r#"sql"x {y}""#;
    let out = lex(FileId(0), src);
    assert!(out.diagnostics.is_empty());
    let texts: Vec<&str> = out
        .tokens
        .iter()
        .filter(|t| t.kind != K::Eof)
        .map(|t| &src[t.span.lo as usize..t.span.hi as usize])
        .collect();
    assert_eq!(texts, ["sql\"", "x ", "{", "y", "}", "\""]);
    assert_eq!(texts.concat(), src);
}
