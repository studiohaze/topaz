//! Phase C-6 witnesses: module-aware unit checking (SPEC §17) —
//! exported signatures consumed by importers, namespace member
//! typing, qualified/imported type aliases, the TPZ5002 graduation
//! (closed unit name space), and TPZ5025 qualified-type errors.

use topaz_check::{UnitModule, check_unit, check_unit_typed_with_version, check_unit_with_version};
use topaz_diag::FileId;
use topaz_parser::{ParseOptions, parse_with_options};
use topaz_syntax::LangVersion;
use topaz_syntax::ast::Program;

fn parse(file: u32, src: &str) -> Program {
    parse_at(file, src, LangVersion::V5_2)
}

fn parse_at(file: u32, src: &str, version: LangVersion) -> Program {
    let out = parse_with_options(
        FileId(file),
        src,
        ParseOptions {
            language_version: version,
        },
    );
    assert!(out.diagnostics.is_empty(), "test source must parse: {src}");
    out.program
}

fn check(unit: &[(&str, &str)]) -> Vec<String> {
    let programs: Vec<Program> = unit
        .iter()
        .enumerate()
        .map(|(i, (_, src))| parse(i as u32, src))
        .collect();
    let modules: Vec<UnitModule> = unit
        .iter()
        .zip(programs.iter())
        .enumerate()
        .map(|(i, ((identity, src), program))| UnitModule {
            identity: (*identity).to_string(),
            is_entry: i == 0,
            is_extern: false,
            is_generated_std: false,
            extern_replay_error: None,
            src,
            program,
        })
        .collect();
    check_unit(&modules)
        .diagnostics
        .iter()
        .map(|d| format!("{} {}", d.code.as_str(), d.message))
        .collect()
}

fn check_output_with_version(
    unit: &[(&str, &str)],
    version: LangVersion,
) -> topaz_check::CheckOutput {
    let programs: Vec<Program> = unit
        .iter()
        .enumerate()
        .map(|(i, (_, src))| parse_at(i as u32, src, version))
        .collect();
    let modules: Vec<UnitModule> = unit
        .iter()
        .zip(programs.iter())
        .enumerate()
        .map(|(i, ((identity, src), program))| UnitModule {
            identity: (*identity).to_string(),
            is_entry: i == 0,
            is_extern: false,
            is_generated_std: false,
            extern_replay_error: None,
            src,
            program,
        })
        .collect();
    check_unit_with_version(&modules, version)
}

fn typed_output_with_version(
    unit: &[(&str, &str)],
    version: LangVersion,
) -> topaz_check::CheckedUnit {
    let programs = unit
        .iter()
        .enumerate()
        .map(|(i, (_, src))| parse_at(i as u32, src, version))
        .collect::<Vec<_>>();
    let modules = unit
        .iter()
        .zip(programs.iter())
        .enumerate()
        .map(|(i, ((identity, src), program))| UnitModule {
            identity: (*identity).to_string(),
            is_entry: i == 0,
            is_extern: false,
            is_generated_std: false,
            extern_replay_error: None,
            src,
            program,
        })
        .collect::<Vec<_>>();
    check_unit_typed_with_version(&modules, version)
}

fn assert_clean(unit: &[(&str, &str)]) {
    let diags = check(unit);
    assert!(diags.is_empty(), "expected clean, got: {diags:?}");
}

fn assert_code(unit: &[(&str, &str)], code: &str) {
    let diags = check(unit);
    assert!(
        diags.iter().any(|d| d.starts_with(code)),
        "expected {code}, got: {diags:?}"
    );
}

fn assert_code_at(unit: &[(&str, &str)], version: LangVersion, code: &str) {
    let out = check_output_with_version(unit, version);
    let diagnostics: Vec<String> = out
        .diagnostics
        .iter()
        .map(|diag| format!("{} {}", diag.code.as_str(), diag.message))
        .collect();
    assert!(
        diagnostics.iter().any(|diag| diag.starts_with(code)),
        "expected {code}, got: {diagnostics:?}"
    );
}

fn assert_message_contains(unit: &[(&str, &str)], needle: &str) {
    let diags = check(unit);
    assert!(
        diags.iter().any(|d| d.contains(needle)),
        "want a diagnostic containing {needle:?}, got: {diags:?}"
    );
}

#[path = "c6/imports_and_suggestions.rs"]
mod imports_and_suggestions;
#[path = "c6/init_order_and_surface.rs"]
mod init_order_and_surface;
#[path = "c6/nominals.rs"]
mod nominals;
#[path = "c6/protocols.rs"]
mod protocols;
#[path = "c6/unit_returns.rs"]
mod unit_returns;
