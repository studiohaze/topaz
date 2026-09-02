use crate::*;

/// Runs the corpus gates exactly as CI does: extractor drift, every
/// manifest row with `expect = "parse_ok"`, the layout-conformance
/// fixtures, and the invalid fixtures' primary codes.
pub(super) fn check_corpus() -> ExitCode {
    // Cardinality gates, matching the CI harness: shrinkage is a
    // failure even when everything that remains passes.
    // corpus/v5_4: structured rows for the v5.4-only surface. These are
    // separate from the historical v5.1/v5.2 compatibility corpus so the
    // v5.4 gate can grow without mutating frozen older-version counts.
    const EXPECTED_V54: [(&str, usize); 9] = [
        ("parse", 2),
        ("check", 2),
        ("exec", 10),
        ("diagnostics", 7),
        ("stdlib", 12),
        ("native-negative", 0),
        ("packages", 23),
        ("extern", 3),
        ("performance-smoke", 1),
    ];

    let root = corpus_extract::repo_root();
    let mut failures = 0usize;

    // 1. Extractor drift.
    match corpus_extract::drift(&root) {
        Ok(problems) if problems.is_empty() => println!("drift:    clean"),
        Ok(problems) => {
            for p in &problems {
                eprintln!("drift: {p}");
            }
            failures += problems.len();
        }
        Err(e) => {
            eprintln!("topaz: corpus generation failed: {e}");
            return ExitCode::FAILURE;
        }
    }

    // 2. Positive corpus: manifest-selected parse_ok rows.
    let generated = match corpus_extract::generate(&root) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("topaz: corpus generation failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut parse_ok = 0usize;
    for row in &generated.rows {
        if row.expect != "parse_ok" {
            continue;
        }
        let file = row.file.as_deref().expect("parse_ok rows carry files");
        match fs::read_to_string(root.join(file)) {
            Ok(src) => {
                // Snippets are checked at v5.2: the locked v5.2 surface
                // is a strict superset of frozen v5.1, and the site
                // documents v5.2 (module fences are canonical there).
                let out = parse_with_options(
                    FileId(0),
                    &src,
                    ParseOptions {
                        language_version: LangVersion::V5_2,
                    },
                );
                if out.diagnostics.is_empty() {
                    parse_ok += 1;
                } else {
                    eprintln!("corpus: {file}: {} diagnostic(s)", out.diagnostics.len());
                    failures += 1;
                }
            }
            Err(e) => {
                eprintln!("corpus: {file}: {e}");
                failures += 1;
            }
        }
    }
    if parse_ok != corpus_extract::V51_PARSE_OK_EXPECTED {
        eprintln!(
            "corpus: expected {} parse-ok rows, counted {parse_ok}",
            corpus_extract::V51_PARSE_OK_EXPECTED
        );
        failures += 1;
    }
    println!("corpus:   {parse_ok} parse-ok");

    // 3. Layout fixtures.
    let mut layout_ok = 0usize;
    for path in tpz_files(&root.join("corpus/v5.1/layout")) {
        let src = fs::read_to_string(&path).expect("fixture readable");
        if parse(FileId(0), &src).diagnostics.is_empty() {
            layout_ok += 1;
        } else {
            eprintln!("layout: {} failed to parse", path.display());
            failures += 1;
        }
    }
    if layout_ok != corpus_extract::V51_LAYOUT_EXPECTED {
        eprintln!(
            "layout: expected {} fixtures, counted {layout_ok}",
            corpus_extract::V51_LAYOUT_EXPECTED
        );
        failures += 1;
    }
    println!("layout:   {layout_ok} fixtures ok");

    // 4. Invalid fixtures: filename prefix = expected primary code.
    let mut invalid_ok = 0usize;
    for path in tpz_files(&root.join("corpus/v5.1/invalid")) {
        let expected = path
            .file_stem()
            .expect("fixture stem")
            .to_string_lossy()
            .split('-')
            .next()
            .expect("code prefix")
            .to_uppercase();
        let src = fs::read_to_string(&path).expect("fixture readable");
        let out = parse(FileId(0), &src);
        match out.diagnostics.first() {
            Some(d) if d.code.as_str() == expected => invalid_ok += 1,
            other => {
                eprintln!(
                    "invalid: {} expected primary {expected}, got {:?}",
                    path.display(),
                    other.map(|d| d.code.as_str())
                );
                failures += 1;
            }
        }
    }
    if invalid_ok != corpus_extract::V51_INVALID_EXPECTED {
        eprintln!(
            "invalid: expected {} fixtures, counted {invalid_ok}",
            corpus_extract::V51_INVALID_EXPECTED
        );
        failures += 1;
    }
    println!("invalid:  {invalid_ok} fixtures ok");

    // 5. corpus/v5.2 areas (CDR-002 §3/§4): manifest-driven. Parse,
    // resolve, and check fixtures run in their owning branch; exec
    // fixtures receive source coverage here and run below. Manifest
    // totals account for every row, including any pending row.
    let mut v52_ok = 0usize;
    let mut module_exec_rows = None;
    for (area, expected_count) in corpus_extract::V52_EXPECTED {
        let dir = root.join("corpus/v5.2").join(area);
        let mut admitted_module_exec_files = BTreeMap::new();
        let (totals, fixtures) = match corpus_extract::read_v52_manifest(&dir.join("MANIFEST.toml"))
        {
            Ok(read) => read,
            Err(e) => {
                eprintln!("v5.2: {area}: {e}");
                failures += 1;
                continue;
            }
        };
        let runnable_rows = fixtures
            .iter()
            .filter(|f| {
                f.phase == "parse" || f.status == corpus_extract::V52FixtureStatus::Runnable
            })
            .count();
        if fixtures.len() != expected_count || totals.runnable != runnable_rows {
            eprintln!(
                "v5.2: {area}: expected {expected_count} fixtures, manifest records {} (runnable {}, pending {})",
                totals.recorded, totals.runnable, totals.pending
            );
            failures += 1;
        }
        for (fixture_index, fixture) in fixtures.iter().enumerate() {
            if (fixture.phase == "resolve" || fixture.phase == "check")
                && fixture.status == corpus_extract::V52FixtureStatus::Runnable
            {
                let base = dir.join(&fixture.dir);
                let mut provider = InMemoryProvider::new();
                let mut fixture_root: Option<String> = None;
                let files = match corpus_extract::read_fixture_directory(&base, &fixture.entry) {
                    Ok(files) => files,
                    Err(error) => {
                        eprintln!(
                            "v5.2: {area}/{}: fixture intake failed: {error}",
                            fixture.dir
                        );
                        failures += 1;
                        continue;
                    }
                };
                for (relative_path, contents) in files {
                    match relative_path.as_str() {
                        "ROOT" => fixture_root = Some(contents.trim().to_string()),
                        "LINKS.txt" => {
                            for line in contents.lines() {
                                if let Some((from, to)) = line.split_once("->") {
                                    provider.add_link(from.trim(), to.trim());
                                }
                            }
                        }
                        "VFILES.txt" => {
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
                // Frozen v5.2 corpus: pinned to V5_2 so it never gains v5.3 (enum).
                let out = resolve_with_version(
                    &provider,
                    &fixture.entry,
                    fixture_root.as_deref(),
                    LangVersion::V5_2,
                );
                let mut asserts_ok = true;
                if !fixture.modules.is_empty() {
                    let mut got: Vec<String> =
                        out.modules.iter().map(|m| m.identity.clone()).collect();
                    got.sort();
                    let want: Vec<String> =
                        fixture.modules.split(',').map(str::to_string).collect();
                    asserts_ok &= got == want;
                }
                if !fixture.order.is_empty() {
                    let got: Vec<String> = out.modules.iter().map(|m| m.identity.clone()).collect();
                    let want: Vec<String> = fixture.order.split(',').map(str::to_string).collect();
                    asserts_ok &= got == want;
                }
                if !fixture.not_read.is_empty() {
                    let reads = provider.reads();
                    asserts_ok &= fixture
                        .not_read
                        .split(',')
                        .all(|forbidden| !reads.contains(forbidden));
                }
                let ok = asserts_ok
                    && if fixture.phase == "check" {
                        // CDR-004 C-6: a check-phase fixture resolves
                        // cleanly, then the whole-unit checker
                        // produces (or doesn't) the recorded code.
                        out.diagnostics.is_empty() && {
                            let unit = unit_modules(&out);
                            let checked = topaz_check::check_unit(&unit);
                            match fixture.result.as_str() {
                                "ok" => checked.diagnostics.is_empty(),
                                "error" => {
                                    let expected = fixture.code.as_deref().unwrap_or("");
                                    checked
                                        .diagnostics
                                        .first()
                                        .is_some_and(|d| d.code.as_str() == expected)
                                }
                                _ => false,
                            }
                        }
                    } else {
                        match fixture.result.as_str() {
                            "ok" => out.diagnostics.is_empty(),
                            "error" => {
                                let expected = fixture.code.as_deref().unwrap_or("");
                                out.diagnostics
                                    .first()
                                    .is_some_and(|d| d.code.as_str() == expected)
                            }
                            _ => false,
                        }
                    };
                if ok {
                    v52_ok += 1;
                } else {
                    eprintln!(
                        "v5.2: {area}/{} did not match its manifest row",
                        fixture.dir
                    );
                    failures += 1;
                }
                continue;
            }
            if fixture.phase != "parse" {
                // Pending (resolve-phase) anti-rot: marker, entry
                // file, recorded expectation, and v5.2 parseability
                // of every source — mirrors the test harness.
                if !fixture.dir.is_empty() {
                    let base = dir.join(&fixture.dir);
                    let files = match corpus_extract::read_fixture_directory(&base, &fixture.entry)
                    {
                        Ok(files) => files,
                        Err(error) => {
                            eprintln!(
                                "v5.2: {area}/{}: fixture intake failed: {error}",
                                fixture.dir
                            );
                            failures += 1;
                            continue;
                        }
                    };
                    if fixture.expect.is_empty() {
                        eprintln!(
                            "v5.2: {area}/{}: recorded fixture is incomplete",
                            fixture.dir
                        );
                        failures += 1;
                        continue;
                    }
                    for (relative_path, contents) in &files {
                        if std::path::Path::new(relative_path)
                            .extension()
                            .is_some_and(|extension| extension == "tpz")
                        {
                            let out = parse_with_options(
                                FileId(0),
                                contents,
                                ParseOptions {
                                    language_version: LangVersion::V5_2,
                                },
                            );
                            if !out.diagnostics.is_empty() {
                                eprintln!(
                                    "v5.2: {area}/{}: {} does not parse at 5.2",
                                    fixture.dir, relative_path
                                );
                                failures += 1;
                            }
                        }
                    }
                    if area == "modules" && fixture.phase == "exec" {
                        admitted_module_exec_files.insert(fixture_index, files);
                    }
                }
                continue;
            }
            let (src, label) = if fixture.dir.is_empty() {
                let path = dir.join(&fixture.file);
                let src = match fs::read_to_string(&path) {
                    Ok(src) => src,
                    Err(e) => {
                        eprintln!("v5.2: {}: {e}", path.display());
                        failures += 1;
                        continue;
                    }
                };
                (src, fixture.file.as_str())
            } else {
                let base = dir.join(&fixture.dir);
                let mut files = match corpus_extract::read_fixture_directory(&base, &fixture.entry)
                {
                    Ok(files) => files,
                    Err(error) => {
                        eprintln!(
                            "v5.2: {area}/{}: fixture intake failed: {error}",
                            fixture.dir
                        );
                        failures += 1;
                        continue;
                    }
                };
                let src = files
                    .remove(&fixture.entry)
                    .expect("admitted fixture retains its entry source");
                (src, fixture.dir.as_str())
            };
            let v52_opts = ParseOptions {
                language_version: LangVersion::V5_2,
            };
            let ok = match (fixture.versions.as_str(), fixture.result.as_str()) {
                ("both", "ok") => {
                    let base = parse(FileId(0), &src);
                    let v52 = parse_with_options(FileId(0), &src, v52_opts);
                    base.diagnostics.is_empty()
                        && v52.diagnostics.is_empty()
                        && base.program == v52.program
                }
                ("both", "error") => {
                    let expected = fixture.code.as_deref().unwrap_or("");
                    let base = parse(FileId(0), &src);
                    let v52 = parse_with_options(FileId(0), &src, v52_opts);
                    base.diagnostics
                        .first()
                        .is_some_and(|d| d.code.as_str() == expected)
                        && v52
                            .diagnostics
                            .first()
                            .is_some_and(|d| d.code.as_str() == expected)
                }
                ("5.2", "ok") => parse_with_options(FileId(0), &src, v52_opts)
                    .diagnostics
                    .is_empty(),
                (version @ ("5.1" | "5.2"), "error") => {
                    let out = if version == "5.1" {
                        parse(FileId(0), &src)
                    } else {
                        parse_with_options(FileId(0), &src, v52_opts)
                    };
                    let expected = fixture.code.as_deref().unwrap_or("");
                    out.diagnostics
                        .first()
                        .is_some_and(|d| d.code.as_str() == expected)
                }
                _ => false,
            };
            if ok {
                v52_ok += 1;
            } else {
                eprintln!("v5.2: {area}/{} did not match its manifest row", label);
                failures += 1;
            }
        }
        if area == "modules" {
            let exec_row_count = fixtures
                .iter()
                .filter(|fixture| fixture.phase == "exec")
                .count();
            let admitted_rows = fixtures
                .into_iter()
                .enumerate()
                .filter_map(|(index, fixture)| {
                    if fixture.phase != "exec" {
                        return None;
                    }
                    admitted_module_exec_files
                        .remove(&index)
                        .map(|files| (fixture, files))
                })
                .collect::<Vec<_>>();
            module_exec_rows = Some((exec_row_count, admitted_rows));
        }
    }
    println!(
        "v5.2:     {v52_ok} fixtures ok ({} areas)",
        corpus_extract::V52_EXPECTED.len()
    );

    let v54_ok = check_v54_corpus(&root, &EXPECTED_V54, &mut failures);
    println!(
        "v5.4:     {v54_ok} fixtures ok ({} areas)",
        EXPECTED_V54.len()
    );

    // corpus/exec transcript gates (CDR-003 §11): single-file rows
    // run on a TestHost; goldens are exact; counts are pinned.
    let mut exec_ok = 0usize;
    for (area, expected_count) in corpus_extract::EXEC_EXPECTED {
        let dir = root.join("corpus/exec").join(area);
        let source_dir = corpus_extract::exec_source_dir(&root, area);
        let (totals, fixtures) = match corpus_extract::read_v52_manifest(&dir.join("MANIFEST.toml"))
        {
            Ok(v) => v,
            Err(e) => {
                eprintln!("exec/{area}: manifest unreadable: {e}");
                failures += 1;
                continue;
            }
        };
        if totals.runnable != fixtures.len() || fixtures.len() != expected_count {
            eprintln!(
                "exec/{area}: expected {expected_count} runnable fixtures, manifest disagrees"
            );
            failures += 1;
        }
        for fixture in &fixtures {
            let label = format!("exec/{area}/{}", fixture.file);
            let Ok(src) = std::fs::read_to_string(source_dir.join(&fixture.file)) else {
                eprintln!("{label}: unreadable");
                failures += 1;
                continue;
            };
            let Ok(golden) =
                std::fs::read_to_string(dir.join(fixture.file.replace(".tpz", ".stdout")))
            else {
                eprintln!("{label}: missing .stdout sidecar");
                failures += 1;
                continue;
            };
            if fixture.result == "ok" && golden.trim().is_empty() {
                eprintln!("{label}: ok rows must pin meaningful output");
                failures += 1;
                continue;
            }
            let Some(language_version) = corpus_fixture_language_version(&fixture.versions) else {
                eprintln!(
                    "{label}: unsupported language version `{}`",
                    fixture.versions
                );
                failures += 1;
                continue;
            };
            let expected_stdout = topaz_interp::transcript::transcript_lines(&golden);
            let out = parse_with_options(FileId(0), &src, ParseOptions { language_version });
            if !out.diagnostics.is_empty() {
                eprintln!("{label}: does not parse");
                failures += 1;
                continue;
            }
            let test_host = topaz_interp::TestHost::new();
            if let Ok(tick) = fixture.clock_tick.parse::<u64>() {
                test_host.set_tick_per_poll(tick);
            }
            let seed_path = dir.join(fixture.file.replace(".tpz", ".files-in"));
            if let Err(error) = seed_corpus_test_host(&test_host, &seed_path) {
                eprintln!("{label}: unreadable .files-in sidecar: {error}");
                failures += 1;
                continue;
            }
            let mut machine = Machine::new(&src, &test_host);
            let outcome = machine.run_program(&out.program);
            let mut bad = false;
            match (fixture.result.as_str(), outcome) {
                ("ok", Ok(_)) => {}
                ("error", Err(e)) => {
                    let code = fixture.code.as_deref().unwrap_or("");
                    if e.code != code
                        || !topaz_interp::transcript::message_matches(&fixture.message, &e.message)
                    {
                        eprintln!("{label}: wrong stop {}: {}", e.code, e.message);
                        bad = true;
                    }
                }
                (_, _) => {
                    eprintln!("{label}: outcome class mismatch");
                    bad = true;
                }
            }
            if test_host.stdout() != expected_stdout {
                eprintln!("{label}: transcript mismatch");
                bad = true;
            }
            let final_files_path = dir.join(fixture.file.replace(".tpz", ".files"));
            match corpus_virtual_files_match(&test_host, &final_files_path) {
                Ok(true) => {}
                Ok(false) => {
                    eprintln!("{label}: virtual-file state mismatch");
                    bad = true;
                }
                Err(error) => {
                    eprintln!("{label}: unreadable .files sidecar: {error}");
                    bad = true;
                }
            }
            let defer_path = dir.join(fixture.file.replace(".tpz", ".defer"));
            match corpus_defer_errors_match(&test_host, &defer_path) {
                Ok(true) => {}
                Ok(false) => {
                    eprintln!("{label}: deferred-error log mismatch");
                    bad = true;
                }
                Err(error) => {
                    eprintln!("{label}: unreadable .defer sidecar: {error}");
                    bad = true;
                }
            }
            if bad {
                failures += 1;
            } else {
                exec_ok += 1;
            }
        }
    }
    // §17 module exec rows (corpus/v5.2/modules, phase = "exec").
    if let Some((exec_row_count, exec_rows)) = module_exec_rows {
        if exec_row_count != 11 {
            eprintln!(
                "exec/modules: expected 11 exec rows, found {}",
                exec_row_count
            );
            failures += 1;
        }
        for (fixture, files) in exec_rows {
            let label = format!("exec/modules/{}", fixture.dir);
            let mut provider = InMemoryProvider::new();
            for (relative_path, contents) in files {
                provider.add_file(relative_path, contents);
            }
            // Frozen v5.2/modules exec corpus: pinned to V5_2 (never gains enum).
            let unit = resolve_with_version(&provider, &fixture.entry, None, LangVersion::V5_2);
            if !unit.diagnostics.is_empty() {
                eprintln!("{label}: did not resolve");
                failures += 1;
                continue;
            }
            let test_host = topaz_interp::TestHost::new();
            match (
                fixture.result.as_str(),
                Machine::run_unit(&unit, &test_host),
            ) {
                ("error", Err(e)) => {
                    let code = fixture.code.as_deref().unwrap_or("");
                    if e.code != code
                        || (!fixture.message.is_empty() && !e.message.contains(&fixture.message))
                        || !e.message.contains("import chain:")
                    {
                        eprintln!("{label}: wrong stop {}: {}", e.code, e.message);
                        failures += 1;
                    } else {
                        exec_ok += 1;
                    }
                }
                ("ok", Ok(_)) => exec_ok += 1,
                _ => {
                    eprintln!("{label}: outcome class mismatch");
                    failures += 1;
                }
            }
        }
    }
    println!("exec:     {exec_ok} fixtures ok (14 areas incl. modules)");

    if failures == 0 {
        println!("check-corpus: all gates green");
        ExitCode::SUCCESS
    } else {
        eprintln!("check-corpus: {failures} failure(s)");
        ExitCode::FAILURE
    }
}

pub(super) fn check_v54_corpus(
    root: &Path,
    expected: &[(&str, usize)],
    failures: &mut usize,
) -> usize {
    let mut ok = 0usize;
    for (area, expected_count) in expected {
        let dir = root.join("corpus/v5_4").join(area);
        let (totals, fixtures) = match corpus_extract::read_v52_manifest(&dir.join("MANIFEST.toml"))
        {
            Ok(read) => read,
            Err(e) => {
                eprintln!("v5.4/{area}: manifest unreadable: {e}");
                *failures += 1;
                continue;
            }
        };
        if totals.runnable != fixtures.len() || fixtures.len() != *expected_count {
            eprintln!(
                "v5.4/{area}: expected {expected_count} runnable fixtures, manifest disagrees"
            );
            *failures += 1;
        }
        for fixture in &fixtures {
            let row_name = if fixture.file.is_empty() {
                fixture.dir.as_str()
            } else {
                fixture.file.as_str()
            };
            let label = format!("v5.4/{area}/{row_name}");
            let Some(language_version) = corpus_fixture_language_version(&fixture.versions) else {
                eprintln!(
                    "{label}: unsupported language version `{}`",
                    fixture.versions
                );
                *failures += 1;
                continue;
            };
            let row_ok = match fixture.phase.as_str() {
                "parse" => {
                    let path = dir.join(&fixture.file);
                    let Ok(src) = fs::read_to_string(&path) else {
                        eprintln!("{label}: unreadable");
                        *failures += 1;
                        continue;
                    };
                    let out =
                        parse_with_options(FileId(0), &src, ParseOptions { language_version });
                    corpus_diagnostics_match(
                        &label,
                        &fixture.result,
                        fixture.code.as_deref(),
                        &fixture.message,
                        &out.diagnostics,
                    )
                }
                "check" | "diagnostics" => {
                    check_v54_source_row(&dir, &label, language_version, fixture)
                }
                "exec" => check_v54_exec_row(&dir, &label, language_version, fixture),
                "native-negative" => {
                    check_v54_native_negative_row(&dir, &label, language_version, fixture)
                }
                "package-check" => check_v54_package_row(&dir, &label, language_version, fixture),
                "package-emit" => check_v54_package_emit_row(
                    &dir,
                    &label,
                    language_version,
                    fixture,
                    "package-emit",
                    Backend::Boxed,
                ),
                "package-native-emit" => check_v54_package_emit_row(
                    &dir,
                    &label,
                    language_version,
                    fixture,
                    "package-native-emit",
                    Backend::Native,
                ),
                "package-run" => check_v54_package_interpreted_row(
                    &dir,
                    &label,
                    language_version,
                    fixture,
                    "package-run",
                ),
                "package-build-run" => {
                    check_v54_package_build_run_row(&dir, &label, language_version, fixture)
                }
                "package-test" => check_v54_package_interpreted_row(
                    &dir,
                    &label,
                    language_version,
                    fixture,
                    "package-test",
                ),
                "package-doc" => check_v54_package_doc_row(&dir, &label, language_version, fixture),
                "performance-smoke" => {
                    check_v54_source_row(&dir, &label, language_version, fixture)
                }
                other => {
                    eprintln!("{label}: unsupported v5.4 corpus phase `{other}`");
                    false
                }
            };
            if row_ok {
                ok += 1;
            } else {
                *failures += 1;
            }
        }
    }
    ok
}

pub(super) fn check_v54_source_row(
    dir: &Path,
    label: &str,
    language_version: LangVersion,
    fixture: &corpus_extract::V52Fixture,
) -> bool {
    let provider = PhysicalProvider::new(dir);
    let resolved = resolve_with_version(&provider, &fixture.file, None, language_version);
    if !resolved.diagnostics.is_empty() {
        eprintln!("{label}: did not resolve before check");
        return false;
    }
    let unit = unit_modules(&resolved);
    let checked = topaz_check::check_unit_typed_with_version(&unit, language_version);
    let diagnostics_ok = corpus_diagnostics_match(
        label,
        &fixture.result,
        fixture.code.as_deref(),
        &fixture.message,
        &checked.diagnostics,
    );
    if !diagnostics_ok {
        return false;
    }
    if fixture.result == "ok" && matches!(fixture.phase.as_str(), "check" | "performance-smoke") {
        // These rows are accepted v5.4 source-surface fixtures without an exec
        // transcript; parse/diagnostics rows are intentionally not emit-gated.
        if lower_checked_unit(&resolved, language_version, Backend::Boxed, Some(&checked)).is_err()
        {
            eprintln!("{label}: boxed emit did not lower");
            return false;
        }
        if lower_checked_unit(&resolved, language_version, Backend::Native, Some(&checked)).is_err()
        {
            eprintln!("{label}: native emit did not lower or fall back");
            return false;
        }
    }
    true
}

pub(super) fn check_v54_exec_row(
    dir: &Path,
    label: &str,
    language_version: LangVersion,
    fixture: &corpus_extract::V52Fixture,
) -> bool {
    let Ok(golden) = fs::read_to_string(dir.join(fixture.file.replace(".tpz", ".stdout"))) else {
        eprintln!("{label}: missing .stdout sidecar");
        return false;
    };
    if fixture.result == "ok" && golden.trim().is_empty() {
        eprintln!("{label}: ok rows must pin meaningful output");
        return false;
    }
    let provider = PhysicalProvider::new(dir);
    let resolved = resolve_with_version(&provider, &fixture.file, None, language_version);
    if !resolved.diagnostics.is_empty() {
        eprintln!("{label}: did not resolve before exec");
        return false;
    }
    let unit = unit_modules(&resolved);
    let checked = topaz_check::check_unit_typed_with_version(&unit, language_version);
    if has_errors(&checked.diagnostics) {
        eprintln!("{label}: did not check before exec");
        return false;
    }
    let host = topaz_interp::TestHost::new();
    if let Ok(tick) = fixture.clock_tick.parse::<u64>() {
        host.set_tick_per_poll(tick);
    }
    let stdin_path = dir.join(fixture.file.replace(".tpz", ".stdin"));
    match corpus_extract::read_optional_sidecar(&stdin_path) {
        Ok(Some(input)) => host.set_input(input),
        Ok(None) => {}
        Err(error) => {
            eprintln!("{label}: unreadable .stdin sidecar: {error}");
            return false;
        }
    }
    let seed_path = dir.join(fixture.file.replace(".tpz", ".files-in"));
    if let Err(error) = seed_corpus_test_host(&host, &seed_path) {
        eprintln!("{label}: unreadable .files-in sidecar: {error}");
        return false;
    }
    let outcome = Machine::run_unit(&resolved, &host);
    let mut ok = match (fixture.result.as_str(), outcome) {
        ("ok", Ok(_)) => true,
        ("error", Err(e)) => {
            let expected = fixture.code.as_deref().unwrap_or("");
            e.code == expected
                && topaz_interp::transcript::message_matches(&fixture.message, &e.message)
        }
        ("ok", Err(e)) => {
            eprintln!("{label}: unexpected stop {}: {}", e.code, e.message);
            false
        }
        ("error", Ok(_)) => {
            eprintln!("{label}: expected a stop, ran clean");
            false
        }
        (other, _) => {
            eprintln!("{label}: unsupported result `{other}`");
            false
        }
    };
    let expected_stdout = topaz_interp::transcript::transcript_lines(&golden);
    if host.stdout() != expected_stdout {
        eprintln!("{label}: transcript mismatch");
        ok = false;
    }
    let final_files_path = dir.join(fixture.file.replace(".tpz", ".files"));
    match corpus_virtual_files_match(&host, &final_files_path) {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("{label}: virtual-file state mismatch");
            ok = false;
        }
        Err(error) => {
            eprintln!("{label}: unreadable .files sidecar: {error}");
            ok = false;
        }
    }
    let defer_path = dir.join(fixture.file.replace(".tpz", ".defer"));
    match corpus_defer_errors_match(&host, &defer_path) {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("{label}: deferred-error log mismatch");
            ok = false;
        }
        Err(error) => {
            eprintln!("{label}: unreadable .defer sidecar: {error}");
            ok = false;
        }
    }
    if lower_checked_unit(&resolved, language_version, Backend::Boxed, Some(&checked)).is_err() {
        eprintln!("{label}: boxed emit did not lower");
        ok = false;
    }
    if lower_checked_unit(&resolved, language_version, Backend::Native, Some(&checked)).is_err() {
        eprintln!("{label}: native emit did not lower or fall back");
        ok = false;
    }
    ok
}

pub(super) fn seed_corpus_test_host(
    host: &topaz_interp::TestHost,
    path: &Path,
) -> std::io::Result<()> {
    if let Some(files) = corpus_extract::read_virtual_file_sidecar(path)? {
        for (path, contents) in files {
            host.add_file(path, contents);
        }
    }
    Ok(())
}

pub(super) fn corpus_virtual_files_match(
    host: &topaz_interp::TestHost,
    path: &Path,
) -> std::io::Result<bool> {
    Ok(corpus_extract::read_virtual_file_sidecar(path)?
        .is_none_or(|expected| host.files().into_iter().eq(expected)))
}

pub(super) fn corpus_defer_errors_match(
    host: &topaz_interp::TestHost,
    path: &Path,
) -> std::io::Result<bool> {
    let Some(expected) = corpus_extract::read_optional_sidecar(path)? else {
        return Ok(true);
    };
    let actual = host.defer_errors();
    Ok(actual
        .iter()
        .map(String::as_str)
        .eq(expected.lines().filter(|line| !line.trim().is_empty())))
}

pub(super) fn check_v54_native_negative_row(
    dir: &Path,
    label: &str,
    language_version: LangVersion,
    fixture: &corpus_extract::V52Fixture,
) -> bool {
    let provider = PhysicalProvider::new(dir);
    let resolved = resolve_with_version(&provider, &fixture.file, None, language_version);
    if !resolved.diagnostics.is_empty() {
        eprintln!("{label}: did not resolve before native-negative");
        return false;
    }
    let unit = unit_modules(&resolved);
    let checked = topaz_check::check_unit_typed_with_version(&unit, language_version);
    if has_errors(&checked.diagnostics) {
        eprintln!("{label}: did not check before native-negative");
        return false;
    }
    let lowered = match topaz_lower::lower_checked(&resolved, &checked) {
        Ok(lowered) => lowered,
        Err(error) => {
            eprintln!("{label}: could not lower native-negative input — {error}");
            return false;
        }
    };
    let input = topaz_emit::NativeInput { unit: &lowered };
    match (
        fixture.result.as_str(),
        topaz_emit::emit_native_checked(&input),
    ) {
        ("error", Err(e)) if e.is_native_decline() => {
            if fixture.code.as_deref() == Some("TPZ6002") {
                true
            } else {
                eprintln!("{label}: native decline row must pin code TPZ6002");
                false
            }
        }
        ("error", Ok(_)) => {
            eprintln!("{label}: expected native decline, lowered natively");
            false
        }
        ("error", Err(e)) => {
            eprintln!("{label}: expected native decline, got {e}");
            false
        }
        (other, _) => {
            eprintln!("{label}: unsupported result `{other}` for native-negative");
            false
        }
    }
}

pub(super) fn check_v54_package_row(
    dir: &Path,
    label: &str,
    language_version: LangVersion,
    fixture: &corpus_extract::V52Fixture,
) -> bool {
    resolve_checked_v54_package_row(dir, label, language_version, fixture, "package-check")
        .is_some()
}

pub(super) fn check_v54_package_emit_row(
    dir: &Path,
    label: &str,
    language_version: LangVersion,
    fixture: &corpus_extract::V52Fixture,
    phase: &str,
    backend: Backend,
) -> bool {
    let Some((out, checked)) =
        resolve_checked_v54_package_row(dir, label, language_version, fixture, phase)
    else {
        return false;
    };
    match lower_checked_unit(&out, language_version, backend, Some(&checked)) {
        Ok(_) => true,
        Err(_) => {
            eprintln!("{label}: package did not emit");
            false
        }
    }
}

pub(super) fn check_v54_package_interpreted_row(
    dir: &Path,
    label: &str,
    language_version: LangVersion,
    fixture: &corpus_extract::V52Fixture,
    phase: &str,
) -> bool {
    if !matches!(fixture.result.as_str(), "ok" | "error") {
        eprintln!(
            "{label}: unsupported result `{}` for {phase}",
            fixture.result
        );
        return false;
    }
    let Some((target, out, _checked)) =
        resolve_checked_v54_package_target_row(dir, label, language_version, fixture)
    else {
        return false;
    };
    let Some(golden) = read_v54_package_stdout_sidecar(dir, label, fixture, phase) else {
        return false;
    };
    let host = topaz_interp::TestHost::new();
    host.set_extern_replay(target.extern_replay.clone());
    let Some(stdin) = read_v54_package_stdin_sidecar(dir, label, fixture) else {
        return false;
    };
    host.set_input(stdin);
    let Some(program_args) = read_v54_package_args_sidecar(dir, label, fixture) else {
        return false;
    };
    let invocation = ResolvedUnitInvocation::new(&out);
    if !program_args_are_admitted(invocation.has_explicit_main(), &program_args) {
        eprintln!("{label}: .args sidecar requires an explicit main");
        return false;
    }
    let outcome = invocation.run(&host, &program_args);
    let mut ok = match (fixture.result.as_str(), outcome) {
        ("ok", Ok(value)) => {
            let exit = explicit_main_exit(value, invocation.has_explicit_main());
            if exit == ExitCode::SUCCESS {
                true
            } else {
                eprintln!("{label}: explicit main returned a non-zero exit status");
                false
            }
        }
        ("error", Err(e)) => {
            let expected = fixture.code.as_deref().unwrap_or("");
            e.code == expected
                && topaz_interp::transcript::message_matches(&fixture.message, &e.message)
        }
        ("ok", Err(e)) => {
            eprintln!("{label}: unexpected stop {}: {}", e.code, e.message);
            false
        }
        ("error", Ok(_)) => {
            eprintln!("{label}: expected a stop, ran clean");
            false
        }
        (other, _) => {
            eprintln!("{label}: unsupported result `{other}` for {phase}");
            false
        }
    };
    let expected_stdout = topaz_interp::transcript::transcript_lines(&golden);
    if host.stdout() != expected_stdout {
        eprintln!("{label}: transcript mismatch");
        ok = false;
    }
    if !host.defer_errors().is_empty() {
        eprintln!("{label}: defer-error log was not empty");
        ok = false;
    }
    ok
}

pub(super) fn check_v54_package_build_run_row(
    dir: &Path,
    label: &str,
    language_version: LangVersion,
    fixture: &corpus_extract::V52Fixture,
) -> bool {
    if fixture.result != "ok" {
        eprintln!("{label}: package-build-run rows only support result `ok`");
        return false;
    }
    let Some((target, out, checked)) =
        resolve_checked_v54_package_target_row(dir, label, language_version, fixture)
    else {
        return false;
    };
    let Some(golden) = read_v54_package_stdout_sidecar(dir, label, fixture, "package-build-run")
    else {
        return false;
    };
    let Some(stdin) = read_v54_package_stdin_sidecar(dir, label, fixture) else {
        return false;
    };
    let Some(program_args) = read_v54_package_args_sidecar(dir, label, fixture) else {
        return false;
    };
    if !program_args_are_admitted(topaz_resolve::has_explicit_main(&out), &program_args) {
        eprintln!("{label}: .args sidecar requires an explicit main");
        return false;
    }
    let Some(stdout) =
        build_and_run_v54_package_for_corpus(label, &target, &out, &checked, &program_args, &stdin)
    else {
        return false;
    };
    if !topaz_interp::transcript::same_text(&golden, &stdout) {
        eprintln!("{label}: build-run stdout mismatch");
        return false;
    }
    true
}

pub(super) fn read_v54_package_stdout_sidecar(
    dir: &Path,
    label: &str,
    fixture: &corpus_extract::V52Fixture,
    phase: &str,
) -> Option<String> {
    let path = dir.join(format!("{}.stdout", fixture.dir));
    let Ok(golden) = fs::read_to_string(&path) else {
        eprintln!("{label}: missing {phase} .stdout sidecar");
        return None;
    };
    if fixture.result == "ok" && golden.trim().is_empty() {
        eprintln!("{label}: ok rows must pin meaningful output");
        return None;
    }
    Some(golden)
}

pub(super) fn read_v54_package_stdin_sidecar(
    dir: &Path,
    label: &str,
    fixture: &corpus_extract::V52Fixture,
) -> Option<String> {
    let path = dir.join(format!("{}.stdin", fixture.dir));
    match corpus_extract::read_optional_sidecar(&path) {
        Ok(input) => Some(input.unwrap_or_default()),
        Err(error) => {
            eprintln!("{label}: unreadable .stdin sidecar: {error}");
            None
        }
    }
}

pub(super) fn read_v54_package_args_sidecar(
    dir: &Path,
    label: &str,
    fixture: &corpus_extract::V52Fixture,
) -> Option<Vec<String>> {
    let path = dir.join(format!("{}.args", fixture.dir));
    match corpus_extract::read_optional_sidecar(&path) {
        Ok(Some(raw)) => Some(raw.lines().map(|line| line.to_string()).collect()),
        Ok(None) => Some(Vec::new()),
        Err(error) => {
            eprintln!("{label}: unreadable .args sidecar: {error}");
            None
        }
    }
}

pub(super) fn build_and_run_v54_package_for_corpus(
    label: &str,
    target: &PackageTarget,
    unit: &topaz_resolve::ResolveOutput,
    checked: &topaz_check::CheckedUnit,
    program_args: &[String],
    stdin: &str,
) -> Option<String> {
    let out_dir = match make_temp_root() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("{label}: could not create temp build dir: {e}");
            return None;
        }
    };
    let result = build_and_run_v54_package_for_corpus_inner(
        label,
        target,
        unit,
        checked,
        &out_dir,
        program_args,
        stdin,
    );
    let _ = fs::remove_dir_all(&out_dir);
    result.ok()
}

pub(super) fn build_and_run_v54_package_for_corpus_inner(
    label: &str,
    target: &PackageTarget,
    unit: &topaz_resolve::ResolveOutput,
    checked: &topaz_check::CheckedUnit,
    out_dir: &Path,
    program_args: &[String],
    stdin: &str,
) -> Result<String, ExitCode> {
    let rust = lower_resolved_package_with_report(
        target,
        unit,
        Some(checked),
        Backend::Native,
        None,
        "build",
        "native",
    )?
    .text;
    if let Err(e) = scaffold_crate(out_dir, &rust, package_harness(target)) {
        eprintln!(
            "{label}: could not write build-run crate to `{}`: {e}",
            out_dir.display()
        );
        return Err(ExitCode::FAILURE);
    }
    let env = prepare_build_env(out_dir)?;
    if let Err(code) = generate_lockfile(&env) {
        env.cleanup();
        return Err(code);
    }
    eprintln!(
        "topaz: compiling package `{}` for v5.4 corpus build-run (offline, locked, sanitized) …",
        target.entry
    );
    let mut cmd = env.cargo();
    cmd.args(["build", "--offline", "--locked", "--manifest-path"])
        .arg(&env.manifest);
    if let Err(code) = run_cargo_logged(&env, "build", cmd) {
        env.cleanup();
        return Err(code);
    }
    let bin = env
        .target
        .join("debug")
        .join(format!("program{}", std::env::consts::EXE_SUFFIX));
    let mut child = match std::process::Command::new(&bin)
        .args(program_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            env.cleanup();
            eprintln!(
                "{label}: built `{}` but could not run it: {e}",
                bin.display()
            );
            return Err(ExitCode::FAILURE);
        }
    };
    if let Some(mut child_stdin) = child.stdin.take()
        && let Err(e) = child_stdin.write_all(stdin.as_bytes())
    {
        env.cleanup();
        eprintln!("{label}: could not write build-run stdin: {e}");
        return Err(ExitCode::FAILURE);
    }
    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(e) => {
            env.cleanup();
            eprintln!("{label}: could not wait for `{}`: {e}", bin.display());
            return Err(ExitCode::FAILURE);
        }
    };
    env.cleanup();
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!(
            "{label}: build-run exited with {} (stdout: {:?}, stderr: {:?})",
            output.status, stdout, stderr
        );
        return Err(ExitCode::FAILURE);
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(super) fn check_v54_package_doc_row(
    dir: &Path,
    label: &str,
    language_version: LangVersion,
    fixture: &corpus_extract::V52Fixture,
) -> bool {
    if fixture.result != "ok" {
        eprintln!("{label}: package-doc rows only support result `ok`");
        return false;
    }
    let Some((target, out, checked)) =
        resolve_checked_v54_package_target_row(dir, label, language_version, fixture)
    else {
        return false;
    };
    let doc_comments = collect_doc_comments(&out);
    let exports_json = render_export_surface_json(&checked.exports);
    let index_md = render_docs_markdown(&target, &checked.exports, &doc_comments);

    let mut ok = true;
    let golden_dir = dir.join(format!("{}.docs", fixture.dir));
    let exports_path = golden_dir.join("exports.json");
    match fs::read_to_string(&exports_path) {
        Ok(expected) if expected == exports_json => {}
        Ok(_) => {
            eprintln!("{label}: exports.json golden mismatch");
            ok = false;
        }
        Err(e) => {
            eprintln!(
                "{label}: unreadable exports.json golden `{}`: {e}",
                exports_path.to_string_lossy()
            );
            ok = false;
        }
    }
    let index_path = golden_dir.join("index.md");
    match fs::read_to_string(&index_path) {
        Ok(expected) if expected == index_md => {}
        Ok(_) => {
            eprintln!("{label}: index.md golden mismatch");
            ok = false;
        }
        Err(e) => {
            eprintln!(
                "{label}: unreadable index.md golden `{}`: {e}",
                index_path.to_string_lossy()
            );
            ok = false;
        }
    }
    ok
}

pub(super) fn resolve_checked_v54_package_row(
    dir: &Path,
    label: &str,
    language_version: LangVersion,
    fixture: &corpus_extract::V52Fixture,
    phase: &str,
) -> Option<(topaz_resolve::ResolveOutput, topaz_check::CheckedUnit)> {
    if fixture.result != "ok" {
        eprintln!("{label}: {phase} rows only support result `ok`");
        return None;
    }
    resolve_checked_v54_package_target_row(dir, label, language_version, fixture)
        .map(|(_, out, checked)| (out, checked))
}

pub(super) fn resolve_checked_v54_package_target_row(
    dir: &Path,
    label: &str,
    language_version: LangVersion,
    fixture: &corpus_extract::V52Fixture,
) -> Option<(
    PackageTarget,
    topaz_resolve::ResolveOutput,
    topaz_check::CheckedUnit,
)> {
    let package_root = dir.join(&fixture.dir);
    let package_root = package_root.to_string_lossy().into_owned();
    let locked = fixture.status == corpus_extract::V52FixtureStatus::Locked;
    let target = match package_target(Some(&package_root), Some(language_version), locked) {
        Ok(target) => target,
        Err(_) => return None,
    };
    let out = resolve_package_target(&target);
    if has_errors(&out.diagnostics) {
        eprintln!("{label}: package did not resolve");
        return None;
    }
    match check_resolved_unit(&out, false, target.version) {
        Ok(checked) => Some((target, out, checked)),
        Err(n) => {
            eprintln!(
                "{label}: package produced {n} type diagnostic{}",
                if n == 1 { "" } else { "s" }
            );
            None
        }
    }
}

pub(super) fn corpus_diagnostics_match(
    label: &str,
    result: &str,
    code: Option<&str>,
    message: &str,
    diagnostics: &[Diagnostic],
) -> bool {
    match result {
        "ok" => {
            if diagnostics.is_empty() {
                true
            } else {
                let first = &diagnostics[0];
                eprintln!(
                    "{label}: expected ok, got {}: {}",
                    first.code.as_str(),
                    first.message
                );
                false
            }
        }
        "error" => {
            let Some(first) = diagnostics.first() else {
                eprintln!("{label}: expected an error, got ok");
                return false;
            };
            let expected = code.unwrap_or("");
            if first.code.as_str() != expected {
                eprintln!(
                    "{label}: expected primary {expected}, got {}",
                    first.code.as_str()
                );
                return false;
            }
            if !message.is_empty() && !first.message.contains(message) {
                eprintln!(
                    "{label}: expected message containing `{message}`, got `{}`",
                    first.message
                );
                return false;
            }
            true
        }
        other => {
            eprintln!("{label}: unsupported result `{other}`");
            false
        }
    }
}

pub(super) fn tpz_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out: Vec<_> = fs::read_dir(dir)
        .expect("fixture directory exists")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "tpz"))
        .collect();
    out.sort();
    out
}
