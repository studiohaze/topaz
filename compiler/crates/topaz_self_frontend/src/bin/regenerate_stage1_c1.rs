use std::ffi::OsString;
use std::path::PathBuf;

#[path = "support/generator.rs"]
mod generator;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Producer {
    Interpreted,
    ProgramImage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ManifestIdentity {
    Require,
    Refresh,
}

impl Producer {
    fn route(self) -> &'static str {
        match self {
            Self::Interpreted => "rust-stage0-direct",
            Self::ProgramImage => "checked-in-program-image",
        }
    }
}

struct Arguments {
    output_rust: PathBuf,
    output_manifest: PathBuf,
    artifact_manifest: PathBuf,
    producer: Producer,
    manifest_identity: ManifestIdentity,
}

fn parse_arguments(arguments: impl IntoIterator<Item = OsString>) -> Result<Arguments, String> {
    let mut options = generator::OptionValues::parse(
        arguments,
        &[
            "--out-rust",
            "--out-manifest",
            "--artifact-manifest",
            "--producer",
            "--manifest-identity",
        ],
    )?;
    let producer = options
        .value("--producer", true)?
        .ok_or_else(|| "missing --producer".to_string())?
        .into_string()
        .map_err(|value| format!("--producer is not UTF-8: {}", value.to_string_lossy()))?;
    let producer = match producer.as_str() {
        "interpreted" => Producer::Interpreted,
        "program-image" => Producer::ProgramImage,
        other => {
            return Err(format!(
                "unsupported --producer `{other}`; expected interpreted or program-image"
            ));
        }
    };
    let manifest_identity = options
        .value("--manifest-identity", false)?
        .map(|value| {
            value.into_string().map_err(|value| {
                format!(
                    "--manifest-identity is not UTF-8: {}",
                    value.to_string_lossy()
                )
            })
        })
        .transpose()?
        .unwrap_or_else(|| "require".to_string());
    let manifest_identity = match manifest_identity.as_str() {
        "require" => ManifestIdentity::Require,
        "refresh" if producer == Producer::Interpreted => ManifestIdentity::Refresh,
        "refresh" => {
            return Err("--manifest-identity refresh requires --producer interpreted".to_string());
        }
        other => {
            return Err(format!(
                "unsupported --manifest-identity `{other}`; expected require or refresh"
            ));
        }
    };
    Ok(Arguments {
        output_rust: options
            .path("--out-rust", true)?
            .ok_or_else(|| "missing --out-rust".to_string())?,
        output_manifest: options
            .path("--out-manifest", true)?
            .ok_or_else(|| "missing --out-manifest".to_string())?,
        artifact_manifest: options
            .path("--artifact-manifest", true)?
            .ok_or_else(|| "missing --artifact-manifest".to_string())?,
        producer,
        manifest_identity,
    })
}

fn main() -> Result<(), String> {
    let arguments = parse_arguments(std::env::args_os().skip(1))?;
    let artifact_manifest = std::fs::read(&arguments.artifact_manifest).map_err(|error| {
        format!(
            "cannot read {}: {error}",
            arguments.artifact_manifest.display()
        )
    })?;
    let artifact_manifest_sha256 = format!("sha256:{}", generator::sha256_hex(&artifact_manifest));
    let descriptor = topaz_stage1_runtime::compiler_image_descriptor(
        topaz_stage1_runtime::CompilerImageStage::C1,
    );
    if artifact_manifest_sha256 != descriptor.manifest_sha256 {
        return Err(format!(
            "generated artifacts manifest identity drifted: expected {}, observed {}",
            descriptor.manifest_sha256, artifact_manifest_sha256
        ));
    }

    let mut request = topaz_kernel::KernelRequest::checked(
        "src/main.tpz",
        Some(""),
        topaz_syntax::LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::RustSource);
    topaz_self_frontend::supply_embedded_compiler_source_facts(&mut request)
        .map_err(|error| format!("cannot seed embedded compiler source facts: {error:?}"))?;
    let mut generated = match arguments.producer {
        Producer::Interpreted => topaz_self_frontend::preview_stage1_generated(
            &topaz_self_frontend::EmbeddedCompilerSourceHost,
            request,
        )?,
        Producer::ProgramImage => topaz_self_frontend::preview_linked_stage1_generated(
            &topaz_self_frontend::EmbeddedCompilerSourceHost,
            request,
        )?,
    };
    topaz_self_frontend::seal_compiler_program_target_facts(&mut generated)?;
    let generated_rust_sha256 = format!(
        "sha256:{}",
        generator::sha256_hex(generated.generated_rust.as_bytes())
    );
    if arguments.manifest_identity == ManifestIdentity::Require
        && (generated.generated_rust.len() as u64 != descriptor.generated_rust_bytes
            || generated_rust_sha256 != descriptor.generated_rust_sha256)
    {
        return Err(format!(
            "fresh generated Rust does not match manifest identity: observed {} bytes {}, expected {} bytes {}",
            generated.generated_rust.len(),
            generated_rust_sha256,
            descriptor.generated_rust_bytes,
            descriptor.generated_rust_sha256,
        ));
    }
    if generated.provenance_source_set_id != topaz_self_frontend::source_set_id() {
        return Err("fresh generation returned a stale compiler source set".to_string());
    }

    let receipt = format!(
        concat!(
            "{{\n",
            "  \"schema\": \"topaz.compiler.generated-artifact-route/v1\",\n",
            "  \"route\": \"{}\",\n",
            "  \"executionDisposition\": \"fresh\",\n",
            "  \"sourceSetId\": \"{}\",\n",
            "  \"compilationRequestSha256\": \"{}\",\n",
            "  \"artifactManifestSha256\": \"{}\",\n",
            "  \"generatedRustSha256\": \"{}\",\n",
            "  \"generatedRustBytes\": {},\n",
            "  \"manifestIdentityDisposition\": \"{}\",\n",
            "  \"normalization\": \"none\",\n",
            "  \"targetCompilerFallback\": false\n",
            "}}\n"
        ),
        arguments.producer.route(),
        generated.provenance_source_set_id,
        topaz_stage1_runtime::embedded_compilation_request_sha256(),
        artifact_manifest_sha256,
        generated_rust_sha256,
        generated.generated_rust.len(),
        match arguments.manifest_identity {
            ManifestIdentity::Require => "match",
            ManifestIdentity::Refresh => "refresh",
        },
    );
    generator::write_atomic(&arguments.output_rust, generated.generated_rust.as_bytes())?;
    generator::write_atomic(&arguments.output_manifest, receipt.as_bytes())?;
    println!(
        "PASS manifest identity {}: route={} {} bytes {}",
        match arguments.manifest_identity {
            ManifestIdentity::Require => "match",
            ManifestIdentity::Refresh => "refresh",
        },
        arguments.producer.route(),
        generated.generated_rust.len(),
        generated_rust_sha256,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arguments: &[&str]) -> Result<Arguments, String> {
        parse_arguments(arguments.iter().map(OsString::from))
    }

    #[test]
    fn accepts_each_explicit_generation_route() {
        for producer in ["interpreted", "program-image"] {
            let arguments = parse(&[
                "--producer",
                producer,
                "--artifact-manifest",
                "artifacts.json",
                "--out-rust",
                "out.rs",
                "--out-manifest",
                "out.json",
            ])
            .expect("complete route arguments");
            assert_eq!(arguments.artifact_manifest, PathBuf::from("artifacts.json"));
            assert_eq!(arguments.manifest_identity, ManifestIdentity::Require);
        }
        assert_eq!(
            parse(&[
                "--producer",
                "interpreted",
                "--artifact-manifest",
                "artifacts.json",
                "--out-rust",
                "out.rs",
                "--out-manifest",
                "out.json",
                "--manifest-identity",
                "refresh",
            ])
            .expect("explicit artifact refresh")
            .manifest_identity,
            ManifestIdentity::Refresh
        );
    }

    #[test]
    fn rejects_implicit_or_legacy_route_inputs() {
        let required = [
            "--artifact-manifest",
            "artifacts.json",
            "--out-rust",
            "out.rs",
            "--out-manifest",
            "out.json",
        ];
        assert!(parse(&required).is_err());
        assert!(
            parse(&[
                "--producer",
                "linked-c1",
                required[0],
                required[1],
                required[2],
                required[3],
                required[4],
                required[5],
            ])
            .is_err()
        );
    }
}
