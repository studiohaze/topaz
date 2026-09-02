//! Embeds the CDR-006 §7 vendored runtime closure into the `topaz` binary.
//!
//! `topaz build` lowers a program to a Rust crate and compiles it. For a user
//! who installed only the binary (no source checkout), that crate must carry its
//! OWN copy of the runtime crates it links — version-exact, no registry, no
//! network. This script embeds the exact source of the closure crates
//! ({topaz_diag, topaz_syntax, topaz_value, topaz_product_runtime, topaz_rt,
//! topaz_host_native}) via
//! compile-time `include_bytes!`, so the embedded bytes are byte-identical to
//! the workspace by construction (the CDR-006 §7 identity property). It also
//! synthesizes the `vendor/Cargo.toml` workspace root that the BYTE-IDENTICAL
//! vendored crate manifests inherit from (so they need no rewriting), reusing
//! the real workspace's `[workspace.package]` / `[workspace.lints]` verbatim.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// The runtime closure an emitted `topaz build` crate links, in dependency
/// order. This is the minimal self-contained set needed by the interpreter.
const CLOSURE: &[&str] = &[
    "topaz_diag",
    "topaz_syntax",
    "topaz_value",
    "topaz_product_runtime",
    "topaz_rt",
    "topaz_host_native",
];

const SERVICE_CRATE: &str = "topaz_host_http";
const LISPEX_APPLICATION_CRATE: &str = "topaz_lispex_embed";
const LISPEX_APPLICATION_WORKSPACE_DEPS: &[&str] =
    &["num-bigint", "num-integer", "sha2", "wasmtime"];
const SERVICE_REGISTRY_DEPS: &[&str] = &["http-body-util", "hyper", "hyper-util", "tokio"];
const SERVICE_VENDOR_PACKAGES: &[&str] = &[
    "atomic-waker-1.1.2",
    "bytes-1.12.1",
    "errno-0.3.14",
    "futures-channel-0.3.33",
    "futures-core-0.3.33",
    "http-1.4.2",
    "http-body-1.1.0",
    "http-body-util-0.1.4",
    "httparse-1.10.1",
    "httpdate-1.0.3",
    "hyper-1.11.0",
    "hyper-util-0.1.20",
    "itoa-1.0.18",
    "libc-0.2.189",
    "mio-1.2.2",
    "pin-project-lite-0.2.17",
    "signal-hook-registry-1.4.8",
    "smallvec-1.15.2",
    "socket2-0.6.5",
    "tokio-1.53.1",
    "wasi-0.11.1+wasi-snapshot-preview1",
    "windows-link-0.2.1",
    "windows-sys-0.61.2",
];

// Exact all-target normal dependency closure consumed by the embedded Lispex
// application. This build script checks the Cargo.lock-backed inventory before
// generated native crates extract it for reachable Lispex rules.
const LISPEX_APPLICATION_VENDOR_PACKAGES: &[&str] = &[
    "allocator-api2-0.2.21",
    "addr2line-0.25.1",
    "anyhow-1.0.104",
    "arbitrary-1.4.2",
    "async-trait-0.1.91",
    "autocfg-1.5.1",
    "bitflags-2.13.1",
    "block-buffer-0.10.4",
    "bumpalo-3.20.3",
    "cc-1.4.0",
    "cfg-if-1.0.4",
    "cobs-0.3.0",
    "cpufeatures-0.2.17",
    "cranelift-assembler-x64-0.125.4",
    "cranelift-assembler-x64-meta-0.125.4",
    "cranelift-bforest-0.125.4",
    "cranelift-bitset-0.125.4",
    "cranelift-codegen-0.125.4",
    "cranelift-codegen-meta-0.125.4",
    "cranelift-codegen-shared-0.125.4",
    "cranelift-control-0.125.4",
    "cranelift-entity-0.125.4",
    "cranelift-frontend-0.125.4",
    "cranelift-isle-0.125.4",
    "cranelift-native-0.125.4",
    "cranelift-srcgen-0.125.4",
    "crc32fast-1.5.0",
    "crypto-common-0.1.7",
    "digest-0.10.7",
    "either-1.17.0",
    "embedded-io-0.4.0",
    "embedded-io-0.6.1",
    "equivalent-1.0.2",
    "errno-0.3.14",
    "fallible-iterator-0.3.0",
    "find-msvc-tools-0.1.9",
    "foldhash-0.1.5",
    "generic-array-0.14.7",
    "gimli-0.32.3",
    "hashbrown-0.15.5",
    "hashbrown-0.17.1",
    "heck-0.5.0",
    "indexmap-2.14.0",
    "itertools-0.14.0",
    "leb128fmt-0.1.0",
    "libc-0.2.189",
    "libm-0.2.16",
    "linux-raw-sys-0.12.1",
    "log-0.4.33",
    "mach2-0.4.3",
    "memchr-2.8.3",
    "memfd-0.6.5",
    "num-bigint-0.4.6",
    "num-integer-0.1.46",
    "num-traits-0.2.19",
    "object-0.37.3",
    "once_cell-1.21.4",
    "postcard-1.1.3",
    "proc-macro2-1.0.107",
    "pulley-interpreter-38.0.4",
    "pulley-macros-38.0.4",
    "quote-1.0.47",
    "regalloc2-0.13.5",
    "rustc-hash-2.1.3",
    "rustix-1.1.4",
    "semver-1.0.28",
    "serde-1.0.229",
    "serde_core-1.0.229",
    "serde_derive-1.0.229",
    "sha2-0.10.9",
    "shlex-2.0.1",
    "smallvec-1.15.2",
    "stable_deref_trait-1.2.1",
    "syn-2.0.119",
    "syn-3.0.3",
    "target-lexicon-0.13.5",
    "termcolor-1.4.1",
    "thiserror-2.0.19",
    "thiserror-impl-2.0.19",
    "typenum-1.20.1",
    "unicode-ident-1.0.24",
    "version_check-0.9.5",
    "wasm-encoder-0.239.0",
    "wasmparser-0.239.0",
    "wasmprinter-0.239.0",
    "wasmtime-38.0.4",
    "wasmtime-environ-38.0.4",
    "wasmtime-internal-cranelift-38.0.4",
    "wasmtime-internal-fiber-38.0.4",
    "wasmtime-internal-jit-icache-coherence-38.0.4",
    "wasmtime-internal-jit-debug-38.0.4",
    "wasmtime-internal-math-38.0.4",
    "wasmtime-internal-slab-38.0.4",
    "wasmtime-internal-unwinder-38.0.4",
    "wasmtime-internal-versioned-export-macros-38.0.4",
    "winapi-util-0.1.11",
    "windows-link-0.2.1",
    "windows-sys-0.60.2",
    "windows-sys-0.61.2",
    "windows-targets-0.53.5",
    "windows_aarch64_gnullvm-0.53.1",
    "windows_aarch64_msvc-0.53.1",
    "windows_i686_gnu-0.53.1",
    "windows_i686_gnullvm-0.53.1",
    "windows_i686_msvc-0.53.1",
    "windows_x86_64_gnu-0.53.1",
    "windows_x86_64_gnullvm-0.53.1",
    "windows_x86_64_msvc-0.53.1",
];

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let ws = Path::new(&manifest_dir).join("..").join("..");
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");

    let root_manifest = ws.join("Cargo.toml");
    println!("cargo:rerun-if-changed={}", root_manifest.display());

    let mut table = String::from(
        "/// (relative-path, bytes) of every vendored closure file.\npub static VENDOR_FILES: &[(&str, &[u8])] = &[\n",
    );
    for c in CLOSURE {
        let crate_dir = ws.join("crates").join(c);
        let cargo_toml = crate_dir.join("Cargo.toml");
        // Watch the (currently-absent) crate `build.rs` so ADDING one later
        // re-triggers this script — otherwise an incremental build could keep a
        // stale `vendor.rs` and never run the audit that bans a closure build.rs.
        println!(
            "cargo:rerun-if-changed={}",
            crate_dir.join("build.rs").display()
        );
        audit_manifest(&crate_dir, &cargo_toml);
        emit_include(&mut table, c, &crate_dir, &cargo_toml);
        collect(&mut table, c, &crate_dir, &crate_dir.join("src"));
    }
    table.push_str("];\n");

    let mut service_table = String::from(
        "/// Topaz-owned source added only to http-service scaffolds.\npub static SERVICE_VENDOR_FILES: &[(&str, &[u8])] = &[\n",
    );
    let service_dir = ws.join("crates").join(SERVICE_CRATE);
    let service_manifest_path = service_dir.join("Cargo.toml");
    println!(
        "cargo:rerun-if-changed={}",
        service_dir.join("build.rs").display()
    );
    audit_service_manifest(&service_dir, &service_manifest_path);
    emit_include(
        &mut service_table,
        SERVICE_CRATE,
        &service_dir,
        &service_manifest_path,
    );
    collect(
        &mut service_table,
        SERVICE_CRATE,
        &service_dir,
        &service_dir.join("src"),
    );
    service_table.push_str("];\n");

    let mut lispex_application_table = String::from(
        "/// Bounded Lispex adapter source added only to a reached application scaffold.\npub static LISPEX_APPLICATION_VENDOR_FILES: &[(&str, &[u8])] = &[\n",
    );
    let lispex_application_dir = ws.join("crates").join(LISPEX_APPLICATION_CRATE);
    let lispex_application_manifest_path = lispex_application_dir.join("Cargo.toml");
    audit_lispex_application_manifest(&lispex_application_dir, &lispex_application_manifest_path);
    emit_include(
        &mut lispex_application_table,
        LISPEX_APPLICATION_CRATE,
        &lispex_application_dir,
        &lispex_application_manifest_path,
    );
    for relative in [
        "src/lib.rs",
        "src/application.rs",
        "src/application_host.rs",
        "src/artifact.rs",
        "src/limits.rs",
        "src/protocol.rs",
        "src/report.rs",
        "src/runtime.rs",
        "src/value_codec.rs",
    ] {
        let file = lispex_application_dir.join(relative);
        audit_lispex_application_source(&file);
        emit_include(
            &mut lispex_application_table,
            LISPEX_APPLICATION_CRATE,
            &lispex_application_dir,
            &file,
        );
    }
    lispex_application_table.push_str("];\n");

    let mut full_lispex_application_table = String::from(
        "/// Complete-profile adapter source added only to a reached 5.20 application scaffold.\npub static FULL_LISPEX_APPLICATION_VENDOR_FILES: &[(&str, &[u8])] = &[\n",
    );
    let full_artifact_file = lispex_application_dir.join("src/full_artifact.rs");
    emit_include(
        &mut full_lispex_application_table,
        LISPEX_APPLICATION_CRATE,
        &lispex_application_dir,
        &full_artifact_file,
    );
    full_lispex_application_table.push_str("];\n");

    let service_registry = ws.join("vendor").join("stage0-recovery");
    let archive_path = Path::new(&out_dir).join("http-service-vendor.bin");
    write_directory_archive(&service_registry, &archive_path);
    let notices_path = Path::new(&out_dir).join("http-service-notices.txt");
    write_service_notices(&service_registry, &notices_path);
    let lispex_application_archive_path = Path::new(&out_dir).join("lispex-application-vendor.bin");
    write_directory_archive_with_magic(
        &service_registry,
        &lispex_application_archive_path,
        b"TPZLPXA1",
        LISPEX_APPLICATION_VENDOR_PACKAGES,
    );
    let lispex_application_notices_path =
        Path::new(&out_dir).join("lispex-application-notices.txt");
    write_package_notices(
        &service_registry,
        &lispex_application_notices_path,
        "Topaz bounded Lispex application runtime third-party notices",
        LISPEX_APPLICATION_VENDOR_PACKAGES,
    );

    let manifest = synthesize_vendor_workspace(
        &fs::read_to_string(&root_manifest).expect("read root Cargo.toml"),
        false,
    );
    let service_manifest = synthesize_vendor_workspace(
        &fs::read_to_string(&root_manifest).expect("read root Cargo.toml"),
        true,
    );
    let lispex_application_manifest = synthesize_lispex_application_vendor_workspace(
        &fs::read_to_string(&root_manifest).expect("read root Cargo.toml"),
    );

    let toolchain_path = ws.join("rust-toolchain.toml");
    println!("cargo:rerun-if-changed={}", toolchain_path.display());
    let toolchain = fs::read_to_string(&toolchain_path).expect("read rust-toolchain.toml");

    let generated = format!(
        "{table}\n\
         {service_table}\n\
         {lispex_application_table}\n\
         {full_lispex_application_table}\n\
         /// The synthetic `vendor/Cargo.toml` workspace root the byte-identical\n\
         /// closure manifests inherit from.\n\
         pub static VENDOR_WORKSPACE_MANIFEST: &str = r##\"{manifest}\"##;\n\n\
         pub static SERVICE_VENDOR_WORKSPACE_MANIFEST: &str = r##\"{service_manifest}\"##;\n\n\
         pub static SERVICE_VENDOR_CONFIG: &str = \"[source.crates-io]\\nreplace-with = \\\"topaz-http-service-vendor\\\"\\n\\n[source.topaz-http-service-vendor]\\ndirectory = \\\"vendor/registry\\\"\\n\";\n\n\
         pub static SERVICE_VENDOR_ARCHIVE: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/http-service-vendor.bin\"));\n\n\
         pub static SERVICE_THIRD_PARTY_NOTICES: &str = include_str!(concat!(env!(\"OUT_DIR\"), \"/http-service-notices.txt\"));\n\n\
         pub static LISPEX_APPLICATION_VENDOR_WORKSPACE_MANIFEST: &str = r##\"{lispex_application_manifest}\"##;\n\n\
         pub static LISPEX_APPLICATION_VENDOR_CONFIG: &str = \"[source.crates-io]\\nreplace-with = \\\"topaz-lispex-application-vendor\\\"\\n\\n[source.topaz-lispex-application-vendor]\\ndirectory = \\\"vendor/registry\\\"\\n\";\n\n\
         pub static LISPEX_APPLICATION_VENDOR_ARCHIVE: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/lispex-application-vendor.bin\"));\n\n\
         pub static LISPEX_APPLICATION_THIRD_PARTY_NOTICES: &str = include_str!(concat!(env!(\"OUT_DIR\"), \"/lispex-application-notices.txt\"));\n\n\
         /// The pinned toolchain the closure was built against (CDR-006 §7).\n\
         pub static VENDOR_TOOLCHAIN: &str = r##\"{toolchain}\"##;\n"
    );
    fs::write(Path::new(&out_dir).join("vendor.rs"), generated).expect("write vendor.rs");
}

fn audit_service_manifest(crate_dir: &Path, cargo_toml: &Path) {
    assert!(
        !crate_dir.join("build.rs").exists(),
        "service host crate {} must not have a build.rs",
        crate_dir.display()
    );
    let manifest = fs::read_to_string(cargo_toml).expect("read service host manifest");
    let mut in_dependencies = false;
    for line in manifest.lines() {
        let text = line.trim();
        if text.starts_with('[') {
            in_dependencies = text == "[dependencies]";
            continue;
        }
        if !in_dependencies || text.is_empty() || text.starts_with('#') {
            continue;
        }
        let (name, value) = text
            .split_once('=')
            .unwrap_or_else(|| panic!("invalid service dependency line `{text}`"));
        let name = name.trim();
        let normalized: String = value.chars().filter(|ch| !ch.is_whitespace()).collect();
        assert!(
            SERVICE_REGISTRY_DEPS.contains(&name) && normalized == "{workspace=true}",
            "service host dependency `{text}` is not in the exact registry allowlist"
        );
    }
}

fn audit_lispex_application_manifest(crate_dir: &Path, cargo_toml: &Path) {
    assert!(
        !crate_dir.join("build.rs").exists(),
        "bounded Lispex adapter {} must not have a build.rs",
        crate_dir.display()
    );
    let manifest = fs::read_to_string(cargo_toml).expect("read Lispex adapter manifest");
    let mut in_dependencies = false;
    for line in manifest.lines() {
        let text = line.trim();
        if text.starts_with('[') {
            in_dependencies = text == "[dependencies]";
            continue;
        }
        if !in_dependencies || text.is_empty() || text.starts_with('#') {
            continue;
        }
        let (name, value) = text
            .split_once('=')
            .unwrap_or_else(|| panic!("invalid Lispex adapter dependency line `{text}`"));
        let name = name.trim();
        let normalized: String = value.chars().filter(|ch| !ch.is_whitespace()).collect();
        let allowed = name == "topaz_value" || LISPEX_APPLICATION_WORKSPACE_DEPS.contains(&name);
        assert!(
            allowed && normalized == "{workspace=true}",
            "Lispex adapter dependency `{text}` is not in the exact application allowlist"
        );
    }
}

fn audit_lispex_application_source(file: &Path) {
    let source = fs::read_to_string(file).expect("read bounded Lispex adapter source");
    let production = source
        .split_once("#[cfg(test)]")
        .map_or(source.as_str(), |(production, _)| production);
    assert!(
        !source.contains("lispex-evaluator/rust-vm-current-profile")
            && !source.contains("lispex/r7rs-rule-current-profile-bounded")
            && !source.contains("lispex-full-vm-meter"),
        "bounded generated-product source {} references the private full-profile component",
        file.display()
    );
    let stripped: String = production
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect();
    for forbidden in ["include_str!", "include!", "env!", "#[path"] {
        assert!(
            !stripped.contains(forbidden),
            "bounded Lispex adapter source {} uses forbidden `{forbidden}`",
            file.display()
        );
    }
    if stripped.contains("include_bytes!") {
        assert!(
            file.ends_with("src/lib.rs")
                && stripped.matches("include_bytes!").count() == 2
                && production.contains("lispex-embed-evaluator.wasm"),
            "bounded Lispex adapter source {} has an unapproved include_bytes!",
            file.display()
        );
    }
}

/// Compact deterministic container for the Cargo directory source. Keeping a
/// single `include_bytes!` avoids generating thousands of Rust static entries;
/// the bytes and `.cargo-checksum.json` files remain exactly those from
/// `cargo vendor --locked --versioned-dirs`.
fn write_directory_archive(root: &Path, destination: &Path) {
    write_directory_archive_with_magic(root, destination, b"TPZHTTP1", SERVICE_VENDOR_PACKAGES);
}

fn write_directory_archive_with_magic(
    root: &Path,
    destination: &Path,
    magic: &[u8; 8],
    packages: &[&str],
) {
    assert!(
        root.is_dir(),
        "missing service registry vendor `{}`",
        root.display()
    );
    for package in packages {
        assert!(
            root.join(package).is_dir(),
            "service registry vendor lacks exact package `{package}`"
        );
        println!(
            "cargo:rerun-if-changed={}",
            root.join(package).join(".cargo-checksum.json").display()
        );
    }
    let mut files = Vec::new();
    for package in packages {
        collect_archive_paths(root, &root.join(package), &mut files);
    }
    files.sort();
    let count = u32::try_from(files.len()).expect("service vendor file count fits u32");
    let mut archive = Vec::new();
    archive.extend_from_slice(magic);
    archive.extend_from_slice(&count.to_le_bytes());
    for path in files {
        let relative = path
            .strip_prefix(root)
            .expect("service vendor path under root")
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = fs::read(&path).expect("read service vendor file");
        let path_len = u32::try_from(relative.len()).expect("service vendor path fits u32");
        let data_len = u64::try_from(bytes.len()).expect("service vendor file fits u64");
        archive.extend_from_slice(&path_len.to_le_bytes());
        archive.extend_from_slice(&data_len.to_le_bytes());
        archive.extend_from_slice(relative.as_bytes());
        archive.extend_from_slice(&bytes);
    }
    fs::write(destination, archive).expect("write service vendor archive");
}

fn write_service_notices(root: &Path, destination: &Path) {
    write_package_notices(
        root,
        destination,
        "Topaz HTTP service host third-party notices",
        SERVICE_VENDOR_PACKAGES,
    );
}

fn write_package_notices(root: &Path, destination: &Path, heading: &str, packages: &[&str]) {
    let mut notices = format!(
        "{heading}\n\nThe following exact source packages are bundled with this generated product.\nTheir license files are reproduced below.\n"
    );
    for package in packages {
        let directory = root.join(package);
        let manifest = format!(
            "{}\n{}",
            fs::read_to_string(directory.join("Cargo.toml.orig")).unwrap_or_default(),
            fs::read_to_string(directory.join("Cargo.toml"))
                .expect("read normalized vendored package manifest")
        );
        let license = manifest
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once('=')?;
                (key.trim() == "license").then(|| value.trim())
            })
            .unwrap_or_else(|| panic!("vendored package `{package}` has no license metadata"));
        writeln!(notices, "\n===== {package} ({license}) =====").unwrap();
        let mut license_files = fs::read_dir(&directory)
            .expect("read vendored package root")
            .collect::<Result<Vec<_>, _>>()
            .expect("read vendored package root entry")
            .into_iter()
            .filter(|entry| {
                let name = entry.file_name().to_string_lossy().to_ascii_uppercase();
                entry.file_type().is_ok_and(|kind| kind.is_file())
                    && (name.starts_with("LICENSE") || name.starts_with("COPYING"))
            })
            .collect::<Vec<_>>();
        license_files.sort_by_key(|entry| entry.file_name());
        if license_files.is_empty() {
            assert!(
                license.contains("Apache-2.0 WITH LLVM-exception"),
                "vendored package `{package}` has no root license file and no approved shared license"
            );
            let shared = root.join("wasmtime-38.0.4/LICENSE");
            writeln!(
                notices,
                "\n--- shared Apache-2.0 WITH LLVM-exception text ---"
            )
            .unwrap();
            notices
                .push_str(&fs::read_to_string(shared).expect("read shared Wasmtime license text"));
            if !notices.ends_with('\n') {
                notices.push('\n');
            }
            continue;
        }
        for entry in license_files {
            writeln!(notices, "\n--- {} ---", entry.file_name().to_string_lossy()).unwrap();
            notices.push_str(
                &fs::read_to_string(entry.path()).expect("vendored license file must be UTF-8"),
            );
            if !notices.ends_with('\n') {
                notices.push('\n');
            }
        }
    }
    fs::write(destination, notices).expect("write generated-product third-party notices");
}

fn collect_archive_paths(root: &Path, directory: &Path, out: &mut Vec<PathBuf>) {
    let mut entries = fs::read_dir(directory)
        .expect("read service vendor directory")
        .collect::<Result<Vec<_>, _>>()
        .expect("read service vendor entry");
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type().expect("service vendor file type");
        assert!(
            !file_type.is_symlink(),
            "service registry vendor contains symlink `{}`",
            path.display()
        );
        if file_type.is_dir() {
            collect_archive_paths(root, &path, out);
        } else if file_type.is_file() {
            assert!(path.starts_with(root));
            out.push(path);
        }
    }
}

/// Fail-closed: the vendored closure must be plain library crates. If a closure
/// crate ever grows a build script, proc-macro, `links`, or a non-closure
/// dependency, vendoring breaks LOUDLY here rather than silently shipping a
/// bundle that is not self-contained or not sound. This is a best-effort
/// developer TRIPWIRE over our own (non-adversarial) manifests — the
/// AUTHORITATIVE self-containment guarantee is that the emitted tree actually
/// builds `--offline --locked`; the CI byte-identity and offline-smoke checks
/// repeat this boundary. The audit catches mistakes early, at compiler build time.
fn audit_manifest(crate_dir: &Path, cargo_toml: &Path) {
    assert!(
        !crate_dir.join("build.rs").exists(),
        "vendored closure crate {} must not have a build.rs",
        crate_dir.display()
    );
    let m = fs::read_to_string(cargo_toml).expect("read closure Cargo.toml");
    // Line-based, WHITESPACE-NORMALIZED scan (zero external deps, so no TOML
    // crate). Headers and keys are normalized so `[ build-dependencies ]`,
    // `[ target.'cfg(...)'.dependencies ]`, or an indented `  build = …` cannot
    // evade the checks. Forbidden anywhere: a `build`/`links` key or a
    // `proc-macro = true`. In ANY `…dependencies]` section, the ONLY allowed
    // dependency is a closure crate via workspace inheritance; everything else
    // (registry/git/external, or a build-dependencies table) is rejected, since
    // it would need the network the vendored closure exists to avoid.
    let norm = |s: &str| -> String { s.chars().filter(|c| !c.is_whitespace()).collect() };
    let mut in_deps = false;
    let mut header = String::new();
    for line in m.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t.starts_with('[') {
            header = norm(t);
            in_deps = header.ends_with("dependencies]");
            continue;
        }
        let (key, val) = t
            .split_once('=')
            .map(|(k, v)| (k.trim(), v.trim()))
            .unwrap_or((t, ""));
        let val_norm = norm(val);
        // Normalize the key's leaf: strip quotes and dotted prefixes, so
        // `package.build`, `"build"`, and `lib.proc-macro` cannot evade.
        let leaf = key.rsplit('.').next().unwrap_or(key).trim_matches('"');
        assert!(
            leaf != "build",
            "vendored closure manifest {} declares a build script (`build` key) — banned",
            cargo_toml.display()
        );
        assert!(
            leaf != "links",
            "vendored closure manifest {} declares a `links` key — banned",
            cargo_toml.display()
        );
        assert!(
            !(leaf == "proc-macro" && val_norm == "true"),
            "vendored closure manifest {} is a proc-macro — banned",
            cargo_toml.display()
        );
        if in_deps {
            let dep_ok = (CLOSURE.contains(&key) && val_norm == "{workspace=true}")
                || (key
                    .strip_suffix(".workspace")
                    .is_some_and(|k| CLOSURE.contains(&k))
                    && val_norm == "true");
            assert!(
                dep_ok && header != "[build-dependencies]",
                "vendored closure manifest {} has a forbidden dependency `{t}` in `{header}` \
                 (only `<closure> = {{ workspace = true }}` in a normal-deps table is allowed — \
                 external/build deps would break the offline vendored closure)",
                cargo_toml.display()
            );
        }
    }
}

/// Fail-closed source audit: a vendored `src/**.rs` must not pull in out-of-tree
/// content (`include_*!`, `#[path]`) or compile-time env the bundle can't
/// reproduce — any of those would mean a file outside `src/**.rs` is needed and
/// not embedded. Scan with whitespace removed so `include_str !(…)`, `env !(…)`,
/// and `#[ path = … ]` cannot evade. (Best-effort developer tripwire; the
/// AUTHORITATIVE self-containment proof is the offline build + the CI gate.)
fn audit_source(file: &Path, contents: &str) {
    let stripped: String = contents.chars().filter(|c| !c.is_whitespace()).collect();
    for forbidden in [
        "include_str!",
        "include_bytes!",
        "include!",
        "env!",
        "#[path",
    ] {
        assert!(
            !stripped.contains(forbidden),
            "vendored closure source {} uses forbidden `{forbidden}`",
            file.display()
        );
    }
}

fn emit_include(table: &mut String, crate_name: &str, crate_dir: &Path, file: &Path) {
    let rel = file
        .strip_prefix(crate_dir)
        .expect("file under crate dir")
        .to_string_lossy()
        .replace('\\', "/");
    println!("cargo:rerun-if-changed={}", file.display());
    writeln!(
        table,
        "    (\"{crate_name}/{rel}\", include_bytes!(r\"{}\")),",
        file.display()
    )
    .unwrap();
}

fn collect(table: &mut String, crate_name: &str, crate_dir: &Path, dir: &Path) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut entries: Vec<_> = fs::read_dir(dir)
        .expect("read closure src dir")
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            collect(table, crate_name, crate_dir, &p);
        } else if p.extension().is_some_and(|e| e == "rs") {
            audit_source(&p, &fs::read_to_string(&p).expect("read closure source"));
            emit_include(table, crate_name, crate_dir, &p);
        }
    }
}

/// Build the `vendor/Cargo.toml` workspace root: a fresh `[workspace]` +
/// closure-only `[workspace.dependencies]`, with the real workspace's
/// `[workspace.package]` and `[workspace.lints.*]` reused VERBATIM (so the
/// version/edition/rust-version/lints the closure inherits stay in lockstep
/// with the compiler, with no TOML rewriter).
fn synthesize_vendor_workspace(root: &str, service: bool) -> String {
    let mut out = String::from("[workspace]\nresolver = \"3\"\nmembers = [\n");
    for c in CLOSURE {
        writeln!(out, "    \"crates/{c}\",").unwrap();
    }
    if service {
        writeln!(out, "    \"crates/{SERVICE_CRATE}\",").unwrap();
    }
    out.push_str("]\n\n");
    out.push_str(section(root, "[workspace.package]").expect("[workspace.package] in root"));
    out.push_str("\n[workspace.dependencies]\n");
    for c in CLOSURE {
        writeln!(out, "{c} = {{ path = \"crates/{c}\" }}").unwrap();
    }
    if service {
        writeln!(
            out,
            "{SERVICE_CRATE} = {{ path = \"crates/{SERVICE_CRATE}\" }}"
        )
        .unwrap();
        for dependency in SERVICE_REGISTRY_DEPS {
            let line = workspace_dependency_line(root, dependency)
                .unwrap_or_else(|| panic!("missing exact workspace dependency `{dependency}`"));
            writeln!(out, "{line}").unwrap();
        }
    }
    for header in ["[workspace.lints.rust]", "[workspace.lints.clippy]"] {
        if let Some(s) = section(root, header) {
            out.push('\n');
            out.push_str(s);
        }
    }
    out
}

fn synthesize_lispex_application_vendor_workspace(root: &str) -> String {
    let mut out = String::from("[workspace]\nresolver = \"3\"\nmembers = [\n");
    for crate_name in CLOSURE {
        writeln!(out, "    \"crates/{crate_name}\",").unwrap();
    }
    writeln!(out, "    \"crates/{LISPEX_APPLICATION_CRATE}\",").unwrap();
    out.push_str("]\n\n");
    out.push_str(section(root, "[workspace.package]").expect("[workspace.package] in root"));
    out.push_str("\n[workspace.dependencies]\n");
    for crate_name in CLOSURE {
        writeln!(out, "{crate_name} = {{ path = \"crates/{crate_name}\" }}").unwrap();
    }
    writeln!(
        out,
        "{LISPEX_APPLICATION_CRATE} = {{ path = \"crates/{LISPEX_APPLICATION_CRATE}\" }}"
    )
    .unwrap();
    for dependency in LISPEX_APPLICATION_WORKSPACE_DEPS {
        let line = workspace_dependency_line(root, dependency)
            .unwrap_or_else(|| panic!("missing exact workspace dependency `{dependency}`"));
        writeln!(out, "{line}").unwrap();
    }
    for header in ["[workspace.lints.rust]", "[workspace.lints.clippy]"] {
        if let Some(section) = section(root, header) {
            out.push('\n');
            out.push_str(section);
        }
    }
    out
}

fn workspace_dependency_line(root: &str, name: &str) -> Option<String> {
    let dependencies = section(root, "[workspace.dependencies]")?;
    dependencies.lines().find_map(|line| {
        let (key, _) = line.split_once('=')?;
        (key.trim() == name).then(|| line.trim().to_string())
    })
}

/// The text of a top-level TOML section, from its `[header]` line up to (but not
/// including) the next line that starts a new `[`-section.
fn section<'a>(src: &'a str, header: &str) -> Option<&'a str> {
    let start = src.find(header)?;
    let after = &src[start + header.len()..];
    let end = after
        .match_indices('\n')
        .find(|&(i, _)| after[i + 1..].trim_start().starts_with('['))
        .map(|(i, _)| start + header.len() + i + 1)
        .unwrap_or(src.len());
    Some(&src[start..end])
}
