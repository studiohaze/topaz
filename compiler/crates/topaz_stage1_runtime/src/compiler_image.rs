use std::collections::BTreeMap;
use std::rc::Rc;

use topaz_value::value::{JsonValue, json_parse};

pub(crate) const GENERATED_ARTIFACTS_SCHEMA: &str = "topaz.compiler.generated-artifacts/v1";
pub(crate) const PROGRAM_IMAGE_SCHEMA: &str = "topaz.compiler.program-image/v1";
pub(crate) const TARGET_FACTS_SCHEMA: &str = "topaz.self-target-adapter-facts/v1";
pub(crate) const RUNTIME_REGISTRY_SCHEMA: &str = "topaz.compiler.fixed-point-runtime-templates/v1";
pub(crate) const RUNTIME_TEMPLATE: &str = "compiler-ir-table/v2";
pub(crate) const PAYLOAD_SCHEMA: &str = "topaz.compiler.fixed-point-ir-payload/v1";
pub(crate) const IR_SCHEMA: &str = "topaz.compiler.stage1-ir/v1";
pub(crate) const GENERATION_PROFILE: &str = "topaz-bootstrap-canonical/v1";
pub(crate) const PROGRAM_IMAGE_HEADER: &[u8] = b"TPZIMAGE\x01";

const ROOT_FIELDS: &[&str] = &[
    "schema",
    "compilerSourceSet",
    "compilationRequest",
    "generatedRust",
    "targetFacts",
    "programImage",
    "extraction",
];
const SOURCE_SET_FIELDS: &[&str] = &["sourceSetId", "fileManifestSha256"];
const COMPILATION_REQUEST_FIELDS: &[&str] = &[
    "sha256",
    "canonicalization",
    "generationProfileId",
    "targetFactsSha256",
    "targetTriple",
    "descriptor",
];
const COMPILATION_REQUEST_DESCRIPTOR_FIELDS: &[&str] = &[
    "schema",
    "generationProfileId",
    "entrySource",
    "sourceSetId",
    "fileManifestSha256",
    "sourcePaths",
    "sourceOrder",
    "languageMode",
    "packageFactsProfile",
    "standardLibrary",
    "prelude",
    "featureSet",
    "stdinSha256",
    "terminalPhase",
    "outputFormat",
    "runtimeRegistry",
    "runtimeTemplate",
    "generatedPayloadSchema",
    "irSchema",
    "diagnosticMode",
    "targetTriple",
    "targetTripleDisposition",
    "targetFactsSha256",
];
const GENERATED_RUST_FIELDS: &[&str] = &[
    "formatSchema",
    "sha256",
    "bytes",
    "normalization",
    "storageDisposition",
    "distribution",
];
const TARGET_FACTS_FIELDS: &[&str] = &[
    "formatSchema",
    "path",
    "sha256",
    "bytes",
    "storageDisposition",
    "selection",
    "reason",
];
const PROGRAM_IMAGE_FIELDS: &[&str] = &["formatSchema", "path", "sha256", "bytes", "stageIdentity"];
const EXTRACTION_FIELDS: &[&str] = &[
    "operation",
    "inputGeneratedRustSha256",
    "outputProgramImageSha256",
    "outputTargetFactsSha256",
    "extractorSourceSetId",
    "extractorBinarySha256",
    "extractorCommand",
    "toolchainId",
    "toolchainManifestSha256",
    "lockfileSha256",
    "configurationSha256",
    "configuration",
    "compressionAlgorithm",
    "compressionVersion",
    "compressionParameters",
    "dictionarySha256",
    "normalization",
    "executionDisposition",
];
const EXTRACTION_CONFIGURATION_FIELDS: &[&str] = &[
    "schema",
    "formatMagic",
    "formatVersion",
    "integerEncoding",
    "operationOrdering",
    "moduleOrdering",
    "normalization",
    "compression",
];
const COMPRESSION_FIELDS: &[&str] = &["algorithm", "version", "parameters", "dictionarySha256"];

pub(crate) struct GeneratedArtifactsManifest {
    pub source_set_id: String,
    pub compilation_request_sha256: String,
    pub generated_rust_bytes: u64,
    pub generated_rust_sha256: String,
    pub target_facts_bytes: u64,
    pub target_facts_sha256: String,
    pub program_image_bytes: u64,
    pub program_image_sha256: String,
}

type JsonObject = BTreeMap<Rc<str>, JsonValue>;

fn object<'a>(value: &'a JsonValue, context: &str) -> Result<&'a JsonObject, String> {
    match value {
        JsonValue::Object(value) => Ok(value),
        _ => Err(format!("{context} is not an object")),
    }
}

fn exact_object<'a>(
    value: &'a JsonValue,
    context: &str,
    fields: &[&str],
) -> Result<&'a JsonObject, String> {
    let value = object(value, context)?;
    if let Some(field) = value.keys().find(|field| !fields.contains(&field.as_ref())) {
        return Err(format!("{context} has unknown field `{field}`"));
    }
    if let Some(field) = fields.iter().find(|field| !value.contains_key(**field)) {
        return Err(format!("{context} omitted `{field}`"));
    }
    Ok(value)
}

fn field<'a>(value: &'a JsonObject, name: &str, context: &str) -> Result<&'a JsonValue, String> {
    value
        .get(name)
        .ok_or_else(|| format!("{context} omitted `{name}`"))
}

fn string<'a>(value: &'a JsonObject, name: &str, context: &str) -> Result<&'a str, String> {
    match field(value, name, context)? {
        JsonValue::String(value) => Ok(value),
        _ => Err(format!("{context}.{name} is not a string")),
    }
}

fn integer(value: &JsonObject, name: &str, context: &str) -> Result<u64, String> {
    match field(value, name, context)? {
        JsonValue::Number(value) => value
            .lexeme
            .parse()
            .map_err(|_| format!("{context}.{name} is not a u64")),
        _ => Err(format!("{context}.{name} is not a number")),
    }
}

fn require_string(
    value: &JsonObject,
    name: &str,
    expected: &str,
    context: &str,
) -> Result<(), String> {
    let observed = string(value, name, context)?;
    if observed == expected {
        Ok(())
    } else {
        Err(format!(
            "{context}.{name} is `{observed}`, expected `{expected}`"
        ))
    }
}

fn require_null(value: &JsonObject, name: &str, context: &str) -> Result<(), String> {
    match field(value, name, context)? {
        JsonValue::Null => Ok(()),
        _ => Err(format!("{context}.{name} is not null")),
    }
}

fn require_empty_array(value: &JsonObject, name: &str, context: &str) -> Result<(), String> {
    match field(value, name, context)? {
        JsonValue::Array(value) if value.is_empty() => Ok(()),
        JsonValue::Array(_) => Err(format!("{context}.{name} is not empty")),
        _ => Err(format!("{context}.{name} is not an array")),
    }
}

fn require_sha256(value: &str, context: &str) -> Result<(), String> {
    let Some(digest) = value.strip_prefix("sha256:") else {
        return Err(format!("{context} is not a SHA-256 identity"));
    };
    if digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(format!("{context} is not a canonical SHA-256 identity"))
    }
}

fn require_sha256_field(value: &JsonObject, name: &str, context: &str) -> Result<String, String> {
    let identity = string(value, name, context)?;
    require_sha256(identity, &format!("{context}.{name}"))?;
    Ok(identity.to_string())
}

fn equal(left: &str, right: &str, context: &str) -> Result<(), String> {
    if left == right {
        Ok(())
    } else {
        Err(format!("{context} disagrees: `{left}` != `{right}`"))
    }
}

pub(crate) fn decode_generated_artifacts_manifest(
    bytes: &[u8],
) -> Result<GeneratedArtifactsManifest, String> {
    const CONTEXT: &str = "generated artifacts manifest";
    let text =
        std::str::from_utf8(bytes).map_err(|error| format!("{CONTEXT} is not UTF-8: {error}"))?;
    let parsed = json_parse(text).map_err(|error| format!("{CONTEXT} is not JSON: {error:?}"))?;
    let root = exact_object(&parsed, CONTEXT, ROOT_FIELDS)?;
    require_string(root, "schema", GENERATED_ARTIFACTS_SCHEMA, CONTEXT)?;

    let source = exact_object(
        field(root, "compilerSourceSet", CONTEXT)?,
        "compilerSourceSet",
        SOURCE_SET_FIELDS,
    )?;
    let source_set_id = require_sha256_field(source, "sourceSetId", "compilerSourceSet")?;
    let file_manifest_sha256 =
        require_sha256_field(source, "fileManifestSha256", "compilerSourceSet")?;

    let request = exact_object(
        field(root, "compilationRequest", CONTEXT)?,
        "compilationRequest",
        COMPILATION_REQUEST_FIELDS,
    )?;
    let compilation_request_sha256 = require_sha256_field(request, "sha256", "compilationRequest")?;
    require_string(
        request,
        "canonicalization",
        "recursive-lexicographic-key-order-json-utf8/v1",
        "compilationRequest",
    )?;
    require_string(
        request,
        "generationProfileId",
        GENERATION_PROFILE,
        "compilationRequest",
    )?;
    require_string(request, "targetTriple", "none", "compilationRequest")?;
    let request_target_facts =
        require_sha256_field(request, "targetFactsSha256", "compilationRequest")?;
    let descriptor = exact_object(
        field(request, "descriptor", "compilationRequest")?,
        "compilationRequest.descriptor",
        COMPILATION_REQUEST_DESCRIPTOR_FIELDS,
    )?;
    for (name, expected) in [
        ("schema", "topaz.compiler.compilation-request/v1"),
        ("generationProfileId", GENERATION_PROFILE),
        ("entrySource", "src/main.tpz"),
        ("sourceOrder", "manifest-order"),
        ("languageMode", "topaz-5.20"),
        ("packageFactsProfile", "standalone"),
        ("standardLibrary", "none"),
        ("prelude", "none"),
        ("terminalPhase", "rust-source"),
        ("outputFormat", "topaz.compiler.generated-rust/v1"),
        ("runtimeRegistry", RUNTIME_REGISTRY_SCHEMA),
        ("runtimeTemplate", RUNTIME_TEMPLATE),
        ("generatedPayloadSchema", PAYLOAD_SCHEMA),
        ("irSchema", IR_SCHEMA),
        ("diagnosticMode", "compiler-exchange"),
        ("targetTriple", "none"),
        ("targetTripleDisposition", "stage-neutral-generated-source"),
    ] {
        require_string(descriptor, name, expected, "compilationRequest.descriptor")?;
    }
    require_empty_array(descriptor, "featureSet", "compilationRequest.descriptor")?;
    let descriptor_source =
        require_sha256_field(descriptor, "sourceSetId", "compilationRequest.descriptor")?;
    let descriptor_file_manifest = require_sha256_field(
        descriptor,
        "fileManifestSha256",
        "compilationRequest.descriptor",
    )?;
    let descriptor_target_facts = require_sha256_field(
        descriptor,
        "targetFactsSha256",
        "compilationRequest.descriptor",
    )?;
    require_sha256_field(descriptor, "stdinSha256", "compilationRequest.descriptor")?;
    match field(descriptor, "sourcePaths", "compilationRequest.descriptor")? {
        JsonValue::Array(paths) => {
            if paths
                .iter()
                .any(|path| !matches!(path, JsonValue::String(_)))
            {
                return Err(
                    "compilationRequest.descriptor.sourcePaths contains a non-string".to_string(),
                );
            }
        }
        _ => return Err("compilationRequest.descriptor.sourcePaths is not an array".to_string()),
    }
    equal(&source_set_id, &descriptor_source, "request source set")?;
    equal(
        &file_manifest_sha256,
        &descriptor_file_manifest,
        "request file manifest",
    )?;
    equal(
        &request_target_facts,
        &descriptor_target_facts,
        "request target facts",
    )?;

    let generated = exact_object(
        field(root, "generatedRust", CONTEXT)?,
        "generatedRust",
        GENERATED_RUST_FIELDS,
    )?;
    require_string(
        generated,
        "formatSchema",
        "topaz.compiler.generated-rust/v1",
        "generatedRust",
    )?;
    require_string(generated, "normalization", "none", "generatedRust")?;
    require_string(
        generated,
        "storageDisposition",
        "generated-on-verification",
        "generatedRust",
    )?;
    require_null(generated, "distribution", "generatedRust")?;
    let generated_rust_sha256 = require_sha256_field(generated, "sha256", "generatedRust")?;
    let generated_rust_bytes = integer(generated, "bytes", "generatedRust")?;

    let target_facts = exact_object(
        field(root, "targetFacts", CONTEXT)?,
        "targetFacts",
        TARGET_FACTS_FIELDS,
    )?;
    require_string(
        target_facts,
        "formatSchema",
        TARGET_FACTS_SCHEMA,
        "targetFacts",
    )?;
    require_string(
        target_facts,
        "storageDisposition",
        "checked-in-sidecar",
        "targetFacts",
    )?;
    require_string(
        target_facts,
        "selection",
        "option-2-sidecar-artifact",
        "targetFacts",
    )?;
    let target_facts_sha256 = require_sha256_field(target_facts, "sha256", "targetFacts")?;
    let target_facts_bytes = integer(target_facts, "bytes", "targetFacts")?;
    equal(
        &request_target_facts,
        &target_facts_sha256,
        "request/sidecar target facts",
    )?;

    let image = exact_object(
        field(root, "programImage", CONTEXT)?,
        "programImage",
        PROGRAM_IMAGE_FIELDS,
    )?;
    require_string(image, "formatSchema", PROGRAM_IMAGE_SCHEMA, "programImage")?;
    require_string(image, "stageIdentity", "none", "programImage")?;
    let program_image_sha256 = require_sha256_field(image, "sha256", "programImage")?;
    let program_image_bytes = integer(image, "bytes", "programImage")?;

    let extraction = exact_object(
        field(root, "extraction", CONTEXT)?,
        "extraction",
        EXTRACTION_FIELDS,
    )?;
    for name in [
        "inputGeneratedRustSha256",
        "outputProgramImageSha256",
        "outputTargetFactsSha256",
        "extractorSourceSetId",
        "extractorBinarySha256",
        "toolchainManifestSha256",
        "lockfileSha256",
        "configurationSha256",
    ] {
        require_sha256_field(extraction, name, "extraction")?;
    }
    require_string(
        extraction,
        "operation",
        "extract-program-image",
        "extraction",
    )?;
    require_string(extraction, "compressionAlgorithm", "none", "extraction")?;
    require_string(extraction, "compressionVersion", "none", "extraction")?;
    require_string(extraction, "compressionParameters", "none", "extraction")?;
    require_null(extraction, "dictionarySha256", "extraction")?;
    require_string(extraction, "normalization", "none", "extraction")?;
    require_string(extraction, "executionDisposition", "fresh", "extraction")?;
    equal(
        string(extraction, "inputGeneratedRustSha256", "extraction")?,
        &generated_rust_sha256,
        "extraction generated Rust",
    )?;
    equal(
        string(extraction, "outputProgramImageSha256", "extraction")?,
        &program_image_sha256,
        "extraction program image",
    )?;
    equal(
        string(extraction, "outputTargetFactsSha256", "extraction")?,
        &target_facts_sha256,
        "extraction target facts",
    )?;
    let configuration = exact_object(
        field(extraction, "configuration", "extraction")?,
        "extraction.configuration",
        EXTRACTION_CONFIGURATION_FIELDS,
    )?;
    for (name, expected) in [
        (
            "schema",
            "topaz.compiler.program-image-extraction-configuration/v1",
        ),
        ("formatMagic", "TPZIMAGE"),
        ("integerEncoding", "little-endian-u32"),
        ("operationOrdering", "source-lowered-operation-order"),
        ("moduleOrdering", "source-lowered-module-order"),
        ("normalization", "none"),
    ] {
        require_string(configuration, name, expected, "extraction.configuration")?;
    }
    if integer(configuration, "formatVersion", "extraction.configuration")? != 1 {
        return Err("extraction.configuration.formatVersion is not 1".to_string());
    }
    let compression = exact_object(
        field(configuration, "compression", "extraction.configuration")?,
        "extraction.configuration.compression",
        COMPRESSION_FIELDS,
    )?;
    for name in ["algorithm", "version", "parameters"] {
        require_string(
            compression,
            name,
            "none",
            "extraction.configuration.compression",
        )?;
    }
    require_null(
        compression,
        "dictionarySha256",
        "extraction.configuration.compression",
    )?;

    Ok(GeneratedArtifactsManifest {
        source_set_id,
        compilation_request_sha256,
        generated_rust_bytes,
        generated_rust_sha256,
        target_facts_bytes,
        target_facts_sha256,
        program_image_bytes,
        program_image_sha256,
    })
}

pub(crate) fn validate_target_facts(bytes: &[u8], source_set_id: &str) -> Result<(), String> {
    const CONTEXT: &str = "compiler target facts sidecar";
    let text =
        std::str::from_utf8(bytes).map_err(|error| format!("{CONTEXT} is not UTF-8: {error}"))?;
    let parsed = json_parse(text).map_err(|error| format!("{CONTEXT} is not JSON: {error:?}"))?;
    let root = object(&parsed, CONTEXT)?;
    require_string(root, "schema", TARGET_FACTS_SCHEMA, CONTEXT)?;
    require_string(root, "sourceSetId", source_set_id, CONTEXT)
}
