use super::*;
use std::collections::BTreeSet;

/// The runtime closure crates the emitted tree vendors (CDR-006 §7). Hardcoded
/// so adding/removing a closure crate (or its build.rs CLOSURE list) without
/// updating BOTH places fails this gate.
const EXPECTED_CLOSURE: &[&str] = &[
    "topaz_diag",
    "topaz_syntax",
    "topaz_value",
    "topaz_product_runtime",
    "topaz_rt",
    "topaz_host_native",
];

fn ws() -> PathBuf {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).to_path_buf()
}

fn collect_rs(crate_name: &str, crate_dir: &Path, dir: &Path, out: &mut BTreeSet<String>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .expect("closure src dir")
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            collect_rs(crate_name, crate_dir, &p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            let rel = p
                .strip_prefix(crate_dir)
                .expect("under crate dir")
                .to_string_lossy()
                .replace('\\', "/");
            out.insert(format!("{crate_name}/{rel}"));
        }
    }
}

/// CDR-006 §7 identity gate: the embedded `VENDOR_FILES` must be EXACTLY the
/// closure crates' `Cargo.toml` + every `src/**/*.rs`, byte-identical to the
/// live workspace. Runs in CI (3-OS) via `cargo test`.
#[test]
fn vendored_closure_is_byte_identical_to_workspace() {
    let mut expected = BTreeSet::new();
    for c in EXPECTED_CLOSURE {
        let crate_dir = ws().join("crates").join(c);
        expected.insert(format!("{c}/Cargo.toml"));
        collect_rs(c, &crate_dir, &crate_dir.join("src"), &mut expected);
    }
    let mut vendored = BTreeSet::new();
    for (rel, _) in VENDOR_FILES {
        assert!(
            vendored.insert((*rel).to_string()),
            "duplicate vendored relative path: {rel}"
        );
    }
    assert_eq!(
        vendored, expected,
        "the embedded vendored closure set drifted from the workspace"
    );
    for (rel, bytes) in VENDOR_FILES {
        let live = fs::read(ws().join("crates").join(rel))
            .unwrap_or_else(|e| panic!("read workspace `{rel}`: {e}"));
        assert_eq!(
            &live[..],
            *bytes,
            "embedded `{rel}` is not byte-identical to the workspace"
        );
    }
}

#[test]
fn web_app_files_reject_remote_css_and_escape_symlinks() {
    let root = std::env::temp_dir().join("topaz_web_app_input_safety_test");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("styles")).expect("styles");
    fs::create_dir_all(root.join("assets")).expect("assets");
    fs::write(root.join("styles/app.css"), "body { color: black; }\n").expect("css");
    fs::write(root.join("assets/logo.svg"), "<svg/>\n").expect("asset");
    let config = topaz_package::WebConfig {
        title: "Safe".into(),
        styles: vec!["styles/app.css".into()],
        assets: vec!["assets/logo.svg".into()],
        lifecycle: topaz_package::WebLifecycle::V1,
    };
    let capabilities = topaz_package::WebCapabilities::default();
    let files = web_app_files(&root, "safe-app", &config, &capabilities).expect("safe inputs");
    assert!(files.iter().any(|file| file.path == "styles/app.css"));
    assert!(files.iter().any(|file| file.path == "assets/logo.svg"));

    fs::write(
        root.join("styles/app.css"),
        "@import url(https://example.com/app.css);\n",
    )
    .expect("remote css");
    let error =
        web_app_files(&root, "safe-app", &config, &capabilities).expect_err("CSS import rejects");
    assert!(error.contains("unsupported CSS @import"), "{error}");

    fs::write(
        root.join("styles/app.css"),
        "body { background: url(  HTTPS://example.com/app.png ); }\n",
    )
    .expect("remote CSS URL");
    let error =
        web_app_files(&root, "safe-app", &config, &capabilities).expect_err("remote URL rejects");
    assert!(error.contains("remote or escaped CSS url()"), "{error}");

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        fs::write(root.join("outside.txt"), "outside").expect("outside");
        symlink(root.join("outside.txt"), root.join("assets/escape.txt")).expect("symlink");
        let symlink_config = topaz_package::WebConfig {
            title: "Safe".into(),
            styles: Vec::new(),
            assets: vec!["assets/escape.txt".into()],
            lifecycle: topaz_package::WebLifecycle::V1,
        };
        let error = web_app_files(&root, "safe-app", &symlink_config, &capabilities)
            .expect_err("symlink input rejects");
        assert!(error.contains("must not be a symlink"), "{error}");
    }
    let _ = fs::remove_dir_all(root);
}

#[cfg(target_os = "linux")]
#[test]
fn web_app_files_reject_non_unicode_discovered_path() {
    use std::os::unix::ffi::OsStringExt;

    let root = std::env::temp_dir().join("topaz_web_app_non_unicode_input_test");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("assets")).expect("assets");
    let file_name = std::ffi::OsString::from_vec(b"logo-\xff.svg".to_vec());
    fs::write(root.join("assets").join(file_name), "<svg/>\n").expect("asset");
    let config = topaz_package::WebConfig {
        title: "Safe".into(),
        styles: Vec::new(),
        assets: vec!["assets".into()],
        lifecycle: topaz_package::WebLifecycle::V1,
    };

    let error = web_app_files(
        &root,
        "safe-app",
        &config,
        &topaz_package::WebCapabilities::default(),
    )
    .expect_err("non-Unicode artifact path rejects");
    assert!(
        error.contains("cannot be represented as a Unicode artifact path"),
        "{error}"
    );
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn web_artifact_path_rejects_non_unicode_path() {
    use std::os::unix::ffi::OsStringExt;

    let relative =
        Path::new("assets").join(std::ffi::OsString::from_vec(b"logo-\xff.svg".to_vec()));
    let error = web_artifact_path(&relative).expect_err("non-Unicode path rejects");
    assert!(
        error.contains("cannot be represented as a Unicode artifact path"),
        "{error}"
    );
}

#[test]
fn web_app_watch_inputs_exclude_generated_output_and_include_declared_inputs() {
    let root = std::env::temp_dir().join("topaz_web_app_watch_boundary_test");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).expect("src");
    fs::create_dir_all(root.join("styles")).expect("styles");
    fs::create_dir_all(root.join("assets/icons")).expect("assets");
    fs::create_dir_all(root.join("dist/styles")).expect("dist");
    fs::write(root.join("src/main.tpz"), "export const n = 1\n").expect("source");
    fs::write(root.join("styles/app.css"), "body {}\n").expect("style");
    fs::write(root.join("assets/icons/logo.svg"), "<svg/>\n").expect("asset");
    fs::write(root.join("dist/styles/app.css"), "generated\n").expect("generated");

    let mut inputs = Vec::new();
    collect_topaz_sources(&root, &mut inputs);
    collect_declared_web_inputs(&root.join("styles/app.css"), &mut inputs);
    collect_declared_web_inputs(&root.join("assets"), &mut inputs);
    assert!(inputs.contains(&root.join("src/main.tpz")), "{inputs:?}");
    assert!(inputs.contains(&root.join("styles/app.css")), "{inputs:?}");
    assert!(
        inputs.contains(&root.join("assets/icons/logo.svg")),
        "{inputs:?}"
    );
    assert!(
        !inputs.contains(&root.join("dist/styles/app.css")),
        "{inputs:?}"
    );
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn dev_server_rejects_symlink_escape_from_product_root() {
    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join("topaz_dev_server_symlink_test");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("product")).expect("product");
    fs::write(root.join("outside.txt"), "secret").expect("outside");
    symlink(root.join("outside.txt"), root.join("product/escape.txt")).expect("symlink");

    assert_eq!(
        resolve_dev_file(&root.join("product"), Path::new("escape.txt")),
        Err(400)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn classifier_buckets_environment_vs_internal() {
    for s in [
        "error: linking with `cc` failed: exit status: 1",
        "error: linker `cc` not found\n  = note: No such file or directory (os error 2)",
        "= note: ld: library not found for -lfoo",
        "error: linking with link.exe failed",
        "xcrun: error: invalid active developer path",
        "error: No space left on device (os error 28)",
        "Permission denied (os error 13)",
        "error: toolchain '1.96.0-x86_64' is not installed",
        "error[E0463]: can't find crate for `std`",
        // cargo could not run rustc itself (toolchain missing / not on PATH):
        // a user environment problem, NOT a Topaz emission defect.
        "error: could not execute process `rustc -vV` (never executed)\n  No such file or directory (os error 2)",
    ] {
        assert!(
            matches!(classify_text(s), CargoFailure::Environment(_)),
            "expected environment for: {s}"
        );
    }
    for s in [
        "error[E0308]: mismatched types",
        "error: cannot find function `foo` in this scope",
        "thread 'main' panicked at 'x'",
        // a generated-Rust error whose source snippet contains `ld: ` inside an
        // identifier (`let field: Value`) must NOT be misread as a linker error.
        "error[E0412]: cannot find type `Value`\n  |\n5 |     let field: Value = compute();",
        // a USER STRING in a source snippet (`"permission denied"`) on an
        // internal error line must NOT be misread as an environment failure.
        "error[E0382]: borrow of moved value\n  |\n7 |     print(\"permission denied\")",
        // the phrase "could not execute process" inside an echoed source
        // snippet (not cargo's own `error:` line) must stay Internal — the
        // toolchain token is anchored to cargo's `error:` prefix.
        "error[E0384]: cannot assign twice\n  |\n7 |     let msg = \"could not execute process\"",
    ] {
        assert_eq!(
            classify_text(s),
            CargoFailure::Internal,
            "expected internal for: {s}"
        );
    }
}

#[test]
fn pinned_build_toolchain_resolves_direct_cargo_and_rustc_paths() {
    let (channel, cargo, rustc) = validate_build_toolchain().expect("pinned build toolchain");
    assert_eq!(channel, pinned_channel().expect("embedded channel"));
    assert!(
        cargo.is_absolute(),
        "cargo path must be absolute: {cargo:?}"
    );
    assert!(
        rustc.is_absolute(),
        "rustc path must be absolute: {rustc:?}"
    );
    assert_ne!(cargo, rustc, "cargo and rustc paths must remain distinct");
    assert_eq!(
        cargo.file_stem().and_then(|name| name.to_str()),
        Some("cargo")
    );
    assert_eq!(
        rustc.file_stem().and_then(|name| name.to_str()),
        Some("rustc")
    );
}

#[test]
fn classifier_reports_an_msrv_mismatch_not_a_bug() {
    // cargo's real refusal when the active rustc is below the vendored crates'
    // pinned `rust-version`. This is a toolchain problem, not a Topaz defect.
    let s = "error: rustc 1.95.0 is not supported by the following packages:\n  \
                 topaz_rt@5.4.0-dev requires rustc 1.96.0\n  topaz_value@5.4.0-dev requires rustc 1.96.0";
    match classify_text(s) {
        CargoFailure::Msrv(remedy) => {
            assert!(
                remedy.contains("1.95.0"),
                "names the active version: {remedy}"
            );
            assert!(
                remedy.contains("1.96.0"),
                "names the required version: {remedy}"
            );
            assert!(
                remedy.contains("rustup update"),
                "offers an actionable fix: {remedy}"
            );
        }
        other => panic!("expected Msrv, got {other:?}"),
    }
    // Cargo's other MSRV phrasing (a single line, no "is not supported" header):
    // the active version comes from "currently active rustc version is <X>".
    let b = "error: package `clap_derive v4.2.0` cannot be built because it requires \
                 rustc 1.64.0 or newer, while the currently active rustc version is 1.63.0";
    match classify_text(b) {
        CargoFailure::Msrv(remedy) => {
            assert!(
                remedy.contains("1.63.0"),
                "names the active version: {remedy}"
            );
            assert!(
                remedy.contains("1.64.0"),
                "names the required version: {remedy}"
            );
        }
        other => panic!("expected Msrv for format B, got {other:?}"),
    }
    // a bare "requires rustc" note WITHOUT either active-too-old signal is not an
    // MSRV failure (no false positive); it stays Internal.
    assert_eq!(
        classify_text("note: topaz_rt@5.4.0-dev requires rustc 1.96.0"),
        CargoFailure::Internal,
    );
}
