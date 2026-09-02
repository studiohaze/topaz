fn main() {
    let payload = topaz_stage1_runtime::embedded_program_payload_sha256(1)
        .expect("validate embedded compiler program");
    println!(
        concat!(
            "{{",
            "\"schema\":\"topaz.compiler.program-image-identities/v2\",",
            "\"programImageSha256\":\"sha256:{}\",",
            "\"programImagePayloadSha256\":\"sha256:{}\",",
            "\"compilationRequestSha256\":\"{}\",",
            "\"stageIdentity\":\"none\",",
            "\"stage1DerivationRoute\":\"rust-stage0-direct->program-image->topaz-stage1\",",
            "\"stage2DerivationRoute\":\"not-performed\"",
            "}}"
        ),
        topaz_stage1_runtime::embedded_compiler_program_sha256(),
        payload,
        topaz_stage1_runtime::embedded_compilation_request_sha256(),
    );
}
