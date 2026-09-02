use super::support::*;

#[test]
fn vendor_copies_registry_dependency_and_locked_run_uses_it() {
    let dir = std::env::temp_dir().join("topaz_vendor_registry_test");
    let _ = std::fs::remove_dir_all(&dir);
    let app = dir.join("app");
    let registry_pkg = dir.join("registry/greeter/1.0.0");
    std::fs::create_dir_all(app.join("src")).expect("app src");
    std::fs::create_dir_all(registry_pkg.join("src")).expect("dep src");

    std::fs::write(
        registry_pkg.join("topaz.toml"),
        r#"[package]
name = "greeter"
version = "1.0.0"
language = "5.4"
entry = "src/lib.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.4"

[exports]
module = "src/lib.tpz"
"#,
    )
    .expect("dep manifest");
    std::fs::write(
        registry_pkg.join("src/lib.tpz"),
        "export function greet() -> string { \"hello vendored\" }\n",
    )
    .expect("dep source");

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
greeter = "1.0.0"
"#,
    )
    .expect("app manifest");
    std::fs::write(
        app.join("src/main.tpz"),
        "import greeter { greet }\n\
         export function main(args: Array<string>, stdin: string) -> Result<int, string> {\n\
             print(greet())\n\
             Ok(0)\n\
         }\n",
    )
    .expect("app entry");

    let out = topaz()
        .arg("run")
        .arg("--root")
        .arg(&app)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("requires `--locked`"),
        "{out:?}"
    );

    let out = topaz()
        .arg("vendor")
        .arg("--root")
        .arg(&app)
        .arg("--from")
        .arg(dir.join("registry"))
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert!(
        app.join("vendor/greeter/1.0.0/src/lib.tpz").exists(),
        "vendored dependency copied"
    );
    let lock = std::fs::read_to_string(app.join("topaz.lock")).expect("lock");
    assert!(lock.contains("name = \"greeter\""), "{lock}");
    assert!(lock.contains("source = \"registry\""), "{lock}");

    let out = topaz()
        .arg("vendor")
        .arg("--root")
        .arg(&app)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");

    std::fs::remove_dir_all(dir.join("registry")).expect("remove source registry");

    let out = topaz()
        .arg("run")
        .arg("--root")
        .arg(&app)
        .arg("--locked")
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello vendored\n");

    let out_dir = app.join("out-no-network");
    let out = topaz()
        .arg("build")
        .arg("--root")
        .arg(&app)
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--locked")
        .arg("--run")
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello vendored\n");

    std::fs::write(
        app.join("vendor/greeter/1.0.0/src/lib.tpz"),
        "export function greet() -> string { \"drift\" }\n",
    )
    .expect("drift vendor");
    let out = topaz()
        .arg("check")
        .arg("--root")
        .arg(&app)
        .arg("--locked")
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("content hash is stale"),
        "{out:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn vendor_rejects_a_linked_destination_parent_without_touching_outside_content() {
    use std::os::unix::fs::symlink;

    let dir = std::env::temp_dir().join(format!(
        "topaz_vendor_parent_link_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let app = dir.join("app");
    let registry_pkg = dir.join("registry/greeter/1.0.0");
    let outside = dir.join("outside");
    std::fs::create_dir_all(app.join("src")).expect("app src");
    std::fs::create_dir_all(registry_pkg.join("src")).expect("registry src");
    std::fs::create_dir_all(outside.join("1.0.0/src")).expect("outside source");
    std::fs::write(
        registry_pkg.join("topaz.toml"),
        r#"[package]
name = "greeter"
version = "1.0.0"
language = "5.19"
entry = "src/lib.tpz"
"#,
    )
    .expect("registry manifest");
    std::fs::write(registry_pkg.join("src/lib.tpz"), "export const value = 2\n")
        .expect("registry source");
    std::fs::write(
        app.join("topaz.toml"),
        r#"[package]
name = "app"
version = "0.1.0"
language = "5.19"
entry = "src/main.tpz"

[dependencies]
std = "5.19"
greeter = "1.0.0"
"#,
    )
    .expect("app manifest");
    std::fs::write(app.join("src/main.tpz"), "import greeter { value }\n").expect("app source");
    std::fs::write(
        outside.join("1.0.0/src/lib.tpz"),
        "outside vendor must stay unchanged\n",
    )
    .expect("outside source");
    std::fs::create_dir_all(app.join("vendor")).expect("vendor directory");
    symlink(&outside, app.join("vendor/greeter")).expect("linked vendor parent");

    let out = topaz()
        .arg("vendor")
        .arg("--root")
        .arg(&app)
        .arg("--from")
        .arg(dir.join("registry"))
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not a plain directory"),
        "{out:?}"
    );
    let outside_source = std::fs::read_to_string(outside.join("1.0.0/src/lib.tpz"))
        .expect("outside source remains readable");
    assert_eq!(outside_source, "outside vendor must stay unchanged\n");
    assert!(!app.join("topaz.lock").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn failed_vendor_lock_publish_restores_the_previous_vendor_and_outside_lock() {
    use std::os::unix::fs::symlink;

    let dir = std::env::temp_dir().join(format!(
        "topaz_vendor_lock_rollback_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let app = dir.join("app");
    let registry_pkg = dir.join("registry/greeter/1.0.0");
    let existing_vendor = app.join("vendor/greeter/1.0.0");
    let outside_lock = dir.join("outside.lock");
    std::fs::create_dir_all(app.join("src")).expect("app src");
    std::fs::create_dir_all(registry_pkg.join("src")).expect("registry src");
    std::fs::create_dir_all(existing_vendor.join("src")).expect("existing vendor src");
    let dependency_manifest = r#"[package]
name = "greeter"
version = "1.0.0"
language = "5.19"
entry = "src/lib.tpz"
"#;
    std::fs::write(registry_pkg.join("topaz.toml"), dependency_manifest)
        .expect("registry manifest");
    std::fs::write(registry_pkg.join("src/lib.tpz"), "export const value = 2\n")
        .expect("registry source");
    std::fs::write(existing_vendor.join("topaz.toml"), dependency_manifest)
        .expect("existing vendor manifest");
    std::fs::write(
        existing_vendor.join("src/lib.tpz"),
        "existing vendor must stay unchanged\n",
    )
    .expect("existing vendor source");
    std::fs::write(
        app.join("topaz.toml"),
        r#"[package]
name = "app"
version = "0.1.0"
language = "5.19"
entry = "src/main.tpz"

[dependencies]
std = "5.19"
greeter = "1.0.0"
"#,
    )
    .expect("app manifest");
    std::fs::write(app.join("src/main.tpz"), "import greeter { value }\n").expect("app source");
    std::fs::write(&outside_lock, "outside lock must stay unchanged\n").expect("outside lock");
    symlink(&outside_lock, app.join("topaz.lock")).expect("linked lock");

    let out = topaz()
        .arg("vendor")
        .arg("--root")
        .arg(&app)
        .arg("--from")
        .arg(dir.join("registry"))
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("refusing to replace non-regular"),
        "{out:?}"
    );
    let vendor_source = std::fs::read_to_string(existing_vendor.join("src/lib.tpz"))
        .expect("restored vendor source");
    assert_eq!(vendor_source, "existing vendor must stay unchanged\n");
    let outside_text = std::fs::read_to_string(&outside_lock).expect("outside lock readable");
    assert_eq!(outside_text, "outside lock must stay unchanged\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(target_os = "linux")]
#[test]
fn package_source_paths_reject_non_unicode_without_replacing_existing_vendor() {
    use std::os::unix::ffi::OsStringExt;

    let dir = std::env::temp_dir().join(format!(
        "topaz_non_unicode_package_source_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let app = dir.join("app");
    let registry_pkg = dir.join("registry/greeter/1.0.0");
    let existing_vendor = app.join("vendor/greeter/1.0.0");
    std::fs::create_dir_all(app.join("src")).expect("app src");
    std::fs::create_dir_all(registry_pkg.join("src")).expect("registry src");
    std::fs::create_dir_all(existing_vendor.join("src")).expect("existing vendor src");
    let package_manifest = r#"[package]
name = "greeter"
version = "1.0.0"
language = "5.19"
entry = "src/lib.tpz"

[dependencies]
std = "5.19"
"#;
    std::fs::write(registry_pkg.join("topaz.toml"), package_manifest).expect("registry manifest");
    std::fs::write(registry_pkg.join("src/lib.tpz"), "export const value = 2\n")
        .expect("registry source");
    let invalid = std::ffi::OsString::from_vec(b"module-\xff.tpz".to_vec());
    std::fs::write(
        registry_pkg.join("src").join(invalid),
        "export const hidden = 3\n",
    )
    .expect("invalid-byte registry source");
    std::fs::write(existing_vendor.join("topaz.toml"), package_manifest)
        .expect("existing vendor manifest");
    std::fs::write(
        existing_vendor.join("src/lib.tpz"),
        "export const value = 1\n",
    )
    .expect("existing vendor source");
    std::fs::write(
        app.join("topaz.toml"),
        r#"[package]
name = "app"
version = "0.1.0"
language = "5.19"
entry = "src/main.tpz"

[dependencies]
std = "5.19"
greeter = "1.0.0"
"#,
    )
    .expect("app manifest");
    std::fs::write(app.join("src/main.tpz"), "import greeter { value }\n").expect("app source");

    let hash_error = topaz_package::package_content_hash(&registry_pkg)
        .expect_err("hash rejects non-Unicode package source path");
    let out = topaz()
        .arg("vendor")
        .arg("--root")
        .arg(&app)
        .arg("--from")
        .arg(dir.join("registry"))
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    for message in [hash_error.message(), stderr.as_ref()] {
        assert!(
            message.contains("package content path")
                && message.contains("cannot be represented as Unicode"),
            "{message}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(existing_vendor.join("src/lib.tpz"))
            .expect("preserved vendor source"),
        "export const value = 1\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fetch_copies_registry_dependency_and_locked_check_uses_it() {
    let dir = std::env::temp_dir().join("topaz_fetch_registry_test");
    let _ = std::fs::remove_dir_all(&dir);
    let app = dir.join("app");
    let registry_pkg = dir.join("registry/helper/0.2.0");
    std::fs::create_dir_all(app.join("src")).expect("app src");
    std::fs::create_dir_all(registry_pkg.join("src")).expect("dep src");

    std::fs::write(
        registry_pkg.join("topaz.toml"),
        r#"[package]
name = "helper"
version = "0.2.0"
language = "5.4"
entry = "src/lib.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.4"

[exports]
module = "src/lib.tpz"
"#,
    )
    .expect("dep manifest");
    std::fs::write(
        registry_pkg.join("src/lib.tpz"),
        "export function answer() -> int { 42 }\n",
    )
    .expect("dep source");

    std::fs::write(
        app.join("topaz.toml"),
        r#"[package]
name = "fetchapp"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.4"
helper = "0.2.0"
"#,
    )
    .expect("app manifest");
    std::fs::write(
        app.join("src/main.tpz"),
        "import helper { answer }\n\
         export function main(args: Array<string>, stdin: string) -> Result<int, string> {\n\
             Ok(answer())\n\
         }\n",
    )
    .expect("app entry");

    let out = topaz()
        .arg("fetch")
        .arg("--root")
        .arg(&app)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("requires `--from <local-registry>`"),
        "{out:?}"
    );

    let out = topaz()
        .arg("fetch")
        .arg("--root")
        .arg(&app)
        .arg("--from")
        .arg(dir.join("registry"))
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("fetched 1 registry package"),
        "{out:?}"
    );
    assert!(
        app.join("vendor/helper/0.2.0/src/lib.tpz").exists(),
        "fetched dependency copied"
    );
    let lock = std::fs::read_to_string(app.join("topaz.lock")).expect("lock");
    assert!(lock.contains("name = \"helper\""), "{lock}");
    assert!(lock.contains("source = \"registry\""), "{lock}");

    std::fs::write(
        registry_pkg.join("src/lib.tpz"),
        "export function answer() -> int { 0 }\n",
    )
    .expect("mutate source registry");

    let out = topaz()
        .arg("check")
        .arg("--root")
        .arg(&app)
        .arg("--locked")
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");

    let _ = std::fs::remove_dir_all(&dir);
}
