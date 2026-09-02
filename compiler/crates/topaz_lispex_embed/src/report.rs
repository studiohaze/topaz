use crate::runtime::verify_embedded_evaluator;
use crate::runtime::{Operation, ReportInputs};
use crate::*;

/// Reports the admitted evaluator, profile, limits, and contract identities as JSON.
pub fn info_json() -> Result<String, RunError> {
    verify_embedded_evaluator()?;
    let _ = runtime()?;
    Ok(canonical_json(object([
        (
            "admission",
            object([
                ("contractPinsSha256", string(CONTRACT_PINS_SHA256)),
                ("handoffSha256", string(PROVIDER_INPUT_SHA256)),
                ("intakeDispositionSha256", string(INTAKE_DISPOSITION_SHA256)),
                (
                    "providerVerificationSha256",
                    string(PROVIDER_VERIFICATION_SHA256),
                ),
                ("releaseAuthority", boolean(RELEASE_AUTHORITY)),
                ("scope", string("release")),
            ]),
        ),
        (
            "component",
            object([
                ("artifactSha256", string(EVALUATOR_SHA256)),
                ("id", string(COMPONENT_ID)),
                ("manifestSha256", string(COMPONENT_MANIFEST_SHA256)),
            ]),
        ),
        (
            "contract",
            object([
                ("id", string(CONTRACT_ID)),
                ("manifestSha256", string(CONTRACT_MANIFEST_SHA256)),
            ]),
        ),
        ("discovery", boolean(false)),
        ("fallback", boolean(false)),
        ("portableProviderReceipt", boolean(false)),
        (
            "runtime",
            object([
                ("id", string(RUNTIME_ID)),
                ("policyId", string(RUNTIME_POLICY_ID)),
                ("policySha256", string(RUNTIME_POLICY_SHA256)),
                ("safetyFuel", number(SAFETY_FUEL)),
                ("target", string(TARGET)),
            ]),
        ),
        ("schema", string(INFO_SCHEMA)),
        (
            "semantic",
            object([
                ("abiId", string(ABI_ID)),
                ("costModelId", string(MODEL_ID)),
                ("profileId", string(PROFILE_ID)),
                ("valueCodecId", string(VALUE_CODEC_ID)),
            ]),
        ),
        ("selectors", boolean(false)),
    ])))
}

pub(crate) fn build_report(inputs: ReportInputs<'_>) -> RunRecord {
    let ReportInputs {
        source,
        input,
        limits,
        prepare_request_sha256,
        prepare_code,
        evaluate,
        safety_fuel,
    } = inputs;
    let (category, operation, code, result, fresh_instances) = match evaluate {
        None => (
            SettledCategory::LimitExhaustion,
            Operation::Prepare,
            prepare_code.to_string(),
            None,
            1,
        ),
        Some(response) => (
            response.category,
            Operation::Evaluate,
            response.code.clone(),
            response.result.clone(),
            2,
        ),
    };
    let result_sha256 = result.as_deref().map(sha256_hex);
    let evaluate_json = match evaluate {
        Some(response) => object([
            ("code", string(&response.code)),
            ("requestSha256", string(&response.request_sha256)),
            ("usage", usage_json(response.usage)),
        ]),
        _ => JsonValue::Null,
    };
    let report = object([
        (
            "admission",
            object([
                ("adapterId", string(ADAPTER_ID)),
                ("contractPinsSha256", string(CONTRACT_PINS_SHA256)),
                ("contractManifestSha256", string(CONTRACT_MANIFEST_SHA256)),
                ("handoffSha256", string(PROVIDER_INPUT_SHA256)),
                ("intakeDispositionSha256", string(INTAKE_DISPOSITION_SHA256)),
                (
                    "providerVerificationSha256",
                    string(PROVIDER_VERIFICATION_SHA256),
                ),
                ("releaseAuthority", boolean(RELEASE_AUTHORITY)),
                ("scope", string("release")),
            ]),
        ),
        (
            "execution",
            object([
                ("freshInstances", number(fresh_instances)),
                ("runtimeId", string(RUNTIME_ID)),
                ("runtimePolicyId", string(RUNTIME_POLICY_ID)),
                ("runtimePolicySha256", string(RUNTIME_POLICY_SHA256)),
                ("safetyFuel", number(safety_fuel)),
                ("target", string(TARGET)),
            ]),
        ),
        (
            "operations",
            object([
                ("evaluate", evaluate_json),
                (
                    "prepare",
                    object([
                        ("code", string(prepare_code)),
                        ("requestSha256", string(prepare_request_sha256)),
                    ]),
                ),
            ]),
        ),
        (
            "outcome",
            object([
                ("category", string(category.as_str())),
                ("code", string(&code)),
                ("operation", string(operation.as_str())),
            ]),
        ),
        ("portableProviderReceipt", boolean(false)),
        ("schema", string(REPORT_SCHEMA)),
        (
            "semantic",
            object([
                ("abiId", string(ABI_ID)),
                ("componentArtifactSha256", string(EVALUATOR_SHA256)),
                ("componentId", string(COMPONENT_ID)),
                ("costModelId", string(MODEL_ID)),
                ("inputSha256", string(&sha256_hex(input))),
                ("limits", limits_json(limits)),
                ("profileId", string(PROFILE_ID)),
                (
                    "resultSha256",
                    result_sha256
                        .as_deref()
                        .map(string)
                        .unwrap_or(JsonValue::Null),
                ),
                ("sourceSha256", string(&sha256_hex(source))),
                ("valueCodecId", string(VALUE_CODEC_ID)),
            ]),
        ),
    ]);
    RunRecord {
        category,
        operation: operation.as_str(),
        code,
        result,
        report_json: canonical_json(report),
        fresh_instances,
    }
}

pub(crate) fn limits_json(limits: Limits) -> JsonValue {
    object([
        (
            "evaluate",
            object([
                (
                    "canonical_input_bytes",
                    number(limits.evaluate.canonical_input_bytes),
                ),
                ("diagnostic_bytes", number(limits.evaluate.diagnostic_bytes)),
                ("eval_work", number(limits.evaluate.eval_work)),
                (
                    "logical_allocation",
                    number(limits.evaluate.logical_allocation),
                ),
                ("output_bytes", number(limits.evaluate.output_bytes)),
                ("result_bytes", number(limits.evaluate.result_bytes)),
                ("semantic_frames", number(limits.evaluate.semantic_frames)),
                ("transcript_bytes", number(limits.evaluate.transcript_bytes)),
                (
                    "transcript_events",
                    number(limits.evaluate.transcript_events),
                ),
                ("traversal_depth", number(limits.evaluate.traversal_depth)),
            ]),
        ),
        (
            "prepare",
            object([
                (
                    "logical_allocation",
                    number(limits.prepare.logical_allocation),
                ),
                ("prepare_work", number(limits.prepare.prepare_work)),
                ("raw_source_bytes", number(limits.prepare.raw_source_bytes)),
                ("syntax_depth", number(limits.prepare.syntax_depth)),
            ]),
        ),
        ("schema", string(LIMITS_SCHEMA)),
    ])
}

fn usage_json(usage: Option<[u64; 9]>) -> JsonValue {
    let Some(usage) = usage else {
        return JsonValue::Null;
    };
    object([
        ("diagnostic_bytes", number(usage[5])),
        ("eval_work", number(usage[0])),
        ("logical_allocation", number(usage[1])),
        ("output_bytes", number(usage[4])),
        ("result_bytes", number(usage[8])),
        ("semantic_frames", number(usage[2])),
        ("transcript_bytes", number(usage[6])),
        ("transcript_events", number(usage[7])),
        ("traversal_depth", number(usage[3])),
    ])
}

fn object<const N: usize>(entries: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(Rc::new(
        entries
            .into_iter()
            .map(|(key, value)| (Rc::<str>::from(key), value))
            .collect(),
    ))
}

fn string(value: &str) -> JsonValue {
    JsonValue::String(Rc::<str>::from(value))
}

fn boolean(value: bool) -> JsonValue {
    JsonValue::Bool(value)
}

fn number(value: impl ToString) -> JsonValue {
    let lexeme = value.to_string();
    JsonValue::Number(JsonNumber {
        int: lexeme.parse::<i64>().ok(),
        lexeme: Rc::<str>::from(lexeme),
    })
}

pub(crate) fn canonical_json(value: JsonValue) -> String {
    let mut output = String::new();
    write_json_node(&mut output, &value);
    output
}
