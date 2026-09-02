//! Golden parse-corpus harness (CDR-001 §7/§8).
//!
//! Selection is manifest-driven: every row the extractor classifies
//! with `expect = "parse_ok"` must parse with zero diagnostics and
//! the full file consumed. Skipped rows (signature notation,
//! historical, excluded) are never globbed in.

use std::fs;

use corpus_extract::{V51_INVALID_EXPECTED, V51_LAYOUT_EXPECTED, V51_PARSE_OK_EXPECTED};
use topaz_diag::FileId;
use topaz_parser::{ParseOptions, parse, parse_with_options};
use topaz_syntax::LangVersion;

#[test]
fn every_parse_ok_corpus_row_parses_clean() {
    let root = corpus_extract::repo_root();
    let generated = corpus_extract::generate(&root).expect("corpus generation");

    let mut checked = 0usize;
    let mut failures = Vec::new();
    for row in &generated.rows {
        if row.expect != "parse_ok" {
            continue;
        }
        let file = row.file.as_deref().expect("parse_ok rows carry files");
        let src = fs::read_to_string(root.join(file)).expect("committed corpus file");
        // The corpus contract is v5.2-current: the locked v5.2 surface
        // is a strict superset of frozen v5.1, and site sources contain
        // canonical module fences that only exist at v5.2.
        let out = parse_with_options(
            FileId(0),
            &src,
            ParseOptions {
                language_version: LangVersion::V5_2,
            },
        );
        checked += 1;
        if !out.diagnostics.is_empty() {
            let diags: Vec<String> = out
                .diagnostics
                .iter()
                .map(|d| format!("{} {}", d.code.as_str(), d.message))
                .collect();
            failures.push(format!(
                "{file}  (from {} #{})\n    {}",
                row.fence.source,
                row.fence.index,
                diags.join("\n    ")
            ));
        }
    }

    assert_eq!(checked, V51_PARSE_OK_EXPECTED, "positive corpus size moved");
    assert!(
        failures.is_empty(),
        "{} corpus files failed to parse:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// Hand-written layout-conformance fixtures (CDR-001 §7 fixture
/// classes); each must parse clean end to end.
#[test]
fn layout_conformance_fixtures_parse_clean() {
    let dir = corpus_extract::repo_root().join("corpus/v5.1/layout");
    let mut count = 0usize;
    for entry in fs::read_dir(&dir).expect("layout fixtures dir") {
        let path = entry.expect("entry").path();
        if path.extension().is_none_or(|e| e != "tpz") {
            continue;
        }
        let src = fs::read_to_string(&path).expect("fixture");
        let out = parse(FileId(0), &src);
        assert!(
            out.diagnostics.is_empty(),
            "{}: {:?}",
            path.display(),
            out.diagnostics
        );
        count += 1;
    }
    assert_eq!(count, V51_LAYOUT_EXPECTED, "layout fixture count moved");
}

/// Negative fixtures (CDR-001 §7/§8 item 6): the filename prefix is
/// the expected **primary** diagnostic code; additional recovery
/// diagnostics are allowed behind it.
#[test]
fn invalid_fixtures_report_their_primary_code() {
    let dir = corpus_extract::repo_root().join("corpus/v5.1/invalid");
    let mut count = 0usize;
    for entry in fs::read_dir(&dir).expect("invalid fixtures dir") {
        let path = entry.expect("entry").path();
        if path.extension().is_none_or(|e| e != "tpz") {
            continue;
        }
        let name = path
            .file_stem()
            .expect("stem")
            .to_string_lossy()
            .to_string();
        let expected = name.split('-').next().expect("code prefix").to_uppercase();
        let src = fs::read_to_string(&path).expect("fixture");
        let out = parse(FileId(0), &src);
        assert!(
            !out.diagnostics.is_empty(),
            "{}: expected a diagnostic, parsed clean",
            path.display()
        );
        assert_eq!(
            out.diagnostics[0].code.as_str(),
            expected,
            "{}: wrong primary code (got {:?})",
            path.display(),
            out.diagnostics
        );
        count += 1;
    }
    assert_eq!(count, V51_INVALID_EXPECTED, "invalid fixture count moved");
}

/// C1 superset evidence, seed form (CDR-002 §4): every positive v5.1
/// corpus row parses clean at `--language-version 5.2` and yields a
/// structurally identical AST to its 5.1 parse. The comparison is
/// structural equality of two live parses of the same source — not a
/// golden snapshot — so AST schema evolution cannot rot it.
#[test]
fn every_parse_ok_corpus_row_is_version_stable() {
    let root = corpus_extract::repo_root();
    let generated = corpus_extract::generate(&root).expect("corpus generation");

    let mut checked = 0usize;
    let mut v52_only = Vec::new();
    let mut failures = Vec::new();
    for row in &generated.rows {
        if row.expect != "parse_ok" {
            continue;
        }
        let file = row.file.as_deref().expect("parse_ok rows carry files");
        let src = fs::read_to_string(root.join(file)).expect("committed corpus file");
        let base = parse(FileId(0), &src);
        let v52 = parse_with_options(
            FileId(0),
            &src,
            ParseOptions {
                language_version: LangVersion::V5_2,
            },
        );
        checked += 1;
        if !v52.diagnostics.is_empty() {
            failures.push(format!("{file}: diagnostics at 5.2"));
            continue;
        }
        // v5.2-only rows (module fences) have no v5.1 parse to compare
        // against; their 5.2-clean parse is asserted above and in the
        // positive-corpus test. Superset stability is asserted for
        // every row the frozen v5.1 grammar accepts.
        if !base.diagnostics.is_empty() {
            v52_only.push(file.to_string());
            continue;
        }
        if base.program != v52.program {
            failures.push(format!("{file}: AST differs between 5.1 and 5.2"));
        }
    }

    assert_eq!(checked, V51_PARSE_OK_EXPECTED, "positive corpus size moved");
    // The v5.2-only set is pinned by PATH, not just count: a v5.1
    // regression elsewhere cannot hide behind a module fence that
    // happens to start parsing at 5.1.
    let mut expected_v52_only = Vec::new();
    for locale in ["en", "ko", "ru"] {
        for n in 2..=10 {
            expected_v52_only.push(format!(
                "corpus/v5.1/site/{locale}/concepts-modules/{n:02}.tpz"
            ));
        }
        for n in 10..=11 {
            expected_v52_only.push(format!(
                "corpus/v5.1/site/{locale}/getting-started/{n:02}.tpz"
            ));
        }
    }
    expected_v52_only.sort();
    v52_only.sort();
    assert_eq!(v52_only, expected_v52_only, "v5.2-only corpus rows moved");
    assert!(
        failures.is_empty(),
        "{} corpus files are not version-stable:
{}",
        failures.len(),
        failures.join(
            "
"
        )
    );
}
