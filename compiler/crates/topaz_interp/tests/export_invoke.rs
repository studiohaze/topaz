use topaz_interp::{Machine, TestHost, Value};
use topaz_resolve::{InMemoryProvider, resolve};

#[test]
fn invokes_only_an_entry_export_with_explicit_values() {
    let mut provider = InMemoryProvider::new();
    provider.add_file(
        "main.tpz",
        r#"
function privateOffset() -> int {
    2
}

export function transform(value: int) -> int {
    value * 3 + privateOffset()
}
"#,
    );
    let unit = resolve(&provider, "main.tpz", None);
    assert!(unit.diagnostics.is_empty(), "{:?}", unit.diagnostics);

    let result =
        Machine::run_unit_export(&unit, &TestHost::new(), "transform", vec![Value::Int(4)])
            .expect("export invocation");
    assert!(matches!(result, Value::Int(14)));

    let error = Machine::run_unit_export(&unit, &TestHost::new(), "privateOffset", Vec::new())
        .expect_err("private entry binding must not be invocable");
    assert!(error.message.contains("is not exported"));
}
