#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use std::path::{Path, PathBuf};
use std::time::Duration;

use topaz_execution_sandbox::protocol::{ProbeRequest, WorkerRequest, WorkerStatus};
use topaz_execution_sandbox::sandbox::{DarwinSandbox, PlatformSandbox};

fn compiler_manifest() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../Cargo.toml")
        .canonicalize()
        .expect("compiler manifest")
}

fn private_write_path() -> PathBuf {
    PathBuf::from(format!("/tmp/topaz-x3s-probe-{}", std::process::id()))
}

#[test]
fn darwin_probe_denies_ambient_capabilities_with_empty_environment() {
    let sandbox = DarwinSandbox::qualify().expect("qualified Darwin backend");
    let write_path = private_write_path();
    let _ = std::fs::remove_file(&write_path);
    let result = sandbox
        .run_probe(
            Path::new(env!("CARGO_BIN_EXE_topaz-sandbox-probe")),
            &ProbeRequest {
                forbidden_read_path: compiler_manifest().display().to_string(),
                forbidden_write_path: write_path.display().to_string(),
            },
            Duration::from_secs(5),
        )
        .expect("sandboxed probe");

    assert_eq!(result.response.inherited_environment_entries, 0);
    assert!(result.response.filesystem_read_denied);
    assert!(result.response.filesystem_write_denied);
    assert!(result.response.network_denied);
    assert!(result.response.child_process_denied);
    assert!(!write_path.exists());
    assert_eq!(
        result.execution_environment_identity.target,
        "aarch64-apple-darwin"
    );
    assert!(
        result
            .execution_environment_identity
            .backend_program_sha256
            .starts_with("sha256:")
    );
    assert!(
        result
            .execution_environment_identity
            .generated_profile_sha256
            .starts_with("sha256:")
    );
    assert_eq!(
        result.admission_identity.backend_policy,
        "topaz.execution-sandbox/darwin-sandbox-exec/v1"
    );
    println!(
        "probe execution identity: {:?}",
        result.execution_environment_identity
    );
}

#[test]
fn darwin_worker_receives_source_only_over_pipe_and_uses_no_capability_host() {
    let sandbox = DarwinSandbox::qualify().expect("qualified Darwin backend");
    let result = sandbox
        .run_worker(
            Path::new(env!("CARGO_BIN_EXE_topaz-private-worker")),
            &WorkerRequest {
                source: "FS.readText(\"secret.txt\")".to_string(),
                input: String::new(),
            },
            Duration::from_secs(5),
        )
        .expect("sandboxed worker");

    assert_eq!(result.semantic_identity.language_profile, "topaz-5.17");
    assert_eq!(result.response.status, WorkerStatus::Success);
    assert_eq!(
        result.response.value,
        "Err(no-capability host denies `open`)"
    );
    assert_ne!(
        result
            .execution_environment_identity
            .generated_profile_sha256,
        result.execution_environment_identity.executable_sha256
    );
    assert_ne!(
        result.semantic_identity.language_profile,
        result.admission_identity.policy
    );
    println!(
        "worker execution identity: {:?}",
        result.execution_environment_identity
    );
}
