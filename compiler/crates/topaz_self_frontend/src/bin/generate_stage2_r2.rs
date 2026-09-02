use std::ffi::OsString;
use std::path::PathBuf;

#[path = "support/generator.rs"]
mod generator;

struct Arguments {
    output_rust: PathBuf,
    output_manifest: PathBuf,
    c2_manifest: PathBuf,
}

fn parse_arguments(arguments: impl IntoIterator<Item = OsString>) -> Result<Arguments, String> {
    let mut options = generator::OptionValues::parse(
        arguments,
        &["--out-rust", "--out-manifest", "--c2-manifest"],
    )?;
    Ok(Arguments {
        output_rust: options
            .path("--out-rust", true)?
            .ok_or_else(|| "missing --out-rust".to_string())?,
        output_manifest: options
            .path("--out-manifest", true)?
            .ok_or_else(|| "missing --out-manifest".to_string())?,
        c2_manifest: options
            .path("--c2-manifest", true)?
            .ok_or_else(|| "missing --c2-manifest".to_string())?,
    })
}

fn main() -> Result<(), String> {
    let arguments = parse_arguments(std::env::args_os().skip(1))?;
    let c2_manifest = std::fs::read(&arguments.c2_manifest)
        .map_err(|error| format!("cannot read {}: {error}", arguments.c2_manifest.display()))?;
    let mut request = topaz_kernel::KernelRequest::checked(
        "src/main.tpz",
        Some(""),
        topaz_syntax::LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::RustSource);
    topaz_self_frontend::supply_embedded_compiler_source_facts(&mut request)
        .map_err(|error| format!("cannot seed embedded compiler source facts: {error:?}"))?;
    let mut generated = topaz_self_frontend::preview_linked_stage2_generated(
        &topaz_self_frontend::EmbeddedCompilerSourceHost,
        request,
    )?;
    if generated.producer != topaz_self_frontend::CompilerProducer::Stage2 {
        return Err("C2 generation returned the wrong producer identity".to_string());
    }
    topaz_self_frontend::seal_compiler_program_target_facts(&mut generated)?;
    let generated_sha256 = generator::sha256_hex(generated.generated_rust.as_bytes());
    let c2_manifest_sha256 = generator::sha256_hex(&c2_manifest);
    let c2_program_sha256 = topaz_stage1_runtime::embedded_stage2_program_sha256();
    let invocation_material = format!(
        "topaz-stage2\n2\n{}\n{c2_manifest_sha256}\n{c2_program_sha256}\n{generated_sha256}\n",
        generated.provenance_source_set_id,
    );
    let invocation_id = generator::sha256_hex(invocation_material.as_bytes());
    let manifest = format!(
        concat!(
            "{{\n",
            "  \"schema\": \"topaz.compiler.stage2-r2-production/v1\",\n",
            "  \"producerLabel\": \"Stage2\",\n",
            "  \"producerDerivation\": \"aliased-from-c1-payload\",\n",
            "  \"producer\": \"topaz-stage2\",\n",
            "  \"producerStage\": 2,\n",
            "  \"resultStage\": 2,\n",
            "  \"fixedPoint\": \"not-run\",\n",
            "  \"sourceSetId\": \"{}\",\n",
            "  \"runtimeTemplate\": \"compiler-ir-table/v2\",\n",
            "  \"generatedRustSha256\": \"sha256:{}\",\n",
            "  \"generatedRustBytes\": {},\n",
            "  \"inputC2ManifestSha256\": \"sha256:{}\",\n",
            "  \"inputC2ProgramImageSha256\": \"sha256:{}\",\n",
            "  \"producerInvocationId\": \"sha256:{}\",\n",
            "  \"targetCompilerFallback\": false\n",
            "}}\n"
        ),
        generated.provenance_source_set_id,
        generated_sha256,
        generated.generated_rust.len(),
        c2_manifest_sha256,
        c2_program_sha256,
        invocation_id,
    );
    generator::write_atomic(&arguments.output_rust, generated.generated_rust.as_bytes())?;
    generator::write_atomic(&arguments.output_manifest, manifest.as_bytes())?;
    println!(
        "stage2-r2: {} bytes sha256:{} source-set {} invocation sha256:{}",
        generated.generated_rust.len(),
        generated_sha256,
        generated.provenance_source_set_id,
        invocation_id,
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
    fn accepts_the_complete_r2_generation_input() {
        let arguments = parse(&[
            "--out-rust",
            "r2.rs",
            "--out-manifest",
            "r2.json",
            "--c2-manifest",
            "c2.json",
        ])
        .expect("valid R2 generator arguments");
        assert_eq!(arguments.output_rust, PathBuf::from("r2.rs"));
        assert_eq!(arguments.output_manifest, PathBuf::from("r2.json"));
        assert_eq!(arguments.c2_manifest, PathBuf::from("c2.json"));
    }

    #[test]
    fn rejects_ambiguous_r2_generation_inputs() {
        assert!(
            parse(&[
                "--out-rust",
                "--out-manifest",
                "r2.json",
                "--c2-manifest",
                "c2.json",
            ])
            .is_err()
        );
        assert!(
            parse(&[
                "--out-rust",
                "r2.rs",
                "--out-rust",
                "other.rs",
                "--out-manifest",
                "r2.json",
                "--c2-manifest",
                "c2.json",
            ])
            .is_err()
        );
        assert!(
            parse(&[
                "--out-rust",
                "r2.rs",
                "--out-manifest",
                "r2.json",
                "--c2-manifest",
                "c2.json",
                "--unknown",
                "value",
            ])
            .is_err()
        );
    }
}
