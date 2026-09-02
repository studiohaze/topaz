use super::support::*;

#[test]
fn package_manifest_mode_resolves_before_compiler_work() {
    let dir = std::env::temp_dir().join(format!("topaz_cli_legacy_mode_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("create package");
    std::fs::write(
        dir.join("topaz.toml"),
        "[package]\nname = \"legacy_mode_package\"\nversion = \"0.1.0\"\nlanguage = \"5.13\"\nentry = \"src/main.tpz\"\n\n[build]\ntarget = \"native\"\ndeterministic = true\n\n[dependencies]\nstd = \"5.13\"\n",
    )
    .expect("write manifest");
    std::fs::write(dir.join("src/main.tpz"), "let answer: int = 42\n").expect("write entry");

    let omitted = topaz()
        .args(["check", "--root"])
        .arg(&dir)
        .output()
        .expect("omitted compiler runs");
    let explicit_rust = topaz()
        .args(["check", "--root"])
        .arg(&dir)
        .args(["--compiler", "rust"])
        .output()
        .expect("explicit Rust runs");
    assert!(omitted.status.success(), "{omitted:?}");
    assert_eq!(omitted.status, explicit_rust.status);
    assert_eq!(omitted.stdout, explicit_rust.stdout);
    assert_eq!(omitted.stderr, explicit_rust.stderr);

    let compatibility = topaz()
        .args(["check", "--root"])
        .arg(&dir)
        .arg("--verbose")
        .output()
        .expect("verbose compatibility runs");
    assert!(compatibility.status.success(), "{compatibility:?}");
    assert_eq!(
        String::from_utf8_lossy(&compatibility.stderr),
        "topaz: compiler selection: rust (compatibility)\n"
    );

    std::fs::remove_file(dir.join("src/main.tpz")).expect("remove target source");
    let explicit_self = topaz()
        .args(["check", "--root"])
        .arg(&dir)
        .args(["--compiler", "self"])
        .output()
        .expect("explicit self declines");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!explicit_self.status.success(), "{explicit_self:?}");
    assert!(explicit_self.stdout.is_empty(), "{explicit_self:?}");
    let stderr = String::from_utf8_lossy(&explicit_self.stderr);
    assert!(
        stderr.contains("exact language profile admitted"),
        "{stderr}"
    );
    assert!(stderr.contains("--compiler rust"), "{stderr}");
    assert!(stderr.contains("not executed"), "{stderr}");
    assert!(!stderr.contains("cannot read"), "{stderr}");
    assert!(!stderr.contains("No such file"), "{stderr}");
}

#[cfg(target_os = "linux")]
#[test]
fn package_selected_test_preserves_unicode_path_for_non_unicode_physical_target() {
    use std::os::unix::ffi::OsStringExt;

    let root = std::env::temp_dir().join(format!(
        "topaz_cli_selected_test_non_unicode_target_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let physical = root.join(std::ffi::OsString::from_vec(vec![b'd', 0xff]));
    std::fs::create_dir_all(root.join("src")).expect("create package source directory");
    std::fs::create_dir_all(root.join("tests")).expect("create package test directory");
    std::fs::create_dir_all(&physical).expect("create physical test directory");
    std::fs::write(root.join("src/main.tpz"), "let packageValue = 1\n")
        .expect("write package entry");
    std::fs::write(
        physical.join("selected.tpz"),
        "print(\"selected linked test\")\n",
    )
    .expect("write physical test module");
    std::os::unix::fs::symlink(physical.join("selected.tpz"), root.join("tests/linked.tpz"))
        .expect("link logical test module");
    let manifest = package_manifest().replace("language = \"5.4\"", "language = \"5.19\"");
    std::fs::write(root.join("topaz.toml"), &manifest).expect("write package manifest");
    std::fs::write(root.join("topaz.lock"), package_lock(&manifest)).expect("write package lock");

    for compiler in ["rust", "self"] {
        let output = topaz()
            .args(["test", "tests/linked.tpz", "--root"])
            .arg(&root)
            .args(["--locked", "--compiler", compiler])
            .output()
            .expect("selected package test runs");
        assert!(output.status.success(), "{compiler}: {output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("selected linked test") && stdout.contains("tests/linked.tpz: test-ok"),
            "{compiler}: {stdout}"
        );
    }

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn package_default_target_enforces_manifest_specific_build_flags() {
    let base = std::env::temp_dir().join("topaz_manifest_target_flag_boundary_test");
    let _ = std::fs::remove_dir_all(&base);
    let web = base.join("web");
    let python = base.join("python");
    std::fs::create_dir_all(web.join("src")).expect("web src");
    std::fs::create_dir_all(python.join("src")).expect("python src");
    std::fs::write(
        web.join("topaz.toml"),
        "[package]\nname = \"web_flags\"\nversion = \"0.1.0\"\nlanguage = \"5.6\"\nentry = \"src/main.tpz\"\n\n[build]\ntarget = \"web-app\"\n",
    )
    .expect("web manifest");
    std::fs::write(web.join("src/main.tpz"), "export const placeholder = 1\n").expect("web source");

    for (name, flag, expected) in [
        ("run", "--run", "cannot be combined with `--run`"),
        ("unchecked", "--unchecked", "requires the checked build"),
        (
            "experimental",
            "--experimental",
            "legacy `--experimental` applies only",
        ),
    ] {
        let output = base.join(format!("web-{name}-out"));
        let out = topaz()
            .args(["build", "--root"])
            .arg(&web)
            .arg(flag)
            .arg("--out-dir")
            .arg(&output)
            .output()
            .expect("binary runs");
        assert!(!out.status.success(), "{name}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(expected),
            "{name}: {out:?}"
        );
    }

    std::fs::write(
        python.join("topaz.toml"),
        "[package]\nname = \"python_flags\"\nversion = \"0.1.0\"\nlanguage = \"5.6\"\nentry = \"src/main.tpz\"\n\n[build]\ntarget = \"python\"\n",
    )
    .expect("python manifest");
    std::fs::write(
        python.join("src/main.tpz"),
        "export function main(args: Array<string>, stdin: string) -> Result<int, string> { Ok(0) }\n",
    )
    .expect("python source");
    for (name, args, expected) in [
        ("run", vec!["--run"], "cannot be combined with `--run`"),
        ("release", vec!["--release"], "`--release` does not apply"),
        (
            "backend",
            vec!["--backend", "boxed"],
            "`--backend` applies to Rust targets",
        ),
    ] {
        let output = base.join(format!("python-{name}-out"));
        let mut command = topaz();
        command.args(["build", "--root"]).arg(&python).args(args);
        let out = command
            .arg("--out-dir")
            .arg(&output)
            .output()
            .expect("binary runs");
        assert!(!out.status.success(), "{name}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(expected),
            "{name}: {out:?}"
        );
    }

    let experimental_output = base.join("python-experimental-out");
    let out = topaz()
        .args(["build", "--root"])
        .arg(&python)
        .arg("--experimental")
        .arg("--out-dir")
        .arg(&experimental_output)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert!(experimental_output.join("program.py").is_file(), "{out:?}");

    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn add_updates_manifest_for_registry_and_local_dependencies() {
    let dir = std::env::temp_dir().join("topaz_add_dependency_test");
    let _ = std::fs::remove_dir_all(&dir);
    let app = dir.join("app");
    let local = dir.join("local_dep");
    std::fs::create_dir_all(app.join("src")).expect("app src");
    std::fs::create_dir_all(local.join("src")).expect("dep src");
    std::fs::write(
        app.join("topaz.toml"),
        r#"[package]
name = "app"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.4"
"#,
    )
    .expect("app manifest");
    std::fs::write(app.join("src/main.tpz"), "print(\"add\")\n").expect("app source");
    std::fs::write(
        local.join("topaz.toml"),
        r#"[package]
name = "local_dep"
version = "0.1.0"
language = "5.4"
entry = "src/lib.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.4"
"#,
    )
    .expect("dep manifest");
    std::fs::write(
        local.join("src/lib.tpz"),
        "export function answer() -> int { 42 }\n",
    )
    .expect("dep source");

    let out = topaz()
        .arg("add")
        .arg("csv_tools@1.2.0")
        .arg("--root")
        .arg(&app)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let out = topaz()
        .arg("add")
        .arg("local_dep")
        .arg("--path")
        .arg("../local_dep")
        .arg("--root")
        .arg(&app)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let manifest = std::fs::read_to_string(app.join("topaz.toml")).expect("manifest");
    assert!(manifest.contains("csv_tools = \"1.2.0\""), "{manifest}");
    assert!(
        manifest.contains("local_dep = { path = \"../local_dep\", hash = \"sha256:"),
        "{manifest}"
    );

    let out = topaz()
        .arg("add")
        .arg("csv_tools@1.2.0")
        .arg("--root")
        .arg(&app)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("already exists"),
        "{out:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn migrate_updates_package_language_metadata_from_v53_to_v54() {
    let dir = std::env::temp_dir().join("topaz_migrate_package_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("src");
    std::fs::write(
        dir.join("topaz.toml"),
        r#"[package]
name = "migrating"
version = "0.1.0"
language = "5.3"
entry = "src/main.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.3"
"#,
    )
    .expect("manifest");
    std::fs::write(dir.join("src/main.tpz"), "let value = 1\n").expect("source");

    let out = topaz()
        .arg("migrate")
        .arg("--from")
        .arg("5.3")
        .arg("--to")
        .arg("5.4")
        .arg("--root")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let manifest = std::fs::read_to_string(dir.join("topaz.toml")).expect("manifest");
    assert!(manifest.contains("language = \"5.4\""), "{manifest}");
    assert!(manifest.contains("std = \"5.4\""), "{manifest}");
    assert!(!manifest.contains("\"5.3\""), "{manifest}");

    let out = topaz()
        .arg("migrate")
        .arg("--from")
        .arg("5.3")
        .arg("--to")
        .arg("5.4")
        .arg("--root")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("package language is 5.4"),
        "{out:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn migrate_entry_checks_parse_compatibility_without_rewriting_source() {
    let path = std::env::temp_dir().join("topaz_migrate_entry.tpz");
    let _ = std::fs::remove_file(&path);
    let original = "let value = 1\n";
    std::fs::write(&path, original).expect("source");

    let out = topaz()
        .arg("migrate")
        .arg("--from")
        .arg("5.3")
        .arg("--to")
        .arg("5.4")
        .arg(&path)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let src = std::fs::read_to_string(&path).expect("source");
    assert_eq!(src, original);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn current_v57_migration_validates_whole_unit_and_updates_only_manifest() {
    let dir = std::env::temp_dir().join("topaz_migrate_dormant_v57_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("src");
    let manifest = r#"[package]
name = "dormant_migration"
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
    let lock = "migration lock sentinel\n";
    std::fs::write(dir.join("topaz.toml"), manifest).expect("manifest");
    std::fs::write(dir.join("src/main.tpz"), entry).expect("entry");
    std::fs::write(dir.join("src/model.tpz"), model).expect("model");
    std::fs::write(dir.join("topaz.lock"), lock).expect("lock");

    let out = topaz()
        .arg("migrate")
        .arg("--from")
        .arg("5.6")
        .arg("--to")
        .arg("5.7")
        .arg("--root")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("migrated package `dormant_migration`"),
        "{out:?}"
    );
    let migrated = std::fs::read_to_string(dir.join("topaz.toml")).unwrap();
    assert!(migrated.contains("language = \"5.7\""), "{migrated}");
    assert!(migrated.contains("std = \"5.7\""), "{migrated}");
    assert_eq!(
        std::fs::read_to_string(dir.join("src/main.tpz")).unwrap(),
        entry
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("src/model.tpz")).unwrap(),
        model
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("topaz.lock")).unwrap(),
        lock
    );

    std::fs::write(dir.join("topaz.toml"), manifest).expect("reset manifest");
    std::fs::write(
        dir.join("src/model.tpz"),
        "export let value: int = \"bad\"\n",
    )
    .expect("invalid model");
    let invalid_model = std::fs::read_to_string(dir.join("src/model.tpz")).unwrap();
    let out = topaz()
        .arg("migrate")
        .arg("--from")
        .arg("5.6")
        .arg("--to")
        .arg("5.7")
        .arg("--root")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("does not check as topaz-5.6"),
        "{out:?}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("topaz.toml")).unwrap(),
        manifest
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("src/model.tpz")).unwrap(),
        invalid_model
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("topaz.lock")).unwrap(),
        lock
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn current_v57_entry_migration_checks_both_modes_without_rewrite() {
    let path = std::env::temp_dir().join("topaz_migrate_dormant_v57_entry.tpz");
    let _ = std::fs::remove_file(&path);
    let original = "let value = ByteBuffer.allocate(4)\n";
    std::fs::write(&path, original).expect("source");

    let out = topaz()
        .arg("migrate")
        .arg("--from")
        .arg("5.6")
        .arg("--to")
        .arg("5.7")
        .arg(&path)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("no source rewrite needed"),
        "{out:?}"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn compatible_v58_package_migration_updates_only_version_metadata() {
    let dir = std::env::temp_dir().join("topaz_migrate_compatible_v58_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("src");
    let manifest = r#"[package]
name = "compatible_v58"
version = "0.1.0"
language = "5.7"
entry = "src/main.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.7"
"#;
    std::fs::write(dir.join("topaz.toml"), manifest).expect("manifest");
    std::fs::write(dir.join("src/main.tpz"), "print(\"ready\")\n").expect("entry");

    let out = topaz()
        .arg("migrate")
        .arg("--from")
        .arg("5.7")
        .arg("--to")
        .arg("5.8")
        .arg("--root")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("migrated package `compatible_v58`"),
        "{out:?}"
    );
    let migrated = std::fs::read_to_string(dir.join("topaz.toml")).unwrap();
    assert!(migrated.contains("language = \"5.8\""), "{migrated}");
    assert!(migrated.contains("std = \"5.8\""), "{migrated}");
    assert_eq!(
        std::fs::read_to_string(dir.join("src/main.tpz")).unwrap(),
        "print(\"ready\")\n"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v59_identity_does_not_add_a_migration_boundary() {
    let out = topaz()
        .arg("migrate")
        .arg("--from")
        .arg("5.8")
        .arg("--to")
        .arg("5.9")
        .arg("main.tpz")
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("`migrate` supports only adjacent adopted boundaries"),
        "{out:?}"
    );
}

#[test]
fn bench_entry_reports_check_pipeline_timing() {
    let path = std::env::temp_dir().join("topaz_bench_entry.tpz");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, "let value: int = 1\n").expect("source");

    let out = topaz()
        .arg("bench")
        .arg(&path)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("bench check-ok"), "{stdout}");
    assert!(stdout.contains("1 module"), "{stdout}");
    assert!(stdout.contains("elapsed_ms="), "{stdout}");

    let out = topaz()
        .arg("bench")
        .arg("--json")
        .arg(&path)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.trim();
    assert!(line.starts_with("{\"status\":\"check-ok\""), "{stdout}");
    assert!(line.contains("\"entry\":"), "{stdout}");
    assert!(line.contains("\"modules\":1"), "{stdout}");
    assert!(line.contains("\"elapsedMs\":"), "{stdout}");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn bench_package_mode_reports_json_timing() {
    let dir = std::env::temp_dir().join("topaz_bench_package_json_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("src");
    let manifest = package_manifest();
    std::fs::write(dir.join("topaz.toml"), manifest).expect("manifest");
    std::fs::write(dir.join("src/main.tpz"), "let value: int = 1\n").expect("entry");

    let out = topaz()
        .arg("bench")
        .arg("--json")
        .arg("--root")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.trim();
    assert!(line.starts_with("{\"status\":\"check-ok\""), "{stdout}");
    assert!(line.contains("\"entry\":\"src/main.tpz\""), "{stdout}");
    assert!(line.contains("\"modules\":1"), "{stdout}");
    assert!(line.contains("\"elapsedMs\":"), "{stdout}");

    let out = topaz()
        .arg("parse")
        .arg("--json")
        .arg(dir.join("src/main.tpz"))
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(
            "`--json` applies to `compiler status`, `explain`, `bench`, and `lispex info` only",
        ),
        "{out:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn doc_generates_deterministic_package_exports() {
    let dir = std::env::temp_dir().join("topaz_doc_package_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("src");
    let manifest = r#"[package]
name = "docpkg"
version = "0.1.0"
language = "5.4"
entry = "src/lib.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.4"
"#;
    std::fs::write(dir.join("topaz.toml"), manifest).expect("manifest");
    std::fs::write(
        dir.join("src/lib.tpz"),
        "/// A user that can be rendered.\n\
         export record User derives Show { name: string }\n\
         /// Stable numeric user identity.\n\
         export newtype UserId = int\n\
         /// Build a greeting for a user.\n\
         export function greet(user: User) -> string { \"hi {user.name}\" }\n",
    )
    .expect("source");

    let out = topaz()
        .arg("lock")
        .arg("--root")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");

    let docs = dir.join("docs-out");
    let out = topaz()
        .arg("doc")
        .arg("--root")
        .arg(&dir)
        .arg("--out-dir")
        .arg(&docs)
        .arg("--locked")
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let json = std::fs::read_to_string(docs.join("exports.json")).expect("exports json");
    assert!(json.contains("\"signatureHash\":\"sha256:"), "{json}");
    assert!(json.contains("\"name\":\"greet\""), "{json}");
    assert!(json.contains("\"records\":[{\"name\":\"User\""), "{json}");
    let index = std::fs::read_to_string(docs.join("index.md")).expect("index");
    assert!(index.contains("# docpkg 0.1.0"), "{index}");
    assert!(index.contains("#### Values"), "{index}");
    assert!(index.contains("`greet`"), "{index}");
    assert!(index.contains("Build a greeting for a user."), "{index}");
    assert!(index.contains("#### Records"), "{index}");
    assert!(index.contains("`User`"), "{index}");
    assert!(index.contains("A user that can be rendered."), "{index}");
    assert!(index.contains("#### Newtypes"), "{index}");
    assert!(index.contains("`UserId`"), "{index}");
    assert!(index.contains("Stable numeric user identity."), "{index}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn package_mode_loads_manifest_entry_and_verifies_lock() {
    // v5.4 packages: no-entry check/run/emit load topaz.toml from the package
    // root, then feed the manifest entry through the same compiler path as an
    // explicit CLI entry. --locked pins topaz.lock's root manifest_hash.
    let dir = std::env::temp_dir().join("topaz_package_mode_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("mkdir");
    let manifest = package_manifest();
    std::fs::write(dir.join("topaz.toml"), &manifest).expect("manifest");
    std::fs::write(dir.join("topaz.lock"), package_lock(&manifest)).expect("lock");
    std::fs::write(dir.join("src/main.tpz"), "print(\"package ok\")\n").expect("entry");

    let out = topaz()
        .arg("check")
        .arg("--root")
        .arg(&dir)
        .arg("--locked")
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");

    let out = topaz()
        .current_dir(&dir)
        .arg("run")
        .arg("--locked")
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("package ok"),
        "{out:?}"
    );

    let out = topaz()
        .current_dir(&dir)
        .arg("emit")
        .arg("--locked")
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("pub fn run_with_host"),
        "{out:?}"
    );

    std::fs::write(
        dir.join("topaz.lock"),
        package_lock(&manifest).replace(
            &topaz_package::manifest_sha256(&manifest),
            "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        ),
    )
    .expect("stale lock");
    let out = topaz()
        .arg("check")
        .arg("--root")
        .arg(&dir)
        .arg("--locked")
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("manifest_hash is stale"),
        "{out:?}"
    );

    let out = topaz()
        .arg("check")
        .arg("--locked")
        .arg(dir.join("src/main.tpz"))
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("package-mode"),
        "{out:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
