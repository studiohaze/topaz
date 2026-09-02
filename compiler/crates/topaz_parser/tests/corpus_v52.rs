//! corpus/v5.2 harness (CDR-002 §3/§4): manifest-driven, never
//! globbed. Parse and runnable resolve fixtures execute here. Check
//! and exec rows receive source anti-rot coverage here and execute in
//! their owning product suites. The deferral register rejects any new
//! pending row, so no fixture is silently skipped.

use std::fs;

use corpus_extract::{V52_EXPECTED, V52FixtureStatus, read_fixture_directory, read_v52_manifest};
use topaz_diag::FileId;
use topaz_parser::{ParseOptions, parse, parse_with_options};
use topaz_resolve::{InMemoryProvider, resolve_with_version};
use topaz_syntax::LangVersion;

fn parse_v52(src: &str) -> topaz_parser::ParseOutput {
    parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: LangVersion::V5_2,
        },
    )
}

#[test]
fn every_v52_manifest_row_holds() {
    let root = corpus_extract::repo_root();
    let mut failures: Vec<String> = Vec::new();

    for (area, expected_count) in V52_EXPECTED {
        let dir = root.join("corpus/v5.2").join(area);
        let (totals, fixtures) =
            read_v52_manifest(&dir.join("MANIFEST.toml")).expect("manifest readable");

        // Cardinality: pinned count, internal consistency, no silent
        // skips (every parse-phase fixture must be runnable).
        if fixtures.len() != expected_count {
            failures.push(format!(
                "{area}: expected {expected_count} fixtures, manifest has {}",
                fixtures.len()
            ));
        }
        let runnable_rows = fixtures
            .iter()
            .filter(|f| f.phase == "parse" || f.status == V52FixtureStatus::Runnable)
            .count();
        if totals.runnable != runnable_rows {
            failures.push(format!(
                "{area}: {runnable_rows} runnable fixtures but runnable = {}",
                totals.runnable
            ));
        }

        for fixture in &fixtures {
            if fixture.phase == "resolve" && fixture.status == V52FixtureStatus::Runnable {
                for problem in run_resolve_fixture(&dir, fixture) {
                    failures.push(format!("{area}/{problem}"));
                }
                continue;
            }
            if fixture.phase != "parse" {
                // Resolve-phase rows activate with the resolver; the
                // ENTRY marker and the entry file must already exist
                // so recorded fixtures cannot rot silently.
                if !fixture.dir.is_empty() {
                    for problem in pending_fixture_problems(&dir, fixture) {
                        failures.push(format!("{area}/{problem}"));
                    }
                }
                continue;
            }
            let (src, label) = if fixture.dir.is_empty() {
                let path = dir.join(&fixture.file);
                (
                    fs::read_to_string(&path).expect("fixture readable"),
                    format!("{area}/{}", fixture.file),
                )
            } else {
                let base = dir.join(&fixture.dir);
                let mut files = match read_fixture_directory(&base, &fixture.entry) {
                    Ok(files) => files,
                    Err(error) => {
                        failures.push(format!(
                            "{area}/{}: fixture intake failed: {error}",
                            fixture.dir
                        ));
                        continue;
                    }
                };
                let src = files
                    .remove(&fixture.entry)
                    .expect("admitted fixture retains its entry source");
                (src, format!("{area}/{}", fixture.dir))
            };

            match (fixture.versions.as_str(), fixture.result.as_str()) {
                ("both", "ok") => {
                    let base = parse(FileId(0), &src);
                    let v52 = parse_v52(&src);
                    if !base.diagnostics.is_empty() || !v52.diagnostics.is_empty() {
                        failures.push(format!("{label}: expected ok at both versions"));
                    } else if base.program != v52.program {
                        failures.push(format!("{label}: ASTs differ between versions"));
                    }
                }
                ("both", "error") => {
                    // Base-owned malformed shapes: the same primary
                    // diagnostic at both versions (C3 pins).
                    let expected = fixture.code.as_deref().expect("error rows carry codes");
                    for (version, out) in
                        [("5.1", parse(FileId(0), &src)), ("5.2", parse_v52(&src))]
                    {
                        match out.diagnostics.first() {
                            Some(d) if d.code.as_str() == expected => {}
                            other => failures.push(format!(
                                "{label}@{version}: expected primary {expected}, got {:?}",
                                other.map(|d| d.code.as_str())
                            )),
                        }
                    }
                }
                ("5.2", "ok") => {
                    let out = parse_v52(&src);
                    if !out.diagnostics.is_empty() {
                        failures.push(format!(
                            "{label}: expected parse-ok at 5.2, got {:?}",
                            out.diagnostics
                                .iter()
                                .map(|d| d.code.as_str())
                                .collect::<Vec<_>>()
                        ));
                    }
                }
                (version @ ("5.1" | "5.2"), "error") => {
                    let out = if version == "5.1" {
                        parse(FileId(0), &src)
                    } else {
                        parse_v52(&src)
                    };
                    let expected = fixture.code.as_deref().expect("error rows carry codes");
                    match out.diagnostics.first() {
                        Some(d) if d.code.as_str() == expected => {}
                        other => failures.push(format!(
                            "{label}: expected primary {expected}, got {:?}",
                            other.map(|d| d.code.as_str())
                        )),
                    }
                }
                (v, r) => {
                    failures.push(format!("{label}: unsupported row versions={v} result={r}"))
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} corpus/v5.2 failures:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

/// Anti-rot validation for a recorded (pending) directory fixture:
/// the ENTRY marker matches the manifest, the entry file exists, the
/// expectation text is recorded, and every `.tpz` source under the
/// fixture parses cleanly at v5.2 (resolve-phase fixtures must not
/// hand Phase B dead sources).
fn pending_fixture_problems(
    area_dir: &std::path::Path,
    fixture: &corpus_extract::V52Fixture,
) -> Vec<String> {
    let mut problems = Vec::new();
    let base = area_dir.join(&fixture.dir);
    let files = match read_fixture_directory(&base, &fixture.entry) {
        Ok(files) => files,
        Err(error) => {
            problems.push(format!("{}: fixture intake failed: {error}", fixture.dir));
            return problems;
        }
    };
    if fixture.expect.is_empty() {
        problems.push(format!("{}: pending row has no expectation", fixture.dir));
    }
    for (relative_path, contents) in files {
        if std::path::Path::new(&relative_path)
            .extension()
            .is_some_and(|extension| extension == "tpz")
        {
            let out = parse_v52(&contents);
            if !out.diagnostics.is_empty() {
                problems.push(format!(
                    "{}: {} does not parse at 5.2",
                    fixture.dir, relative_path
                ));
            }
        }
    }
    problems
}

/// Runs a runnable resolve-phase directory fixture hermetically: the
/// fixture tree becomes an in-memory provider (LINKS.txt records
/// become virtual links), ROOT supplies the explicit root, and the
/// manifest row asserts result and primary code.
fn run_resolve_fixture(
    area_dir: &std::path::Path,
    fixture: &corpus_extract::V52Fixture,
) -> Vec<String> {
    let mut problems = Vec::new();
    let base = area_dir.join(&fixture.dir);
    let mut provider = InMemoryProvider::new();
    let mut root: Option<String> = None;
    let files = match read_fixture_directory(&base, &fixture.entry) {
        Ok(files) => files,
        Err(error) => {
            problems.push(format!("{}: fixture intake failed: {error}", fixture.dir));
            return problems;
        }
    };
    for (relative_path, contents) in files {
        match relative_path.as_str() {
            "ROOT" => root = Some(contents.trim().to_string()),
            "LINKS.txt" => {
                for line in contents.lines() {
                    if let Some((from, to)) = line.split_once("->") {
                        provider.add_link(from.trim(), to.trim());
                    }
                }
            }
            "VFILES.txt" => {
                // Virtual files for shapes a case-insensitive
                // checkout cannot store (e.g. case-colliding
                // names); same stub body as exporting fixtures.
                for line in contents.lines() {
                    let line = line.trim();
                    if !line.is_empty() {
                        provider.add_file(
                            line,
                            "export function ok() -> int { 1 }
",
                        );
                    }
                }
            }
            _ => provider.add_file(relative_path, contents),
        }
    }
    let out = resolve_with_version(
        &provider,
        &fixture.entry,
        root.as_deref(),
        LangVersion::V5_2,
    );
    if !fixture.modules.is_empty() {
        let mut got: Vec<String> = out.modules.iter().map(|m| m.identity.clone()).collect();
        got.sort();
        let want: Vec<String> = fixture.modules.split(',').map(str::to_string).collect();
        if got != want {
            problems.push(format!(
                "{}: module identities {got:?} != expected {want:?}",
                fixture.dir
            ));
        }
    }
    if !fixture.order.is_empty() {
        let got: Vec<String> = out.modules.iter().map(|m| m.identity.clone()).collect();
        let want: Vec<String> = fixture.order.split(',').map(str::to_string).collect();
        if got != want {
            problems.push(format!(
                "{}: processing order {got:?} != expected {want:?}",
                fixture.dir
            ));
        }
    }
    if !fixture.not_read.is_empty() {
        let reads = provider.reads();
        for forbidden in fixture.not_read.split(',') {
            if reads.contains(forbidden) {
                problems.push(format!(
                    "{}: `{forbidden}` was read during resolution",
                    fixture.dir
                ));
            }
        }
    }
    match fixture.result.as_str() {
        "ok" => {
            if !out.diagnostics.is_empty() {
                problems.push(format!(
                    "{}: expected resolve-ok, got {:?}",
                    fixture.dir,
                    out.diagnostics
                        .iter()
                        .map(|d| d.code.as_str())
                        .collect::<Vec<_>>()
                ));
            }
        }
        "error" => {
            let expected = fixture.code.as_deref().unwrap_or("");
            match out.diagnostics.first() {
                Some(d) if d.code.as_str() == expected => {}
                other => problems.push(format!(
                    "{}: expected primary {expected}, got {:?}",
                    fixture.dir,
                    other.map(|d| d.code.as_str())
                )),
            }
        }
        other => problems.push(format!("{}: unsupported result {other}", fixture.dir)),
    }
    problems
}

/// CDR-002 Phase B DoD: the TPZ3xxx registry is fixture-pinned.
/// Every resolver code must be the pinned primary of at least one
/// modules-area fixture — except TPZ3003, the loader bound
/// (`MAX_SOURCE_LEN` = 4 GiB), which no practical fixture can reach;
/// it is pinned by the `topaz_diag` source-bound unit tests instead.
/// The exemption is asserted too, so it cannot widen silently.
#[test]
fn tpz3_registry_is_fixture_pinned() {
    let root = corpus_extract::repo_root();
    let (_, fixtures) =
        read_v52_manifest(&root.join("corpus/v5.2/modules/MANIFEST.toml")).expect("manifest");
    let pinned: std::collections::BTreeSet<&str> = fixtures
        .iter()
        .filter_map(|f| f.code.as_deref())
        .filter(|c| c.starts_with("TPZ3"))
        .collect();
    let expected: std::collections::BTreeSet<String> = (1..=18)
        .filter(|n| *n != 3)
        .map(|n| format!("TPZ3{n:03}"))
        .collect();
    let expected: std::collections::BTreeSet<&str> = expected.iter().map(String::as_str).collect();
    assert_eq!(pinned, expected, "TPZ3xxx pin set drifted");
}

/// CDR-002 Phase B item 1 deferral register (amendment dated
/// 2026-06-12): the register only shrinks — m079-10 activated at
/// v0.3 (exec phase), and the last four (m081-02, m083-01/02,
/// m084-14) activated as check-phase TPZ5002 fixtures with
/// module-aware checking (CDR-004 C-6). The pending set is now
/// empty and may not widen — any new pending row fails here.
#[test]
fn pending_set_matches_the_deferral_register() {
    let root = corpus_extract::repo_root();
    let (_, fixtures) =
        read_v52_manifest(&root.join("corpus/v5.2/modules/MANIFEST.toml")).expect("manifest");
    let pending: Vec<&str> = fixtures
        .iter()
        .filter(|f| f.status == V52FixtureStatus::Pending)
        .map(|f| f.dir.as_str())
        .collect();
    let register: [&str; 0] = [];
    assert_eq!(pending, register, "deferral register drifted");
}

/// SPEC v5.2 §17 `ExportLetBinding ::= "let" Identifier
/// TypeAnnotation? "=" Expression`: the annotated single-identifier
/// form is legal (regression: TPZ2007 misfired on it — caught by the
/// site sample-verification harness on its first run).
#[test]
fn annotated_export_let_parses() {
    let ok = parse_v52("export let size: int = 1\n");
    assert!(ok.diagnostics.is_empty(), "{:?}", ok.diagnostics);
    let still_rejected = parse_v52("export let { a, b } = pair\n");
    assert!(
        still_rejected
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "TPZ2007")
    );
}
