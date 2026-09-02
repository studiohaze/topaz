use super::support::*;

fn compiler_root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

const LIT_SOURCE_PATHS: [&str; 20] = [
    "lit.tpz",
    "lit/model.tpz",
    "lit/numeric.tpz",
    "lit/kernel_decode.tpz",
    "lit/frontend_lex_read.tpz",
    "lit/normalize_core.tpz",
    "lit/request_decode.tpz",
    "lit/value_runtime.tpz",
    "lit/unicode_text.tpz",
    "lit/primitives/numeric_pairs.tpz",
    "lit/primitives/control_hof.tpz",
    "lit/primitives/containers.tpz",
    "lit/primitives/text_io.tpz",
    "lit/primitives/dispatch.tpz",
    "lit/machine/invoke.tpz",
    "lit/machine/eval.tpz",
    "lit/machine/deliver.tpz",
    "lit/machine/transfer.tpz",
    "lit/render.tpz",
    "lit/protocol_v1.tpz",
];

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = topaz_value::value::sha256(bytes);
    let mut output = String::with_capacity(64);
    topaz_value::bytes_to_hex_into(&mut output, &digest);
    output
}

fn write_installed_fixture(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("installed fixture path has a parent"))
        .expect("create installed Lispex fixture directory");
    std::fs::write(path, bytes).expect("write installed Lispex fixture file");
}

fn installed_identity(kind: &str, path: &str, bytes: &[u8]) -> JsonValue {
    serde_json::json!({
        "identity_kind": kind,
        "path": path,
        "byte_len": bytes.len(),
        "sha256": sha256_hex(bytes),
    })
}

fn write_json_fixture(path: &Path, value: &JsonValue) {
    let mut bytes = serde_json::to_vec_pretty(value).expect("serialize installed Lispex manifest");
    bytes.push(b'\n');
    std::fs::write(path, bytes).expect("write installed Lispex manifest");
}

#[test]
fn installed_lispex_info_admits_producer_checkpoint_and_rejects_mismatch() {
    let root = compiler_root().join("target").join(format!(
        "topaz-cli-installed-lispex-info-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let installed_executable = root.join("bin/topaz.exe");
    std::fs::create_dir_all(
        installed_executable
            .parent()
            .expect("installed bin has a parent"),
    )
    .expect("create installed Topaz bin directory");
    std::fs::copy(env!("CARGO_BIN_EXE_topaz"), &installed_executable)
        .expect("install current Topaz executable");
    let executable_bytes =
        std::fs::read(&installed_executable).expect("read installed current Topaz executable");
    let canonical_sources = LIT_SOURCE_PATHS.map(|path| {
        (
            path,
            std::fs::read(compiler_root().join("lit").join(path))
                .expect("read canonical LIT source tree file"),
        )
    });
    let canonical_source = &canonical_sources[0].1;
    let native_artifact = b"installed native Lispex artifact\n";
    let python_artifact = b"installed Python Lispex artifact\n";
    let web_artifact = b"installed Web Lispex artifact\n";
    for (path, bytes) in &canonical_sources {
        write_installed_fixture(&root, &format!("share/topaz/lispex/{path}"), bytes);
    }
    write_installed_fixture(
        &root,
        "libexec/topaz/lispex/lit-runner.exe",
        native_artifact,
    );
    write_installed_fixture(
        &root,
        "share/topaz/lispex/python/managed-files.bundle",
        python_artifact,
    );
    write_installed_fixture(
        &root,
        "share/topaz/lispex/web/managed-files.bundle",
        web_artifact,
    );

    let product_version = env!("CARGO_PKG_VERSION");
    let language_mode = format!(
        "topaz-{}.{}",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR")
    );
    let producer_checkpoint = concat!("J", "57-2C-step-7");
    let manifest_path = root.join("share/topaz/lispex/lit-artifact-manifest.v1.json");
    let mut managed_files = vec![installed_identity(
        "installed-managed-file-bytes",
        "bin/topaz.exe",
        &executable_bytes,
    )];
    managed_files.extend(canonical_sources.iter().map(|(path, bytes)| {
        installed_identity(
            "installed-managed-file-bytes",
            &format!("share/topaz/lispex/{path}"),
            bytes,
        )
    }));
    let mut manifest = serde_json::json!({
        "schema": "topaz.lit-artifact-manifest/v1",
        "instance_schema": "topaz.lit-artifact-instance/v1",
        "status": "activated-current",
        "profile": "lispex-profile-1.5",
        "abi": "lispex-lit-abi/v1",
        "availability": {
            "available": true,
            "discoverable_by_product": true,
        },
        "toolchain": {
            "product_version": product_version,
            "minimum_version": product_version,
            "active_generator_version": product_version,
            "backend_version": product_version,
            "language_mode": language_mode,
        },
        "target": {
            "product_target": "test-target",
            "host_variant": "test-host",
        },
        "managed_files": managed_files,
        "canonical_source": installed_identity(
            "topaz-lit-canonical-source-bytes",
            "share/topaz/lispex/lit.tpz",
            canonical_source,
        ),
        "artifacts": [
            {
                "id": "topaz-interpreter",
                "status": "activated-current",
                "host_variant": "topaz-interpreter",
                "artifact": installed_identity(
                    "topaz-lit-product-artifact-bytes",
                    "share/topaz/lispex/lit.tpz",
                    canonical_source,
                ),
            },
            {
                "id": "generated-rust",
                "status": "activated-current",
                "host_variant": "generated-rust",
                "artifact": installed_identity(
                    "topaz-lit-product-artifact-bytes",
                    "libexec/topaz/lispex/lit-runner.exe",
                    native_artifact,
                ),
                "executable": installed_identity(
                    "executed-native-binary-bytes",
                    "libexec/topaz/lispex/lit-runner.exe",
                    native_artifact,
                ),
            },
            {
                "id": "generated-python",
                "status": "activated-current",
                "host_variant": "generated-python",
                "artifact": installed_identity(
                    "topaz-lit-product-artifact-bytes",
                    "share/topaz/lispex/python/managed-files.bundle",
                    python_artifact,
                ),
            },
            {
                "id": "web",
                "status": "activated-current",
                "host_variant": "web",
                "artifact": installed_identity(
                    "topaz-lit-product-artifact-bytes",
                    "share/topaz/lispex/web/managed-files.bundle",
                    web_artifact,
                ),
            },
        ],
        "forbidden_fallback_counts": {
            "debug_binary": 0,
            "host_apply": 0,
            "host_callback": 0,
            "host_control": 0,
            "host_eval": 0,
            "host_source_decoder": 0,
            "runtime_download": 0,
            "rust_backend": 0,
            "sibling_checkout": 0,
        },
        "claim_boundary": {
            "installed_product": true,
            "product_discovery": true,
            "version_activation": true,
            "public_packaging_or_release_change": false,
            "capability_promotion": false,
        },
    });
    manifest[concat!("check", "point")] = JsonValue::String(producer_checkpoint.to_owned());
    write_json_fixture(&manifest_path, &manifest);

    let admitted = Command::new(&installed_executable)
        .args(["lispex", "info", "--json"])
        .output()
        .expect("installed current Topaz executable runs");
    assert!(admitted.status.success(), "{admitted:?}");
    assert!(admitted.stderr.is_empty(), "{admitted:?}");
    let info: JsonValue =
        serde_json::from_slice(&admitted.stdout).expect("Lispex info output is JSON");
    assert_eq!(info["schema"], "topaz.lispex-info/v1");
    assert_eq!(info["available"], true);
    assert_eq!(info["language_mode"], language_mode);

    manifest["checkpoint"] = JsonValue::String("incompatible-checkpoint".to_owned());
    write_json_fixture(&manifest_path, &manifest);
    let rejected = Command::new(&installed_executable)
        .args(["lispex", "info", "--json"])
        .output()
        .expect("installed current Topaz executable reruns");
    let _ = std::fs::remove_dir_all(&root);
    assert!(!rejected.status.success(), "{rejected:?}");
    assert!(rejected.stdout.is_empty(), "{rejected:?}");
    assert!(
        String::from_utf8_lossy(&rejected.stderr)
            .contains("installed JSON field `checkpoint` is incompatible"),
        "{rejected:?}"
    );
}

#[test]
fn managed_artifact_records_current_default_and_explicit_origins() {
    let root =
        std::env::temp_dir().join(format!("topaz_cli_compiler_origin_{}", std::process::id()));
    let entry = root.join("main.tpz");
    let default_out = root.join("default");
    let self_out = root.join("self");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create tempdir");
    std::fs::write(&entry, "function main() -> int { 0 }\n").expect("write entry");

    let default_build = topaz()
        .args(["build", "--target", "python"])
        .arg(&entry)
        .arg("--out-dir")
        .arg(&default_out)
        .output()
        .expect("default build runs");
    assert!(default_build.status.success(), "{default_build:?}");
    let default_manifest =
        std::fs::read_to_string(default_out.join("topaz-artifact.json")).expect("manifest");
    assert!(
        default_manifest.contains("\"selector\": \"rust\""),
        "{default_manifest}"
    );
    assert!(
        default_manifest.contains("\"selectionOrigin\": \"current-default\""),
        "{default_manifest}"
    );

    let self_build = topaz()
        .args(["build", "--target", "python"])
        .arg(&entry)
        .args(["--compiler", "self", "--out-dir"])
        .arg(&self_out)
        .output()
        .expect("self build runs");
    assert!(self_build.status.success(), "{self_build:?}");
    let self_manifest =
        std::fs::read_to_string(self_out.join("topaz-artifact.json")).expect("manifest");
    assert!(
        self_manifest.contains("\"selector\": \"self\""),
        "{self_manifest}"
    );
    assert!(
        self_manifest.contains("\"selectionOrigin\": \"explicit\""),
        "{self_manifest}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
