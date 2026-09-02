use super::*;

#[test]
fn compatible_v517_preserves_the_complete_data_lens_web_app_pipeline() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../examples/data-lens")
        .canonicalize()
        .expect("Data Lens fixture");
    let root_arg = root.to_string_lossy().into_owned();
    let mut target = package_target(Some(&root_arg), None, true).expect("package target");
    target.version = LangVersion::V5_17;

    let lowered = resolve_and_lower_package_for_web(&target, Backend::Boxed)
        .expect("compatible V5_17 Web lowering");
    validate_web_app_lifecycle(&lowered, target.web.lifecycle).expect("compatible V5_17 lifecycle");
    assert!(!lowered.rust.is_empty());
    assert_eq!(target.version.as_str(), "5.17");
    assert!(target.version.is_selectable());
}
