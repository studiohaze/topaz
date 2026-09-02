//! Parser integration tests: grammar coverage plus the CDR-001 §7
//! AST-shape smoke suite (ordinary assertions, not golden snapshots).

use topaz_diag::FileId;
use topaz_parser::{ParseOptions, parse, parse_with_options};
use topaz_syntax::LangVersion;
use topaz_syntax::ast::*;

/// Parses expecting zero diagnostics from the whole front end.
fn parse_ok(src: &str) -> Program {
    let out = parse(FileId(0), src);
    assert!(
        out.diagnostics.is_empty(),
        "unexpected diagnostics for {src:?}: {:?}",
        out.diagnostics
    );
    out.program
}

/// Diagnostic code strings for a source, in order.
fn codes(src: &str) -> Vec<String> {
    parse(FileId(0), src)
        .diagnostics
        .iter()
        .map(|d| d.code.as_str().to_owned())
        .collect()
}

/// Parses a single expression statement.
fn one_expr(src: &str) -> Expr {
    let mut program = parse_ok(src);
    assert_eq!(program.items.len(), 1, "expected one item in {src:?}");
    match program.items.pop().unwrap().kind {
        StmtKind::Expr(e) => e,
        other => panic!("expected an expression statement, got {other:?}"),
    }
}

/// Parses `let x = <src-value>` and returns the bound value.
fn let_value(src: &str) -> Expr {
    let mut program = parse_ok(src);
    assert_eq!(program.items.len(), 1);
    match program.items.pop().unwrap().kind {
        StmtKind::Let { value, .. } => value,
        other => panic!("expected a let binding, got {other:?}"),
    }
}

/// Parses at v5.2 expecting zero diagnostics.
fn parse_ok_v52(src: &str) -> Program {
    let out = parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: LangVersion::V5_2,
        },
    );
    assert!(
        out.diagnostics.is_empty(),
        "unexpected diagnostics for {src:?}: {:?}",
        out.diagnostics
    );
    out.program
}

/// Diagnostic codes at v5.2 for a source.
fn codes_v52(src: &str) -> Vec<String> {
    parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: LangVersion::V5_2,
        },
    )
    .diagnostics
    .iter()
    .map(|d| d.code.as_str().to_owned())
    .collect()
}

/// Parses at v5.3 expecting zero diagnostics.
fn parse_ok_v53(src: &str) -> Program {
    let out = parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: LangVersion::V5_3,
        },
    );
    assert!(
        out.diagnostics.is_empty(),
        "unexpected diagnostics for {src:?}: {:?}",
        out.diagnostics
    );
    out.program
}

fn parse_ok_v54(src: &str) -> Program {
    let out = parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: LangVersion::V5_4,
        },
    );
    assert!(
        out.diagnostics.is_empty(),
        "unexpected diagnostics for {src:?}: {:?}",
        out.diagnostics
    );
    out.program
}

fn one_expr_v54(src: &str) -> Expr {
    let mut program = parse_ok_v54(src);
    assert_eq!(program.items.len(), 1, "expected one item in {src:?}");
    match program.items.pop().unwrap().kind {
        StmtKind::Expr(e) => e,
        other => panic!("expected an expression statement, got {other:?}"),
    }
}

#[path = "parser/diagnostics.rs"]
mod diagnostics;
#[path = "parser/modules_and_patterns.rs"]
mod modules_and_patterns;
#[path = "parser/syntax.rs"]
mod syntax;
#[path = "parser/v54_calls_and_control.rs"]
mod v54_calls_and_control;
#[path = "parser/v54_collections.rs"]
mod v54_collections;
