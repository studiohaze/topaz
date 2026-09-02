use crate::*;

/// Retains decoded roots so typed and lowered consumers never reparse response bytes.
pub struct Stage1GeneratedPreviewResult {
    pub producer: CompilerProducer,
    pub request: topaz_kernel::KernelRequest,
    pub status: String,
    pub profile: CompilationProfile,
    pub front_end: String,
    pub generated_rust: String,
    pub provenance_source_set_id: String,
    pub rounds: u64,
    pub(crate) response: Vec<u8>,
    pub(crate) response_root: Rc<JsonObject>,
    pub(crate) front_end_root: Rc<JsonObject>,
    pub(crate) resolved_diagnostic_shape: ResolvedDiagnosticShape,
}

pub(crate) struct Stage1GeneratedDecode {
    response: Vec<u8>,
    response_root: Rc<JsonObject>,
    request: topaz_kernel::KernelRequest,
    status: String,
    front_end: String,
    front_end_root: Rc<JsonObject>,
    producer: CompilerProducer,
    profile: CompilationProfile,
    accept_rejected: bool,
    resolved_diagnostic_shape: ResolvedDiagnosticShape,
    rounds: u64,
}

pub(crate) fn decode_stage1_generated_response(
    decoded: Stage1GeneratedDecode,
) -> Result<Stage1GeneratedPreviewResult, String> {
    let Stage1GeneratedDecode {
        response,
        response_root,
        request,
        status,
        front_end,
        front_end_root,
        producer,
        profile,
        accept_rejected,
        resolved_diagnostic_shape,
        rounds,
    } = decoded;
    let root = response_root.as_ref();
    if status != "completed" && !(accept_rejected && status == "rejected") {
        return Err(format!(
            "Stage 1 generation did not complete: status `{status}`"
        ));
    }
    if !json_array_field(root, "unsupported")?.is_empty() {
        return Err("Stage 1 generation completed with unsupported rows".to_string());
    }
    let generated_rust = json_string_field(root, "generatedRust")?.to_string();
    if status == "completed" {
        validate_stage1_generated_rust(
            &generated_rust,
            request.budgets().max_generated_rust_bytes,
        )?;
    } else if !generated_rust.is_empty() {
        return Err("rejected Stage 1 generation returned generated Rust".to_string());
    }
    let provenance_source_set_id = parse_stage1_provenance_source_set_id(root, producer)?;
    Ok(Stage1GeneratedPreviewResult {
        producer,
        request,
        status,
        profile,
        front_end,
        generated_rust,
        provenance_source_set_id,
        rounds,
        response,
        response_root,
        front_end_root,
        resolved_diagnostic_shape,
    })
}

pub(crate) fn preview_stage1_generated_by(
    invoke: impl Fn(&[u8]) -> Result<Vec<u8>, String>,
    source: &dyn topaz_kernel::HostFactSource,
    mut request: topaz_kernel::KernelRequest,
    producer: CompilerProducer,
    profile: CompilationProfile,
    accept_rejected: bool,
    resolved_diagnostic_shape: ResolvedDiagnosticShape,
) -> Result<Stage1GeneratedPreviewResult, String> {
    if request.terminal_phase() != topaz_kernel::TerminalPhase::RustSource {
        return Err("Stage 1 generation preview requires the rust-source terminal".to_string());
    }
    let max_rounds = request
        .budgets()
        .max_source_facts
        .saturating_mul(3)
        .saturating_add(4);
    let mut rounds = 0u64;
    loop {
        if rounds >= max_rounds {
            return Err(format!(
                "Stage 1 generation fact rounds exceed {max_rounds}"
            ));
        }
        rounds += 1;
        let response = invoke(&encode_compiler_request_with_profile(
            &request, producer, profile,
        )?)?;
        let response_root = decode_stage1_response_root(&response, "Stage 1 generation response")?;
        let root = response_root.as_ref();
        let Stage1ResponseEnvelope {
            status,
            front_end: front_end_text,
            front_end_root,
            queries,
        } = parse_stage1_response_envelope(root, "Stage 1 generation")?;
        if advance_compiler_fact_round(
            source,
            &mut request,
            &status,
            queries,
            "Stage 1 generation",
        )? {
            continue;
        }
        return decode_stage1_generated_response(Stage1GeneratedDecode {
            response,
            response_root,
            request,
            status,
            front_end: front_end_text,
            front_end_root,
            producer,
            profile,
            accept_rejected,
            resolved_diagnostic_shape,
            rounds,
        });
    }
}

/// Produces Stage 1 Rust source through a reusable self-front-end session.
pub fn preview_stage1_generated_with(
    session: &FrontEndSession,
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
) -> Result<Stage1GeneratedPreviewResult, String> {
    preview_stage1_generated_by(
        |encoded| session.invoke_stage1(encoded),
        source,
        request,
        CompilerProducer::Stage1,
        CompilationProfile::None,
        false,
        ResolvedDiagnosticShape::Current,
    )
}

/// Produces a Stage 2-labelled result through a reusable self-front-end session.
pub fn preview_stage2_generated_with(
    session: &FrontEndSession,
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
) -> Result<Stage1GeneratedPreviewResult, String> {
    preview_stage1_generated_by(
        |encoded| session.invoke_stage1(encoded),
        source,
        request,
        CompilerProducer::Stage2,
        CompilationProfile::None,
        false,
        ResolvedDiagnosticShape::Current,
    )
}

/// Produces Stage 1 Rust source with a fresh self-front-end session.
pub fn preview_stage1_generated(
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
) -> Result<Stage1GeneratedPreviewResult, String> {
    preview_stage1_generated_with(&FrontEndSession::new()?, source, request)
}

/// Runs the embedded C1 image and retains its generated-source response.
pub fn preview_linked_stage1_generated(
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
) -> Result<Stage1GeneratedPreviewResult, String> {
    preview_stage1_generated_by(
        topaz_stage1_runtime::execute_embedded_compiler,
        source,
        request,
        CompilerProducer::Stage1,
        CompilationProfile::None,
        false,
        ResolvedDiagnosticShape::SealedImage,
    )
}

/// Runs the embedded C2 image and retains its generated-source response.
pub fn preview_linked_stage2_generated(
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
) -> Result<Stage1GeneratedPreviewResult, String> {
    preview_stage1_generated_by(
        topaz_stage1_runtime::execute_embedded_stage2_compiler,
        source,
        request,
        CompilerProducer::Stage2,
        CompilationProfile::None,
        false,
        ResolvedDiagnosticShape::SealedImage,
    )
}

/// Runs the embedded C2 image under a named compilation profile.
pub fn preview_linked_stage2_profiled_generated(
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
    profile: CompilationProfile,
) -> Result<Stage1GeneratedPreviewResult, String> {
    if profile == CompilationProfile::None {
        return Err("profiled self compilation requires a named profile".to_string());
    }
    preview_stage1_generated_by(
        topaz_stage1_runtime::execute_embedded_stage2_compiler,
        source,
        request,
        CompilerProducer::Stage2,
        profile,
        false,
        ResolvedDiagnosticShape::SealedImage,
    )
}
