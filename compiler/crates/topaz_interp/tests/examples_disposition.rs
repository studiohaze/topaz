//! Disposition of every EXAMPLES.md complete program (mode =
//! "program") under the reference interpreter: each one either runs
//! clean with output (and then MUST be an exec/examples fixture),
//! runs clean silently, or stops on a TPZ5099 deferral. The sets are
//! pinned by file, so an interpreter feature landing (or regressing)
//! moves a named example and fails here until the corpus follows.

use std::fs;

use topaz_diag::FileId;
use topaz_interp::{Machine, TestHost};
use topaz_parser::{ParseOptions, parse_with_options};
use topaz_syntax::LangVersion;

/// Programs that print today — exercised from their canonical source by the
/// `corpus/exec/examples` transcript harness.
const PRINTED: [&str; 8] = ["002", "017", "028", "040", "044", "045", "050", "052"];

/// Programs stopped by a TPZ5099 deferral. Empty because type
/// conformance, variadic/default parameters, and spread/named call
/// arguments all landed; the set may not widen.
const DEFERRED: [&str; 0] = [];

fn stem(path: &str) -> &str {
    path.rsplit('/')
        .next()
        .and_then(|f| f.strip_suffix(".tpz"))
        .expect("example file name")
}

#[test]
fn every_example_program_has_a_pinned_disposition() {
    let root = corpus_extract::repo_root();
    let generated = corpus_extract::generate(&root).expect("corpus generation");

    let mut printed = Vec::new();
    let mut silent = 0usize;
    let mut deferred = Vec::new();
    let mut failures = Vec::new();

    for row in generated.rows.iter().filter(|r| r.mode == "program") {
        let file = row.file.as_deref().expect("program rows carry files");
        let src = fs::read_to_string(root.join(file)).expect("committed corpus file");
        let out = parse_with_options(
            FileId(0),
            &src,
            ParseOptions {
                language_version: LangVersion::V5_2,
            },
        );
        assert!(out.diagnostics.is_empty(), "{file}: does not parse");

        let host = TestHost::new();
        let mut machine = Machine::new(&src, &host);
        match machine.run_program(&out.program) {
            Ok(_) if host.stdout().is_empty() => silent += 1,
            Ok(_) => printed.push(stem(file).to_string()),
            Err(e) if e.code == "TPZ5099" => deferred.push(stem(file).to_string()),
            Err(e) => failures.push(format!("{file}: unexpected stop {}: {}", e.code, e.message)),
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(printed, PRINTED, "printed-program set moved");
    assert_eq!(deferred, DEFERRED, "deferred-program set moved");
    assert_eq!(
        printed.len() + silent + deferred.len(),
        44,
        "program count moved"
    );
}
