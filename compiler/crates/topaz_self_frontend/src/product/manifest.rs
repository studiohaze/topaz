use crate::*;

pub(crate) fn stage1_json_string(value: &str) -> JsonValue {
    JsonValue::String(Rc::from(value))
}

pub(crate) fn stage1_json_number(value: u64) -> JsonValue {
    JsonValue::Number(JsonNumber {
        lexeme: Rc::from(value.to_string()),
        int: i64::try_from(value).ok(),
    })
}

pub(crate) fn stage1_json_array(values: Vec<JsonValue>) -> JsonValue {
    JsonValue::Array(Rc::from(values.into_boxed_slice()))
}

pub(crate) fn stage1_json_object<const N: usize>(values: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(Rc::new(
        values
            .into_iter()
            .map(|(key, value)| (Rc::from(key), value))
            .collect(),
    ))
}

pub(crate) fn stage1_source_id(module: &str, path: &str) -> String {
    let mut input = String::from("root");
    input.push('\0');
    input.push_str(module);
    input.push('\0');
    input.push_str(path);
    let digest = topaz_value::value::sha256(input.as_bytes());
    let mut output = String::from("s:");
    topaz_value::bytes_to_hex_into(&mut output, &digest);
    output
}

pub(crate) fn stage1_sha256(bytes: &[u8]) -> String {
    let digest = topaz_value::value::sha256(bytes);
    let mut output = String::from("sha256:");
    topaz_value::bytes_to_hex_into(&mut output, &digest);
    output
}

pub(crate) fn stage1_encode_json(value: &JsonValue) -> String {
    let mut output = String::new();
    topaz_value::value::write_json_node(&mut output, value);
    output
}

pub(crate) fn require_completed_self_compilation_product(
    product: &SelfCompilationProduct,
    error: &str,
) -> Result<(), String> {
    if product.status() != "completed" {
        return Err(error.to_string());
    }
    Ok(())
}

/// Encodes the canonical manifest for one completed or rejected self compilation.
pub fn encode_self_compilation_product_manifest(
    product: &SelfCompilationProduct,
) -> Result<Vec<u8>, String> {
    let diagnostic_count =
        product.typed.resolved.diagnostics.len() + product.typed.diagnostics.len();
    let profile = match product.profile {
        CompilationProfile::None => "none",
        value => value.identity(),
    };
    let language_mode = self_compilation_language_mode(&product.lowered.request);
    let manifest = stage1_json_object([
        (
            "categories",
            stage1_json_array(
                SELF_PRODUCT_CATEGORIES
                    .iter()
                    .map(|value| stage1_json_string(value))
                    .collect(),
            ),
        ),
        (
            "compiler",
            stage1_json_object([
                (
                    "exchangeSchema",
                    stage1_json_string(product.compiler.exchange_schema),
                ),
                ("irSchema", stage1_json_string(product.compiler.ir_schema)),
                ("producer", stage1_json_string(product.compiler.producer)),
                (
                    "producerStage",
                    stage1_json_number(product.compiler.producer_stage.into()),
                ),
                (
                    "programImageSha256",
                    stage1_json_string(&product.compiler.program_image_sha256),
                ),
                (
                    "runtimeTemplate",
                    stage1_json_string(product.compiler.runtime_template),
                ),
                (
                    "sourceSetId",
                    stage1_json_string(&product.compiler.source_set_id),
                ),
            ]),
        ),
        (
            "counts",
            stage1_json_object([
                ("diagnostics", stage1_json_number(diagnostic_count as u64)),
                (
                    "exports",
                    stage1_json_number(product.typed.resolved.exports.len() as u64),
                ),
                (
                    "loweredModules",
                    stage1_json_number(product.lowered.modules.len() as u64),
                ),
                (
                    "loweredOperations",
                    stage1_json_number(product.lowered.operations.len() as u64),
                ),
                (
                    "modules",
                    stage1_json_number(product.typed.resolved.modules.len() as u64),
                ),
                ("rounds", stage1_json_number(product.rounds())),
                (
                    "typedCalls",
                    stage1_json_number(product.typed.calls.len() as u64),
                ),
                (
                    "typedCaptures",
                    stage1_json_number(product.typed.captures.len() as u64),
                ),
                (
                    "typedNodes",
                    stage1_json_number(product.typed.nodes.len() as u64),
                ),
            ]),
        ),
        (
            "frontEndSha256",
            stage1_json_string(&product.front_end_sha256),
        ),
        (
            "generatedRust",
            stage1_json_object([
                (
                    "byteLength",
                    stage1_json_number(product.generated_rust().len() as u64),
                ),
                ("sha256", stage1_json_string(&product.generated_rust_sha256)),
            ]),
        ),
        ("invocationId", stage1_json_string(&product.invocation_id)),
        ("languageMode", stage1_json_string(&language_mode)),
        (
            "phaseTrace",
            stage1_json_array(
                SELF_PRODUCT_PHASE_TRACE
                    .iter()
                    .map(|value| stage1_json_string(value))
                    .collect(),
            ),
        ),
        ("profile", stage1_json_string(profile)),
        ("resultId", stage1_json_string(&product.result_id)),
        (
            "responseSha256",
            stage1_json_string(&product.response_sha256),
        ),
        (
            "schema",
            stage1_json_string(SELF_COMPILATION_PRODUCT_SCHEMA),
        ),
        ("selectedEngine", stage1_json_string("self")),
        ("status", stage1_json_string(product.status())),
        ("targetCompilerFallback", JsonValue::Bool(false)),
        (
            "targetSourceSetId",
            stage1_json_string(&product.target_source_set_id),
        ),
    ]);
    let mut encoded = stage1_encode_json(&manifest);
    encoded.push('\n');
    Ok(encoded.into_bytes())
}

pub(crate) fn validate_product_sha256(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field} is not a sha256 identity"));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{field} is not a canonical sha256 identity"));
    }
    Ok(())
}

pub(crate) fn expect_product_string_array(
    root: &JsonObject,
    field: &str,
    expected: &[&str],
) -> Result<(), String> {
    let actual = json_array_field(root, field)?;
    if actual.len() != expected.len() {
        return Err(format!(
            "self compilation product `{field}` inventory is incomplete"
        ));
    }
    for (ordinal, (value, expected)) in actual.iter().zip(expected).enumerate() {
        match value {
            JsonValue::String(value) if value.as_ref() == *expected => {}
            _ => {
                return Err(format!(
                    "self compilation product `{field}` item {ordinal} is out of contract order"
                ));
            }
        }
    }
    Ok(())
}

/// Admits the exact schema and cross-field identities of a self-product manifest.
pub fn validate_self_compilation_product_manifest(bytes: &[u8]) -> Result<(), String> {
    let parsed = json_parse(
        std::str::from_utf8(bytes)
            .map_err(|error| format!("self compilation product is not UTF-8: {error}"))?,
    )
    .map_err(|error| format!("self compilation product is not JSON: {error:?}"))?;
    let root = exact_object(
        &parsed,
        "self compilation product",
        &[
            "categories",
            "compiler",
            "counts",
            "frontEndSha256",
            "generatedRust",
            "invocationId",
            "languageMode",
            "phaseTrace",
            "profile",
            "responseSha256",
            "resultId",
            "schema",
            "selectedEngine",
            "status",
            "targetCompilerFallback",
            "targetSourceSetId",
        ],
    )?;
    expect_json_string(root, "schema", SELF_COMPILATION_PRODUCT_SCHEMA)?;
    expect_json_string(root, "selectedEngine", "self")?;
    let status = json_string_field(root, "status")?;
    if !matches!(status, "completed" | "rejected") {
        return Err("self compilation product carries an invalid status".to_string());
    }
    if json_bool_field(root, "targetCompilerFallback")? {
        return Err("self compilation product enables target compiler fallback".to_string());
    }
    let language_mode = json_string_field(root, "languageMode")?;
    let language_version = language_mode
        .strip_prefix("topaz-")
        .and_then(topaz_syntax::LangVersion::parse_exact);
    if !language_version.is_some_and(|version| version.uses_self_hosted_product_default()) {
        return Err("self compilation product carries the wrong language mode".to_string());
    }
    let profile = json_string_field(root, "profile")?;
    if !matches!(
        profile,
        "none" | "agent-pack" | "test-profile" | "bootstrap"
    ) {
        return Err("self compilation product carries an unknown profile".to_string());
    }
    for field in [
        "frontEndSha256",
        "invocationId",
        "responseSha256",
        "resultId",
        "targetSourceSetId",
    ] {
        validate_product_sha256(json_string_field(root, field)?, field)?;
    }
    let compiler = exact_object(
        root.get("compiler")
            .ok_or_else(|| "self compilation product omitted compiler".to_string())?,
        "self compilation product compiler",
        &[
            "exchangeSchema",
            "irSchema",
            "producer",
            "producerStage",
            "programImageSha256",
            "runtimeTemplate",
            "sourceSetId",
        ],
    )?;
    expect_json_string(compiler, "producer", "topaz-stage2")?;
    if json_i64(compiler, "producerStage")? != 2 {
        return Err("self compilation product carries the wrong producer stage".to_string());
    }
    expect_json_string(compiler, "exchangeSchema", STAGE1_EXCHANGE_SCHEMA)?;
    expect_json_string(compiler, "irSchema", STAGE1_IR_SCHEMA)?;
    expect_json_string(compiler, "runtimeTemplate", FIXED_POINT_RUNTIME_TEMPLATE)?;
    let installed = installed_stage2_identity()?;
    for (field, expected) in [
        (
            "programImageSha256",
            installed.program_image_sha256.as_str(),
        ),
        ("sourceSetId", installed.source_set_id.as_str()),
    ] {
        validate_product_sha256(json_string_field(compiler, field)?, field)?;
        expect_json_string(compiler, field, expected)?;
    }
    let target_source_set_id = json_string_field(root, "targetSourceSetId")?;
    let invocation_profile = if profile == "none" { "" } else { profile };
    let expected_invocation = stage1_sha256(
        format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n",
            CompilerProducer::Stage2.identity(),
            installed.program_image_sha256,
            installed.source_set_id,
            language_mode,
            invocation_profile,
            target_source_set_id,
        )
        .as_bytes(),
    );
    expect_json_string(root, "invocationId", &expected_invocation)?;
    let expected_result = stage1_sha256(
        format!(
            "{}\n{}\n",
            expected_invocation,
            json_string_field(root, "responseSha256")?
        )
        .as_bytes(),
    );
    expect_json_string(root, "resultId", &expected_result)?;
    let generated = exact_object(
        root.get("generatedRust")
            .ok_or_else(|| "self compilation product omitted generated Rust".to_string())?,
        "self compilation product generated Rust",
        &["byteLength", "sha256"],
    )?;
    let generated_byte_length = json_i64(generated, "byteLength")?;
    if status == "completed" && generated_byte_length <= 0 {
        return Err("self compilation product generated Rust is empty".to_string());
    }
    if status == "rejected" && generated_byte_length != 0 {
        return Err("rejected self compilation product carries generated Rust".to_string());
    }
    validate_product_sha256(
        json_string_field(generated, "sha256")?,
        "generated Rust sha256",
    )?;
    if status == "rejected" {
        expect_json_string(generated, "sha256", &stage1_sha256(&[]))?;
    }
    expect_product_string_array(root, "categories", &SELF_PRODUCT_CATEGORIES)?;
    expect_product_string_array(root, "phaseTrace", &SELF_PRODUCT_PHASE_TRACE)?;
    let counts = exact_object(
        root.get("counts")
            .ok_or_else(|| "self compilation product omitted counts".to_string())?,
        "self compilation product counts",
        &[
            "diagnostics",
            "exports",
            "loweredModules",
            "loweredOperations",
            "modules",
            "rounds",
            "typedCalls",
            "typedCaptures",
            "typedNodes",
        ],
    )?;
    for field in [
        "diagnostics",
        "exports",
        "loweredModules",
        "loweredOperations",
        "modules",
        "rounds",
        "typedCalls",
        "typedCaptures",
        "typedNodes",
    ] {
        if json_i64(counts, field)? < 0 {
            return Err(format!(
                "self compilation product count `{field}` is negative"
            ));
        }
    }
    if status == "rejected" && json_i64(counts, "diagnostics")? <= 0 {
        return Err("rejected self compilation product omitted diagnostics".to_string());
    }
    Ok(())
}

pub(crate) fn compiler_files_product_json(compiler: &EmbeddedSourceManifest) -> JsonValue {
    stage1_json_array(
        compiler
            .files
            .iter()
            .map(|file| {
                stage1_json_object([
                    ("byteLength", stage1_json_number(file.byte_length as u64)),
                    ("path", stage1_json_string(file.path)),
                    ("sha256", stage1_json_string(&file.content_sha256)),
                ])
            })
            .collect(),
    )
}

pub(crate) fn runtime_template_product_json() -> JsonValue {
    stage1_json_object([
        ("identity", stage1_json_string(FIXED_POINT_RUNTIME_TEMPLATE)),
        (
            "sha256",
            stage1_json_string(FIXED_POINT_RUNTIME_TEMPLATE_SHA256),
        ),
    ])
}

pub(crate) fn target_product_json(generated_rust: &str, target_source_set_id: &str) -> JsonValue {
    stage1_json_object([
        (
            "generatedRustBytes",
            stage1_json_number(generated_rust.len() as u64),
        ),
        (
            "generatedRustSha256",
            stage1_json_string(&stage1_sha256(generated_rust.as_bytes())),
        ),
        ("sourceSetId", stage1_json_string(target_source_set_id)),
    ])
}

pub(crate) fn compiler_image_product_json(
    manifest: &topaz_stage1_runtime::CompilerImageDescriptor,
) -> JsonValue {
    stage1_json_object([
        ("executable", stage1_json_string(manifest.executable)),
        (
            "executableStage",
            stage1_json_number(manifest.executable_stage),
        ),
        (
            "generatedRustBytes",
            stage1_json_number(manifest.generated_rust_bytes),
        ),
        (
            "generatedRustSha256",
            stage1_json_string(manifest.generated_rust_sha256),
        ),
        (
            "manifestSha256",
            stage1_json_string(manifest.manifest_sha256),
        ),
        (
            "programImageSha256",
            stage1_json_string(manifest.program_image_sha256),
        ),
        (
            "sourceProducer",
            stage1_json_string(manifest.source_producer),
        ),
        (
            "sourceProducerStage",
            stage1_json_number(manifest.source_producer_stage),
        ),
    ])
}

/// Encodes a C1 product manifest bound to its target source-set identity.
pub fn encode_stage1_product_manifest(
    result: &Stage1GeneratedPreviewResult,
    target_source_set_id: &str,
) -> Result<Vec<u8>, String> {
    if result.producer != CompilerProducer::Stage1 {
        return Err("Stage 1 product requires the Stage 1 producer".to_string());
    }
    let c1 = topaz_stage1_runtime::compiler_image_descriptor(
        topaz_stage1_runtime::CompilerImageStage::C1,
    );
    let compiler = source_manifest();
    let product = stage1_json_object([
        (
            "comparison",
            stage1_json_object([
                ("generatedSource", stage1_json_string("stable-per-producer")),
                ("provenance", stage1_json_string("complete")),
                ("semantic", stage1_json_string("declared-corpus-equal")),
            ]),
        ),
        ("compilerFiles", compiler_files_product_json(&compiler)),
        (
            "compilerSourceSetId",
            stage1_json_string(&compiler.source_set_id),
        ),
        ("defaultEngine", stage1_json_string("rust-stage0")),
        ("exchangeSchema", stage1_json_string(STAGE1_EXCHANGE_SCHEMA)),
        ("fixedPoint", stage1_json_string("not-run")),
        (
            "generatedC1ManifestSha256",
            stage1_json_string(c1.manifest_sha256),
        ),
        (
            "generatedC1RustBytes",
            stage1_json_number(c1.generated_rust_bytes),
        ),
        (
            "generatedC1RustSha256",
            stage1_json_string(c1.generated_rust_sha256),
        ),
        ("irSchema", stage1_json_string(STAGE1_IR_SCHEMA)),
        (
            "languageMode",
            stage1_json_string(&format!(
                "topaz-{}",
                result.request.language_version().as_str()
            )),
        ),
        ("producerStage", stage1_json_number(1)),
        ("productVersion", stage1_json_string(topaz_check::VERSION)),
        (
            "recovery",
            stage1_json_object([
                (
                    "manifestSha256",
                    stage1_json_string(
                        "sha256:cc76a712cd7d6fccf2e2d226fd978568d9fb32f91d06531a5951cf1f64eb53b3",
                    ),
                ),
                (
                    "sourceArchiveSha256",
                    stage1_json_string(
                        "sha256:b539ea7284c5bfa09863d5af148e1ab33d59daa78a30d46974fafb80d97be4b6",
                    ),
                ),
                ("version", stage1_json_string("5.12.0")),
            ]),
        ),
        ("resultStage", stage1_json_number(1)),
        ("runtimeTemplate", runtime_template_product_json()),
        (
            "schema",
            stage1_json_string(topaz_kernel::STAGE1_PRODUCT_SCHEMA),
        ),
        ("selectedEngine", stage1_json_string("topaz-stage1")),
        (
            "target",
            target_product_json(&result.generated_rust, target_source_set_id),
        ),
        ("targetCompilerFallback", JsonValue::Bool(false)),
    ]);
    let mut encoded = stage1_encode_json(&product);
    encoded.push('\n');
    Ok(encoded.into_bytes())
}

/// Encodes a C2 product manifest and its self-source disposition.
pub fn encode_stage2_product_manifest(
    result: &Stage1GeneratedPreviewResult,
    target_source_set_id: &str,
    self_source: bool,
) -> Result<Vec<u8>, String> {
    if result.producer != CompilerProducer::Stage2 {
        return Err("Stage 2 product requires the Stage 2 producer".to_string());
    }
    encode_stage2_product_manifest_fields(
        result.request.language_version(),
        &result.generated_rust,
        target_source_set_id,
        self_source,
    )
}

pub(crate) fn encode_stage2_product_manifest_fields(
    language_version: topaz_syntax::LangVersion,
    generated_rust: &str,
    target_source_set_id: &str,
    self_source: bool,
) -> Result<Vec<u8>, String> {
    let c1 = topaz_stage1_runtime::compiler_image_descriptor(
        topaz_stage1_runtime::CompilerImageStage::C1,
    );
    let c2 = topaz_stage1_runtime::compiler_image_descriptor(
        topaz_stage1_runtime::CompilerImageStage::C2,
    );
    let compiler = source_manifest();
    let product = stage1_json_object([
        ("c1", compiler_image_product_json(&c1)),
        ("c2", compiler_image_product_json(&c2)),
        ("compilerFiles", compiler_files_product_json(&compiler)),
        (
            "compilerSourceSetId",
            stage1_json_string(&compiler.source_set_id),
        ),
        ("defaultEngine", stage1_json_string("rust-stage0")),
        ("exchangeSchema", stage1_json_string(STAGE1_EXCHANGE_SCHEMA)),
        (
            "fixedPoint",
            stage1_json_string(if self_source {
                "not-established"
            } else {
                "not-run"
            }),
        ),
        (
            "generatedPayloadSchema",
            stage1_json_string(FIXED_POINT_PAYLOAD_SCHEMA),
        ),
        ("irSchema", stage1_json_string(STAGE1_IR_SCHEMA)),
        (
            "languageMode",
            stage1_json_string(&format!("topaz-{}", language_version.as_str())),
        ),
        ("producerStage", stage1_json_number(2)),
        ("productVersion", stage1_json_string(topaz_check::VERSION)),
        ("recoveryEngine", stage1_json_string("rust-stage0")),
        ("resultStage", stage1_json_number(2)),
        ("runtimeTemplate", runtime_template_product_json()),
        (
            "schema",
            stage1_json_string(topaz_kernel::STAGE2_PRODUCT_SCHEMA),
        ),
        ("selectedEngine", stage1_json_string("topaz-stage2")),
        ("selfSource", JsonValue::Bool(self_source)),
        (
            "target",
            target_product_json(generated_rust, target_source_set_id),
        ),
        ("targetCompilerFallback", JsonValue::Bool(false)),
    ]);
    let mut encoded = stage1_encode_json(&product);
    encoded.push('\n');
    Ok(encoded.into_bytes())
}

/// Canonicalizes the runtime-supplied fixed-point assessment for an admitted C2 result.
pub fn encode_stage2_fixed_point_record(
    result: &Stage1GeneratedPreviewResult,
) -> Result<Vec<u8>, String> {
    if result.producer != CompilerProducer::Stage2 {
        return Err("Stage 2 fixed-point record requires the Stage 2 producer".to_string());
    }
    let path = std::env::var_os("TOPAZ_STAGE2_FIXED_POINT_RECORD")
        .ok_or_else(|| "Stage 2 fixed-point record runtime input is not configured".to_string())?;
    let record = std::fs::read(&path).map_err(|error| {
        format!(
            "cannot read Stage 2 fixed-point record `{}`: {error}",
            std::path::Path::new(&path).display()
        )
    })?;
    let parsed = json_parse(
        std::str::from_utf8(&record)
            .map_err(|error| format!("Stage 2 fixed-point record is not UTF-8: {error}"))?,
    )
    .map_err(|error| format!("Stage 2 fixed-point record is not JSON: {error:?}"))?;
    let mut encoded = stage1_encode_json(&parsed);
    encoded.push('\n');
    Ok(encoded.into_bytes())
}
