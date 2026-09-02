use crate::*;

/// Build the ordinary observation surface from one already validated C2
/// compilation product. This is a mechanical projection only: it never
/// reinvokes a target compiler phase or regenerates a missing field.
pub fn build_self_compilation_observation(
    product: &SelfCompilationProduct,
    terminal: topaz_kernel::TerminalPhase,
) -> Result<topaz_kernel::ObservationBundle, String> {
    if !matches!(
        terminal,
        topaz_kernel::TerminalPhase::Typed | topaz_kernel::TerminalPhase::RustSource
    ) {
        return Err("self compiler observation supports typed or rust-source terminal".to_string());
    }
    let typed = topaz_kernel::build_typed_preview_observation(product.typed.observation_input())?;
    if terminal == topaz_kernel::TerminalPhase::Typed {
        return Ok(typed);
    }
    if product.status() != "completed" {
        return Err(
            "rejected self compilation cannot claim a completed rust-source observation"
                .to_string(),
        );
    }
    let rust_request = product
        .lowered
        .request
        .clone()
        .with_terminal_phase(topaz_kernel::TerminalPhase::RustSource);
    let lowered_jsonl = encode_stage1_lowered_projection(&product.lowered)?;
    let stage2_product = encode_stage2_product_manifest_fields(
        rust_request.language_version(),
        product.generated_rust(),
        &product.target_source_set_id,
        false,
    )?;
    topaz_kernel::complete_compiler_preview_observation(
        typed,
        &rust_request,
        topaz_kernel::CompilerPreviewCompletion {
            lowered_jsonl,
            generated_rust: product.generated_rust(),
            product: stage2_product,
            runtime_template_identity: FIXED_POINT_RUNTIME_TEMPLATE,
            runtime_template_sha256: FIXED_POINT_RUNTIME_TEMPLATE_SHA256,
            producer_stage: 2,
            fixed_point: None,
        },
    )
}
