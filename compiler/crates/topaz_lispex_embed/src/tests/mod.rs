//! Contract tests for bounded application, codec, limits, and runtime behavior.
//! Shared payloads and quota fixtures live at this boundary so each family runs
//! against the same admitted evaluator identity.

mod application;
mod codec;
mod limits;
mod runtime;

use crate::report::canonical_json;
use crate::*;

pub(super) const SOURCE: &[u8] = b"(if (< 10 15) \"allow\" \"deny\")\n";
pub(super) const LOOP_SOURCE: &[u8] =
    b"((lambda (repeat) (repeat repeat)) (lambda (repeat) (repeat repeat)))\n";
pub(super) const INPUT: &[u8] = &[0];
pub(super) const RESULT_HEX: &str = "090000000000000001080000000000000005616c6c6f77";

pub(super) fn limits_json() -> String {
    canonical_json(crate::report::limits_json(Limits::MAXIMUM))
}

pub(super) fn application_quotas() -> ApplicationQuotas {
    ApplicationQuotas {
        concurrent_evaluations: 2,
        queued_evaluations: 2,
        total_evaluations: 16,
        aggregate_input_bytes: 65_536,
        aggregate_result_bytes: 16_000_000,
        aggregate_output_bytes: 16_000_000,
        aggregate_transcript_bytes: 16_000_000,
        aggregate_safety_fuel: 16 * SAFETY_FUEL,
        prepared_bytes: 1_000_000,
        wall_millis: 5_000,
    }
}

pub(super) fn prepared(source: &[u8]) -> PreparedRule {
    let runtime = ReusableRuntime::embedded().expect("runtime");
    match runtime
        .prepare(source, Limits::MAXIMUM.prepare)
        .expect("prepare")
    {
        PrepareOutcome::Prepared(prepared) => *prepared,
        PrepareOutcome::LimitExhaustion(_) => panic!("unexpected preparation exhaustion"),
    }
}

pub(super) fn canonical_field(tag: u8, bytes: &[u8]) -> Vec<u8> {
    let mut value = vec![tag];
    value.extend((bytes.len() as u64).to_be_bytes());
    value.extend(bytes);
    value
}

pub(super) fn canonical_sequence(tag: u8, values: &[Vec<u8>]) -> Vec<u8> {
    let mut value = vec![tag];
    value.extend((values.len() as u64).to_be_bytes());
    for item in values {
        value.extend(item);
    }
    value
}

pub(super) fn canonical_record(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut value = vec![13];
    value.extend((entries.len() as u64).to_be_bytes());
    for (key, item) in entries {
        value.extend((key.len() as u64).to_be_bytes());
        value.extend(key.as_bytes());
        value.extend(item);
    }
    value
}

#[cfg(feature = "full-profile-contract")]
pub(super) fn full_evaluate_request_inputs(request: &[u8]) -> (Vec<u8>, EvaluateLimits) {
    assert_eq!(
        request.get(..8),
        Some(crate::protocol::EVALUATE_MAGIC.as_slice())
    );
    let mut offset = 8;
    let field = |bytes: &[u8], offset: &mut usize| {
        let length = u32::from_be_bytes(
            bytes[*offset..*offset + 4]
                .try_into()
                .expect("full request field length"),
        ) as usize;
        *offset += 4;
        let value = bytes[*offset..*offset + length].to_vec();
        *offset += length;
        value
    };
    let _prepared = field(request, &mut offset);
    let input = field(request, &mut offset);
    let mut values = [0_u64; 10];
    for value in &mut values {
        *value = u64::from_be_bytes(
            request[offset..offset + 8]
                .try_into()
                .expect("full request limit"),
        );
        offset += 8;
    }
    assert_eq!(offset, request.len());
    (
        input,
        EvaluateLimits {
            canonical_input_bytes: values[0],
            eval_work: values[1],
            logical_allocation: values[2],
            semantic_frames: values[3],
            traversal_depth: values[4],
            output_bytes: values[5],
            diagnostic_bytes: values[6],
            transcript_bytes: values[7],
            transcript_events: values[8],
            result_bytes: values[9],
        },
    )
}
