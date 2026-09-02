//! Exec-phase corpus rows (CDR-003 §11): manifest-driven, never
//! globbed. Resolve the fixture unit hermetically, run it on a
//! TestHost, and pin the recorded outcome.

use std::fs;

use corpus_extract::{
    read_fixture_directory, read_optional_sidecar, read_v52_manifest, read_virtual_file_sidecar,
};
use topaz_interp::{Machine, TestHost};
use topaz_resolve::{InMemoryProvider, resolve};
use topaz_syntax::LangVersion;

#[test]
fn every_exec_row_holds() {
    let root = corpus_extract::repo_root();
    let dir = root.join("corpus/v5.2/modules");
    let (_, fixtures) = read_v52_manifest(&dir.join("MANIFEST.toml")).expect("manifest");
    let mut failures: Vec<String> = Vec::new();
    let mut ran = 0usize;

    for fixture in fixtures.iter().filter(|f| f.phase == "exec") {
        ran += 1;
        let base = dir.join(&fixture.dir);
        let mut provider = InMemoryProvider::new();
        let files = match read_fixture_directory(&base, &fixture.entry) {
            Ok(files) => files,
            Err(error) => {
                failures.push(format!("{}: fixture intake failed: {error}", fixture.dir));
                continue;
            }
        };
        for (relative_path, contents) in files {
            provider.add_file(relative_path, contents);
        }
        let unit = resolve(&provider, &fixture.entry, None);
        if !unit.diagnostics.is_empty() {
            failures.push(format!(
                "{}: did not resolve: {:?}",
                fixture.dir, unit.diagnostics
            ));
            continue;
        }
        let host = TestHost::new();
        let outcome = Machine::run_unit(&unit, &host);
        match (fixture.result.as_str(), outcome) {
            ("error", Err(e)) => {
                let expected = fixture.code.as_deref().expect("error rows carry codes");
                if e.code != expected {
                    failures.push(format!(
                        "{}: expected {expected}, got {} ({})",
                        fixture.dir, e.code, e.message
                    ));
                } else if !fixture.message.is_empty() && !e.message.contains(&fixture.message) {
                    failures.push(format!(
                        "{}: message `{}` lacks `{}`",
                        fixture.dir, e.message, fixture.message
                    ));
                } else if !e.message.contains("module `") || !e.message.contains("import chain:") {
                    failures.push(format!(
                        "{}: fault lacks module context/import chain: {}",
                        fixture.dir, e.message
                    ));
                }
            }
            ("error", Ok(_)) => {
                failures.push(format!(
                    "{}: expected a runtime stop, ran clean",
                    fixture.dir
                ));
            }
            ("ok", Ok(_)) => {}
            ("ok", Err(e)) => {
                failures.push(format!(
                    "{}: unexpected stop {}: {}",
                    fixture.dir, e.code, e.message
                ));
            }
            (other, _) => failures.push(format!("{}: unsupported result {other}", fixture.dir)),
        }
    }

    assert!(ran >= 1, "exec rows must not silently vanish");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `corpus/exec/<area>` single-file transcript rows: parse at the row's pinned
/// language version, run on a TestHost, compare the full stdout transcript
/// against the `.stdout` golden, and pin stop codes/messages.
#[test]
fn every_exec_area_row_holds() {
    use topaz_diag::FileId;
    use topaz_parser::{ParseOptions, parse_with_options};

    let root = corpus_extract::repo_root();
    let mut failures: Vec<String> = Vec::new();

    for (area, expected_count) in corpus_extract::EXEC_EXPECTED {
        let dir = root.join("corpus/exec").join(area);
        let source_dir = corpus_extract::exec_source_dir(&root, area);
        let (totals, fixtures) =
            read_v52_manifest(&dir.join("MANIFEST.toml")).expect("manifest readable");
        if totals.runnable != fixtures.len() || fixtures.len() != expected_count {
            failures.push(format!(
                "{area}: expected {expected_count} runnable fixtures, manifest disagrees"
            ));
        }
        for fixture in &fixtures {
            let label = format!("{area}/{}", fixture.file);
            let src = fs::read_to_string(source_dir.join(&fixture.file)).expect("fixture readable");
            let Ok(golden) = fs::read_to_string(dir.join(fixture.file.replace(".tpz", ".stdout")))
            else {
                failures.push(format!("{label}: missing .stdout sidecar"));
                continue;
            };
            if fixture.result == "ok" && golden.trim().is_empty() {
                failures.push(format!("{label}: ok rows must pin meaningful output"));
                continue;
            }
            let Some(language_version) = exec_fixture_language_version(&fixture.versions) else {
                failures.push(format!(
                    "{label}: unsupported language version `{}`",
                    fixture.versions
                ));
                continue;
            };
            let expected_stdout = topaz_interp::transcript::transcript_lines(&golden);
            let out = parse_with_options(FileId(0), &src, ParseOptions { language_version });
            if !out.diagnostics.is_empty() {
                failures.push(format!("{label}: does not parse: {:?}", out.diagnostics));
                continue;
            }
            let host = TestHost::new();
            if let Ok(tick) = fixture.clock_tick.parse::<u64>() {
                host.set_tick_per_poll(tick);
            }
            let seed_path = dir.join(fixture.file.replace(".tpz", ".files-in"));
            match read_virtual_file_sidecar(&seed_path) {
                Ok(Some(seed)) => {
                    for (path, contents) in seed {
                        host.add_file(path, contents);
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    failures.push(format!("{label}: unreadable .files-in sidecar: {error}"));
                    continue;
                }
            }
            let mut machine = Machine::new(&src, &host);
            let outcome = machine.run_program(&out.program);
            match (fixture.result.as_str(), outcome) {
                ("ok", Ok(_)) => {}
                ("ok", Err(e)) => {
                    failures.push(format!(
                        "{label}: unexpected stop {}: {}",
                        e.code, e.message
                    ));
                    continue;
                }
                ("error", Err(e)) => {
                    let code = fixture.code.as_deref().expect("error rows carry codes");
                    if e.code != code {
                        failures.push(format!("{label}: expected {code}, got {}", e.code));
                    }
                    if !topaz_interp::transcript::message_matches(&fixture.message, &e.message) {
                        failures.push(format!(
                            "{label}: message `{}` != `{}`",
                            e.message, fixture.message
                        ));
                    }
                }
                ("error", Ok(_)) => {
                    failures.push(format!("{label}: expected a stop, ran clean"));
                    continue;
                }
                (other, _) => failures.push(format!("{label}: unsupported result {other}")),
            }
            let got: Vec<String> = host.stdout();
            if got != expected_stdout {
                failures.push(format!(
                    "{label}: transcript mismatch\n  expected: {expected_stdout:?}\n  got:      {got:?}"
                ));
            }
            let final_files_path = dir.join(fixture.file.replace(".tpz", ".files"));
            match read_virtual_file_sidecar(&final_files_path) {
                Ok(Some(want)) => {
                    if !host.files().into_iter().eq(want) {
                        failures.push(format!("{label}: virtual-file state mismatch"));
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    failures.push(format!("{label}: unreadable .files sidecar: {error}"));
                }
            }
            let defer_path = dir.join(fixture.file.replace(".tpz", ".defer"));
            match read_optional_sidecar(&defer_path) {
                Ok(Some(defers)) => {
                    let want: Vec<&str> = defers.lines().filter(|l| !l.trim().is_empty()).collect();
                    if host.defer_errors() != want {
                        failures.push(format!(
                            "{label}: deferred-error log mismatch: {:?}",
                            host.defer_errors()
                        ));
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    failures.push(format!("{label}: unreadable .defer sidecar: {error}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn exec_fixture_language_version(value: &str) -> Option<LangVersion> {
    match value {
        "5.1" => Some(LangVersion::V5_1),
        "5.2" | "both" => Some(LangVersion::V5_2),
        "5.3" => Some(LangVersion::V5_3),
        "5.4" => Some(LangVersion::V5_4),
        _ => None,
    }
}
