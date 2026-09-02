use super::*;
use crate::*;

#[cfg(unix)]
#[test]
fn package_content_path_rejects_non_unicode_source_path() {
    use std::os::unix::ffi::OsStringExt;

    let relative = Path::new("src").join(std::ffi::OsString::from_vec(b"module-\xff.tpz".to_vec()));
    let error = package_content_relative_path(&relative)
        .expect_err("non-Unicode package source path rejects");
    assert!(
        error.message().contains("cannot be represented as Unicode"),
        "{error}"
    );
}

#[cfg(unix)]
#[test]
fn package_content_hash_rejects_a_non_regular_topaz_source() {
    use std::os::unix::net::UnixListener;

    let root = PathBuf::from(format!("/tmp/tpz-pkg-source-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("short Unix package root");
    write_file(
        &root,
        "topaz.toml",
        r#"[package]
name = "non_regular"
version = "0.1.0"
language = "5.19"
entry = "src/main.tpz"
"#,
    );
    std::fs::create_dir_all(root.join("src")).expect("source directory");
    let listener = UnixListener::bind(root.join("src/main.tpz")).expect("source socket");

    let error = package_content_hash(&root)
        .expect_err("a non-regular Topaz source must not disappear from its package hash");
    assert!(
        error.message().contains("must be a regular file"),
        "{error}"
    );
    drop(listener);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn package_content_hash_keeps_the_v1_portable_byte_contract() {
    let root = temp_root("content-hash-v1");
    write_file(
        &root,
        "topaz.toml",
        r#"[package]
name = "hash_contract"
version = "0.1.0"
language = "5.19"
entry = "src/main.tpz"
"#,
    );
    write_file(&root, "src/main.tpz", "export const value = 42\n");

    let hash = package_content_hash(&root).expect("portable package hash");
    assert_eq!(
        hash,
        "sha256:09da93376463ae5f11328b17a5901af246d944315faf7a96a3898704ea921874"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn registry_candidate_identity_and_hash_reject_before_existing_vendor_replacement() {
    let root = temp_root("vendor-candidate-identity");
    let source = temp_root("vendor-candidate-identity-source");
    write_file(
        &source,
        "topaz.toml",
        r#"[package]
name = "imposter"
version = "1.0.0"
language = "5.19"
entry = "src/lib.tpz"
"#,
    );
    write_file(&source, "src/lib.tpz", "export const value = 2\n");
    write_file(
        &root,
        "vendor/greeter/1.0.0/src/lib.tpz",
        "export const value = 1\n",
    );

    let error = replace_registry_vendor_package(&root, &source, "greeter", "1.0.0", None)
        .expect_err("a mismatched registry candidate must reject");
    assert!(
        error.message().contains("has [package] `imposter`"),
        "{error}"
    );
    let existing = std::fs::read_to_string(root.join("vendor/greeter/1.0.0/src/lib.tpz"))
        .expect("existing vendor remains readable");
    assert_eq!(existing, "export const value = 1\n");

    write_file(
        &source,
        "topaz.toml",
        r#"[package]
name = "greeter"
version = "1.0.0"
language = "5.19"
entry = "src/lib.tpz"
"#,
    );
    let error = replace_registry_vendor_package(&root, &source, "greeter", "1.0.0", Some(HASH))
        .expect_err("a stale registry candidate hash must reject");
    assert!(error.message().contains("content hash is stale"), "{error}");
    let existing = std::fs::read_to_string(root.join("vendor/greeter/1.0.0/src/lib.tpz"))
        .expect("existing vendor remains readable after hash rejection");
    assert_eq!(existing, "export const value = 1\n");
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(source);
}

#[cfg(unix)]
#[test]
fn registry_vendor_parent_link_rejects_without_touching_outside_content() {
    use std::os::unix::fs::symlink;

    let root = temp_root("vendor-parent-link");
    let source = temp_root("vendor-parent-link-source");
    let outside = temp_root("vendor-parent-link-outside");
    write_file(
        &source,
        "topaz.toml",
        r#"[package]
name = "greeter"
version = "1.0.0"
language = "5.19"
entry = "src/lib.tpz"
"#,
    );
    write_file(&source, "src/lib.tpz", "export const value = 2\n");
    write_file(
        &outside,
        "1.0.0/src/lib.tpz",
        "outside vendor must stay unchanged\n",
    );
    std::fs::create_dir_all(root.join("vendor")).expect("vendor directory");
    symlink(&outside, root.join("vendor/greeter")).expect("linked vendor parent");

    let error = replace_registry_vendor_package(&root, &source, "greeter", "1.0.0", None)
        .expect_err("a linked vendor parent must reject");
    assert!(error.message().contains("not a plain directory"), "{error}");
    let outside_source = std::fs::read_to_string(outside.join("1.0.0/src/lib.tpz"))
        .expect("outside source remains readable");
    assert_eq!(outside_source, "outside vendor must stay unchanged\n");
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(source);
    let _ = std::fs::remove_dir_all(outside);
}
