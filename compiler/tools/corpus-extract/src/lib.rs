//! Deterministic fence extraction and classification for the v5.1
//! golden parse corpus (CDR-001 §7).
//!
//! Inputs (vendored, read-only): `spec/v5.1/EXAMPLES.md`,
//! `spec/v5.1/SPEC.md`, and the site documentation snapshot under
//! `corpus/v5.1/site/source/{en,ko,ru}/docs/`. Outputs (owned by this
//! tool, regenerated wholesale): `corpus/v5.1/examples/`,
//! `corpus/v5.1/spec/`, `corpus/v5.1/site/{en,ko,ru}/`, and the three
//! `MANIFEST.toml` files. Every fence in every input gets a manifest
//! row; `.tpz` corpus files are written for `topaz` fences only.
//!
//! Classification (CDR-001 §7):
//! - EXAMPLES entries titled `(Snippet)` are `snippet`, the rest are
//!   `program`; both are the positive parse gate.
//! - SPEC `topaz` fences inside §22 are `signature_notation` —
//!   declaration-style notation, not Program grammar — and skipped.
//! - Site `topaz` fences are `snippet` (documentation snippets with
//!   ambient names); fences on archival legacy-guide routes are
//!   `historical` and skipped.
//! - Non-`topaz` fences are `excluded` and skipped, but inventoried.

use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const SITE_LOCALES: [&str; 3] = ["en", "ko", "ru"];

/// Archival legacy-guide routes (the site's "Legacy Guides" nav
/// group): historical pages kept off the canonical learning path.
/// Their `topaz` fences, if any, are excluded from the v5.1 corpus.
pub const ARCHIVAL_ROUTES: [&str; 3] = [
    "guides/recursion",
    "guides/understanding-closures",
    "guides/using-map",
];

const SITE_PROVENANCE: &str = "\
[provenance]
language_version     = \"5.1\"
ssot_commit          = \"a0db8c8\"
site_content_commit  = \"aaa8a12\"   # topaz.ooo documentation snapshot (2026-06-12)
fence_count_expected = 558           # `topaz` fences: 186 each for en, ko, ru
";

/// One fenced code block found in a source document.
#[derive(Debug)]
pub struct Fence {
    /// Repo-relative source path, forward slashes.
    pub source: String,
    /// Nearest preceding heading (frontmatter title before the first
    /// heading).
    pub section: String,
    /// 1-based ordinal among all fences in the source file.
    pub index: usize,
    /// First word of the fence info string; empty when untagged.
    pub language: String,
    /// Verbatim body, LF-normalized.
    pub body: String,
}

/// A classified manifest row.
#[derive(Debug)]
pub struct Row {
    pub fence: Fence,
    pub mode: &'static str,
    pub expect: &'static str,
    pub reason: Option<&'static str>,
    /// Corpus-relative `.tpz` path, for `topaz` fences.
    pub file: Option<String>,
}

/// Everything the extractor produces: output files (path relative to
/// the repo root → content) plus the classified rows for inspection.
pub struct Generated {
    pub files: BTreeMap<String, String>,
    pub rows: Vec<Row>,
}

/// Directories owned (purged and regenerated) by this tool, relative
/// to the repo root. `corpus/v5.1/site/source/` is vendored input and
/// is never touched.
pub fn owned_dirs() -> Vec<String> {
    let mut dirs = vec![
        "corpus/v5.1/examples".to_string(),
        "corpus/v5.1/spec".to_string(),
    ];
    for locale in SITE_LOCALES {
        dirs.push(format!("corpus/v5.1/site/{locale}"));
    }
    dirs
}

// ---- fence scanning ---------------------------------------------------

// ---- corpus/v5.2 manifests (CDR-002 SS3) ---------------------------

/// Totals header of a `corpus/v5.2` area manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct V52Totals {
    pub recorded: usize,
    pub runnable: usize,
    pub pending: usize,
}

/// Exact activation or package-lock disposition admitted from a corpus
/// fixture's optional `status` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V52FixtureStatus {
    Unspecified,
    Runnable,
    Pending,
    Locked,
}

/// One fixture row of a `corpus/v5.2` area manifest.
#[derive(Debug, Clone)]
pub struct V52Fixture {
    pub file: String,
    /// Directory-fixture fields (`corpus/v5.2/modules/`): the fixture
    /// directory and its `ENTRY`-named entry file.
    pub dir: String,
    pub entry: String,
    /// Recorded expectation text for directory fixtures.
    pub expect: String,
    /// Owning product phase, such as `parse`, `resolve`, `check`, or
    /// `exec`; newer corpus families add their own admitted phases.
    pub phase: String,
    /// Comma-separated expected module processing order (the
    /// normative ADR-078 sequence) for runnable resolve rows;
    /// empty = unasserted.
    pub order: String,
    /// Comma-separated expected module identities (sorted) for
    /// runnable resolve rows; empty = unasserted.
    pub modules: String,
    /// Comma-separated paths that must NOT be read during
    /// resolution (negative-space assertions); empty = unasserted.
    pub not_read: String,
    /// Directory-fixture activation (`runnable` or `pending`) or package
    /// dependency admission (`locked`); omitted when neither applies.
    pub status: V52FixtureStatus,
    /// `5.1`, `5.2`, `5.3`, `5.4`, or `both` (both = parse under 5.1 and
    /// 5.2 and require structurally identical ASTs — the same-shape class).
    pub versions: String,
    /// `ok` or `error`.
    pub result: String,
    /// Expected primary diagnostic code when `result = "error"`.
    pub code: Option<String>,
    /// Exec-phase rows: expected message substring for stops.
    pub message: String,
    /// Exec-phase rows: TestHost virtual-clock tick per poll
    /// (string-encoded integer; empty = frozen clock).
    pub clock_tick: String,
}

/// Reads one directory-shaped corpus fixture exactly. Every directory entry,
/// path, and file must be readable; `ENTRY` must agree with the manifest and
/// name a file present in the admitted tree. The marker itself is omitted from
/// the returned source/support files.
pub fn read_fixture_directory(
    base: &Path,
    expected_entry: &str,
) -> io::Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![base.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).map_err(|error| path_io_error(&dir, error))?;
        for entry in entries {
            let entry = entry.map_err(|error| path_io_error(&dir, error))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| path_io_error(&path, error))?;
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if !file_type.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}: unsupported fixture entry type", path.display()),
                ));
            }
            let relative_path = path
                .strip_prefix(base)
                .expect("walked fixture path stays below its root")
                .to_str()
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{}: fixture path is not valid UTF-8", path.display()),
                    )
                })?
                .replace('\\', "/");
            let contents =
                fs::read_to_string(&path).map_err(|error| path_io_error(&path, error))?;
            match files.entry(relative_path) {
                Entry::Vacant(entry) => {
                    entry.insert(contents);
                }
                Entry::Occupied(entry) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "{}: duplicate normalized fixture path `{}`",
                            base.display(),
                            entry.key()
                        ),
                    ));
                }
            }
        }
    }

    let marker = files.remove("ENTRY").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: missing ENTRY marker", base.display()),
        )
    })?;
    if marker.trim() != expected_entry {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{}: ENTRY names `{}`, manifest names `{expected_entry}`",
                base.display(),
                marker.trim()
            ),
        ));
    }
    if !files.contains_key(expected_entry) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{}: entry file `{expected_entry}` is missing",
                base.display()
            ),
        ));
    }
    Ok(files)
}

fn path_io_error(path: &Path, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}

/// Reads an optional corpus sidecar. Absence is admitted, but an existing
/// sidecar must be readable UTF-8 rather than silently disappearing on an I/O
/// or decoding failure.
pub fn read_optional_sidecar(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(path_io_error(path, error)),
    }
}

/// Reads the optional `path = contents` sidecar used to seed or observe the
/// corpus TestHost virtual filesystem. Every non-empty row must have a path,
/// the delimiter, and a unique path. Rows remain in sidecar order so final-file
/// observations retain their canonical-order check.
pub fn read_virtual_file_sidecar(path: &Path) -> io::Result<Option<Vec<(String, String)>>> {
    let Some(contents) = read_optional_sidecar(path)? else {
        return Ok(None);
    };
    parse_virtual_file_sidecar(path, &contents).map(Some)
}

fn parse_virtual_file_sidecar(path: &Path, contents: &str) -> io::Result<Vec<(String, String)>> {
    let mut files = Vec::new();
    let mut paths = BTreeSet::new();
    for (line_index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let line_number = line_index + 1;
        let Some((raw_path, contents)) = line.split_once(" = ") else {
            return Err(sidecar_error(
                path,
                line_number,
                "virtual-file row must use `path = contents`",
            ));
        };
        let virtual_path = raw_path.trim();
        if virtual_path.is_empty() {
            return Err(sidecar_error(
                path,
                line_number,
                "virtual-file path must not be empty",
            ));
        }
        if !paths.insert(virtual_path.to_string()) {
            return Err(sidecar_error(
                path,
                line_number,
                &format!("duplicate virtual-file path `{virtual_path}`"),
            ));
        }
        files.push((virtual_path.to_string(), contents.to_string()));
    }
    Ok(files)
}

fn sidecar_error(path: &Path, line_number: usize, message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{}:{line_number}: {message}", path.display()),
    )
}

/// Reads a corpus `MANIFEST.toml`. The format is the narrow TOML subset this
/// repository writes: one internally consistent `[totals]` table followed by
/// `[[fixture]]` tables with one source identity and explicit phase, version,
/// and `ok` or coded-`error` outcome. Values use quoted strings, totals use bare
/// integers, and comments occupy full lines.
pub fn read_v52_manifest(path: &Path) -> io::Result<(V52Totals, Vec<V52Fixture>)> {
    let text = fs::read_to_string(path)?;
    parse_v52_manifest(path, &text)
}

fn parse_v52_manifest(path: &Path, text: &str) -> io::Result<(V52Totals, Vec<V52Fixture>)> {
    let mut recorded = None;
    let mut runnable = None;
    let mut pending = None;
    let mut fixtures: Vec<V52Fixture> = Vec::new();
    let mut fixture_lines = Vec::new();
    let mut section = "";
    let mut saw_totals = false;
    let mut fixture_keys = std::collections::BTreeSet::new();
    for (line_index, raw) in text.lines().enumerate() {
        let line_number = line_index + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[totals]" {
            if saw_totals || !fixtures.is_empty() {
                return Err(manifest_error(
                    path,
                    line_number,
                    "[totals] must occur exactly once before every fixture",
                ));
            }
            saw_totals = true;
            section = "totals";
            continue;
        }
        if line == "[[fixture]]" {
            if !saw_totals {
                return Err(manifest_error(
                    path,
                    line_number,
                    "fixture occurs before [totals]",
                ));
            }
            section = "fixture";
            fixture_keys.clear();
            fixture_lines.push(line_number);
            fixtures.push(V52Fixture {
                file: String::new(),
                dir: String::new(),
                entry: String::new(),
                expect: String::new(),
                order: String::new(),
                modules: String::new(),
                not_read: String::new(),
                phase: String::new(),
                status: V52FixtureStatus::Unspecified,
                message: String::new(),
                clock_tick: String::new(),
                versions: String::new(),
                result: String::new(),
                code: None,
            });
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(manifest_error(
                path,
                line_number,
                &format!("unparseable line: {raw}"),
            ));
        };
        let key = key.trim();
        match (section, key) {
            ("totals", "recorded") => {
                set_manifest_count(path, line_number, key, value, &mut recorded)?
            }
            ("totals", "runnable") => {
                set_manifest_count(path, line_number, key, value, &mut runnable)?
            }
            ("totals", "pending") => {
                set_manifest_count(path, line_number, key, value, &mut pending)?
            }
            (
                "fixture",
                key @ ("file" | "dir" | "entry" | "expect" | "phase" | "status" | "modules"
                | "order" | "not_read" | "versions" | "result" | "code" | "message"
                | "clock_tick"),
            ) => {
                if !fixture_keys.insert(key) {
                    return Err(manifest_error(
                        path,
                        line_number,
                        &format!("duplicate fixture key `{key}`"),
                    ));
                }
                let value = manifest_string(path, line_number, key, value)?;
                let fixture = fixtures.last_mut().ok_or_else(|| {
                    manifest_error(path, line_number, "fixture key occurs before [[fixture]]")
                })?;
                match key {
                    "file" => fixture.file = value,
                    "dir" => fixture.dir = value,
                    "entry" => fixture.entry = value,
                    "expect" => fixture.expect = value,
                    "phase" => fixture.phase = value,
                    "status" => {
                        fixture.status = match value.as_str() {
                            "" => V52FixtureStatus::Unspecified,
                            "runnable" => V52FixtureStatus::Runnable,
                            "pending" => V52FixtureStatus::Pending,
                            "locked" => V52FixtureStatus::Locked,
                            status => {
                                return Err(manifest_error(
                                    path,
                                    line_number,
                                    &format!(
                                        "fixture status must be `runnable`, `pending`, or `locked`, got `{status}`"
                                    ),
                                ));
                            }
                        };
                    }
                    "modules" => fixture.modules = value,
                    "order" => fixture.order = value,
                    "not_read" => fixture.not_read = value,
                    "versions" => fixture.versions = value,
                    "result" => fixture.result = value,
                    "code" => fixture.code = Some(value),
                    "message" => fixture.message = value,
                    "clock_tick" => {
                        if !value.is_empty() && value.parse::<u64>().is_err() {
                            return Err(manifest_error(
                                path,
                                line_number,
                                "clock_tick must be an unsigned integer string",
                            ));
                        }
                        fixture.clock_tick = value;
                    }
                    _ => unreachable!("admitted fixture key"),
                }
            }
            _ => {
                return Err(manifest_error(
                    path,
                    line_number,
                    &format!("unknown key `{key}` in [{section}]"),
                ));
            }
        }
    }

    let required_total = |key: &str, value: Option<usize>| {
        value.ok_or_else(|| manifest_error(path, 0, &format!("missing [totals].{key}")))
    };
    let totals = V52Totals {
        recorded: required_total("recorded", recorded)?,
        runnable: required_total("runnable", runnable)?,
        pending: required_total("pending", pending)?,
    };
    let Some(classified) = totals.runnable.checked_add(totals.pending) else {
        return Err(manifest_error(
            path,
            0,
            "[totals] runnable + pending overflows usize",
        ));
    };
    if totals.recorded != fixtures.len() || classified != totals.recorded {
        return Err(manifest_error(
            path,
            0,
            &format!(
                "[totals] records {} fixtures as {} runnable + {} pending, but the manifest contains {} fixture rows",
                totals.recorded,
                totals.runnable,
                totals.pending,
                fixtures.len()
            ),
        ));
    }
    for (fixture, line_number) in fixtures.iter().zip(fixture_lines) {
        validate_v52_fixture(path, line_number, fixture)?;
    }
    Ok((totals, fixtures))
}

fn validate_v52_fixture(path: &Path, line_number: usize, fixture: &V52Fixture) -> io::Result<()> {
    for (key, value) in [
        ("phase", fixture.phase.as_str()),
        ("versions", fixture.versions.as_str()),
        ("result", fixture.result.as_str()),
    ] {
        if value.is_empty() {
            return Err(manifest_error(
                path,
                line_number,
                &format!("fixture key `{key}` is required and must not be empty"),
            ));
        }
    }

    match (fixture.file.is_empty(), fixture.dir.is_empty()) {
        (false, true) if fixture.entry.is_empty() => {}
        (true, false) => {}
        (false, true) => {
            return Err(manifest_error(
                path,
                line_number,
                "fixture `entry` requires a `dir` identity",
            ));
        }
        _ => {
            return Err(manifest_error(
                path,
                line_number,
                "fixture must name exactly one non-empty `file` or `dir` identity",
            ));
        }
    }

    match (fixture.result.as_str(), fixture.code.as_deref()) {
        ("ok", None) => {}
        ("ok", Some(_)) => {
            return Err(manifest_error(
                path,
                line_number,
                "an `ok` fixture must not declare `code`",
            ));
        }
        ("error", Some(code)) if !code.is_empty() => {}
        ("error", _) => {
            return Err(manifest_error(
                path,
                line_number,
                "an `error` fixture requires a non-empty `code`",
            ));
        }
        (result, _) => {
            return Err(manifest_error(
                path,
                line_number,
                &format!("fixture result must be `ok` or `error`, got `{result}`"),
            ));
        }
    }
    Ok(())
}

fn set_manifest_count(
    path: &Path,
    line_number: usize,
    key: &str,
    raw: &str,
    slot: &mut Option<usize>,
) -> io::Result<()> {
    if slot.is_some() {
        return Err(manifest_error(
            path,
            line_number,
            &format!("duplicate [totals] key `{key}`"),
        ));
    }
    let value = raw.trim().parse::<usize>().map_err(|_| {
        manifest_error(
            path,
            line_number,
            &format!("[totals].{key} must be an unsigned integer"),
        )
    })?;
    *slot = Some(value);
    Ok(())
}

fn manifest_string(path: &Path, line_number: usize, key: &str, raw: &str) -> io::Result<String> {
    let raw = raw.trim();
    let Some(value) = raw
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return Err(manifest_error(
            path,
            line_number,
            &format!("fixture key `{key}` must have a quoted string value"),
        ));
    };
    if value.contains('"') {
        return Err(manifest_error(
            path,
            line_number,
            &format!("fixture key `{key}` contains an unsupported quote"),
        ));
    }
    Ok(value.to_string())
}

fn manifest_error(path: &Path, line_number: usize, message: &str) -> io::Error {
    let location = if line_number == 0 {
        path.display().to_string()
    } else {
        format!("{}:{line_number}", path.display())
    };
    io::Error::new(io::ErrorKind::InvalidData, format!("{location}: {message}"))
}

#[cfg(test)]
mod manifest_tests {
    use super::{
        parse_v52_manifest, read_fixture_directory, read_optional_sidecar,
        read_virtual_file_sidecar,
    };
    use std::io;
    use std::path::Path;

    const EMPTY_MANIFEST: &str = "[totals]\nrecorded = 0\nrunnable = 0\npending = 0\n";

    #[test]
    fn admits_an_explicit_empty_manifest() {
        let (totals, fixtures) =
            parse_v52_manifest(Path::new("MANIFEST.toml"), EMPTY_MANIFEST).expect("manifest");
        assert_eq!(totals.recorded, 0);
        assert_eq!(totals.runnable, 0);
        assert_eq!(totals.pending, 0);
        assert!(fixtures.is_empty());
    }

    #[test]
    fn rejects_missing_malformed_and_duplicate_totals() {
        for text in [
            "",
            "[totals]\nrecorded = 0\nrunnable = 0\n",
            "[totals]\nrecorded = many\nrunnable = 0\npending = 0\n",
            "[totals]\nrecorded = 0\nrecorded = 0\nrunnable = 0\npending = 0\n",
        ] {
            assert!(parse_v52_manifest(Path::new("MANIFEST.toml"), text).is_err());
        }
    }

    #[test]
    fn rejects_duplicate_fixture_keys() {
        let text = format!("{EMPTY_MANIFEST}[[fixture]]\nfile = \"a.tpz\"\nfile = \"b.tpz\"\n");
        assert!(parse_v52_manifest(Path::new("MANIFEST.toml"), &text).is_err());
    }

    #[test]
    fn rejects_a_missing_fixture_directory() {
        let missing = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("__missing_fixture_directory_for_admission_test__");
        let error = read_fixture_directory(&missing, "main.tpz").expect_err("missing fixture");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn rejects_unreadable_and_malformed_optional_sidecars() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(read_optional_sidecar(directory).is_err());

        let missing_delimiter = directory.join("missing-delimiter.files-in");
        assert!(super::parse_virtual_file_sidecar(&missing_delimiter, "config.txt").is_err());
        let duplicate_path = directory.join("duplicate-path.files-in");
        assert!(
            super::parse_virtual_file_sidecar(
                &duplicate_path,
                "config.txt = first\nconfig.txt = second\n",
            )
            .is_err()
        );

        let missing = directory.join("__missing_optional_sidecar_for_admission_test__");
        assert_eq!(read_virtual_file_sidecar(&missing).unwrap(), None);
    }
}

/// The `corpus/v5.2` areas, in harness order.
/// `corpus/exec/` areas (CDR-003 §11). The `modules` exec rows live
/// in `corpus/v5.2/modules/` (phase = "exec").
/// Single source of truth for the single-file exec areas AND their
/// fixture counts. The CLI gate and the interp corpus test both
/// consume this table — a new area or count changes exactly one row.
pub const EXEC_EXPECTED: [(&str, usize); 13] = [
    ("values", 3),
    ("control", 4),
    ("functions-closures", 2),
    ("collections", 3),
    ("strings", 3),
    ("records", 2),
    ("faults", 4),
    ("guards", 3),
    ("defer", 3),
    ("concurrent", 2),
    ("files", 1),
    ("templates", 2),
    ("examples", 8),
];

/// Returns the canonical source directory for one single-file exec area.
/// Example transcripts exercise the generated v5.1 example programs directly;
/// every other area owns its source beside the transcript sidecars.
pub fn exec_source_dir(root: &Path, area: &str) -> PathBuf {
    if area == "examples" {
        root.join("corpus/v5.1/examples")
    } else {
        root.join("corpus/exec").join(area)
    }
}

/// Frozen v5.1 corpus cardinalities. The parser tests and the official CLI
/// corpus gate consume these values directly so fixture coverage has one
/// owner.
pub const V51_PARSE_OK_EXPECTED: usize = 610;
pub const V51_LAYOUT_EXPECTED: usize = 10;
pub const V51_INVALID_EXPECTED: usize = 14;

/// Frozen v5.2 corpus areas and cardinalities, shared by the parser tests and
/// the official CLI corpus gate.
pub const V52_EXPECTED: [(&str, usize); 6] = [
    ("syntax", 9),
    ("layout", 4),
    ("compat/same-shape", 7),
    ("compat/non-module-diagnostic", 5),
    ("compat/module-eligible", 9),
    ("modules", 137),
];

/// Scans one markdown/MDX document for fenced code blocks. Tracks the
/// nearest heading for section names; MDX frontmatter `title:` seeds
/// the section before the first heading. Lines inside fences never
/// count as headings, and escaped backticks (the EXAMPLES format
/// template) never open a fence.
pub fn scan_fences(source: &str, text: &str) -> Vec<Fence> {
    let mut fences = Vec::new();
    let mut section = String::new();
    let mut in_frontmatter = false;
    let mut open: Option<(String, Vec<String>, usize)> = None;
    let mut index = 0usize;

    for (i, line) in text.lines().enumerate() {
        if i == 0 && line.trim_end() == "---" {
            in_frontmatter = true;
            continue;
        }
        if in_frontmatter {
            if line.trim_end() == "---" {
                in_frontmatter = false;
            } else if let Some(title) = line.strip_prefix("title:")
                && section.is_empty()
            {
                section = title
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
            }
            continue;
        }
        if let Some((language, body, idx)) = open.as_mut() {
            if line.trim_end() == "```" {
                fences.push(Fence {
                    source: source.to_string(),
                    section: section.clone(),
                    index: *idx,
                    language: std::mem::take(language),
                    body: body.join("\n"),
                });
                open = None;
            } else {
                body.push(line.to_string());
            }
            continue;
        }
        if let Some(info) = line.strip_prefix("```") {
            index += 1;
            let language = info.split_whitespace().next().unwrap_or("").to_string();
            open = Some((language, Vec::new(), index));
            continue;
        }
        if line.starts_with('#') {
            let text = line.trim_start_matches('#').trim();
            if !text.is_empty() {
                section = text.to_string();
            }
        }
    }
    fences
}

// ---- classification ---------------------------------------------------

#[derive(Debug, Clone)]
pub enum SourceKind {
    Examples,
    Spec,
    Site { route: String },
}

fn classify(
    kind: &SourceKind,
    fence: &Fence,
) -> (&'static str, &'static str, Option<&'static str>) {
    if fence.language != "topaz" {
        return ("excluded", "skip", Some("not a `topaz` fence"));
    }
    match kind {
        SourceKind::Examples => {
            if fence.section.contains("(Snippet)") {
                ("snippet", "parse_ok", None)
            } else {
                ("program", "parse_ok", None)
            }
        }
        SourceKind::Spec => {
            if fence.section.starts_with("§22") {
                (
                    "signature_notation",
                    "skip",
                    Some("§22 signatures use declaration-style notation, not Program grammar"),
                )
            } else {
                ("snippet", "parse_ok", None)
            }
        }
        SourceKind::Site { route } => {
            if ARCHIVAL_ROUTES.contains(&route.as_str()) {
                (
                    "historical",
                    "skip",
                    Some(
                        "archival legacy-guide page; historical forms are excluded from the v5.1 corpus",
                    ),
                )
            } else {
                ("snippet", "parse_ok", None)
            }
        }
    }
}

// ---- generation ---------------------------------------------------------

/// Regenerates the whole corpus in memory from the vendored sources
/// under `root` (the repository root).
pub fn generate(root: &Path) -> io::Result<Generated> {
    let mut files = BTreeMap::new();
    let mut rows = Vec::new();

    // EXAMPLES.md → corpus/v5.1/examples/NNN.tpz
    let examples = read(root, "spec/v5.1/EXAMPLES.md")?;
    let mut example_rows = Vec::new();
    for fence in scan_fences("spec/v5.1/EXAMPLES.md", &examples) {
        let file = (fence.language == "topaz")
            .then(|| format!("corpus/v5.1/examples/{:03}.tpz", fence.index));
        example_rows.push(make_row(&SourceKind::Examples, fence, file, &mut files));
    }
    files.insert(
        "corpus/v5.1/examples/MANIFEST.toml".to_string(),
        manifest("corpus/v5.1/examples", None, &example_rows),
    );
    rows.extend(example_rows);

    // SPEC.md → corpus/v5.1/spec/NN.tpz
    let spec = read(root, "spec/v5.1/SPEC.md")?;
    let mut spec_rows = Vec::new();
    for fence in scan_fences("spec/v5.1/SPEC.md", &spec) {
        let file =
            (fence.language == "topaz").then(|| format!("corpus/v5.1/spec/{:02}.tpz", fence.index));
        spec_rows.push(make_row(&SourceKind::Spec, fence, file, &mut files));
    }
    files.insert(
        "corpus/v5.1/spec/MANIFEST.toml".to_string(),
        manifest("corpus/v5.1/spec", None, &spec_rows),
    );
    rows.extend(spec_rows);

    // site/source/{locale}/docs/**.mdx → corpus/v5.1/site/{locale}/...
    let mut site_rows = Vec::new();
    for locale in SITE_LOCALES {
        let base = format!("corpus/v5.1/site/source/{locale}/docs");
        for path in mdx_files(&root.join(&base))? {
            let rel = path
                .strip_prefix(root)
                .expect("walked under root")
                .to_string_lossy()
                .replace('\\', "/");
            let route = rel
                .strip_prefix(&format!("{base}/"))
                .and_then(|r| r.strip_suffix(".mdx"))
                .expect("mdx under the locale docs dir")
                .to_string();
            let kind = SourceKind::Site {
                route: route.clone(),
            };
            let text = fs::read_to_string(&path)?;
            for fence in scan_fences(&rel, &text) {
                let file = (fence.language == "topaz").then(|| {
                    format!(
                        "corpus/v5.1/site/{locale}/{}/{:02}.tpz",
                        route.replace('/', "-"),
                        fence.index
                    )
                });
                site_rows.push(make_row(&kind, fence, file, &mut files));
            }
        }
    }
    files.insert(
        "corpus/v5.1/site/MANIFEST.toml".to_string(),
        manifest("corpus/v5.1/site", Some(SITE_PROVENANCE), &site_rows),
    );
    rows.extend(site_rows);

    Ok(Generated { files, rows })
}

fn make_row(
    kind: &SourceKind,
    fence: Fence,
    file: Option<String>,
    files: &mut BTreeMap<String, String>,
) -> Row {
    let (mode, expect, reason) = classify(kind, &fence);
    if let Some(path) = &file {
        let mut content = fence.body.clone();
        if !content.is_empty() {
            content.push('\n');
        }
        files.insert(path.clone(), content);
    }
    Row {
        fence,
        mode,
        expect,
        reason,
        file,
    }
}

fn read(root: &Path, rel: &str) -> io::Result<String> {
    fs::read_to_string(root.join(rel))
}

/// All `.mdx` files under `dir`, sorted for determinism.
fn mdx_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "mdx") {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

// ---- manifest writing ----------------------------------------------------

fn toml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn manifest(corpus_dir: &str, provenance: Option<&str>, rows: &[Row]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# {corpus_dir} fence-classification manifest (CDR-001 §7).\n\
         #\n\
         # Generated by tools/corpus-extract — do not edit by hand; CI\n\
         # re-runs the extractor and fails on drift. One row per fence\n\
         # in the sources; `.tpz` corpus files exist for `topaz` fences.\n"
    ));
    if let Some(p) = provenance {
        out.push_str("#\n# Site sources are vendored under corpus/v5.1/site/source/ at\n# the recorded content commit.\n\n");
        out.push_str(p);
    }
    for row in rows {
        out.push_str("\n[[fence]]\n");
        out.push_str(&format!(
            "source   = \"{}\"\n",
            toml_escape(&row.fence.source)
        ));
        out.push_str(&format!(
            "section  = \"{}\"\n",
            toml_escape(&row.fence.section)
        ));
        out.push_str(&format!("index    = {}\n", row.fence.index));
        out.push_str(&format!(
            "language = \"{}\"\n",
            toml_escape(&row.fence.language)
        ));
        out.push_str(&format!("mode     = \"{}\"\n", row.mode));
        out.push_str(&format!("expect   = \"{}\"\n", row.expect));
        if let Some(reason) = row.reason {
            out.push_str(&format!("reason   = \"{}\"\n", toml_escape(reason)));
        }
        if let Some(file) = &row.file {
            out.push_str(&format!("file     = \"{}\"\n", toml_escape(file)));
        }
    }
    out
}

// ---- drift check -----------------------------------------------------------

/// Compares a fresh in-memory generation against the committed corpus.
/// Returns human-readable problems; empty means no drift. Line endings
/// are normalized so checkout style cannot fail the comparison.
pub fn drift(root: &Path) -> io::Result<Vec<String>> {
    let generated = generate(root)?;
    let mut problems = Vec::new();

    for (rel, content) in &generated.files {
        match fs::read_to_string(root.join(rel)) {
            Ok(on_disk) => {
                if normalize(&on_disk) != normalize(content) {
                    problems.push(format!("differs: {rel}"));
                }
            }
            Err(_) => problems.push(format!("missing: {rel}")),
        }
    }
    // Stray files in owned directories (renamed/removed fences).
    for dir in owned_dirs() {
        let abs = root.join(&dir);
        if !abs.exists() {
            continue;
        }
        let mut stack = vec![abs];
        while let Some(d) = stack.pop() {
            for entry in fs::read_dir(&d)? {
                let path = entry?.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    let rel = path
                        .strip_prefix(root)
                        .expect("walked under root")
                        .to_string_lossy()
                        .replace('\\', "/");
                    if !generated.files.contains_key(&rel) {
                        problems.push(format!("stray: {rel}"));
                    }
                }
            }
        }
    }
    Ok(problems)
}

fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// The repository root, resolved from this crate's manifest dir.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root exists")
}
