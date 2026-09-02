use crate::*;

#[non_exhaustive]
/// Private fields prevent product digests from drifting after construction.
pub struct SelfCompilationProduct {
    pub(crate) profile: CompilationProfile,
    pub(crate) target_source_set_id: String,
    pub(crate) invocation_id: String,
    pub(crate) result_id: String,
    pub(crate) response_sha256: String,
    pub(crate) front_end_sha256: String,
    pub(crate) generated_rust_sha256: String,
    pub(crate) compiler: InstalledStage2Identity,
    pub(crate) typed: TypedPreviewResult,
    pub(crate) lowered: Stage1LoweringPreviewResult,
}

impl SelfCompilationProduct {
    pub fn profile(&self) -> CompilationProfile {
        self.profile
    }

    pub fn target_source_set_id(&self) -> &str {
        &self.target_source_set_id
    }

    pub fn invocation_id(&self) -> &str {
        &self.invocation_id
    }

    pub fn result_id(&self) -> &str {
        &self.result_id
    }

    pub fn response_sha256(&self) -> &str {
        &self.response_sha256
    }

    pub fn front_end_sha256(&self) -> &str {
        &self.front_end_sha256
    }

    pub fn generated_rust_sha256(&self) -> &str {
        &self.generated_rust_sha256
    }

    pub fn compiler(&self) -> &InstalledStage2Identity {
        &self.compiler
    }

    pub fn typed(&self) -> &TypedPreviewResult {
        &self.typed
    }

    pub fn lowered(&self) -> &Stage1LoweringPreviewResult {
        &self.lowered
    }

    pub fn status(&self) -> &str {
        &self.lowered.status
    }

    pub fn rounds(&self) -> u64 {
        self.lowered.rounds
    }

    pub fn generated_rust(&self) -> &str {
        &self.lowered.generated_rust
    }
}

pub(crate) fn append_target_source_identity_value(material: &mut Vec<u8>, value: &str) {
    material.extend_from_slice(value.len().to_string().as_bytes());
    material.push(b':');
    material.extend_from_slice(value.as_bytes());
    material.push(0);
}

pub(crate) fn target_source_set_id(
    modules: &[topaz_kernel::CanonicalPreviewModule],
) -> Result<String, String> {
    if modules.is_empty() {
        return Err("self compilation product omitted every target module".to_string());
    }
    let mut identities = std::collections::BTreeSet::new();
    let mut entry_count = 0usize;
    let mut material = Vec::new();
    for (ordinal, module) in modules.iter().enumerate() {
        if !identities.insert(module.identity.as_str()) {
            return Err(format!(
                "self compilation product repeats module identity `{}`",
                module.identity
            ));
        }
        entry_count += usize::from(module.entry);
        append_target_source_identity_value(&mut material, &ordinal.to_string());
        append_target_source_identity_value(&mut material, &module.identity);
        append_target_source_identity_value(&mut material, &module.path);
        append_target_source_identity_value(&mut material, &module.entry.to_string());
        append_target_source_identity_value(&mut material, &module.extern_module.to_string());
        append_target_source_identity_value(&mut material, &module.generated_std.to_string());
        append_target_source_identity_value(
            &mut material,
            &stage1_sha256(module.source.as_bytes()),
        );
    }
    if entry_count != 1 {
        return Err(format!(
            "self compilation product requires one entry module, observed {entry_count}"
        ));
    }
    Ok(stage1_sha256(&material))
}

pub(crate) fn validate_self_compilation_outcome(
    generated: &Stage1GeneratedPreviewResult,
    typed: &TypedPreviewResult,
    lowered: &Stage1LoweringPreviewResult,
) -> Result<(), String> {
    let diagnostic_count = typed.resolved.diagnostics.len() + typed.diagnostics.len();
    if lowered.status != generated.status
        || !lowered.unsupported.is_empty()
        || lowered.generated_rust != generated.generated_rust
    {
        return Err(
            "self compilation product lowering contradicts its generated result".to_string(),
        );
    }
    match generated.status.as_str() {
        "completed" if diagnostic_count == 0 && !generated.generated_rust.is_empty() => Ok(()),
        "completed" => Err(
            "completed self compilation product carries diagnostics or omits generated Rust"
                .to_string(),
        ),
        "rejected"
            if diagnostic_count > 0
                && generated.generated_rust.is_empty()
                && lowered.modules.is_empty()
                && lowered.operations.is_empty() =>
        {
            Ok(())
        }
        "rejected" => Err(
            "rejected self compilation product has no diagnostic or carries generated output"
                .to_string(),
        ),
        status => Err(format!(
            "self compilation product carries unsupported status `{status}`"
        )),
    }
}

pub(crate) struct SelfCompilationIdentity {
    target_source_set_id: String,
    invocation_id: String,
    result_id: String,
    response_sha256: String,
    front_end_sha256: String,
    generated_rust_sha256: String,
}

pub(crate) fn self_compilation_language_mode(request: &topaz_kernel::KernelRequest) -> String {
    format!("topaz-{}", request.language_version().as_str())
}

pub(crate) fn self_compilation_identity(
    generated: &Stage1GeneratedPreviewResult,
    compiler: &InstalledStage2Identity,
    profile: CompilationProfile,
    modules: &[topaz_kernel::CanonicalPreviewModule],
) -> Result<SelfCompilationIdentity, String> {
    let target_source_set_id = target_source_set_id(modules)?;
    let language_mode = self_compilation_language_mode(&generated.request);
    let invocation_material = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n",
        CompilerProducer::Stage2.identity(),
        compiler.program_image_sha256,
        compiler.source_set_id,
        language_mode,
        profile.identity(),
        target_source_set_id,
    );
    let invocation_id = stage1_sha256(invocation_material.as_bytes());
    let response_sha256 = stage1_sha256(&generated.response);
    let result_id = stage1_sha256(format!("{invocation_id}\n{response_sha256}\n").as_bytes());
    Ok(SelfCompilationIdentity {
        target_source_set_id,
        invocation_id,
        result_id,
        response_sha256,
        front_end_sha256: stage1_sha256(generated.front_end.as_bytes()),
        generated_rust_sha256: stage1_sha256(generated.generated_rust.as_bytes()),
    })
}

/// Compiles with the shipped C2 image; the Rust Stage 0 path is never consulted.
pub fn preview_linked_stage2_compilation_product(
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
    profile: CompilationProfile,
) -> Result<SelfCompilationProduct, String> {
    let generated = preview_stage1_generated_by(
        topaz_stage1_runtime::execute_embedded_stage2_compiler,
        source,
        request,
        CompilerProducer::Stage2,
        profile,
        true,
        ResolvedDiagnosticShape::SealedImage,
    )?;
    let typed = decode_stage1_typed_from_generated(&generated)?;
    let lowered = decode_stage1_lowering_from_generated(&generated)?;
    validate_self_compilation_outcome(&generated, &typed, &lowered)?;
    let compiler = installed_stage2_identity()?;
    let identity =
        self_compilation_identity(&generated, &compiler, profile, &typed.resolved.modules)?;
    Ok(SelfCompilationProduct {
        profile,
        target_source_set_id: identity.target_source_set_id,
        invocation_id: identity.invocation_id,
        result_id: identity.result_id,
        response_sha256: identity.response_sha256,
        front_end_sha256: identity.front_end_sha256,
        generated_rust_sha256: identity.generated_rust_sha256,
        compiler,
        typed,
        lowered,
    })
}
