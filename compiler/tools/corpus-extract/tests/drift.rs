//! Extractor drift check (CDR-001 §8 item 7) and locked fence
//! counts. `cargo test --workspace` runs this, so CI fails whenever
//! the committed corpus stops matching a fresh generation from the
//! vendored sources.

use corpus_extract::{Row, drift, generate, repo_root};

#[test]
fn committed_corpus_matches_a_fresh_generation() {
    let problems = drift(&repo_root()).expect("drift check runs");
    assert!(
        problems.is_empty(),
        "corpus drift:\n{}",
        problems.join("\n")
    );
}

#[test]
fn fence_counts_are_locked() {
    let generated = generate(&repo_root()).expect("generation runs");
    let rows = &generated.rows;

    let count = |pred: &dyn Fn(&&Row) -> bool| rows.iter().filter(pred).count();
    let topaz_in = |prefix: &str| {
        count(&|r: &&Row| r.fence.language == "topaz" && r.fence.source.starts_with(prefix))
    };

    // EXAMPLES.md: 52 topaz fences — 44 complete programs, 8
    // ambient-name snippets.
    assert_eq!(topaz_in("spec/v5.1/EXAMPLES.md"), 52);
    assert_eq!(count(&|r: &&Row| r.mode == "program"), 44);
    assert_eq!(
        count(&|r: &&Row| r.mode == "snippet" && r.fence.source.contains("EXAMPLES")),
        8
    );

    // SPEC.md: 4 topaz fences, all §22 signature notation.
    assert_eq!(topaz_in("spec/v5.1/SPEC.md"), 4);
    assert_eq!(count(&|r: &&Row| r.mode == "signature_notation"), 4);

    // Site snapshot: 186 topaz fences per locale, 558 total (matches
    // the [provenance] block in corpus/v5.1/site/MANIFEST.toml).
    for locale in ["en", "ko", "ru"] {
        assert_eq!(
            topaz_in(&format!("corpus/v5.1/site/source/{locale}/")),
            186,
            "locale {locale}"
        );
    }
    assert_eq!(topaz_in("corpus/v5.1/site/source/"), 558);

    // Every topaz fence has a corpus file; no other fence does.
    for row in rows {
        assert_eq!(
            row.file.is_some(),
            row.fence.language == "topaz",
            "file presence mismatch at {} #{}",
            row.fence.source,
            row.fence.index
        );
    }

    // The positive parse gate (CDR-001 §7): program/snippet rows
    // expect parse_ok, everything else is skipped.
    for row in rows {
        let positive = matches!(row.mode, "program" | "snippet");
        assert_eq!(
            row.expect,
            if positive { "parse_ok" } else { "skip" },
            "expect mismatch at {} #{}",
            row.fence.source,
            row.fence.index
        );
    }
}
