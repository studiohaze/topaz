use topaz_resolve::{InMemoryProvider, resolve, resolve_with_version};
use topaz_syntax::LangVersion;

use crate::fixture_index::{
    EXTERN_MODULE_FIXTURES, FIXTURES, MODULE_FIXTURES, VERSIONED_MODULE_FIXTURES,
};

use super::provider::ExternReplayProvider;

/// §15 `concurrent` fixtures whose cross-arm interleaving is implementation-
/// defined (SPEC §15): the differential harness accepts ANY of these full stdout
/// transcripts for EITHER engine. Keyed by fixture name; a fixture absent here
/// keeps the default exact interp==emit comparison. Keep each set SMALL and fully
/// enumerated (no wildcards) so it cannot mask a real divergence.
const FIXTURE_STDOUT_ALTS: &[(&str, &[&[&str]])] = &[
    (
        "concurrent_cross_arm_stdout",
        &[&["a", "b", "3"], &["b", "a", "3"]],
    ),
    (
        "concurrent_two_while_arms",
        &[&["a", "a", "b", "b", "3"], &["a", "b", "a", "b", "3"]],
    ),
    (
        "concurrent_three_arms",
        &[&["a", "b", "c", "6"], &["c", "a", "b", "6"]],
    ),
];

/// Render a fixture's accepted-stdout-set as a Rust literal for the generated
/// `Fixture { stdout_alts: … }` field (`&[]` when the fixture is deterministic).
fn stdout_alts_for(name: &str) -> String {
    let Some((_, alts)) = FIXTURE_STDOUT_ALTS.iter().find(|(n, _)| *n == name) else {
        return "&[]".to_string();
    };
    let transcripts = alts
        .iter()
        .map(|lines| {
            let joined = lines
                .iter()
                .map(|l| format!("{l:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("&[{joined}] as &[&str]")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("&[{transcripts}]")
}

pub(crate) fn render_fixtures() -> String {
    let mut modules = String::new();
    let mut table = String::from(
        "/// Every eligible fixture, in source order.\npub static FIXTURES: &[Fixture] = &[\n",
    );

    for (i, fixture) in FIXTURES.iter().enumerate() {
        let name = fixture.name;
        let source = fixture.source;
        let mut provider = InMemoryProvider::new();
        provider.add_file("main.tpz", source);
        let unit = resolve(&provider, "main.tpz", None);
        let lowered = topaz_lower::lower_resolved_compat(&unit)
            .unwrap_or_else(|error| panic!("eligible fixture `{name}` failed to lower: {error}"));
        let module = topaz_emit::emit_module(&lowered)
            .unwrap_or_else(|e| panic!("eligible fixture `{name}` failed to emit: {e:?}"));
        // Generated code is emitter output, not hand-written; do not
        // hold it to the harness crate's own lints.
        modules.push_str(&format!(
            "#[allow(clippy::all, dead_code, unused, unreachable_code, non_snake_case)]\nmod fixture_{i} {{\n{module}}}\n\n"
        ));
        let stdout_alts = stdout_alts_for(name);
        table.push_str(&format!(
            "    Fixture {{ name: {name:?}, source: {source:?}, stdout_alts: {stdout_alts}, run: fixture_{i}::run_with_host, call_export: fixture_{i}::call_export_with_host }},\n"
        ));
    }
    table.push_str("];\n");

    // E-3 multi-module fixtures, numbered AFTER the single-file ones so the
    // `mod fixture_N` names stay unique. Each resolves a multi-file unit and
    // emits it through the same `emit_module` (its multi-module branch).
    let base = FIXTURES.len();
    let mut module_table = String::from(
        "/// Every eligible multi-module fixture, in source order.\npub static MODULE_FIXTURES: &[ModuleFixture] = &[\n",
    );
    for (k, (name, entry, files)) in MODULE_FIXTURES.iter().enumerate() {
        let i = base + k;
        let mut provider = InMemoryProvider::new();
        for (path, source) in *files {
            provider.add_file(*path, *source);
        }
        let unit = resolve(&provider, entry, None);
        assert!(
            unit.diagnostics.is_empty(),
            "module fixture `{name}` must resolve clean: {:?}",
            unit.diagnostics
        );
        let lowered = topaz_lower::lower_resolved_compat(&unit)
            .unwrap_or_else(|error| panic!("module fixture `{name}` failed to lower: {error}"));
        let module = topaz_emit::emit_module(&lowered)
            .unwrap_or_else(|e| panic!("module fixture `{name}` failed to emit: {e:?}"));
        modules.push_str(&format!(
            "#[allow(clippy::all, dead_code, unused, unreachable_code, non_snake_case)]\nmod fixture_{i} {{\n{module}}}\n\n"
        ));
        let files_lit = files
            .iter()
            .map(|(p, s)| format!("FixtureFile {{ path: {p:?}, source: {s:?} }}"))
            .collect::<Vec<_>>()
            .join(", ");
        module_table.push_str(&format!(
            "    ModuleFixture {{ name: {name:?}, entry: {entry:?}, language_version: topaz_syntax::LangVersion::CURRENT, files: &[{files_lit}], externs: &[], extern_replay_jsonl: \"\", run: fixture_{i}::run_with_host }},\n"
        ));
    }
    for (k, (name, entry, version, files)) in VERSIONED_MODULE_FIXTURES.iter().enumerate() {
        let i = base + MODULE_FIXTURES.len() + k;
        let mut provider = InMemoryProvider::new();
        for (path, source) in *files {
            provider.add_file(*path, *source);
        }
        let unit = resolve_with_version(&provider, entry, None, *version);
        assert!(
            unit.diagnostics.is_empty(),
            "versioned module fixture `{name}` must resolve clean: {:?}",
            unit.diagnostics
        );
        let lowered = topaz_lower::lower_resolved_compat(&unit).unwrap_or_else(|error| {
            panic!("versioned module fixture `{name}` failed to lower: {error}")
        });
        let module = topaz_emit::emit_module(&lowered).unwrap_or_else(|error| {
            panic!("versioned module fixture `{name}` failed to emit: {error:?}")
        });
        modules.push_str(&format!(
            "#[allow(clippy::all, dead_code, unused, unreachable_code, non_snake_case)]\nmod fixture_{i} {{\n{module}}}\n\n"
        ));
        let files_lit = files
            .iter()
            .map(|(path, source)| format!("FixtureFile {{ path: {path:?}, source: {source:?} }}"))
            .collect::<Vec<_>>()
            .join(", ");
        let version_variant = match version {
            LangVersion::V5_20 => "V5_20",
            _ => panic!("versioned fixture `{name}` uses an unregistered profile"),
        };
        module_table.push_str(&format!(
            "    ModuleFixture {{ name: {name:?}, entry: {entry:?}, language_version: topaz_syntax::LangVersion::{version_variant}, files: &[{files_lit}], externs: &[], extern_replay_jsonl: \"\", run: fixture_{i}::run_with_host }},\n"
        ));
    }
    for (k, (name, entry, files, externs, extern_replay_jsonl)) in
        EXTERN_MODULE_FIXTURES.iter().enumerate()
    {
        let i = base + MODULE_FIXTURES.len() + VERSIONED_MODULE_FIXTURES.len() + k;
        let mut provider = ExternReplayProvider::new();
        for (path, source) in *files {
            provider.add_file(path, source);
        }
        for (identity, path, source, replay_error) in *externs {
            provider.add_extern_file(identity, path, source, *replay_error);
        }
        let unit = resolve(&provider, entry, None);
        assert!(
            unit.diagnostics.is_empty(),
            "extern module fixture `{name}` must resolve clean: {:?}",
            unit.diagnostics
        );
        let lowered = topaz_lower::lower_resolved_compat(&unit).unwrap_or_else(|error| {
            panic!("extern module fixture `{name}` failed to lower: {error}")
        });
        let module = topaz_emit::emit_module(&lowered)
            .unwrap_or_else(|e| panic!("extern module fixture `{name}` failed to emit: {e:?}"));
        modules.push_str(&format!(
            "#[allow(clippy::all, dead_code, unused, unreachable_code, non_snake_case)]\nmod fixture_{i} {{\n{module}}}\n\n"
        ));
        let files_lit = files
            .iter()
            .map(|(p, s)| format!("FixtureFile {{ path: {p:?}, source: {s:?} }}"))
            .collect::<Vec<_>>()
            .join(", ");
        let externs_lit = externs
            .iter()
            .map(|(identity, path, source, replay_error)| {
                format!(
                    "ExternFixtureFile {{ identity: {identity:?}, path: {path:?}, source: {source:?}, replay_error: {replay_error:?} }}"
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        module_table.push_str(&format!(
            "    ModuleFixture {{ name: {name:?}, entry: {entry:?}, language_version: topaz_syntax::LangVersion::CURRENT, files: &[{files_lit}], externs: &[{externs_lit}], extern_replay_jsonl: {extern_replay_jsonl:?}, run: fixture_{i}::run_with_host }},\n"
        ));
    }
    module_table.push_str("];\n");

    format!("{modules}{table}{module_table}")
}
