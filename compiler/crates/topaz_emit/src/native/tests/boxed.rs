use super::*;

#[test]
fn native_declines_extern_units_before_the_typed_hir_gate() {
    let mut provider = ExternTestProvider::new();
    provider.add_file("main.tpz", "import host.math { twice }\ntwice(1)");
    provider.add_extern_file(
        "host.math",
        "host/math.tpz",
        "export function twice(x: int) -> int { x }",
    );
    let unit = resolve(&provider, "main.tpz", None);
    assert!(
        unit.diagnostics.is_empty(),
        "extern test unit must resolve clean: {:?}",
        unit.diagnostics
    );

    let unit = lowered(&unit, None);
    let input = NativeInput { unit: &unit };
    let attempt = emit_native_items(&input);
    let err = attempt
        .as_ref()
        .expect_err("extern units must decline natively");

    assert_eq!(err.kind, EmitErrorKind::NativeDeclined("an extern unit"));
    let decision = describe_native_attempt(&input, &attempt);
    assert_eq!(decision.selected_backend, "boxed");
    assert_eq!(decision.decline_reason, Some("extern_unit"));
    assert!(decision.contains_extern);
    assert_eq!(decision.functions.len(), 1);
    assert_eq!(decision.functions[0].module, "host.math");
    assert_eq!(decision.functions[0].name, "twice");
    assert_eq!(decision.functions[0].decline_reason, Some("extern_unit"));
}
