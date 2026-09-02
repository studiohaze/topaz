use super::*;

#[test]
fn native_attempt_decision_uses_full_sorted_module_and_span_identity() {
    let mut provider = InMemoryProvider::new();
    provider.add_file(
        "main.tpz",
        "import util { same }\nfunction local(x: int) -> int { x }\nsame(local(1))",
    );
    provider.add_file(
        "util.tpz",
        "export function same(x: int) -> int { x }\nfunction local(x: int) -> int { x }",
    );
    let unit = resolve(&provider, "main.tpz", None);
    assert!(
        unit.diagnostics.is_empty(),
        "report identity unit must resolve clean: {:?}",
        unit.diagnostics
    );
    let unit = lowered(&unit, None);
    let input = NativeInput { unit: &unit };
    let attempt = Ok(String::from("deterministic-rust"));
    let decision = describe_native_attempt(&input, &attempt);

    assert_eq!(decision.selected_backend, "native");
    assert_eq!(decision.selection_scope, "unit");
    assert_eq!(decision.decline_reason, None);
    assert_eq!(decision.entry_module, "main");
    assert_eq!(decision.module_count, 2);
    assert_eq!(decision.functions.len(), 3);
    assert_eq!(
        decision
            .functions
            .iter()
            .map(|row| (row.module.as_str(), row.name.as_str()))
            .collect::<Vec<_>>(),
        vec![("main", "local"), ("util", "same"), ("util", "local")]
    );
    assert!(
        decision
            .functions
            .iter()
            .all(|row| row.span_lo < row.span_hi && row.decline_reason.is_none())
    );
}

#[test]
fn hybrid_keeps_the_boxed_module_envelope_and_replaces_only_scalar_functions() {
    let mut provider = InMemoryProvider::new();
    provider.add_file(
        "main.tpz",
        "import util\nlet values = [1, 2, 3]\nutil.twice(values.length)",
    );
    provider.add_file(
        "util.tpz",
        "export function twice(x: int) -> int { x * 2 }\n\
         export function boxed(value: string) -> string { value }",
    );
    let unit = resolve(&provider, "main.tpz", None);
    assert!(
        unit.diagnostics.is_empty(),
        "hybrid test unit must resolve clean: {:?}",
        unit.diagnostics
    );
    let util = unit
        .modules
        .iter()
        .find(|module| module.identity == "util")
        .expect("util module");
    let twice = util
        .program
        .items
        .iter()
        .find_map(ast_top_level_function)
        .expect("twice declaration");
    let mut typed = TypedUnit::new();
    typed.push_local("x", twice.params[0].name.span, MonoTy::I64);

    let unit = lowered(&unit, Some(typed));
    let outcome =
        emit_native_or_hybrid(&NativeInput { unit: &unit }).expect("bounded hybrid emits");

    assert_eq!(outcome.decision.selected_backend, "hybrid-native");
    assert_eq!(outcome.decision.selection_scope, "function");
    let twice_row = outcome
        .decision
        .functions
        .iter()
        .find(|row| row.module == "util" && row.name == "twice")
        .expect("twice report row");
    assert_eq!(twice_row.selected_backend, "hybrid-native");
    assert_eq!(twice_row.decline_reason, None);
    let boxed_row = outcome
        .decision
        .functions
        .iter()
        .find(|row| row.module == "util" && row.name == "boxed")
        .expect("boxed report row");
    assert_eq!(boxed_row.selected_backend, "boxed");
    assert_eq!(boxed_row.decline_reason, Some("non_scalar_signature"));
    assert!(
        outcome.rust.contains("pub fn topaz_export_names()"),
        "hybrid must retain the boxed Web/export envelope"
    );
    assert!(
        outcome.rust.contains("async fn __topaz_hybrid_"),
        "selected scalar helper must be present"
    );
    assert!(
        outcome.rust.contains("__hybrid_outer_guard: bool"),
        "hybrid helper must distinguish its already-guarded boxed entry:\n{}",
        outcome.rust
    );
    assert!(
        outcome.rust.contains("params: &[\"x\"]"),
        "hybrid closure must preserve the boxed callable ABI"
    );
}

#[test]
fn hybrid_keys_same_name_helpers_by_module_and_file_backed_spans() {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", "import a\nimport b\na.same(1) + b.same(2)");
    provider.add_file("a.tpz", "export function same(x: int) -> int { x + 10 }");
    provider.add_file("b.tpz", "export function same(x: int) -> int { x + 20 }");
    let unit = resolve(&provider, "main.tpz", None);
    assert!(unit.diagnostics.is_empty(), "{:?}", unit.diagnostics);
    let mut typed = TypedUnit::new();
    for module in unit
        .modules
        .iter()
        .filter(|module| matches!(module.identity.as_str(), "a" | "b"))
    {
        let declaration = module
            .program
            .items
            .iter()
            .find_map(ast_top_level_function)
            .expect("same declaration");
        typed.push_local("x", declaration.params[0].name.span, MonoTy::I64);
    }

    let unit = lowered(&unit, Some(typed));
    let outcome =
        emit_native_or_hybrid(&NativeInput { unit: &unit }).expect("same-name module helpers emit");
    let selected = outcome
        .decision
        .functions
        .iter()
        .filter(|row| row.name == "same" && row.selected_backend == "hybrid-native")
        .count();
    assert_eq!(selected, 2);
    assert!(outcome.rust.contains("__topaz_hybrid__t_61_"));
    assert!(outcome.rust.contains("__topaz_hybrid__t_62_"));
}

#[test]
fn hybrid_declines_named_calls_without_disabling_the_positional_leaf() {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", "import util\nutil.caller(3)");
    provider.add_file(
        "util.tpz",
        "export function add(a: int, b: int) -> int { a + b }\n\
         export function caller(x: int) -> int { add(b: 2, a: x) }",
    );
    let unit = resolve(&provider, "main.tpz", None);
    assert!(unit.diagnostics.is_empty(), "{:?}", unit.diagnostics);
    let util = unit
        .modules
        .iter()
        .find(|module| module.identity == "util")
        .expect("util module");
    let mut typed = TypedUnit::new();
    let util_src = unit.map.file(util.file).src();
    for declaration in util.program.items.iter().filter_map(ast_top_level_function) {
        for param in &declaration.params {
            typed.push_local(
                &util_src[param.name.span.lo as usize..param.name.span.hi as usize],
                param.name.span,
                MonoTy::I64,
            );
        }
    }

    let unit = lowered(&unit, Some(typed));
    let outcome = emit_native_or_hybrid(&NativeInput { unit: &unit })
        .expect("partially eligible helpers emit");
    let add = outcome
        .decision
        .functions
        .iter()
        .find(|row| row.name == "add")
        .expect("add row");
    let caller = outcome
        .decision
        .functions
        .iter()
        .find(|row| row.name == "caller")
        .expect("caller row");
    assert_eq!(add.selected_backend, "hybrid-native");
    assert_eq!(caller.selected_backend, "boxed");
    assert_eq!(caller.decline_reason, Some("unsupported_native_body"));
}
