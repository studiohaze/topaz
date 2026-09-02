use crate::runtime::{Category, Operation, Response};
use crate::*;

const PREPARE_MAGIC: &[u8; 8] = b"LPXPRP01";
pub(crate) const EVALUATE_MAGIC: &[u8; 8] = b"LPXEVA01";
const RESPONSE_MAGIC: &[u8; 8] = b"LPXRSP01";
pub(crate) const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
pub(crate) fn prepare_request(source: &[u8], limits: PrepareLimits) -> Result<Vec<u8>, RunError> {
    let mut request = Vec::with_capacity(12 + source.len() + 32);
    request.extend_from_slice(PREPARE_MAGIC);
    push_field(&mut request, source)?;
    for limit in [
        limits.raw_source_bytes,
        limits.prepare_work,
        limits.logical_allocation,
        limits.syntax_depth,
    ] {
        request.extend_from_slice(&limit.to_be_bytes());
    }
    Ok(request)
}

pub(crate) fn evaluate_request(
    prepared: &[u8],
    input: &[u8],
    limits: EvaluateLimits,
) -> Result<Vec<u8>, RunError> {
    let mut request = Vec::with_capacity(16 + prepared.len() + input.len() + 80);
    request.extend_from_slice(EVALUATE_MAGIC);
    push_field(&mut request, prepared)?;
    push_field(&mut request, input)?;
    for limit in [
        limits.canonical_input_bytes,
        limits.eval_work,
        limits.logical_allocation,
        limits.semantic_frames,
        limits.traversal_depth,
        limits.output_bytes,
        limits.diagnostic_bytes,
        limits.transcript_bytes,
        limits.transcript_events,
        limits.result_bytes,
    ] {
        request.extend_from_slice(&limit.to_be_bytes());
    }
    Ok(request)
}

fn push_field(output: &mut Vec<u8>, field: &[u8]) -> Result<(), RunError> {
    let length = u32::try_from(field.len())
        .map_err(|_| RunError::ContractViolation("request field exceeds canonical u32"))?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(field);
    Ok(())
}

pub(crate) fn parse_response(bytes: &[u8]) -> Result<Response, RunError> {
    if bytes.len() < 16 || bytes.get(..8) != Some(RESPONSE_MAGIC) {
        return Err(RunError::ContractViolation("response framing is invalid"));
    }
    let operation = match bytes[8] {
        1 => Operation::Prepare,
        2 => Operation::Evaluate,
        _ => return Err(RunError::ContractViolation("response operation is invalid")),
    };
    let category = match (operation, bytes[9]) {
        (Operation::Prepare, 0) => Category::Prepared,
        (Operation::Evaluate, 1) => Category::Complete,
        (Operation::Evaluate, 2) => Category::SemanticFailure,
        (_, 3) => Category::LimitExhaustion,
        (_, 4) => Category::RequestRefusal,
        (_, 5) => Category::EngineFault,
        _ => return Err(RunError::ContractViolation("response category is invalid")),
    };
    let code_length = u16::from_be_bytes(
        bytes[10..12]
            .try_into()
            .map_err(|_| RunError::ContractViolation("code length is truncated"))?,
    ) as usize;
    let code_end = 12_usize
        .checked_add(code_length)
        .ok_or(RunError::ContractViolation("code length overflow"))?;
    let payload_length_end = code_end
        .checked_add(4)
        .ok_or(RunError::ContractViolation("payload length overflow"))?;
    let payload_length = u32::from_be_bytes(
        bytes
            .get(code_end..payload_length_end)
            .ok_or(RunError::ContractViolation("payload length is truncated"))?
            .try_into()
            .map_err(|_| RunError::ContractViolation("payload length is truncated"))?,
    ) as usize;
    let payload_end = payload_length_end
        .checked_add(payload_length)
        .ok_or(RunError::ContractViolation("payload length overflow"))?;
    let payload = bytes
        .get(payload_length_end..payload_end)
        .ok_or(RunError::ContractViolation("payload is truncated"))?
        .to_vec();
    let mut offset = payload_end;
    let mut digests: [Option<String>; 6] = std::array::from_fn(|_| None);
    for digest in &mut digests {
        match bytes.get(offset).copied() {
            Some(0) => offset += 1,
            Some(1) => {
                let end = offset
                    .checked_add(65)
                    .ok_or(RunError::ContractViolation("digest overflow"))?;
                let value = bytes
                    .get(offset + 1..end)
                    .ok_or(RunError::ContractViolation("digest is truncated"))?;
                if value.len() != 64
                    || !value
                        .iter()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                {
                    return Err(RunError::ContractViolation(
                        "digest is not lowercase hexadecimal",
                    ));
                }
                *digest = Some(
                    String::from_utf8(value.to_vec())
                        .map_err(|_| RunError::ContractViolation("digest is not ASCII"))?,
                );
                offset = end;
            }
            _ => return Err(RunError::ContractViolation("digest tag is invalid")),
        }
    }
    let usage_tag = bytes
        .get(offset)
        .copied()
        .ok_or(RunError::ContractViolation("usage tag is missing"))?;
    offset += 1;
    let usage = match usage_tag {
        0 => None,
        1 => {
            let mut usage = [0_u64; 9];
            for slot in &mut usage {
                let end = offset
                    .checked_add(8)
                    .ok_or(RunError::ContractViolation("usage overflow"))?;
                *slot = u64::from_be_bytes(
                    bytes
                        .get(offset..end)
                        .ok_or(RunError::ContractViolation("usage is truncated"))?
                        .try_into()
                        .map_err(|_| RunError::ContractViolation("usage is truncated"))?,
                );
                offset = end;
            }
            Some(usage)
        }
        _ => return Err(RunError::ContractViolation("usage tag is invalid")),
    };
    if offset != bytes.len() {
        return Err(RunError::ContractViolation("response has trailing bytes"));
    }
    let code = String::from_utf8(
        bytes
            .get(12..code_end)
            .ok_or(RunError::ContractViolation("code is truncated"))?
            .to_vec(),
    )
    .map_err(|_| RunError::ContractViolation("code is not UTF-8"))?;
    if code.is_empty() || !code.is_ascii() {
        return Err(RunError::ContractViolation("code is not canonical ASCII"));
    }
    Ok(Response {
        operation,
        category,
        code,
        payload,
        digests,
        usage,
    })
}

pub(crate) fn verify_response_contract(response: &Response) -> Result<(), RunError> {
    let valid = match (response.operation, response.category) {
        (Operation::Prepare, Category::Prepared) => {
            response.code == "prepared"
                && !response.payload.is_empty()
                && response.digests[..5].iter().all(Option::is_some)
                && response.digests[5].is_none()
                && response.usage.is_none()
        }
        (Operation::Prepare, Category::LimitExhaustion | Category::RequestRefusal) => {
            response.payload.is_empty() && response.usage.is_none()
        }
        (Operation::Prepare, Category::EngineFault) => {
            response.payload.is_empty() && response.usage.is_none()
        }
        (Operation::Evaluate, Category::Complete) => {
            response.code == "complete"
                && !response.payload.is_empty()
                && response.digests[..5].iter().all(Option::is_some)
                && response.digests[5].is_none()
                && response.usage.is_some()
        }
        (Operation::Evaluate, Category::SemanticFailure) => {
            response.payload.is_empty()
                && response.digests[..5].iter().all(Option::is_some)
                && response.digests[5].is_none()
                && response.usage.is_some()
        }
        (Operation::Evaluate, Category::LimitExhaustion) => {
            response.payload.is_empty()
                && response.digests[..3].iter().all(Option::is_some)
                && response.digests[3..].iter().all(Option::is_none)
                && response.usage.is_some()
        }
        (Operation::Evaluate, Category::RequestRefusal) => {
            response.payload.is_empty()
                && response.digests[4..].iter().all(Option::is_none)
                && response.usage.is_none()
        }
        (Operation::Evaluate, Category::EngineFault) => {
            response.payload.is_empty() && response.usage.is_some()
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(RunError::ContractViolation(
            "response violates the stable outcome envelope",
        ))
    }
}
