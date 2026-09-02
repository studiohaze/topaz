use topaz_resolve::{InMemoryProvider, resolve_with_version};
use topaz_syntax::LangVersion;

use crate::fixture_index::{MODULE_FIXTURES, SERVER_CONTRACT_DEMO, WIDE_FIXTURES};

use super::model::{FixtureFile, ModuleFixture};

pub(crate) fn render() -> String {
    let mut modules = String::new();
    let mut table = String::from("static WIDE_CORE_FIXTURES: &[WideCoreFixture] = &[\n");

    for (idx, fixture) in WIDE_FIXTURES.iter().enumerate() {
        let mut provider = InMemoryProvider::new();
        provider.add_file("main.tpz", fixture.source);
        let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::CURRENT);
        if !unit.diagnostics.is_empty() {
            panic!(
                "wide Python core fixture `{}` failed to resolve: {:?}",
                fixture.name, unit.diagnostics
            );
        }
        let lowered = topaz_lower::lower_resolved_compat(&unit).unwrap_or_else(|error| {
            panic!(
                "wide Python core fixture `{}` failed to lower: {error}",
                fixture.name
            )
        });
        let module = topaz_emit::emit_module(&lowered).unwrap_or_else(|error| {
            panic!(
                "wide Python core fixture `{}` failed to emit boxed Rust: {error:?}",
                fixture.name
            )
        });
        modules.push_str(&format!(
            "#[allow(clippy::all, dead_code, unused, unreachable_code, non_snake_case, while_true)]\nmod wide_core_{idx} {{\n{module}}}\n\n",
        ));
        table.push_str(&format!(
            "    WideCoreFixture {{ name: {:?}, source: {:?}, kind: {}, run: wide_core_{idx}::run_with_host }},\n",
            fixture.name,
            fixture.source,
            fixture.kind.generated_name()
        ));
    }

    table.push_str("];\n");
    let mut module_table = String::from("static MODULE_CORE_FIXTURES: &[ModuleCoreFixture] = &[\n");
    for (idx, fixture) in MODULE_FIXTURES.iter().enumerate() {
        let module = render_module(fixture, "module Python core");
        modules.push_str(&format!(
            "#[allow(clippy::all, dead_code, unused, unreachable_code, non_snake_case, while_true)]\nmod module_core_{idx} {{\n{module}}}\n\n",
        ));
        module_table.push_str(&format!(
            "    ModuleCoreFixture {{ name: {:?}, entry: {:?}, files: {}, run: module_core_{idx}::run_with_host }},\n",
            fixture.name,
            fixture.entry,
            format_module_files(fixture.files)
        ));
    }
    module_table.push_str("];\n");

    let module = render_module(&SERVER_CONTRACT_DEMO, "server contract");
    modules.push_str(&format!(
        "#[allow(clippy::all, dead_code, unused, unreachable_code, non_snake_case, while_true)]\nmod server_contract_demo {{\n{module}}}\n\n",
    ));
    modules.push_str(&format!(
        "static SERVER_CONTRACT_DEMO_FIXTURE: ModuleCoreFixture = ModuleCoreFixture {{ name: {:?}, entry: {:?}, files: {}, run: server_contract_demo::run_with_host }};\n",
        SERVER_CONTRACT_DEMO.name,
        SERVER_CONTRACT_DEMO.entry,
        format_module_files(SERVER_CONTRACT_DEMO.files)
    ));

    modules.push_str(&table);
    modules.push_str(&module_table);
    modules
}

fn render_module(fixture: &ModuleFixture, label: &str) -> String {
    let mut provider = InMemoryProvider::new();
    for file in fixture.files {
        provider.add_file(file.path, file.source);
    }
    let unit = resolve_with_version(&provider, fixture.entry, None, LangVersion::CURRENT);
    if !unit.diagnostics.is_empty() {
        panic!(
            "{label} fixture `{}` failed to resolve: {:?}",
            fixture.name, unit.diagnostics
        );
    }
    let lowered = topaz_lower::lower_resolved_compat(&unit).unwrap_or_else(|error| {
        panic!(
            "{label} fixture `{}` failed to lower: {error}",
            fixture.name
        )
    });
    topaz_emit::emit_module(&lowered).unwrap_or_else(|error| {
        panic!(
            "{label} fixture `{}` failed to emit boxed Rust: {error:?}",
            fixture.name
        )
    })
}

fn format_module_files(files: &[FixtureFile]) -> String {
    let entries = files
        .iter()
        .map(|file| format!("({:?}, {:?})", file.path, file.source))
        .collect::<Vec<_>>()
        .join(", ");
    format!("&[{entries}]")
}
