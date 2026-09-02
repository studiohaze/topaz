use super::*;

#[test]
fn project_load_canonicalizes_root() {
    let root = std::env::temp_dir().join(format!("topaz_project_load_root_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("topaz.toml"),
        r#"[package]
name = "root_pkg"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"
"#,
    )
    .expect("manifest");
    std::fs::write(root.join("src/main.tpz"), "print(\"ok\")\n").expect("entry");

    let project = Project::load(root.join(".")).expect("project loads");
    assert_eq!(
        project.root,
        std::fs::canonicalize(&root).expect("canonical root")
    );

    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn project_load_rejects_a_linked_manifest() {
    use std::os::unix::fs::symlink;

    let root = temp_root("linked-manifest");
    let outside = temp_root("linked-manifest-outside");
    write_file(
        &outside,
        "topaz.toml",
        r#"[package]
name = "outside"
version = "0.1.0"
language = "5.4"
entry = "main.tpz"
"#,
    );
    write_file(&root, "main.tpz", "print(\"inside\")\n");
    symlink(outside.join("topaz.toml"), root.join("topaz.toml")).expect("linked manifest");

    let error = Project::load(&root).expect_err("linked package manifest must reject");
    assert!(
        error
            .message()
            .contains("package manifest `topaz.toml` must not contain a symlink"),
        "{error}"
    );
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}

#[cfg(unix)]
#[test]
fn package_lock_read_and_replace_reject_a_link_without_touching_its_target() {
    use std::os::unix::fs::symlink;

    let root = temp_root("linked-lock");
    let outside = temp_root("linked-lock-outside");
    write_file(
        &root,
        "topaz.toml",
        r#"[package]
name = "inside"
version = "0.1.0"
language = "5.4"
entry = "main.tpz"
"#,
    );
    write_file(&root, "main.tpz", "print(\"inside\")\n");
    write_file(&outside, "topaz.lock", "outside lock must stay unchanged\n");
    symlink(outside.join("topaz.lock"), root.join("topaz.lock")).expect("linked lock");
    let project = Project::load(&root).expect("regular manifest loads");

    let read_error = project
        .verify_locked()
        .expect_err("linked package lock read must reject");
    assert!(
        read_error
            .message()
            .contains("package lockfile `topaz.lock` must not contain a symlink"),
        "{read_error}"
    );
    let write_error = project
        .write_lockfile()
        .expect_err("linked package lock replace must reject");
    assert!(
        write_error
            .message()
            .contains("refusing to replace non-regular package path"),
        "{write_error}"
    );
    let outside_lock =
        std::fs::read_to_string(outside.join("topaz.lock")).expect("outside lock remains readable");
    assert_eq!(outside_lock, "outside lock must stay unchanged\n");
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}
