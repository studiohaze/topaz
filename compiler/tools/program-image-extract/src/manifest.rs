use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use super::{sha256_identity, write_atomic};

const GENERATION_PROFILE_ID: &str = "topaz-bootstrap-canonical/v1";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileObservation {
    path: String,
    bytes: usize,
    sha256: String,
}

struct SourceObservation {
    entries: Vec<FileObservation>,
    source_set_id: String,
    file_manifest_sha256: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EmbeddedSourceManifest<'a> {
    schema: &'static str,
    source_set_id: &'a str,
    files: &'a [FileObservation],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactSourceManifest<'a> {
    schema: &'static str,
    compiler_source_manifest_sha256: &'a str,
    generation_input_set_id: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompilationRequestDescriptor {
    schema: &'static str,
    generation_profile_id: &'static str,
    entry_source: &'static str,
    source_set_id: String,
    file_manifest_sha256: String,
    source_paths: Vec<String>,
    source_order: &'static str,
    language_mode: &'static str,
    package_facts_profile: &'static str,
    standard_library: &'static str,
    prelude: &'static str,
    feature_set: Vec<String>,
    stdin_sha256: String,
    terminal_phase: &'static str,
    output_format: &'static str,
    runtime_registry: &'static str,
    runtime_template: &'static str,
    generated_payload_schema: &'static str,
    ir_schema: &'static str,
    diagnostic_mode: &'static str,
    target_triple: &'static str,
    target_triple_disposition: &'static str,
    target_facts_sha256: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExtractionCompression {
    algorithm: &'static str,
    version: &'static str,
    parameters: &'static str,
    dictionary_sha256: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExtractionConfiguration {
    schema: &'static str,
    format_magic: &'static str,
    format_version: u8,
    integer_encoding: &'static str,
    operation_ordering: &'static str,
    module_ordering: &'static str,
    normalization: &'static str,
    compression: ExtractionCompression,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeneratedArtifactsManifest {
    schema: &'static str,
    compiler_source_set: CompilerSourceSet,
    compilation_request: CompilationRequest,
    generated_rust: GeneratedRust,
    target_facts: TargetFacts,
    program_image: ProgramImage,
    extraction: Extraction,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompilerSourceSet {
    source_set_id: String,
    file_manifest_sha256: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompilationRequest {
    sha256: String,
    canonicalization: &'static str,
    generation_profile_id: &'static str,
    target_facts_sha256: String,
    target_triple: &'static str,
    descriptor: CompilationRequestDescriptor,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeneratedRust {
    format_schema: &'static str,
    sha256: String,
    bytes: usize,
    normalization: &'static str,
    storage_disposition: &'static str,
    distribution: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TargetFacts {
    format_schema: &'static str,
    path: &'static str,
    sha256: String,
    bytes: usize,
    storage_disposition: &'static str,
    selection: &'static str,
    reason: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgramImage {
    format_schema: &'static str,
    path: &'static str,
    sha256: String,
    bytes: usize,
    stage_identity: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Extraction {
    operation: &'static str,
    input_generated_rust_sha256: String,
    output_program_image_sha256: String,
    output_target_facts_sha256: String,
    extractor_source_set_id: String,
    extractor_binary_sha256: String,
    extractor_command: &'static str,
    toolchain_id: String,
    toolchain_manifest_sha256: String,
    lockfile_sha256: String,
    configuration_sha256: String,
    configuration: ExtractionConfiguration,
    compression_algorithm: &'static str,
    compression_version: &'static str,
    compression_parameters: &'static str,
    dictionary_sha256: Option<String>,
    normalization: &'static str,
    execution_disposition: &'static str,
}

pub(super) struct ManifestSummary {
    pub(super) generated_rust_sha256: String,
    pub(super) program_image_sha256: String,
}

fn repository_root() -> Result<PathBuf, String> {
    fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."))
        .map_err(|error| format!("cannot locate repository root: {error}"))
}

fn normalized_git_text(mut bytes: Vec<u8>) -> Vec<u8> {
    let Some(first) = bytes.windows(2).position(|pair| pair == b"\r\n") else {
        return bytes;
    };
    let mut input = first;
    let mut output = first;
    while input < bytes.len() {
        if bytes.get(input..input + 2) == Some(b"\r\n") {
            bytes[output] = b'\n';
            input += 2;
        } else {
            bytes[output] = bytes[input];
            input += 1;
        }
        output += 1;
    }
    bytes.truncate(output);
    bytes
}

fn read_git_text_file(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path)
        .map(normalized_git_text)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))
}

fn walk_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut entries = fs::read_dir(root)
        .map_err(|error| format!("cannot read directory {}: {error}", root.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("cannot read directory {}: {error}", root.display()))?;
    entries.sort_by_key(fs::DirEntry::file_name);
    let mut files = Vec::new();
    for entry in entries {
        let file_type = entry
            .file_type()
            .map_err(|error| format!("cannot inspect {}: {error}", entry.path().display()))?;
        if file_type.is_dir() {
            files.extend(walk_files(&entry.path())?);
        } else if file_type.is_file() {
            files.push(entry.path());
        }
    }
    Ok(files)
}

fn repository_relative_path(path: &Path, base: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(base)
        .map_err(|_| format!("{} is outside {}", path.display(), base.display()))?;
    relative
        .components()
        .map(|component| match component {
            Component::Normal(part) => part
                .to_str()
                .map(str::to_string)
                .ok_or_else(|| format!("path is not UTF-8: {}", path.display())),
            _ => Err(format!("path is not canonical: {}", path.display())),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("/"))
}

fn git_text_rows(paths: Vec<PathBuf>, base: &Path) -> Result<Vec<FileObservation>, String> {
    let mut rows = paths
        .into_iter()
        .map(|path| {
            let bytes = read_git_text_file(&path)?;
            Ok(FileObservation {
                path: repository_relative_path(&path, base)?,
                bytes: bytes.len(),
                sha256: sha256_identity(&bytes),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    rows.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(rows)
}

fn framed_set_identity(entries: &[FileObservation]) -> String {
    let mut framed = Vec::new();
    for entry in entries {
        framed.extend_from_slice(entry.path.as_bytes());
        framed.push(0);
        framed.extend_from_slice(entry.bytes.to_string().as_bytes());
        framed.push(0);
        framed.extend_from_slice(&entry.sha256.as_bytes()["sha256:".len()..]);
        framed.push(b'\n');
    }
    sha256_identity(&framed)
}

fn canonical_sha256(value: &impl Serialize) -> Result<String, String> {
    let value = serde_json::to_value(value)
        .map_err(|error| format!("cannot canonicalize manifest value: {error}"))?;
    let bytes = serde_json::to_vec(&value)
        .map_err(|error| format!("cannot encode canonical manifest value: {error}"))?;
    Ok(sha256_identity(&bytes))
}

fn generation_input_files(repository_root: &Path) -> Result<Vec<PathBuf>, String> {
    let inventory_path =
        repository_root.join("compiler/crates/topaz_stage1_runtime/compiler-generation-inputs.txt");
    let inventory_bytes = read_git_text_file(&inventory_path)?;
    let inventory = std::str::from_utf8(&inventory_bytes)
        .map_err(|error| format!("{} is not UTF-8: {error}", inventory_path.display()))?;
    let mut files = BTreeMap::new();
    for (index, row) in inventory.lines().enumerate() {
        let relative = row.trim();
        if relative.is_empty() {
            continue;
        }
        if Path::new(relative).is_absolute()
            || relative
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(format!(
                "source-set: generation input row {} is not a canonical repository path",
                index + 1
            ));
        }
        let input = relative
            .split('/')
            .fold(repository_root.to_path_buf(), |path, part| path.join(part));
        let observed = if input.is_dir() {
            walk_files(&input)?
        } else if input.is_file() {
            vec![input]
        } else {
            return Err(format!(
                "source-set: generation input {} does not exist",
                input.display()
            ));
        };
        for file in observed {
            let key = repository_relative_path(&file, repository_root)?;
            if files.insert(key.clone(), file).is_some() {
                return Err(format!(
                    "source-set: generation input {key} is listed more than once"
                ));
            }
        }
    }
    Ok(files.into_values().collect())
}

fn source_observation(repository_root: &Path) -> Result<SourceObservation, String> {
    let source_root = repository_root.join("compiler/crates/topaz_self_frontend/topaz");
    let source_files = walk_files(&source_root)?
        .into_iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "tpz"))
        .collect();
    let entries = git_text_rows(source_files, &source_root)?;
    let source_set_id = framed_set_identity(&entries);
    let embedded_source_manifest = EmbeddedSourceManifest {
        schema: "topaz.compiler.embedded-source-manifest/v1",
        source_set_id: &source_set_id,
        files: &entries,
    };
    let compiler_source_manifest_sha256 = canonical_sha256(&embedded_source_manifest)?;
    let generation_inputs =
        git_text_rows(generation_input_files(repository_root)?, repository_root)?;
    let generation_input_set_id = framed_set_identity(&generation_inputs);
    let file_manifest = ArtifactSourceManifest {
        schema: "topaz.compiler.artifact-source-manifest/v1",
        compiler_source_manifest_sha256: &compiler_source_manifest_sha256,
        generation_input_set_id: &generation_input_set_id,
    };
    Ok(SourceObservation {
        entries,
        source_set_id,
        file_manifest_sha256: canonical_sha256(&file_manifest)?,
    })
}

fn extractor_source_set_id(repository_root: &Path) -> Result<String, String> {
    let compiler_root = repository_root.join("compiler");
    let mut files = vec![compiler_root.join("Cargo.toml")];
    for relative in [
        "crates/topaz_diag",
        "crates/topaz_syntax",
        "crates/topaz_value",
        "tools/program-image-extract",
    ] {
        files.extend(walk_files(&compiler_root.join(relative))?);
    }
    Ok(framed_set_identity(&git_text_rows(files, repository_root)?))
}

fn rustc_identity(compiler_root: &Path) -> Result<String, String> {
    let output = Command::new("rustc")
        .arg("-vV")
        .current_dir(compiler_root)
        .output()
        .map_err(|error| format!("cannot identify rustc: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cannot identify rustc: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stdout = std::str::from_utf8(&output.stdout)
        .map_err(|error| format!("rustc -vV output is not UTF-8: {error}"))?;
    let release = stdout
        .lines()
        .find_map(|line| line.strip_prefix("release: "))
        .ok_or_else(|| "rustc -vV omitted release".to_string())?;
    let host = stdout
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or_else(|| "rustc -vV omitted host".to_string())?;
    Ok(format!("rustc-{release}-{host}"))
}

fn build_manifest(
    generated_rust: &[u8],
    program_image: &[u8],
    target_facts: &[u8],
    extracted_source_set_id: &str,
) -> Result<GeneratedArtifactsManifest, String> {
    let repository_root = repository_root()?;
    let compiler_root = repository_root.join("compiler");
    let source = source_observation(&repository_root)?;
    if source.source_set_id != extracted_source_set_id {
        return Err(format!(
            "compiler source set {} != extracted source set {extracted_source_set_id}",
            source.source_set_id
        ));
    }
    let generated_rust_sha256 = sha256_identity(generated_rust);
    let program_image_sha256 = sha256_identity(program_image);
    let target_facts_sha256 = sha256_identity(target_facts);
    let descriptor = CompilationRequestDescriptor {
        schema: "topaz.compiler.compilation-request/v1",
        generation_profile_id: GENERATION_PROFILE_ID,
        entry_source: "src/main.tpz",
        source_set_id: source.source_set_id.clone(),
        file_manifest_sha256: source.file_manifest_sha256.clone(),
        source_paths: source.entries.into_iter().map(|entry| entry.path).collect(),
        source_order: "manifest-order",
        language_mode: "topaz-5.20",
        package_facts_profile: "standalone",
        standard_library: "none",
        prelude: "none",
        feature_set: Vec::new(),
        stdin_sha256: sha256_identity(&[]),
        terminal_phase: "rust-source",
        output_format: "topaz.compiler.generated-rust/v1",
        runtime_registry: "topaz.compiler.fixed-point-runtime-templates/v1",
        runtime_template: "compiler-ir-table/v2",
        generated_payload_schema: "topaz.compiler.fixed-point-ir-payload/v1",
        ir_schema: "topaz.compiler.stage1-ir/v1",
        diagnostic_mode: "compiler-exchange",
        target_triple: "none",
        target_triple_disposition: "stage-neutral-generated-source",
        target_facts_sha256: target_facts_sha256.clone(),
    };
    let compilation_request_sha256 = canonical_sha256(&descriptor)?;
    let configuration = ExtractionConfiguration {
        schema: "topaz.compiler.program-image-extraction-configuration/v1",
        format_magic: "TPZIMAGE",
        format_version: 1,
        integer_encoding: "little-endian-u32",
        operation_ordering: "source-lowered-operation-order",
        module_ordering: "source-lowered-module-order",
        normalization: "none",
        compression: ExtractionCompression {
            algorithm: "none",
            version: "none",
            parameters: "none",
            dictionary_sha256: None,
        },
    };
    let configuration_sha256 = canonical_sha256(&configuration)?;
    let lockfile = read_git_text_file(&compiler_root.join("Cargo.lock"))?;
    let toolchain = read_git_text_file(&compiler_root.join("rust-toolchain.toml"))?;
    let extractor_binary = fs::read(
        std::env::current_exe()
            .map_err(|error| format!("cannot locate extractor binary: {error}"))?,
    )
    .map_err(|error| format!("cannot read extractor binary: {error}"))?;
    Ok(GeneratedArtifactsManifest {
        schema: "topaz.compiler.generated-artifacts/v1",
        compiler_source_set: CompilerSourceSet {
            source_set_id: source.source_set_id,
            file_manifest_sha256: source.file_manifest_sha256,
        },
        compilation_request: CompilationRequest {
            sha256: compilation_request_sha256,
            canonicalization: "recursive-lexicographic-key-order-json-utf8/v1",
            generation_profile_id: GENERATION_PROFILE_ID,
            target_facts_sha256: target_facts_sha256.clone(),
            target_triple: "none",
            descriptor,
        },
        generated_rust: GeneratedRust {
            format_schema: "topaz.compiler.generated-rust/v1",
            sha256: generated_rust_sha256.clone(),
            bytes: generated_rust.len(),
            normalization: "none",
            storage_disposition: "generated-on-verification",
            distribution: None,
        },
        target_facts: TargetFacts {
            format_schema: "topaz.self-target-adapter-facts/v1",
            path: "compiler/generated/topaz_compiler_target_facts.json",
            sha256: target_facts_sha256.clone(),
            bytes: target_facts.len(),
            storage_disposition: "checked-in-sidecar",
            selection: "option-2-sidecar-artifact",
            reason: "The public runtime consumes target facts as a separate input, so their complete bytes are checked in and hash-bound instead of being hidden inside the program image.",
        },
        program_image: ProgramImage {
            format_schema: "topaz.compiler.program-image/v1",
            path: "compiler/generated/topaz_compiler_program_image.bin",
            sha256: program_image_sha256.clone(),
            bytes: program_image.len(),
            stage_identity: "none",
        },
        extraction: Extraction {
            operation: "extract-program-image",
            input_generated_rust_sha256: generated_rust_sha256,
            output_program_image_sha256: program_image_sha256,
            output_target_facts_sha256: target_facts_sha256,
            extractor_source_set_id: extractor_source_set_id(&repository_root)?,
            extractor_binary_sha256: sha256_identity(&extractor_binary),
            extractor_command: "cargo run --release --locked -p topaz_program_image_extract -- --generated-rust <R1> --out-image <IMAGE> --out-target-facts <TARGET_FACTS> --out-manifest <MANIFEST>",
            toolchain_id: rustc_identity(&compiler_root)?,
            toolchain_manifest_sha256: sha256_identity(&toolchain),
            lockfile_sha256: sha256_identity(&lockfile),
            configuration_sha256,
            configuration,
            compression_algorithm: "none",
            compression_version: "none",
            compression_parameters: "none",
            dictionary_sha256: None,
            normalization: "none",
            execution_disposition: "fresh",
        },
    })
}

pub(super) fn write_generated_artifacts_manifest(
    output_path: &Path,
    generated_rust: &[u8],
    program_image: &[u8],
    target_facts: &[u8],
    source_set_id: &str,
) -> Result<ManifestSummary, String> {
    let manifest = build_manifest(generated_rust, program_image, target_facts, source_set_id)?;
    let summary = ManifestSummary {
        generated_rust_sha256: manifest.generated_rust.sha256.clone(),
        program_image_sha256: manifest.program_image.sha256.clone(),
    };
    let mut bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("cannot encode generated artifacts manifest: {error}"))?;
    bytes.push(b'\n');
    write_atomic(output_path, &bytes)?;
    Ok(summary)
}
