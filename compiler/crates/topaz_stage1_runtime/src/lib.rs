//! Audited runtime shell for the Topaz-authored compiler program image.
//!
//! The checked-in bytes carry a generic format identity, not C1/C2 producer
//! provenance. Stage 1 and Stage 2 remain derivation-edge identities selected
//! by their explicit execution routes. The reusable IR executor lives in
//! `topaz_product_runtime`, so emitted applications do not carry this image.

use std::sync::{Arc, OnceLock};

use topaz_product_runtime::{
    PRODUCT_RUNTIME_STACK_BYTES, Program, execute_compiler_program_with_facts,
    parse_embedded_program,
};
use topaz_value::value::JsonValue;

#[cfg(test)]
#[path = "../build_support/repository_file_identity.rs"]
mod repository_file_identity;

mod embedded_image_descriptors {
    include!(concat!(env!("OUT_DIR"), "/compiler_image_descriptors.rs"));
}

const EMBEDDED_PROGRAM_BYTES: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/topaz_compiler_program_image.bin"
));
const EMBEDDED_COMPILER_TARGET_FACTS: &str = include_str!(concat!(
    env!("OUT_DIR"),
    "/topaz_compiler_target_facts.json"
));

static EMBEDDED_STAGE1_PROGRAM: OnceLock<Result<Arc<Program>, String>> = OnceLock::new();

#[derive(Clone)]
pub struct PreparedStage2Identity {
    pub program_image_sha256: String,
    pub program_image_payload_sha256: String,
    pub source_set_id: &'static str,
    pub ir_schema: &'static str,
    pub runtime_template: &'static str,
    pub rust_toolchain: &'static str,
}

struct PreparedStage2Program {
    program: Arc<Program>,
    identity: PreparedStage2Identity,
}

static EMBEDDED_STAGE2_PROGRAM: OnceLock<Result<PreparedStage2Program, String>> = OnceLock::new();
#[cfg(not(feature = "test-invalid-stage2-identity"))]
const EMBEDDED_STAGE2_PROGRAM_SHA256: &str = embedded_image_descriptors::PROGRAM_IMAGE_SHA256;
#[cfg(feature = "test-invalid-stage2-identity")]
const EMBEDDED_STAGE2_PROGRAM_SHA256: &str =
    "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const EMBEDDED_STAGE2_PAYLOAD_SHA256: &str =
    embedded_image_descriptors::PROGRAM_IMAGE_PAYLOAD_SHA256;
const EMBEDDED_STAGE2_SOURCE_SET_ID: &str = embedded_image_descriptors::SOURCE_SET_ID;
const EMBEDDED_STAGE2_RUST_TOOLCHAIN: &str = env!("CARGO_PKG_RUST_VERSION");

fn run_on_self_runtime_stack<T, F>(name: &str, task: F) -> Result<T, String>
where
    T: Send,
    F: FnOnce() -> Result<T, String> + Send,
{
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name(name.to_string())
            .stack_size(PRODUCT_RUNTIME_STACK_BYTES)
            .spawn_scoped(scope, task)
            .map_err(|error| format!("cannot start {name}: {error}"))?
            .join()
            .map_err(|_| format!("{name} panicked"))?
    })
}

pub use topaz_product_runtime::{
    decode_runtime_diagnostic, execute_compiler, execute_product_program,
    execute_product_program_with_facts_and_input,
    execute_product_program_with_host_facts_and_input,
};

pub fn execute_embedded_compiler(request_bytes: &[u8]) -> Result<Vec<u8>, String> {
    require_request_producer(request_bytes, "topaz-stage1")?;
    run_on_self_runtime_stack("Stage 1 compiler runtime", || {
        execute_embedded_compiler_inner(request_bytes)
    })
}

fn execute_embedded_compiler_inner(request_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let program = EMBEDDED_STAGE1_PROGRAM
        .get_or_init(|| {
            parse_embedded_program(EMBEDDED_PROGRAM_BYTES, b"TPZIMAGE\x01", "Stage 1").map(Arc::new)
        })
        .clone()?;
    if EMBEDDED_COMPILER_TARGET_FACTS.is_empty() {
        return Err("embedded compiler image omitted its target facts".to_string());
    }
    execute_compiler_program_with_facts(program, request_bytes, EMBEDDED_COMPILER_TARGET_FACTS)
}

pub fn execute_embedded_stage2_compiler(request_bytes: &[u8]) -> Result<Vec<u8>, String> {
    require_request_producer(request_bytes, "topaz-stage2")?;
    run_on_self_runtime_stack("Stage 2 compiler runtime", || {
        execute_embedded_stage2_compiler_inner(request_bytes)
    })
}

fn execute_embedded_stage2_compiler_inner(request_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let prepared = embedded_stage2_program()?;
    if EMBEDDED_COMPILER_TARGET_FACTS.is_empty() {
        return Err("embedded compiler image omitted its target facts".to_string());
    }
    execute_compiler_program_with_facts(
        prepared.program.clone(),
        request_bytes,
        EMBEDDED_COMPILER_TARGET_FACTS,
    )
}

fn require_request_producer(request_bytes: &[u8], expected: &str) -> Result<(), String> {
    let text = std::str::from_utf8(request_bytes)
        .map_err(|error| format!("compiler request is not UTF-8: {error}"))?;
    let parsed = topaz_value::value::json_parse(text)
        .map_err(|error| format!("compiler request is not JSON: {error:?}"))?;
    let JsonValue::Object(request) = parsed else {
        return Err("compiler request is not an object".to_string());
    };
    let Some(JsonValue::String(producer)) = request.get("producer") else {
        return Err("compiler request omitted string `producer`".to_string());
    };
    if producer.as_ref() != expected {
        return Err(format!(
            "{expected} program image rejects request producer `{producer}`"
        ));
    }
    Ok(())
}

pub fn embedded_compiler_program_sha256() -> &'static str {
    embedded_image_descriptors::PROGRAM_IMAGE_SHA256.trim_start_matches("sha256:")
}

pub fn embedded_stage2_program_sha256() -> &'static str {
    embedded_compiler_program_sha256()
}

pub fn embedded_compilation_request_sha256() -> &'static str {
    embedded_image_descriptors::COMPILATION_REQUEST_SHA256
}

pub fn embedded_target_facts_sha256() -> &'static str {
    embedded_image_descriptors::TARGET_FACTS_SHA256
}

fn prepare_stage2_program(
    bytes: &[u8],
    expected_program_sha256: &str,
    expected_payload_sha256: &str,
) -> Result<PreparedStage2Program, String> {
    let program_image_sha256 = sha256_identity(bytes);
    if program_image_sha256 != expected_program_sha256 {
        return Err(format!(
            "embedded compiler program-image identity drifted: expected `{expected_program_sha256}`, observed `{program_image_sha256}`"
        ));
    }
    let payload_bytes = bytes
        .get(b"TPZIMAGE\x01".len()..)
        .ok_or_else(|| "embedded compiler program image is truncated".to_string())?;
    let program_image_payload_sha256 = sha256_identity(payload_bytes);
    if program_image_payload_sha256 != expected_payload_sha256 {
        return Err(format!(
            "embedded compiler payload identity drifted: expected `{expected_payload_sha256}`, observed `{program_image_payload_sha256}`"
        ));
    }
    let program = parse_embedded_program(bytes, b"TPZIMAGE\x01", "Stage 2")?;
    Ok(PreparedStage2Program {
        program: Arc::new(program),
        identity: PreparedStage2Identity {
            program_image_sha256,
            program_image_payload_sha256,
            source_set_id: EMBEDDED_STAGE2_SOURCE_SET_ID,
            ir_schema: embedded_image_descriptors::IR_SCHEMA,
            runtime_template: embedded_image_descriptors::RUNTIME_TEMPLATE,
            rust_toolchain: EMBEDDED_STAGE2_RUST_TOOLCHAIN,
        },
    })
}

fn embedded_stage2_program() -> Result<&'static PreparedStage2Program, String> {
    match EMBEDDED_STAGE2_PROGRAM.get_or_init(|| {
        prepare_stage2_program(
            EMBEDDED_PROGRAM_BYTES,
            EMBEDDED_STAGE2_PROGRAM_SHA256,
            EMBEDDED_STAGE2_PAYLOAD_SHA256,
        )
    }) {
        Ok(prepared) => Ok(prepared),
        Err(error) => Err(error.clone()),
    }
}

/// Validate and decode the exact installed stage-neutral image once per
/// process. The Stage 2 identity comes from this execution route's derivation
/// edge, never from an image header. Compiler workers share the immutable
/// prepared program; target source and compilation results are never cached.
pub fn prepared_embedded_stage2_identity() -> Result<PreparedStage2Identity, String> {
    Ok(embedded_stage2_program()?.identity.clone())
}

pub fn prepare_embedded_stage2_compiler() -> Result<(), String> {
    prepared_embedded_stage2_identity().map(|_| ())
}

pub fn embedded_program_payload_sha256(stage: u8) -> Result<String, String> {
    let label = match stage {
        1 => "Stage 1",
        2 => "Stage 2",
        _ => return Err(format!("unsupported compiler program stage {stage}")),
    };
    if stage == 2 {
        return Ok(prepared_embedded_stage2_identity()?
            .program_image_payload_sha256
            .trim_start_matches("sha256:")
            .to_string());
    } else {
        let _ = parse_embedded_program(EMBEDDED_PROGRAM_BYTES, b"TPZIMAGE\x01", label)?;
    }
    Ok(embedded_image_descriptors::PROGRAM_IMAGE_PAYLOAD_SHA256
        .trim_start_matches("sha256:")
        .to_string())
}

fn sha256_identity(bytes: &[u8]) -> String {
    let digest = topaz_value::value::sha256(bytes);
    let mut identity = String::from("sha256:");
    topaz_value::bytes_to_hex_into(&mut identity, &digest);
    identity
}

pub struct CompilerImageDescriptor {
    pub executable: &'static str,
    pub executable_stage: u64,
    pub generated_rust_bytes: u64,
    pub generated_rust_sha256: &'static str,
    pub manifest_sha256: &'static str,
    pub program_image_sha256: &'static str,
    pub source_producer: &'static str,
    pub source_producer_stage: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerImageStage {
    C1,
    C2,
}

pub fn compiler_image_descriptor(stage: CompilerImageStage) -> CompilerImageDescriptor {
    let (
        executable,
        executable_stage,
        generated_rust_bytes,
        generated_rust_sha256,
        manifest_sha256,
        program_image_sha256,
        source_producer,
        source_producer_stage,
    ) = match stage {
        CompilerImageStage::C1 => (
            "topaz-stage1",
            1,
            embedded_image_descriptors::GENERATED_RUST_BYTES,
            embedded_image_descriptors::GENERATED_RUST_SHA256,
            embedded_image_descriptors::GENERATED_ARTIFACTS_MANIFEST_SHA256,
            embedded_image_descriptors::PROGRAM_IMAGE_SHA256,
            "topaz-interpreted-bootstrap",
            0,
        ),
        CompilerImageStage::C2 => (
            "topaz-stage2",
            2,
            embedded_image_descriptors::GENERATED_RUST_BYTES,
            embedded_image_descriptors::GENERATED_RUST_SHA256,
            embedded_image_descriptors::GENERATED_ARTIFACTS_MANIFEST_SHA256,
            embedded_image_descriptors::PROGRAM_IMAGE_SHA256,
            "topaz-stage1",
            1,
        ),
    };
    CompilerImageDescriptor {
        executable,
        executable_stage,
        generated_rust_bytes,
        generated_rust_sha256,
        manifest_sha256,
        program_image_sha256,
        source_producer,
        source_producer_stage,
    }
}

pub fn embedded_compiler_identity() -> (&'static str, &'static str, &'static str) {
    (
        embedded_image_descriptors::SOURCE_SET_ID,
        embedded_image_descriptors::RUNTIME_TEMPLATE,
        embedded_image_descriptors::IR_SCHEMA,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_image_header_is_stage_neutral() {
        assert_eq!(&EMBEDDED_PROGRAM_BYTES[..9], b"TPZIMAGE\x01");
        assert_ne!(&EMBEDDED_PROGRAM_BYTES[..9], b"TPZC1BIN\x01");
        assert_ne!(&EMBEDDED_PROGRAM_BYTES[..9], b"TPZC2BIN\x01");
    }

    #[test]
    fn derivation_routes_reject_cross_route_requests() {
        let stage1 = br#"{"producer":"topaz-stage1"}"#;
        let stage2 = br#"{"producer":"topaz-stage2"}"#;
        assert!(
            execute_embedded_compiler(stage2)
                .expect_err("C1 rejects Stage 2")
                .contains("topaz-stage1 program image rejects")
        );
        assert!(
            execute_embedded_stage2_compiler(stage1)
                .expect_err("C2 rejects Stage 1")
                .contains("topaz-stage2 program image rejects")
        );
    }

    #[test]
    fn prepared_stage2_image_rejects_corruption_and_identity_drift() {
        let mut corrupt = EMBEDDED_PROGRAM_BYTES.to_vec();
        corrupt[0] ^= 0xff;
        let error = prepare_stage2_program(
            &corrupt,
            EMBEDDED_STAGE2_PROGRAM_SHA256,
            EMBEDDED_STAGE2_PAYLOAD_SHA256,
        )
        .err()
        .expect("corrupt C2 must fail");
        assert!(error.contains("program-image identity drifted"), "{error}");

        let error = prepare_stage2_program(
            EMBEDDED_PROGRAM_BYTES,
            &"0".repeat(64),
            EMBEDDED_STAGE2_PAYLOAD_SHA256,
        )
        .err()
        .expect("wrong expected identity must fail");
        assert!(error.contains("program-image identity drifted"), "{error}");
    }
}
