use super::*;
use topaz_value::value::{builtin_fs_write_bytes, builtin_fs_write_text, render};

#[test]
fn missing_file_capability_root_can_be_created() {
    let root = super::tests::temp_root("file-root");
    let host = NativeHost::with_fs_capabilities(&root, &[], &["new.txt".to_string()]);
    host.write_bytes("new.txt", "한글".as_bytes()).unwrap();
    host.write_bytes("new.txt", b"x").unwrap();
    assert_eq!(std::fs::read(root.join("new.txt")).unwrap(), b"x");
    assert!(host.write_bytes("other.txt", b"bad").is_err());
    assert!(!root.join("other.txt").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(any(windows, unix))]
#[test]
fn writes_allow_link_parents_inside_write_roots() {
    let root = super::tests::temp_root("inside-link");
    std::fs::create_dir(root.join("out")).unwrap();
    std::fs::create_dir(root.join("out/real")).unwrap();
    link_directory(&root.join("out/real"), &root.join("out/link"));
    let host = NativeHost::with_fs_capabilities(&root, &[], &["out".to_string()]);
    host.write_bytes("out/link/new", b"inside").unwrap();
    assert_eq!(std::fs::read(root.join("out/real/new")).unwrap(), b"inside");
    #[cfg(windows)]
    std::fs::remove_dir(root.join("out/link")).unwrap();
    #[cfg(unix)]
    std::fs::remove_file(root.join("out/link")).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
fn link_directory(target: &Path, link: &Path) {
    // Junctions do not require the symlink privilege on Windows.
    let result = std::process::Command::new("cmd")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(link.to_string_lossy().replace('/', "\\"))
        .arg(target.to_string_lossy().replace('/', "\\"))
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
}

#[cfg(unix)]
fn link_directory(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(any(windows, unix))]
#[test]
fn writes_reject_external_link_parents_and_preserve_outside_files() {
    let root = super::tests::temp_root("link-parent-base");
    let outside = super::tests::temp_root("link-parent-outside");
    std::fs::create_dir(root.join("out")).unwrap();
    link_directory(&outside, &root.join("out/link"));
    std::fs::write(outside.join("existing"), "untouched").unwrap();
    let span = topaz_value::Span::new(topaz_value::FileId(0), 0, 0);
    let mut failures = Vec::new();
    for (index, reads, writes) in [
        (0, vec![], vec!["out".to_string()]),
        (1, vec![".".to_string()], vec!["out".to_string()]),
        (2, vec!["out".to_string()], vec![]),
    ] {
        let host = NativeHost::with_fs_capabilities(&root, &reads, &writes);
        for bytes in [false, true] {
            for suffix in [format!("fresh-{index}-{bytes}"), "existing".to_string()] {
                let path = format!("out/link/{suffix}");
                let result = if bytes {
                    builtin_fs_write_bytes(
                        &host,
                        Value::str(&path),
                        Value::Bytes(std::rc::Rc::from(&b"bad"[..])),
                        span,
                    )
                } else {
                    builtin_fs_write_text(&host, Value::str(&path), Value::str("bad"), span)
                }
                .unwrap();
                let disk_ok = if suffix == "existing" {
                    std::fs::read(outside.join(&suffix)).unwrap() == b"untouched"
                } else {
                    !outside.join(&suffix).exists()
                };
                if !matches!(result, Value::Err(_)) || !disk_ok {
                    failures.push(format!("read={reads:?}, write={writes:?}, bytes={bytes}, {path}: {}, outside preserved={disk_ok}", render(&result)));
                }
            }
        }
        assert!(host.write_bytes("../outside", b"bad").is_err());
        // Missing intermediate directories must still be resolved through the
        // closest existing parent, before the OS attempts file creation.
        if host.resolve_open("out/link/missing/fresh").is_ok() {
            failures.push(format!(
                "read={reads:?}, write={writes:?}: missing ancestor escaped"
            ));
        }
    }
    #[cfg(windows)]
    std::fs::remove_dir(root.join("out/link")).unwrap();
    #[cfg(unix)]
    std::fs::remove_file(root.join("out/link")).unwrap();
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[cfg(any(windows, unix))]
#[test]
fn handle_write_rechecks_parent_after_open() {
    let root = super::tests::temp_root("handle-parent-base");
    let outside = super::tests::temp_root("handle-parent-outside");
    std::fs::create_dir(root.join("out")).unwrap();
    std::fs::create_dir(root.join("out/parent")).unwrap();
    std::fs::write(root.join("out/parent/file"), "inside").unwrap();
    let host = NativeHost::with_fs_capabilities(&root, &[], &["out".to_string()]);
    let handle = host.open("out/parent/file").unwrap();
    std::fs::remove_file(root.join("out/parent/file")).unwrap();
    std::fs::remove_dir(root.join("out/parent")).unwrap();
    link_directory(&outside, &root.join("out/parent"));
    let result = host.write(handle, "bad");
    let escaped = outside.join("file").exists();
    #[cfg(windows)]
    std::fs::remove_dir(root.join("out/parent")).unwrap();
    #[cfg(unix)]
    std::fs::remove_file(root.join("out/parent")).unwrap();
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
    assert!(
        result.is_err() && !escaped,
        "result={result:?}, outside created={escaped}"
    );
}

#[cfg(any(windows, unix))]
#[test]
fn read_permission_does_not_authorize_writes_through_links() {
    let root = super::tests::temp_root("read-vs-write");
    std::fs::create_dir(root.join("out")).unwrap();
    std::fs::create_dir(root.join("private")).unwrap();
    link_directory(&root.join("private"), &root.join("out/link"));
    std::fs::write(root.join("private/existing"), "readable").unwrap();
    let host = NativeHost::with_fs_capabilities(&root, &[".".to_string()], &["out".to_string()]);
    assert_eq!(host.read_bytes("out/link/existing").unwrap(), b"readable");
    let fresh = host.write_bytes("out/link/fresh", b"bad");
    let existing = host.write_bytes("out/link/existing", b"bad");
    let preserved = std::fs::read(root.join("private/existing")).unwrap() == b"readable"
        && !root.join("private/fresh").exists();
    #[cfg(windows)]
    std::fs::remove_dir(root.join("out/link")).unwrap();
    #[cfg(unix)]
    std::fs::remove_file(root.join("out/link")).unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert!(
        fresh.is_err() && existing.is_err() && preserved,
        "fresh={fresh:?}, existing={existing:?}, preserved={preserved}"
    );
}
