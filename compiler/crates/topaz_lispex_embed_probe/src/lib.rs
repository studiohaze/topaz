//! Removable private probe for the exact retained Lispex evaluator component.
//!
//! This crate is deliberately outside the default workspace members and is
//! not wired into the `topaz` command. The evaluator bytes are selected at
//! compile time. Callers can only confirm the expected identity tuple, never
//! discover or substitute another artifact.

use sha2::{Digest, Sha256};
use std::fmt;
use std::sync::OnceLock;
use wasmtime::{Config, Engine, ExternType, Instance, Module, Store};

pub const COMPONENT_ID: &str = "lispex-embed-evaluator/1.20.0";
pub const EVALUATOR_SHA256: &str =
    "8ecc89c1c0b6e83e75f2be23951e99ad9d3405a179b8f062a5cf76360fb16190";
pub const PROFILE_ID: &str = "lispex/r7rs-rule-embedded-core/1";
pub const MODEL_ID: &str = "lispex-vm-meter/1";
pub const ABI_ID: &str = "lispex.embed-wasm-abi/v1";
pub const VALUE_CODEC_ID: &str = "lispex.embed-value/v1";
pub const RECEIPT_ID: &str = "lispex.embed-receipt-core/v1";
pub const RUNTIME_ID: &str = "wasmtime/38.0.4";
pub const RUNTIME_POLICY_ID: &str = "topaz.lispex-embedding-runtime/v0";
pub const ABI_VERSION: u32 = 0x0001_0000;
pub const SAFETY_FUEL: u64 = 1_000_000_000;
pub const GOLDEN_SOURCE: &[u8] = b"(if (< 10 15) \"allow\" \"deny\")\n";
pub const GOLDEN_INPUT: &[u8] = &[0];
pub const GOLDEN_RESULT_HEX: &str = "090000000000000001080000000000000005616c6c6f77";

const EVALUATOR_BYTES: &[u8] = include_bytes!(
    "../../../components/lispex-embed-evaluator/1.20.0/payload/lispex-embed-evaluator.wasm"
);
const PREPARE_MAGIC: &[u8; 8] = b"LPXPRP01";
const EVALUATE_MAGIC: &[u8; 8] = b"LPXEVA01";
const RESPONSE_MAGIC: &[u8; 8] = b"LPXRSP01";
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
static RUNTIME: OnceLock<Result<Runtime, ProbeError>> = OnceLock::new();

struct Runtime {
    engine: Engine,
    module: Module,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub component_id: String,
    pub artifact_sha256: String,
    pub profile_id: String,
    pub model_id: String,
    pub abi_id: String,
    pub value_codec_id: String,
    pub receipt_id: String,
}

impl Selection {
    #[must_use]
    pub fn exact() -> Self {
        Self {
            component_id: COMPONENT_ID.to_string(),
            artifact_sha256: EVALUATOR_SHA256.to_string(),
            profile_id: PROFILE_ID.to_string(),
            model_id: MODEL_ID.to_string(),
            abi_id: ABI_ID.to_string(),
            value_codec_id: VALUE_CODEC_ID.to_string(),
            receipt_id: RECEIPT_ID.to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Prepare,
    Evaluate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DraftCategory {
    Prepared,
    Complete,
    SemanticFailure,
    LimitExhaustion,
    RequestRefusal,
    EngineFault,
}

impl DraftCategory {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Complete => "complete",
            Self::SemanticFailure => "semantic-failure",
            Self::LimitExhaustion => "limit-exhaustion",
            Self::RequestRefusal => "request-refusal",
            Self::EngineFault => "engine-fault",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftOutcome {
    pub operation: Operation,
    pub category: DraftCategory,
    pub code: String,
    pub payload: Vec<u8>,
    pub digests: [Option<String>; 6],
    pub nonportable_usage: Option<[u64; 9]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeEvidence {
    pub prepare: DraftOutcome,
    pub evaluate: DraftOutcome,
    pub prepare_request_sha256: String,
    pub evaluate_request_sha256: String,
    pub result_sha256: String,
    pub fresh_instances: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeError {
    SelectionRefusal(&'static str),
    ContractViolation(&'static str),
    EngineFault,
}

impl fmt::Display for ProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SelectionRefusal(field) => {
                write!(formatter, "embedding selection refused: {field}")
            }
            Self::ContractViolation(reason) => {
                write!(formatter, "embedding contract violation: {reason}")
            }
            Self::EngineFault => formatter.write_str("embedded evaluator engine fault"),
        }
    }
}

impl std::error::Error for ProbeError {}

pub fn run_golden_probe(selection: &Selection) -> Result<ProbeEvidence, ProbeError> {
    validate_selection(selection)?;
    verify_embedded_evaluator()?;

    let prepare_limits = [4_096, 1_000_000, 1_000_000, 64];
    let evaluate_limits = [
        1_000_000, 1_000_000, 1_000, 256, 1_000_000, 1_000_000, 1_000_000, 100, 1_000_000,
    ];
    let prepare_request = prepare_request(GOLDEN_SOURCE, prepare_limits)?;
    let prepare_bytes = invoke(Operation::Prepare, &prepare_request)?;
    let prepare = parse_response(&prepare_bytes)?;
    verify_response_contract(&prepare)?;
    if prepare.category != DraftCategory::Prepared {
        return Err(ProbeError::ContractViolation(
            "golden prepare did not produce a prepared rule",
        ));
    }

    let evaluate_request =
        evaluate_request(&prepare.payload, GOLDEN_INPUT, 4_096, evaluate_limits)?;
    let evaluate_bytes = invoke(Operation::Evaluate, &evaluate_request)?;
    let evaluate = parse_response(&evaluate_bytes)?;
    verify_response_contract(&evaluate)?;
    if evaluate.category != DraftCategory::Complete {
        return Err(ProbeError::ContractViolation(
            "golden evaluation did not complete",
        ));
    }
    if hex_lower(&evaluate.payload) != GOLDEN_RESULT_HEX {
        return Err(ProbeError::ContractViolation(
            "golden result payload changed",
        ));
    }
    if evaluate.digests[0] != prepare.digests[3] || evaluate.digests[1] != prepare.digests[4] {
        return Err(ProbeError::ContractViolation(
            "evaluation is not bound to the prepared rule",
        ));
    }

    Ok(ProbeEvidence {
        prepare,
        evaluate,
        prepare_request_sha256: sha256_hex(&prepare_request),
        evaluate_request_sha256: sha256_hex(&evaluate_request),
        result_sha256: sha256_hex(
            &parse_response(&evaluate_bytes)
                .map_err(|_| ProbeError::ContractViolation("result response did not reparse"))?
                .payload,
        ),
        fresh_instances: 2,
    })
}

pub fn validate_selection(selection: &Selection) -> Result<(), ProbeError> {
    for (actual, expected, field) in [
        (selection.component_id.as_str(), COMPONENT_ID, "component"),
        (
            selection.artifact_sha256.as_str(),
            EVALUATOR_SHA256,
            "artifact-digest",
        ),
        (selection.profile_id.as_str(), PROFILE_ID, "profile"),
        (selection.model_id.as_str(), MODEL_ID, "cost-model"),
        (selection.abi_id.as_str(), ABI_ID, "abi"),
        (
            selection.value_codec_id.as_str(),
            VALUE_CODEC_ID,
            "value-codec",
        ),
        (selection.receipt_id.as_str(), RECEIPT_ID, "receipt-schema"),
    ] {
        if actual != expected {
            return Err(ProbeError::SelectionRefusal(field));
        }
    }
    Ok(())
}

fn prepare_request(source: &[u8], limits: [u64; 4]) -> Result<Vec<u8>, ProbeError> {
    let mut request = Vec::with_capacity(12 + source.len() + limits.len() * 8);
    request.extend_from_slice(PREPARE_MAGIC);
    push_field(&mut request, source)?;
    for limit in limits {
        request.extend_from_slice(&limit.to_be_bytes());
    }
    Ok(request)
}

fn evaluate_request(
    prepared: &[u8],
    input: &[u8],
    canonical_input_limit: u64,
    limits: [u64; 9],
) -> Result<Vec<u8>, ProbeError> {
    let mut request =
        Vec::with_capacity(16 + prepared.len() + input.len() + (limits.len() + 1) * 8);
    request.extend_from_slice(EVALUATE_MAGIC);
    push_field(&mut request, prepared)?;
    push_field(&mut request, input)?;
    request.extend_from_slice(&canonical_input_limit.to_be_bytes());
    for limit in limits {
        request.extend_from_slice(&limit.to_be_bytes());
    }
    Ok(request)
}

fn push_field(output: &mut Vec<u8>, field: &[u8]) -> Result<(), ProbeError> {
    let length = u32::try_from(field.len())
        .map_err(|_| ProbeError::ContractViolation("request field exceeds canonical u32"))?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(field);
    Ok(())
}

fn invoke(operation: Operation, request: &[u8]) -> Result<Vec<u8>, ProbeError> {
    let runtime = runtime()?;
    let mut store = Store::new(&runtime.engine, ());
    store
        .set_fuel(SAFETY_FUEL)
        .map_err(|_| ProbeError::EngineFault)?;
    let instance =
        Instance::new(&mut store, &runtime.module, &[]).map_err(|_| ProbeError::EngineFault)?;
    invoke_instance(operation, request, instance, store)
}

fn runtime() -> Result<&'static Runtime, ProbeError> {
    match RUNTIME.get_or_init(build_runtime) {
        Ok(runtime) => Ok(runtime),
        Err(error) => Err(error.clone()),
    }
}

fn build_runtime() -> Result<Runtime, ProbeError> {
    let mut config = Config::new();
    config.consume_fuel(true);
    config.epoch_interruption(false);
    let engine = Engine::new(&config).map_err(|_| ProbeError::EngineFault)?;
    let module =
        Module::from_binary(&engine, EVALUATOR_BYTES).map_err(|_| ProbeError::EngineFault)?;
    check_module_surface(&module)?;
    Ok(Runtime { engine, module })
}

fn invoke_instance(
    operation: Operation,
    request: &[u8],
    instance: Instance,
    mut store: Store<()>,
) -> Result<Vec<u8>, ProbeError> {
    let version = instance
        .get_typed_func::<(), u32>(&mut store, "lispex_embed_abi_version")
        .map_err(|_| ProbeError::EngineFault)?
        .call(&mut store, ())
        .map_err(|_| ProbeError::EngineFault)?;
    if version != ABI_VERSION {
        return Err(ProbeError::ContractViolation("ABI version mismatch"));
    }
    let alloc = instance
        .get_typed_func::<u32, u32>(&mut store, "lispex_embed_alloc")
        .map_err(|_| ProbeError::EngineFault)?;
    let dealloc = instance
        .get_typed_func::<(u32, u32), u32>(&mut store, "lispex_embed_dealloc")
        .map_err(|_| ProbeError::EngineFault)?;
    let operation = instance
        .get_typed_func::<(u32, u32), u64>(
            &mut store,
            match operation {
                Operation::Prepare => "lispex_embed_prepare",
                Operation::Evaluate => "lispex_embed_evaluate",
            },
        )
        .map_err(|_| ProbeError::EngineFault)?;
    let memory = instance
        .get_memory(&mut store, "memory")
        .ok_or(ProbeError::ContractViolation("memory export missing"))?;
    let request_len = u32::try_from(request.len())
        .map_err(|_| ProbeError::ContractViolation("request exceeds canonical u32"))?;
    let request_ptr = alloc
        .call(&mut store, request_len)
        .map_err(|_| ProbeError::EngineFault)?;
    if request_ptr == 0 {
        return Err(ProbeError::EngineFault);
    }
    memory
        .write(&mut store, request_ptr as usize, request)
        .map_err(|_| ProbeError::EngineFault)?;
    let packed = operation
        .call(&mut store, (request_ptr, request_len))
        .map_err(|_| ProbeError::EngineFault)?;
    if packed == 0 {
        return Err(ProbeError::EngineFault);
    }
    let response_ptr = (packed >> 32) as u32;
    let response_len = packed as u32;
    if response_len as usize > MAX_RESPONSE_BYTES {
        return Err(ProbeError::EngineFault);
    }
    let mut response = vec![0; response_len as usize];
    memory
        .read(&store, response_ptr as usize, &mut response)
        .map_err(|_| ProbeError::EngineFault)?;
    if dealloc
        .call(&mut store, (response_ptr, response_len))
        .map_err(|_| ProbeError::EngineFault)?
        != 1
    {
        return Err(ProbeError::EngineFault);
    }
    Ok(response)
}

fn verify_embedded_evaluator() -> Result<(), ProbeError> {
    if sha256_hex(EVALUATOR_BYTES) != EVALUATOR_SHA256 {
        return Err(ProbeError::SelectionRefusal("artifact-digest"));
    }
    Ok(())
}

fn check_module_surface(module: &Module) -> Result<(), ProbeError> {
    if module.imports().next().is_some() {
        return Err(ProbeError::ContractViolation(
            "evaluator imports a host capability",
        ));
    }
    let mut functions = Vec::new();
    let mut memory = None;
    for export in module.exports() {
        match export.ty() {
            ExternType::Func(_) => functions.push(export.name().to_string()),
            ExternType::Memory(memory_type) if export.name() == "memory" && memory.is_none() => {
                memory = Some((memory_type.minimum(), memory_type.maximum()));
            }
            ExternType::Global(_) if matches!(export.name(), "__data_end" | "__heap_base") => {}
            _ => {
                return Err(ProbeError::ContractViolation(
                    "evaluator export surface mismatch",
                ));
            }
        }
    }
    functions.sort();
    let mut expected = [
        "lispex_embed_abi_version",
        "lispex_embed_alloc",
        "lispex_embed_dealloc",
        "lispex_embed_evaluate",
        "lispex_embed_prepare",
    ]
    .map(str::to_string)
    .to_vec();
    expected.sort();
    if functions != expected || memory != Some((19, Some(256))) {
        return Err(ProbeError::ContractViolation(
            "evaluator ABI or memory surface mismatch",
        ));
    }
    Ok(())
}

fn parse_response(bytes: &[u8]) -> Result<DraftOutcome, ProbeError> {
    if bytes.len() < 16 || bytes.get(..8) != Some(RESPONSE_MAGIC) {
        return Err(ProbeError::ContractViolation("response framing is invalid"));
    }
    let operation = match bytes[8] {
        1 => Operation::Prepare,
        2 => Operation::Evaluate,
        _ => {
            return Err(ProbeError::ContractViolation(
                "response operation is invalid",
            ));
        }
    };
    let category = match (operation, bytes[9]) {
        (Operation::Prepare, 0) => DraftCategory::Prepared,
        (Operation::Evaluate, 1) => DraftCategory::Complete,
        (Operation::Evaluate, 2) => DraftCategory::SemanticFailure,
        (_, 3) => DraftCategory::LimitExhaustion,
        (_, 4) => DraftCategory::RequestRefusal,
        (_, 5) => DraftCategory::EngineFault,
        _ => {
            return Err(ProbeError::ContractViolation(
                "response category is invalid",
            ));
        }
    };
    let code_length = u16::from_be_bytes(
        bytes[10..12]
            .try_into()
            .map_err(|_| ProbeError::ContractViolation("code length is truncated"))?,
    ) as usize;
    let code_end = 12_usize
        .checked_add(code_length)
        .ok_or(ProbeError::ContractViolation("code length overflow"))?;
    let payload_length_end = code_end
        .checked_add(4)
        .ok_or(ProbeError::ContractViolation("payload length overflow"))?;
    let payload_length = u32::from_be_bytes(
        bytes
            .get(code_end..payload_length_end)
            .ok_or(ProbeError::ContractViolation("payload length is truncated"))?
            .try_into()
            .map_err(|_| ProbeError::ContractViolation("payload length is truncated"))?,
    ) as usize;
    let payload_end = payload_length_end
        .checked_add(payload_length)
        .ok_or(ProbeError::ContractViolation("payload length overflow"))?;
    let payload = bytes
        .get(payload_length_end..payload_end)
        .ok_or(ProbeError::ContractViolation("payload is truncated"))?
        .to_vec();
    let mut offset = payload_end;
    let mut digests: [Option<String>; 6] = std::array::from_fn(|_| None);
    for digest in &mut digests {
        match bytes.get(offset).copied() {
            Some(0) => offset += 1,
            Some(1) => {
                let end = offset
                    .checked_add(65)
                    .ok_or(ProbeError::ContractViolation("digest overflow"))?;
                let value = bytes
                    .get(offset + 1..end)
                    .ok_or(ProbeError::ContractViolation("digest is truncated"))?;
                if value.len() != 64
                    || !value
                        .iter()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                {
                    return Err(ProbeError::ContractViolation(
                        "digest is not lowercase hexadecimal",
                    ));
                }
                *digest = Some(
                    String::from_utf8(value.to_vec())
                        .map_err(|_| ProbeError::ContractViolation("digest is not ASCII"))?,
                );
                offset = end;
            }
            _ => return Err(ProbeError::ContractViolation("digest tag is invalid")),
        }
    }
    let usage_tag = bytes
        .get(offset)
        .copied()
        .ok_or(ProbeError::ContractViolation("usage tag is missing"))?;
    offset += 1;
    let nonportable_usage = match usage_tag {
        0 => None,
        1 => {
            let mut usage = [0_u64; 9];
            for slot in &mut usage {
                let end = offset
                    .checked_add(8)
                    .ok_or(ProbeError::ContractViolation("usage overflow"))?;
                *slot = u64::from_be_bytes(
                    bytes
                        .get(offset..end)
                        .ok_or(ProbeError::ContractViolation("usage is truncated"))?
                        .try_into()
                        .map_err(|_| ProbeError::ContractViolation("usage is truncated"))?,
                );
                offset = end;
            }
            Some(usage)
        }
        _ => return Err(ProbeError::ContractViolation("usage tag is invalid")),
    };
    if offset != bytes.len() {
        return Err(ProbeError::ContractViolation("response has trailing bytes"));
    }
    let code = String::from_utf8(
        bytes
            .get(12..code_end)
            .ok_or(ProbeError::ContractViolation("code is truncated"))?
            .to_vec(),
    )
    .map_err(|_| ProbeError::ContractViolation("code is not UTF-8"))?;
    if code.is_empty() || !code.is_ascii() {
        return Err(ProbeError::ContractViolation("code is not canonical ASCII"));
    }
    Ok(DraftOutcome {
        operation,
        category,
        code,
        payload,
        digests,
        nonportable_usage,
    })
}

fn verify_response_contract(response: &DraftOutcome) -> Result<(), ProbeError> {
    let valid = match (response.operation, response.category) {
        (Operation::Prepare, DraftCategory::Prepared) => {
            response.code == "prepared"
                && !response.payload.is_empty()
                && response.digests[..5].iter().all(Option::is_some)
                && response.digests[5].is_none()
                && response.nonportable_usage.is_none()
        }
        (Operation::Prepare, DraftCategory::LimitExhaustion | DraftCategory::RequestRefusal) => {
            response.payload.is_empty() && response.nonportable_usage.is_none()
        }
        (Operation::Prepare, DraftCategory::EngineFault) => {
            response.payload.is_empty() && response.nonportable_usage.is_none()
        }
        (Operation::Evaluate, DraftCategory::Complete) => {
            response.code == "complete"
                && !response.payload.is_empty()
                && response.digests[..5].iter().all(Option::is_some)
                && response.digests[5].is_none()
                && response.nonportable_usage.is_some()
        }
        (Operation::Evaluate, DraftCategory::SemanticFailure) => {
            response.payload.is_empty()
                && response.digests[..5].iter().all(Option::is_some)
                && response.digests[5].is_none()
                && response.nonportable_usage.is_some()
        }
        (Operation::Evaluate, DraftCategory::LimitExhaustion) => {
            response.payload.is_empty()
                && response.digests[..3].iter().all(Option::is_some)
                && response.digests[3..].iter().all(Option::is_none)
                && response.nonportable_usage.is_some()
        }
        (Operation::Evaluate, DraftCategory::RequestRefusal) => {
            response.payload.is_empty()
                && response.digests[4..].iter().all(Option::is_none)
                && response.nonportable_usage.is_none()
        }
        (Operation::Evaluate, DraftCategory::EngineFault) => {
            response.payload.is_empty() && response.nonportable_usage.is_some()
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(ProbeError::ContractViolation(
            "response violates the draft outcome envelope",
        ))
    }
}

#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex_lower(&Sha256::digest(bytes))
}

#[must_use]
pub fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[(byte >> 4) as usize]));
        output.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_golden_probe_uses_fresh_instances_and_returns_expected_value() {
        let evidence = run_golden_probe(&Selection::exact()).expect("golden probe");
        assert_eq!(evidence.fresh_instances, 2);
        assert_eq!(evidence.prepare.category, DraftCategory::Prepared);
        assert_eq!(evidence.evaluate.category, DraftCategory::Complete);
        assert_eq!(hex_lower(&evidence.evaluate.payload), GOLDEN_RESULT_HEX);
    }

    #[test]
    fn selection_mismatch_refuses_before_runtime_dispatch() {
        fn component(selection: &mut Selection) {
            selection.component_id.push_str("-wrong");
        }
        fn artifact(selection: &mut Selection) {
            selection.artifact_sha256.replace_range(..1, "0");
        }
        fn profile(selection: &mut Selection) {
            selection.profile_id.push_str("-wrong");
        }
        fn model(selection: &mut Selection) {
            selection.model_id.push_str("-wrong");
        }
        fn abi(selection: &mut Selection) {
            selection.abi_id.push_str("-wrong");
        }
        fn codec(selection: &mut Selection) {
            selection.value_codec_id.push_str("-wrong");
        }
        fn receipt(selection: &mut Selection) {
            selection.receipt_id.push_str("-wrong");
        }
        type SelectionMutation = (&'static str, fn(&mut Selection));
        let mutations: [SelectionMutation; 7] = [
            ("component", component),
            ("artifact-digest", artifact),
            ("profile", profile),
            ("cost-model", model),
            ("abi", abi),
            ("value-codec", codec),
            ("receipt-schema", receipt),
        ];
        for (field, mutate) in mutations {
            let mut selection = Selection::exact();
            mutate(&mut selection);
            assert_eq!(
                run_golden_probe(&selection),
                Err(ProbeError::SelectionRefusal(field))
            );
        }
    }

    #[test]
    fn repeated_probe_has_no_stale_result_or_shared_evaluation_state() {
        let first = run_golden_probe(&Selection::exact()).expect("first probe");
        let second = run_golden_probe(&Selection::exact()).expect("second probe");
        assert_eq!(first, second);
        assert_eq!(first.fresh_instances, 2);
    }

    #[test]
    fn malformed_response_fails_closed() {
        assert_eq!(
            parse_response(b"LPXRSP01"),
            Err(ProbeError::ContractViolation("response framing is invalid"))
        );
    }
}
