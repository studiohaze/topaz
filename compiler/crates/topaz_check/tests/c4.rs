//! Phase C-4 witnesses: match typing, pattern bindings against the
//! scrutinee, exhaustiveness (TPZ5021) over the decidable domains,
//! type patterns, and the §12/§13 optional operators (`?`, `??`,
//! `?.`, `??=`).

use topaz_check::check_program_with_version;
use topaz_diag::FileId;
use topaz_parser::{ParseOptions, parse_with_options};
use topaz_syntax::LangVersion;

fn check(src: &str) -> Vec<String> {
    check_at(src, LangVersion::V5_4)
}

/// Parse + check at a specific language version (for version-gate tests).
fn check_at(src: &str, version: LangVersion) -> Vec<String> {
    let out = parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: version,
        },
    );
    assert!(out.diagnostics.is_empty(), "test source must parse: {src}");
    check_program_with_version(src, &out.program, version)
        .diagnostics
        .iter()
        .map(|d| format!("{} {}", d.code.as_str(), d.message))
        .collect()
}

fn assert_clean(src: &str) {
    let diags = check(src);
    assert!(diags.is_empty(), "expected clean, got: {diags:?}");
}

fn assert_code(src: &str, code: &str) {
    let diags = check(src);
    assert!(
        diags.iter().any(|d| d.starts_with(code)),
        "expected {code}, got: {diags:?}"
    );
}

fn assert_code_and_message(src: &str, code: &str, message: &str) {
    let diags = check(src);
    assert!(
        diags
            .iter()
            .any(|d| d.starts_with(code) && d.contains(message)),
        "expected {code} containing {message:?}, got: {diags:?}"
    );
}

#[path = "c4/nominals_and_match.rs"]
mod nominals_and_match;
#[path = "c4/optional_and_patterns.rs"]
mod optional_and_patterns;
#[path = "c4/projections_and_protocols.rs"]
mod projections_and_protocols;
