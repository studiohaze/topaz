use super::*;
use crate::lock::parse_lock_document;
use crate::*;

#[test]
fn path_dependency_requires_content_hash() {
    let text = r#"[package]
name = "bad"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[dependencies]
schema = { path = "../schema" }
"#;
    let err = parse_manifest(text).unwrap_err();
    assert!(err.message().contains("content `hash`"), "{err}");
}

#[test]
fn lock_verifies_root_manifest_hash() {
    let text = manifest_text();
    let manifest = parse_manifest(&text).expect("manifest parses");
    let hash = manifest_sha256(&text);
    let lock = format!(
        r#"[[package]]
name = "user_tools"
version = "0.1.0"
source = "root"
manifest_hash = "{hash}"

[[package]]
name = "csv_tools"
version = "1.2.0"
source = "registry"
hash = "{HASH}"

[[package]]
name = "local_schema"
path = "../schema"
hash = "{HASH}"
"#
    );
    verify_lock_text(&lock, &text, &manifest).expect("lock verifies");
    let stale = lock.replace(&hash, HASH);
    let err = verify_lock_text(&stale, &text, &manifest).unwrap_err();
    assert!(err.message().contains("manifest_hash is stale"), "{err}");
}

#[test]
fn lock_requires_the_exact_manifest_package_inventory() {
    let text = manifest_text();
    let manifest = parse_manifest(&text).expect("manifest parses");
    let hash = manifest_sha256(&text);
    let lock = format!(
        r#"[[package]]
name = "user_tools"
version = "0.1.0"
source = "root"
manifest_hash = "{hash}"

[[package]]
name = "csv_tools"
version = "1.2.0"
source = "registry"
hash = "{HASH}"

[[package]]
name = "local_schema"
path = "../schema"
hash = "{HASH}"
"#
    );
    let root_stanza = lock
        .split_once("\n\n")
        .map(|(root, _)| root)
        .expect("root lock stanza");
    let registry_stanza = lock.split("\n\n").nth(1).expect("registry lock stanza");
    let cases = [
        (
            format!("schema = 1\n{lock}"),
            "topaz.lock: unknown key `schema`",
        ),
        (
            lock.replacen(
                &format!("manifest_hash = \"{hash}\""),
                &format!("manifest_hash = \"{hash}\"\nchecksum = \"{HASH}\""),
                1,
            ),
            "topaz.lock [[package]]: unknown key `checksum`",
        ),
        (
            format!("{lock}\n{root_stanza}\n"),
            "duplicate package with source = \"root\"",
        ),
        (
            format!("{lock}\n{registry_stanza}\n"),
            "duplicate package `csv_tools`",
        ),
        (
            format!(
                "{lock}\n[[package]]\nname = \"undeclared\"\npath = \"../undeclared\"\nhash = \"{HASH}\"\n"
            ),
            "local package `undeclared` is not declared in topaz.toml",
        ),
        (
            lock.replace(
                "name = \"local_schema\"\npath = \"../schema\"",
                "name = \"local_schema\"\nversion = \"1.0.0\"\npath = \"../schema\"",
            ),
            "local package `local_schema` permits only `name`, `path`, and `hash`",
        ),
    ];
    for (lock, expected) in cases {
        let error = verify_lock_text(&lock, &text, &manifest)
            .expect_err("non-exact lock inventory must reject");
        assert!(error.message().contains(expected), "{error}");
    }
}

#[test]
fn registry_dependency_manifest_hash_must_match_lock_hash() {
    let text = format!(
        r#"[package]
name = "root_pkg"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[dependencies]
dep = {{ version = "1.0.0", hash = "{HASH}" }}
"#
    );
    let manifest = parse_manifest(&text).expect("manifest parses");
    let root_hash = manifest_sha256(&text);
    let lock = format!(
        r#"[[package]]
name = "root_pkg"
version = "0.1.0"
source = "root"
manifest_hash = "{root_hash}"

[[package]]
name = "dep"
version = "1.0.0"
source = "registry"
hash = "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
"#
    );
    let err = verify_lock_text(&lock, &text, &manifest).unwrap_err();
    assert!(
        err.message().contains("registry package `dep` hash"),
        "{err}"
    );
}

#[test]
fn lock_rejects_remote_registry_source_labels() {
    let lock = format!(
        r#"[[package]]
name = "root_pkg"
version = "0.1.0"
source = "remote-registry"
manifest_hash = "{HASH}"
"#
    );
    let err = parse_lock_document(&lock)
        .err()
        .expect("unsupported source must reject");
    assert!(
        err.message()
            .contains("topaz.lock [[package]].source `remote-registry` is not supported in v5.4"),
        "{err}"
    );
}

#[test]
fn lock_records_and_verifies_extern_replay_hash() {
    let root = temp_root("extern-lock-ok");
    write_extern_lock_package(&root);
    let project = Project::load(&root).expect("project loads");
    let lock = project.render_lockfile().expect("lock renders");
    assert!(lock.contains("[[extern]]"), "{lock}");
    assert!(lock.contains("module = \"host.math\""), "{lock}");
    assert!(
        lock.contains(
            "hash = \"sha256:39587311db2c06b2b2e2a038ddeefa717a74f4f991a023770a54002366d94d49\""
        ),
        "{lock}"
    );
    assert!(
        lock.contains("artifact_path = \"artifacts/host-math.wasm\""),
        "{lock}"
    );
    assert!(lock.contains("sandbox = \"wasm\""), "{lock}");
    assert!(lock.contains("fuel = 1000"), "{lock}");
    assert!(lock.contains("memory_bytes = 65536"), "{lock}");
    assert!(lock.contains("replay_hash = \"sha256:"), "{lock}");
    write_file(&root, "topaz.lock", &lock);
    project
        .verify_locked()
        .expect("extern replay lock verifies");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn lock_rejects_linked_extern_artifact() {
    use std::os::unix::fs::symlink;

    let root = temp_root("extern-artifact-symlink");
    let outside = temp_root("extern-artifact-symlink-outside");
    write_extern_lock_package(&root);
    std::fs::remove_file(root.join("artifacts/host-math.wasm")).expect("remove regular artifact");
    write_file(&outside, "host-math.wasm", ARTIFACT_BYTES);
    symlink(
        outside.join("host-math.wasm"),
        root.join("artifacts/host-math.wasm"),
    )
    .expect("linked artifact");

    let error = Project::load(&root)
        .expect("project loads")
        .render_lockfile()
        .expect_err("linked extern artifact rejects");
    assert!(
        error
            .message()
            .contains("artifact `artifacts/host-math.wasm` must not contain a symlink"),
        "{error}"
    );
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}

#[cfg(unix)]
#[test]
fn lock_rejects_linked_extern_replay_fixture() {
    use std::os::unix::fs::symlink;

    let root = temp_root("extern-replay-symlink");
    let outside = temp_root("extern-replay-symlink-outside");
    write_extern_lock_package(&root);
    std::fs::remove_file(root.join("replay/host-math.jsonl"))
        .expect("remove regular replay fixture");
    write_file(
        &outside,
        "host-math.jsonl",
        r#"{"module":"host.math","function":"twice","args":[{"$":"int","value":"21"}],"result":{"$":"int","value":"42"}}
"#,
    );
    symlink(
        outside.join("host-math.jsonl"),
        root.join("replay/host-math.jsonl"),
    )
    .expect("linked replay fixture");

    let error = Project::load(&root)
        .expect("project loads")
        .render_lockfile()
        .expect_err("linked extern replay rejects");
    assert!(
        error
            .message()
            .contains("replay fixture `replay/host-math.jsonl` must not contain a symlink"),
        "{error}"
    );
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}

#[test]
fn locked_extern_artifact_hash_detects_drift() {
    let root = temp_root("extern-artifact-drift");
    write_extern_lock_package(&root);
    let project = Project::load(&root).expect("project loads");
    let lock = project.render_lockfile().expect("lock renders");
    write_file(&root, "topaz.lock", &lock);
    write_file(&root, "artifacts/host-math.wasm", "changed artifact\n");
    let err = Project::load(&root)
        .expect("project reloads")
        .verify_locked()
        .unwrap_err();
    assert!(err.message().contains("artifact hash is stale"), "{err}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn locked_extern_artifact_hash_requires_artifact_bytes() {
    let root = temp_root("extern-artifact-missing");
    write_extern_lock_package(&root);
    let project = Project::load(&root).expect("project loads");
    let lock = project.render_lockfile().expect("lock renders");
    write_file(&root, "topaz.lock", &lock);
    std::fs::remove_file(root.join("artifacts/host-math.wasm")).expect("remove artifact");
    let err = Project::load(&root)
        .expect("project reloads")
        .verify_locked()
        .unwrap_err();
    assert!(
        err.message()
            .contains("cannot read extern module `host.math` artifact"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn locked_extern_replay_hash_detects_drift() {
    let root = temp_root("extern-lock-drift");
    write_extern_lock_package(&root);
    let project = Project::load(&root).expect("project loads");
    let lock = project.render_lockfile().expect("lock renders");
    write_file(&root, "topaz.lock", &lock);
    write_file(
        &root,
        "replay/host-math.jsonl",
        r#"{"module":"host.math","function":"twice","args":[{"$":"int","value":"21"}],"result":{"$":"int","value":"43"}}
"#,
    );
    let err = Project::load(&root)
        .expect("project reloads")
        .verify_locked()
        .unwrap_err();
    assert!(err.message().contains("replay_hash is stale"), "{err}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn locked_extern_replay_hash_requires_fixture_bytes() {
    let root = temp_root("extern-lock-missing");
    write_extern_lock_package(&root);
    let project = Project::load(&root).expect("project loads");
    let lock = project.render_lockfile().expect("lock renders");
    write_file(&root, "topaz.lock", &lock);
    std::fs::remove_file(root.join("replay/host-math.jsonl")).expect("remove replay");
    let err = Project::load(&root)
        .expect("project reloads")
        .verify_locked()
        .unwrap_err();
    assert!(
        err.message()
            .contains("cannot read extern module `host.math` replay fixture"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn render_lockfile_requires_extern_replay_fixture() {
    let root = temp_root("extern-lock-render-missing");
    write_file(&root, "topaz.toml", &extern_lock_manifest());
    write_file(root.as_path(), "main.tpz", "print(\"ok\")\n");
    write_file(root.as_path(), "artifacts/host-math.wasm", ARTIFACT_BYTES);
    let err = Project::load(&root)
        .expect("project loads")
        .render_lockfile()
        .unwrap_err();
    assert!(
        err.message()
            .contains("cannot read extern module `host.math` replay fixture"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn verify_lock_text_checks_extern_declarations_not_replay_bytes() {
    let root = temp_root("extern-lock-text-asymmetry");
    write_extern_lock_package(&root);
    let project = Project::load(&root).expect("project loads");
    let lock = project.render_lockfile().expect("lock renders");
    write_file(
        &root,
        "replay/host-math.jsonl",
        r#"{"module":"host.math","function":"twice","args":[{"$":"int","value":"21"}],"result":{"$":"int","value":"43"}}
"#,
    );
    verify_lock_text(&lock, &project.manifest_text, &project.manifest)
        .expect("text-only verification ignores replay bytes");

    let stale_decl = lock.replacen(
        &format!("hash = \"{ARTIFACT_HASH}\""),
        &format!("hash = \"{HASH_C}\""),
        1,
    );
    let err = verify_lock_text(&stale_decl, &project.manifest_text, &project.manifest).unwrap_err();
    assert!(
        err.message()
            .contains("extern module `host.math` hash does not match topaz.toml"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn verify_lock_text_checks_extern_artifact_and_policy_metadata() {
    let root = temp_root("extern-lock-policy-text");
    write_extern_lock_package(&root);
    let project = Project::load(&root).expect("project loads");
    let lock = project.render_lockfile().expect("lock renders");

    let stale_artifact_path = lock.replace(
        "artifact_path = \"artifacts/host-math.wasm\"",
        "artifact_path = \"artifacts/other.wasm\"",
    );
    let err = verify_lock_text(
        &stale_artifact_path,
        &project.manifest_text,
        &project.manifest,
    )
    .unwrap_err();
    assert!(
        err.message().contains("artifact_path does not match"),
        "{err}"
    );

    let stale_sandbox = lock.replace("sandbox = \"wasm\"", "sandbox = \"replay\"");
    let err =
        verify_lock_text(&stale_sandbox, &project.manifest_text, &project.manifest).unwrap_err();
    assert!(err.message().contains("sandbox does not match"), "{err}");

    let stale_fuel = lock.replace("fuel = 1000", "fuel = 2000");
    let err = verify_lock_text(&stale_fuel, &project.manifest_text, &project.manifest).unwrap_err();
    assert!(err.message().contains("fuel does not match"), "{err}");

    let stale_memory = lock.replace("memory_bytes = 65536", "memory_bytes = 32768");
    let err =
        verify_lock_text(&stale_memory, &project.manifest_text, &project.manifest).unwrap_err();
    assert!(
        err.message().contains("memory_bytes does not match"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn locked_local_dependency_checks_content_hash() {
    let base = std::env::temp_dir().join("topaz_package_content_hash_test");
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("root");
    let dep = base.join("local_schema");
    std::fs::create_dir_all(root.join("src")).expect("root mkdir");
    std::fs::create_dir_all(dep.join("src")).expect("dep mkdir");
    std::fs::write(
        dep.join("topaz.toml"),
        r#"[package]
name = "local_schema"
version = "0.1.0"
language = "5.4"
entry = "src/lib.tpz"
"#,
    )
    .expect("dep manifest");
    std::fs::write(dep.join("src/lib.tpz"), "export const schema = \"ok\"\n").expect("dep source");
    let dep_hash = package_content_hash(&dep).expect("dep hash");
    let root_manifest = format!(
        r#"[package]
name = "root_pkg"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[dependencies]
local_schema = {{ path = "../local_schema", hash = "{dep_hash}" }}
"#
    );
    std::fs::write(root.join("topaz.toml"), &root_manifest).expect("root manifest");
    std::fs::write(root.join("src/main.tpz"), "print(\"ok\")\n").expect("root source");
    let root_hash = manifest_sha256(&root_manifest);
    let lock = format!(
        r#"[[package]]
name = "root_pkg"
version = "0.1.0"
source = "root"
manifest_hash = "{root_hash}"

[[package]]
name = "local_schema"
path = "../local_schema"
hash = "{dep_hash}"
"#
    );
    std::fs::write(root.join("topaz.lock"), lock).expect("lock");
    Project::load(&root)
        .expect("project")
        .verify_locked()
        .expect("locked graph");

    std::fs::write(
        dep.join("src/lib.tpz"),
        "export const schema = \"changed\"\n",
    )
    .expect("mutate dep");
    let err = Project::load(&root)
        .expect("project")
        .verify_locked()
        .unwrap_err();
    assert!(err.message().contains("content hash is stale"), "{err}");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn locked_registry_dependency_checks_vendored_content_hash() {
    let base = std::env::temp_dir().join("topaz_registry_vendor_hash_test");
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("root");
    let dep = root.join("vendor/greeter/1.0.0");
    std::fs::create_dir_all(root.join("src")).expect("root mkdir");
    std::fs::create_dir_all(dep.join("src")).expect("dep mkdir");
    std::fs::write(
        dep.join("topaz.toml"),
        r#"[package]
name = "greeter"
version = "1.0.0"
language = "5.4"
entry = "src/lib.tpz"
"#,
    )
    .expect("dep manifest");
    std::fs::write(
        dep.join("src/lib.tpz"),
        "export const message = \"hello\"\n",
    )
    .expect("dep source");
    let dep_hash = package_content_hash(&dep).expect("dep hash");
    let root_manifest = r#"[package]
name = "root_pkg"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[dependencies]
greeter = "1.0.0"
"#;
    std::fs::write(root.join("topaz.toml"), root_manifest).expect("root manifest");
    std::fs::write(root.join("src/main.tpz"), "print(\"ok\")\n").expect("root source");
    let root_hash = manifest_sha256(root_manifest);
    let lock = format!(
        r#"[[package]]
name = "root_pkg"
version = "0.1.0"
source = "root"
manifest_hash = "{root_hash}"

[[package]]
name = "greeter"
version = "1.0.0"
source = "registry"
hash = "{dep_hash}"
"#
    );
    std::fs::write(root.join("topaz.lock"), lock).expect("lock");
    Project::load(&root)
        .expect("project")
        .verify_locked()
        .expect("vendored registry graph");

    std::fs::write(
        dep.join("src/lib.tpz"),
        "export const message = \"changed\"\n",
    )
    .expect("mutate dep");
    let err = Project::load(&root)
        .expect("project")
        .verify_locked()
        .unwrap_err();
    assert!(
        err.message()
            .contains("vendored registry package `greeter` version `1.0.0` content hash"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(&base);
}
