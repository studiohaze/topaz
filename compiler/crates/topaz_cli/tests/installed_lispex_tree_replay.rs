#![cfg(all(target_os = "windows", target_arch = "x86_64"))]

// This tracked historical installed tree contains an x86_64 Windows executable.
// Replay is Windows-only, and the installed manifest remains the byte-identity
// authority used by the copied product before evaluation.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn compiler_root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

struct TemporaryTree(PathBuf);

impl TemporaryTree {
    fn new() -> Self {
        let suffix = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "topaz-installed-lispex-replay-{}-{suffix}",
            std::process::id()
        ));
        assert!(
            !path.exists(),
            "temporary test path already exists: {path:?}"
        );
        fs::create_dir(&path).expect("temporary test root is created");
        Self(path)
    }
}

impl Drop for TemporaryTree {
    fn drop(&mut self) {
        let temp = fs::canonicalize(std::env::temp_dir()).expect("temporary root canonicalizes");
        let target = fs::canonicalize(&self.0).expect("test tree canonicalizes before cleanup");
        assert!(target.starts_with(&temp), "refusing to remove {target:?}");
        assert!(
            target
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.starts_with("topaz-installed-lispex-replay-")),
            "refusing to remove unexpected temporary tree {target:?}"
        );
        fs::remove_dir_all(target).expect("temporary test tree is removed");
    }
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("destination directory is created");
    for entry in fs::read_dir(source).expect("source directory is readable") {
        let entry = entry.expect("source directory entry is readable");
        let kind = entry.file_type().expect("source file type is readable");
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            assert!(
                kind.is_file(),
                "installed tree contains no links or special files"
            );
            fs::copy(entry.path(), target).expect("installed file is copied");
        }
    }
}

fn installed_command(executable: &Path, outside: &Path, args: &[&str]) -> Output {
    // These nonexistent paths are discovery decoys under the isolated test tree.
    let discovery_decoy = outside.join("discovery-decoy");
    let mut command = Command::new(executable);
    command
        .args(args)
        .current_dir(outside)
        .env_clear()
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env("PATH", "")
        .env("TOPAZ_ROOT", discovery_decoy.join("source"))
        .env("TOPAZ_LIT_RUNNER", discovery_decoy.join("bin/program.exe"))
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("ALL_PROXY", "http://127.0.0.1:9")
        .env("NO_PROXY", "");
    for name in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command.output().expect("installed Topaz command runs")
}

#[test]
fn copied_installed_tree_is_the_only_lispex_discovery_authority() {
    let temporary = TemporaryTree::new();
    let staged = temporary.0.join("installed");
    let outside = temporary.0.join("outside");
    fs::create_dir(&outside).expect("outside working directory is created");
    copy_tree(
        &compiler_root().join("lit/j57-2c/instances/step-7/x86_64-pc-windows-gnu"),
        &staged,
    );
    let executable = staged.join("bin/topaz.exe");
    let program = outside.join("answer.lspx");
    let source_error_program = outside.join("source-error.lspx");
    let runtime_error_program = outside.join("runtime-error.lspx");
    fs::write(&program, b"(+ 20 22)\n").expect("Lispex source is written");
    fs::write(&source_error_program, b"(\n").expect("source-error program is written");
    fs::write(&runtime_error_program, b"(car 1)\n").expect("runtime-error program is written");

    let info = installed_command(&executable, &outside, &["lispex", "info", "--json"]);
    assert!(info.status.success(), "{info:?}");
    assert!(info.stderr.is_empty(), "{info:?}");
    let info_json = String::from_utf8(info.stdout).expect("info is UTF-8 JSON");
    for required in [
        r#""schema":"topaz.lispex-info/v1""#,
        r#""available":true"#,
        r#""topaz_version":"5.7.0""#,
        r#""language_mode":"topaz-5.6""#,
        r#""lispex_profile":"lispex-profile-1.5""#,
        r#""abi":"lispex-lit-abi/v1""#,
    ] {
        assert!(
            info_json.contains(required),
            "missing {required}: {info_json}"
        );
    }

    let run = installed_command(
        &executable,
        &outside,
        &[
            "lispex",
            "run",
            program.to_str().expect("program path is UTF-8"),
        ],
    );
    assert!(run.status.success(), "{run:?}");
    assert_eq!(run.stdout, b"42\n");
    assert!(run.stderr.is_empty(), "{run:?}");

    for failed_program in [&source_error_program, &runtime_error_program] {
        let failed = installed_command(
            &executable,
            &outside,
            &[
                "lispex",
                "run",
                failed_program
                    .to_str()
                    .expect("failed program path is UTF-8"),
            ],
        );
        assert_eq!(failed.status.code(), Some(1), "{failed:?}");
        assert!(failed.stdout.is_empty(), "{failed:?}");
        assert!(!failed.stderr.is_empty(), "{failed:?}");
    }

    for invalid in [
        vec!["lispex", "info"],
        vec!["lispex", "info", "--verbose"],
        vec!["lispex", "info", "--json", "extra"],
        vec!["lispex", "run"],
        vec!["lispex", "run", "--json", program.to_str().unwrap()],
        vec!["--root", ".", "lispex", "info", "--json"],
    ] {
        let output = installed_command(&executable, &outside, &invalid);
        assert!(
            !output.status.success(),
            "unexpected acceptance: {invalid:?}"
        );
        assert!(output.stdout.is_empty(), "{output:?}");
    }

    let manifest = staged.join("share/topaz/lispex/lit-artifact-manifest.v1.json");
    let original = fs::read(&manifest).expect("installed manifest is readable");
    let mutated = String::from_utf8(original.clone())
        .expect("installed manifest is UTF-8")
        .replacen(
            "\"backend_version\": \"5.7.0\"",
            "\"backend_version\": \"5.6.1\"",
            1,
        );
    fs::write(&manifest, mutated).expect("temporary manifest mutation is written");
    let rejected = installed_command(&executable, &outside, &["lispex", "info", "--json"]);
    assert!(!rejected.status.success(), "{rejected:?}");
    assert!(rejected.stdout.is_empty(), "{rejected:?}");
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("component unavailable"),
        "{rejected:?}"
    );
    fs::write(manifest, original).expect("temporary manifest is restored");
}
