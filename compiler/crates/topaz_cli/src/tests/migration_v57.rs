use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn v57_inherits_the_data_lens_web_app_pipeline() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../examples/data-lens")
        .canonicalize()
        .expect("Data Lens fixture");
    let root_arg = root.to_string_lossy().into_owned();
    let mut target = package_target(Some(&root_arg), None, true).expect("package target");
    target.version = LangVersion::V5_7;

    let lowered = resolve_and_lower_package_for_web(&target, Backend::Boxed)
        .expect("inherited V5_7 Web lowering");
    validate_web_app_lifecycle(&lowered, target.web.lifecycle).expect("inherited V5_7 lifecycle");
    assert!(!lowered.rust.is_empty());
}

#[test]
fn current_boundary_commits_only_exact_manifest_metadata() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "topaz-cli-v57-migration-{}-{nanos}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).expect("src");
    let manifest = r#"# keep this comment
[package]
name = "migration_transaction"
version = "0.1.0"
language = "5.6"
entry = "src/main.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.6"
"#;
    let entry = "import src.model { value }\nprint(\"{value}\")\n";
    let model = "export let value: int = 7\n";
    let lock = "lock remains byte-identical\n";
    fs::write(root.join("topaz.toml"), manifest).expect("manifest");
    fs::write(root.join("src/main.tpz"), entry).expect("entry");
    fs::write(root.join("src/model.tpz"), model).expect("model");
    fs::write(root.join("topaz.lock"), lock).expect("lock");

    let root_arg = root.to_string_lossy().into_owned();
    assert_eq!(
        migrate_package_56_to_57(Some(&root_arg), LangVersion::V5_7),
        ExitCode::SUCCESS
    );
    let updated = fs::read_to_string(root.join("topaz.toml")).expect("updated manifest");
    assert!(updated.starts_with("# keep this comment\n"), "{updated}");
    assert!(updated.contains("language = \"5.7\""), "{updated}");
    assert!(updated.contains("std = \"5.7\""), "{updated}");
    assert!(!updated.contains("\"5.6\""), "{updated}");
    assert_eq!(
        fs::read_to_string(root.join("src/main.tpz")).unwrap(),
        entry
    );
    assert_eq!(
        fs::read_to_string(root.join("src/model.tpz")).unwrap(),
        model
    );
    assert_eq!(fs::read_to_string(root.join("topaz.lock")).unwrap(), lock);
    assert!(fs::read_dir(&root).unwrap().flatten().all(|entry| {
        !entry
            .file_name()
            .to_string_lossy()
            .contains("topaz-migrate")
    }));

    let _ = fs::remove_dir_all(root);
}
