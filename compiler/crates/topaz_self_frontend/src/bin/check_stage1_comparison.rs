use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use topaz_kernel::{
    ContainmentFact, DirectoryEntry, DirectoryEntryKind, DirectoryFact, HostFact, HostFactSource,
    HostQuery, KernelOutcome, KernelRequest, PackageFacts, SourceFact, TerminalPhase,
};
use topaz_syntax::LangVersion;
use topaz_value::value::{JsonValue, json_parse};

struct FixtureHost {
    files: BTreeMap<String, String>,
    alias: &'static str,
}

impl FixtureHost {
    fn new(alias: &'static str, files: &[(&str, &str)]) -> Self {
        Self {
            files: files
                .iter()
                .map(|(path, source)| ((*path).to_string(), (*source).to_string()))
                .collect(),
            alias,
        }
    }
}

impl HostFactSource for FixtureHost {
    fn respond(&self, _request: &KernelRequest, query: &HostQuery) -> HostFact {
        match query {
            HostQuery::ReadSource { logical_path, .. } => HostFact::Source(
                self.files
                    .get(logical_path)
                    .cloned()
                    .map(SourceFact::Present)
                    .unwrap_or(SourceFact::Missing),
            ),
            HostQuery::ListDirectory { logical_path, .. } => {
                let prefix = if logical_path.is_empty() {
                    String::new()
                } else {
                    format!("{logical_path}/")
                };
                let mut entries = BTreeMap::new();
                for path in self.files.keys() {
                    let Some(rest) = path.strip_prefix(&prefix) else {
                        continue;
                    };
                    let (name, kind) = match rest.split_once('/') {
                        Some((name, _)) => (name, DirectoryEntryKind::Directory),
                        None => (rest, DirectoryEntryKind::File),
                    };
                    entries.insert(name.to_string(), kind);
                }
                HostFact::Directory(if entries.is_empty() {
                    DirectoryFact::Missing
                } else {
                    DirectoryFact::Present(
                        entries
                            .into_iter()
                            .map(|(name, kind)| DirectoryEntry { name, kind })
                            .collect(),
                    )
                })
            }
            HostQuery::PhysicalContainment { logical_path, .. } => {
                HostFact::Containment(ContainmentFact::Inside {
                    alias_class: format!("{}:{logical_path}", self.alias),
                })
            }
        }
    }
}

fn request(entry: &str, root: Option<&str>, terminal: TerminalPhase) -> KernelRequest {
    KernelRequest::checked(
        entry,
        root,
        LangVersion::CURRENT,
        PackageFacts::standalone(),
    )
    .with_terminal_phase(terminal)
}

fn sha256(bytes: &[u8]) -> String {
    let digest = topaz_value::value::sha256(bytes);
    let mut output = String::new();
    topaz_value::bytes_to_hex_into(&mut output, &digest);
    output
}

fn manifest_generated_rust_identity(manifest: &[u8]) -> Result<(String, usize), String> {
    let text = std::str::from_utf8(manifest)
        .map_err(|error| format!("generated artifacts manifest is not UTF-8: {error}"))?;
    let parsed = json_parse(text)
        .map_err(|error| format!("generated artifacts manifest is not JSON: {error:?}"))?;
    let JsonValue::Object(root) = parsed else {
        return Err("generated artifacts manifest root is not an object".to_string());
    };
    let Some(JsonValue::Object(generated)) = root.get("generatedRust") else {
        return Err("generated artifacts manifest omits generatedRust".to_string());
    };
    let Some(JsonValue::String(hash)) = generated.get("sha256") else {
        return Err(
            "generated artifacts manifest generatedRust.sha256 is not a string".to_string(),
        );
    };
    let Some(JsonValue::Number(bytes)) = generated.get("bytes") else {
        return Err("generated artifacts manifest generatedRust.bytes is not a number".to_string());
    };
    let bytes = bytes.lexeme.parse::<usize>().map_err(|_| {
        "generated artifacts manifest generatedRust.bytes is not a usize".to_string()
    })?;
    Ok((hash.to_string(), bytes))
}

fn generated_rust_identity_matches(
    expected_hash: &str,
    expected_bytes: usize,
    actual_hash: &str,
    actual_bytes: usize,
) -> bool {
    expected_hash == actual_hash && expected_bytes == actual_bytes
}

fn stage0_operation_projection(
    execution: &topaz_kernel::KernelExecution,
) -> Result<BTreeMap<String, (Vec<String>, String)>, String> {
    let KernelOutcome::Completed(unit) = &execution.outcome else {
        return Err("Stage 0 positive comparison did not complete".to_string());
    };
    let lowered = unit
        .lowered
        .as_ref()
        .ok_or_else(|| "Stage 0 positive comparison omitted lowered IR".to_string())?;
    Ok(lowered
        .operations
        .iter()
        .map(|operation| {
            (
                operation.id.clone(),
                (
                    operation.operands.clone(),
                    operation.runtime_leaf.clone().unwrap_or_default(),
                ),
            )
        })
        .collect())
}

fn stage1_operation_projection(
    result: &topaz_self_frontend::Stage1LoweringPreviewResult,
) -> BTreeMap<String, (Vec<String>, String)> {
    result
        .operations
        .iter()
        .map(|operation| {
            (
                operation.id.clone(),
                (operation.operands.clone(), operation.runtime_leaf.clone()),
            )
        })
        .collect()
}

fn compile_and_run_stage1_product(source: &str) -> Result<Vec<u8>, String> {
    let entrypoint = if source.contains("\npub fn compiler_preview_i64()") {
        "compiler_preview_i64"
    } else if source.contains("\npub fn stage1_preview_i64()") {
        "stage1_preview_i64"
    } else {
        return Err("Stage 1 generated product omits a supported preview entrypoint".to_string());
    };
    let root = std::env::temp_dir().join(format!(
        "topaz-stage1-comparison-product-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root)
            .map_err(|error| format!("cannot remove stale product verification: {error}"))?;
    }
    std::fs::create_dir_all(&root)
        .map_err(|error| format!("cannot create product verification: {error}"))?;
    let generated = root.join("generated.rs");
    let main = root.join("main.rs");
    let binary = root.join("stage1-product");
    std::fs::write(&generated, source)
        .map_err(|error| format!("cannot write Stage 1 generated source: {error}"))?;
    std::fs::write(
        &main,
        format!(
            "mod generated {{ include!({generated:?}); }}\nfn main() {{ print!(\"{{}}\", generated::{entrypoint}().expect(\"preview result\")); }}\n"
        ),
    )
    .map_err(|error| format!("cannot write Stage 1 product wrapper: {error}"))?;
    let compile = std::process::Command::new("rustc")
        .arg("--edition=2024")
        .arg(&main)
        .arg("-o")
        .arg(&binary)
        .output()
        .map_err(|error| format!("cannot launch rustc for Stage 1 product: {error}"))?;
    if !compile.status.success() {
        return Err(format!(
            "Stage 1 generated product did not compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        ));
    }
    let output = std::process::Command::new(&binary)
        .output()
        .map_err(|error| format!("cannot run Stage 1 product: {error}"))?;
    let cleanup = std::fs::remove_dir_all(&root);
    if !output.status.success() {
        return Err(format!(
            "Stage 1 generated product failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    cleanup.map_err(|error| format!("cannot remove product verification: {error}"))?;
    Ok(output.stdout)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "comparison receipt has no parent".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create comparison receipt parent: {error}"))?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&temporary, bytes)
        .map_err(|error| format!("cannot write comparison receipt: {error}"))?;
    std::fs::rename(&temporary, path)
        .map_err(|error| format!("cannot install comparison receipt: {error}"))
}

fn main() -> Result<(), String> {
    let mut output = None;
    let mut compiler_generated_rust = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(flag) = arguments.next() {
        let value = arguments.next().ok_or_else(|| {
            format!("{flag} requires a path; usage: check_stage1_comparison [--out <path>] [--compiler-generated-rust <path>]")
        })?;
        let slot = match flag.as_str() {
            "--out" => &mut output,
            "--compiler-generated-rust" => &mut compiler_generated_rust,
            _ => {
                return Err(
                    "usage: check_stage1_comparison [--out <path>] [--compiler-generated-rust <path>]"
                        .to_string(),
                );
            }
        };
        if slot.replace(PathBuf::from(value)).is_some() {
            return Err(format!("duplicate {flag}"));
        }
    }

    let ordinary = FixtureHost::new(
        "ordinary",
        &[
            (
                "root/main.tpz",
                concat!(
                    "import lib { base }\n",
                    "let values = [base, 2]\n",
                    "let answer = values[0] + values[1]\n",
                ),
            ),
            ("root/lib.tpz", "export const base = 40\n"),
        ],
    );
    let lowered_request = request("root/main.tpz", Some("root"), TerminalPhase::Lowered);
    let stage0 = topaz_kernel::drive_checked(&ordinary, lowered_request.clone());
    let linked =
        topaz_self_frontend::preview_linked_stage1_lowered(&ordinary, lowered_request.clone())?;
    let interpreted = topaz_self_frontend::preview_stage1_lowered(&ordinary, lowered_request)?;
    let stage0_projection = stage0_operation_projection(&stage0)?;
    let linked_projection = stage1_operation_projection(&linked);
    let interpreted_projection = stage1_operation_projection(&interpreted);
    if linked.status != "completed"
        || interpreted.status != "completed"
        || !linked.unsupported.is_empty()
        || linked.front_end != interpreted.front_end
        || linked_projection != interpreted_projection
        || stage0_projection != linked_projection
    {
        let first_mismatch = stage0_projection.iter().find(|(id, value)| {
            linked_projection
                .get(*id)
                .is_none_or(|linked| linked != *value)
        });
        return Err(format!(
            "ordinary semantic comparison disagreed: linked={}, interpreted={}, unsupported={:?}, front_end_equal={}, linked_interpreted_ops={}, stage0_linked_ops={}, stage0_rows={}, linked_rows={}, first_mismatch={first_mismatch:?}",
            linked.status,
            interpreted.status,
            linked.unsupported,
            linked.front_end == interpreted.front_end,
            linked_projection == interpreted_projection,
            stage0_projection == linked_projection,
            stage0_projection.len(),
            linked_projection.len(),
        ));
    }

    let negative = FixtureHost::new(
        "negative",
        &[("root/main.tpz", "let answer: int = \"no\"\n")],
    );
    let negative_request = request("root/main.tpz", Some("root"), TerminalPhase::Lowered);
    let stage0_negative = topaz_kernel::drive_checked(&negative, negative_request.clone());
    let linked_negative =
        topaz_self_frontend::preview_linked_stage1_lowered(&negative, negative_request.clone())?;
    let interpreted_negative =
        topaz_self_frontend::preview_stage1_lowered(&negative, negative_request)?;
    if !matches!(stage0_negative.outcome, KernelOutcome::Rejected(_))
        || linked_negative.status != "rejected"
        || !linked_negative.operations.is_empty()
        || linked_negative.front_end != interpreted_negative.front_end
    {
        return Err("negative diagnostic comparison disagreed".to_string());
    }

    let product = FixtureHost::new("product", &[("main.tpz", "let answer = 40 + 2\n")]);
    let generated_request = request("main.tpz", Some(""), TerminalPhase::RustSource);
    let linked_generated =
        topaz_self_frontend::preview_linked_stage1_generated(&product, generated_request.clone())?;
    let interpreted =
        topaz_self_frontend::preview_stage1_generated(&product, generated_request.clone())?;
    if linked_generated.status != "completed"
        || interpreted.status != "completed"
        || linked_generated.generated_rust != interpreted.generated_rust
    {
        return Err("linked and interpreted Stage 1 generated Rust disagreed".to_string());
    }
    let stage0_product = topaz_kernel::drive_checked(&product, generated_request.clone());
    let KernelOutcome::Completed(stage0_product) = stage0_product.outcome else {
        return Err("Stage 0 generated-source canary did not complete".to_string());
    };
    let stage0_rust = stage0_product
        .rust_source
        .as_deref()
        .ok_or_else(|| "Stage 0 generated-source canary omitted Rust".to_string())?;
    if stage0_rust == linked_generated.generated_rust {
        return Err("cross-producer source difference was not observable".to_string());
    }
    let product_output = compile_and_run_stage1_product(&linked_generated.generated_rust)?;
    if product_output != b"42" {
        return Err("Stage 1 generated product did not observe 42".to_string());
    }

    let encoded = topaz_self_frontend::encode_stage1_request(&generated_request)?;
    let wrong_producer = String::from_utf8(encoded)
        .map_err(|error| format!("request is not UTF-8: {error}"))?
        .replacen(
            "\"producer\":\"topaz-stage1\"",
            "\"producer\":\"rust-stage0\"",
            1,
        );
    if topaz_stage1_runtime::execute_embedded_compiler(wrong_producer.as_bytes()).is_ok() {
        return Err("wrong producer was accepted instead of failing closed".to_string());
    }

    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../generated/topaz_compiler_generated_artifacts.json");
    let manifest = std::fs::read(&manifest_path)
        .map_err(|error| format!("cannot read generated artifacts manifest: {error}"))?;
    let (manifest_generated_hash, manifest_generated_bytes) =
        manifest_generated_rust_identity(&manifest)?;
    let compiler_generated_rust = if let Some(path) = compiler_generated_rust {
        std::fs::read(&path).map_err(|error| {
            format!(
                "cannot read compiler generated Rust {}: {error}",
                path.display()
            )
        })?
    } else {
        let mut compiler_request = request("src/main.tpz", Some(""), TerminalPhase::RustSource);
        topaz_self_frontend::supply_embedded_compiler_source_facts(&mut compiler_request)
            .map_err(|error| format!("cannot seed embedded compiler source facts: {error:?}"))?;
        let mut compiler_generated = topaz_self_frontend::preview_linked_stage1_generated(
            &topaz_self_frontend::EmbeddedCompilerSourceHost,
            compiler_request,
        )?;
        topaz_self_frontend::seal_compiler_program_target_facts(&mut compiler_generated)?;
        compiler_generated.generated_rust.into_bytes()
    };
    let compiler_generated_hash = format!("sha256:{}", sha256(&compiler_generated_rust));
    if !generated_rust_identity_matches(
        &manifest_generated_hash,
        manifest_generated_bytes,
        &compiler_generated_hash,
        compiler_generated_rust.len(),
    ) {
        return Err("fresh generated source does not match its artifact manifest".to_string());
    }
    let digest = compiler_generated_hash
        .strip_prefix("sha256:")
        .ok_or_else(|| "fresh compiler source identity omitted sha256 prefix".to_string())?;
    let mutated_prefix = if digest.starts_with('0') { "1" } else { "0" };
    let mutated_hash = format!("sha256:{mutated_prefix}{}", &digest[1..]);
    if generated_rust_identity_matches(
        &mutated_hash,
        manifest_generated_bytes,
        &compiler_generated_hash,
        compiler_generated_rust.len(),
    ) {
        return Err("managed-artifact corruption canary did not fail".to_string());
    }

    let report = format!(
        concat!(
            "{{\n",
            "  \"schema\": \"topaz.compiler.stage1-comparison-receipt/v1\",\n",
            "  \"productVersion\": \"{}\",\n",
            "  \"languageMode\": \"topaz-{}\",\n",
            "  \"producerStage\": 1,\n",
            "  \"resultStage\": 1,\n",
            "  \"defaultEngine\": \"rust-stage0\",\n",
            "  \"fixedPoint\": \"not-run\",\n",
            "  \"sourceSetId\": \"{}\",\n",
            "  \"semantic\": {{\"ordinaryMultiModule\": \"equal\", \"linkedVsInterpretedC1\": \"equal\", \"operationRows\": {}}},\n",
            "  \"diagnostic\": {{\"typeMismatch\": \"equal\", \"frontEndSha256\": \"{}\"}},\n",
            "  \"generatedSource\": {{\"stage0Disposition\": \"different-from-stage1-observed\", \"stage1Disposition\": \"linked-interpreted-equal\", \"crossProducer\": \"different-observed\", \"stage0Sha256\": \"{}\", \"stage1Sha256\": \"{}\"}},\n",
            "  \"product\": {{\"stage1Output\": \"42\", \"manifestIdentity\": \"match\", \"corruptionCanary\": \"rejected\"}},\n",
            "  \"provenance\": {{\"complete\": true, \"targetCompilerFallback\": false, \"wrongProducer\": \"rejected\"}}\n",
            "}}\n"
        ),
        env!("CARGO_PKG_VERSION"),
        LangVersion::CURRENT.as_str(),
        topaz_self_frontend::source_set_id(),
        linked.operations.len(),
        sha256(linked_negative.front_end.as_bytes()),
        sha256(stage0_rust.as_bytes()),
        sha256(linked_generated.generated_rust.as_bytes()),
    );
    if let Some(path) = output {
        atomic_write(&path, report.as_bytes())?;
    } else {
        print!("{report}");
    }
    Ok(())
}
