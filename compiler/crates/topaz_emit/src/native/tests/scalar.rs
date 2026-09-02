use super::*;

#[test]
fn hybrid_uses_exact_byte_handles_and_only_a_proved_record_projection() {
    let mut provider = InMemoryProvider::new();
    provider.add_file(
        "main.tpz",
        "import util { Image, paint }\n\
         let mut direct = ByteBuffer.allocate(4)\n\
         let image = Image { pixels: direct }\n\
         paint(image, direct)",
    );
    provider.add_file(
        "util.tpz",
        "export record Image { pixels: ByteBuffer }\n\
         export function paint(image: Image, direct: ByteBuffer) -> int {\n\
           let mut pixels = image.pixels\n\
           let mut second = direct\n\
           pixels.set(0, 7)\n\
           second.set(1, 9)\n\
           pixels.get(0) + second.get(1)\n\
         }\n\
         export function nested(image: Image) -> int {\n\
           if true { let pixels = image.pixels; pixels.get(0) } else { 0 }\n\
         }",
    );
    let unit = topaz_resolve::resolve_with_version(
        &provider,
        "main.tpz",
        None,
        topaz_syntax::LangVersion::V5_20,
    );
    assert!(unit.diagnostics.is_empty(), "{:?}", unit.diagnostics);
    let checked_modules = unit
        .modules
        .iter()
        .map(|module| topaz_check::UnitModule {
            identity: module.identity.clone(),
            is_entry: module.is_entry,
            is_extern: module.is_extern,
            is_generated_std: module.is_generated_std,
            extern_replay_error: module.extern_replay_error.clone(),
            src: unit.map.file(module.file).src(),
            program: &module.program,
        })
        .collect::<Vec<_>>();
    let checked = topaz_check::check_unit_typed_with_version(
        &checked_modules,
        topaz_syntax::LangVersion::V5_20,
    );
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let typed = checked
        .typed_hir
        .expect("diagnostics-clean 5.20 unit produces typed HIR");

    let unit = lowered(&unit, Some(typed));
    let outcome =
        emit_native_or_hybrid(&NativeInput { unit: &unit }).expect("bounded byte hybrid emits");
    let paint_row = outcome
        .decision
        .functions
        .iter()
        .find(|row| row.module == "util" && row.name == "paint")
        .expect("paint report row");
    let nested_row = outcome
        .decision
        .functions
        .iter()
        .find(|row| row.module == "util" && row.name == "nested")
        .expect("nested report row");
    assert_eq!(paint_row.selected_backend, "hybrid-native");
    assert_eq!(paint_row.decline_reason, None);
    assert_eq!(nested_row.selected_backend, "boxed");
    assert_eq!(nested_row.decline_reason, Some("unsupported_native_body"));
    assert!(outcome.rust.contains("builtin_byte_buffer_set_i64(&"));
    assert!(outcome.rust.contains("builtin_byte_buffer_get_raw_i64(&"));
    assert!(
        outcome
            .rust
            .contains("if __value.is_nominal_record_declaration(\"util::Image\")")
    );
}
