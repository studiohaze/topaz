use sha2::{Digest, Sha256};
use std::fmt;

use super::{ABI_ID, EVALUATOR_SHA256, MODEL_ID, PROFILE_ID, VALUE_CODEC_ID, validate_value};
use crate::protocol::{parse_response, verify_response_contract};
use crate::runtime::Operation;

const ARTIFACT_MAGIC: &[u8; 8] = b"LPXART01";
const FEATURE_SET_SHA256: &str = "c7ac2d3037b43dd90889467aabdcd2d3c061559bde12bb8330d878886c5ab429";
const TRANSCRIPT_ID: &str = "lispex.embed-transcript/v1";
const RECEIPT_ID: &str = "lispex.embed-receipt-core/v1";
const MAX_FIELD_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Selects the wire's required limit and identity slot counts.
pub enum ArtifactKind {
    Prepare,
    Evaluate,
}

impl ArtifactKind {
    const fn tag(self) -> u8 {
        match self {
            Self::Prepare => 1,
            Self::Evaluate => 2,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, ArtifactError> {
        match tag {
            1 => Ok(Self::Prepare),
            2 => Ok(Self::Evaluate),
            _ => Err(ArtifactError::new("artifact-kind")),
        }
    }

    const fn limit_count(self) -> usize {
        match self {
            Self::Prepare => 4,
            Self::Evaluate => 14,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Keeps semantic failure distinct from refusal and engine failure on the wire.
pub enum ArtifactCategory {
    Prepared,
    Complete,
    SemanticFailure,
    LimitExhaustion,
    RequestRefusal,
    EngineFault,
}

impl ArtifactCategory {
    const fn tag(self) -> u8 {
        match self {
            Self::Prepared => 0,
            Self::Complete => 1,
            Self::SemanticFailure => 2,
            Self::LimitExhaustion => 3,
            Self::RequestRefusal => 4,
            Self::EngineFault => 5,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, ArtifactError> {
        match tag {
            0 => Ok(Self::Prepared),
            1 => Ok(Self::Complete),
            2 => Ok(Self::SemanticFailure),
            3 => Ok(Self::LimitExhaustion),
            4 => Ok(Self::RequestRefusal),
            5 => Ok(Self::EngineFault),
            _ => Err(ArtifactError::new("artifact-category")),
        }
    }

    const fn core_required(self, kind: ArtifactKind) -> bool {
        matches!(
            (kind, self),
            (
                ArtifactKind::Prepare,
                Self::LimitExhaustion | Self::RequestRefusal
            ) | (
                ArtifactKind::Evaluate,
                Self::Complete
                    | Self::SemanticFailure
                    | Self::LimitExhaustion
                    | Self::RequestRefusal
            )
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Makes no issuer claim; locking relies only on verified bytes and embedded identities.
pub struct ConsumerArtifact {
    pub kind: ArtifactKind,
    pub category: ArtifactCategory,
    pub evaluator_sha256: String,
    pub exact_limits: Vec<u64>,
    pub identities: [Option<String>; 6],
    pub response: Vec<u8>,
    pub portable_core: Vec<u8>,
}

/// Stable, non-authoritative facts projected from a fully verified consumer
/// artifact. This projection is intentionally smaller than the portable core
/// and carries no issuer or admission claim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsumerArtifactInspection {
    pub kind: ArtifactKind,
    pub category: ArtifactCategory,
    pub evaluator_sha256: String,
    pub semantic_profile_id: Option<String>,
    pub artifact_sha256: String,
    pub artifact_bytes: u64,
    pub portable_core_sha256: Option<String>,
    pub portable_core_bytes: u64,
    pub authenticated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Closed codes avoid deriving contract meaning from display text.
pub struct ArtifactError {
    code: &'static str,
}

impl ArtifactError {
    const fn new(code: &'static str) -> Self {
        Self { code }
    }

    #[must_use]
    pub const fn code(self) -> &'static str {
        self.code
    }
}

impl fmt::Display for ArtifactError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Lispex consumer artifact refused: {}", self.code)
    }
}

impl std::error::Error for ArtifactError {}

/// Decodes the bounded artifact envelope and validates every encoded field.
pub fn decode_artifact(bytes: &[u8]) -> Result<ConsumerArtifact, ArtifactError> {
    let mut cursor = Cursor::new(bytes);
    if cursor.take(8)? != ARTIFACT_MAGIC {
        return Err(ArtifactError::new("artifact-magic"));
    }
    let kind = ArtifactKind::from_tag(cursor.byte()?)?;
    let category = ArtifactCategory::from_tag(cursor.byte()?)?;
    let evaluator_sha256 = lower_hex(cursor.take(64)?, "artifact-evaluator")?;
    let limit_count = usize::from(cursor.byte()?);
    if limit_count != kind.limit_count() {
        return Err(ArtifactError::new("artifact-limit-count"));
    }
    let mut exact_limits = Vec::with_capacity(limit_count);
    for _ in 0..limit_count {
        exact_limits.push(cursor.u64()?);
    }
    let mut identities: [Option<String>; 6] = std::array::from_fn(|_| None);
    for identity in &mut identities {
        *identity = match cursor.byte()? {
            0 => None,
            1 => Some(lower_hex(cursor.take(64)?, "artifact-identity")?),
            _ => return Err(ArtifactError::new("artifact-identity-tag")),
        };
    }
    let response = cursor.field()?.to_vec();
    let portable_core = cursor.field()?.to_vec();
    if !cursor.done() {
        return Err(ArtifactError::new("artifact-trailing"));
    }
    let artifact = ConsumerArtifact {
        kind,
        category,
        evaluator_sha256,
        exact_limits,
        identities,
        response,
        portable_core,
    };
    verify_decoded(&artifact)?;
    Ok(artifact)
}

/// Encodes an admitted artifact in its single canonical binary form.
pub fn encode_artifact(artifact: &ConsumerArtifact) -> Result<Vec<u8>, ArtifactError> {
    verify_decoded(artifact)?;
    let mut output = Vec::new();
    output.extend_from_slice(ARTIFACT_MAGIC);
    output.push(artifact.kind.tag());
    output.push(artifact.category.tag());
    output.extend_from_slice(artifact.evaluator_sha256.as_bytes());
    output.push(
        u8::try_from(artifact.exact_limits.len())
            .map_err(|_| ArtifactError::new("artifact-limit-count"))?,
    );
    for limit in &artifact.exact_limits {
        output.extend_from_slice(&limit.to_be_bytes());
    }
    for identity in &artifact.identities {
        match identity {
            None => output.push(0),
            Some(identity) => {
                lower_hex(identity.as_bytes(), "artifact-identity")?;
                output.push(1);
                output.extend_from_slice(identity.as_bytes());
            }
        }
    }
    push_field(&mut output, &artifact.response)?;
    push_field(&mut output, &artifact.portable_core)?;
    Ok(output)
}

/// Requires the artifact to decode and round-trip to identical bytes.
pub fn verify_artifact(bytes: &[u8]) -> Result<(), ArtifactError> {
    let artifact = decode_artifact(bytes)?;
    if encode_artifact(&artifact)? != bytes {
        return Err(ArtifactError::new("artifact-noncanonical"));
    }
    Ok(())
}

/// Projects non-authoritative metadata from a fully verified artifact.
pub fn inspect_artifact(bytes: &[u8]) -> Result<ConsumerArtifactInspection, ArtifactError> {
    verify_artifact(bytes)?;
    let artifact = decode_artifact(bytes)?;
    let semantic_profile_id = matches!(
        (artifact.kind, artifact.category),
        (ArtifactKind::Prepare, ArtifactCategory::Prepared)
            | (
                ArtifactKind::Evaluate,
                ArtifactCategory::Complete
                    | ArtifactCategory::SemanticFailure
                    | ArtifactCategory::LimitExhaustion
            )
    )
    .then(|| PROFILE_ID.to_string());
    let portable_core_sha256 = (!artifact.portable_core.is_empty())
        .then(|| hex_lower(&Sha256::digest(&artifact.portable_core)));
    Ok(ConsumerArtifactInspection {
        kind: artifact.kind,
        category: artifact.category,
        evaluator_sha256: artifact.evaluator_sha256,
        semantic_profile_id,
        artifact_sha256: hex_lower(&Sha256::digest(bytes)),
        artifact_bytes: u64::try_from(bytes.len())
            .map_err(|_| ArtifactError::new("artifact-field-size"))?,
        portable_core_sha256,
        portable_core_bytes: u64::try_from(artifact.portable_core.len())
            .map_err(|_| ArtifactError::new("artifact-field-size"))?,
        authenticated: false,
    })
}

/// Wraps an exact prepare request and response as a bounded consumer artifact.
pub fn wrap_prepare_artifact(
    response_bytes: &[u8],
    request_bytes: &[u8],
    exact_limits: [u64; 4],
) -> Result<Vec<u8>, ArtifactError> {
    let response =
        parse_response(response_bytes).map_err(|_| ArtifactError::new("artifact-response"))?;
    verify_response_contract(&response)
        .map_err(|_| ArtifactError::new("artifact-response-contract"))?;
    if response.operation != Operation::Prepare {
        return Err(ArtifactError::new("artifact-operation"));
    }
    let category = category_from_response(response.category);
    let mut identities: [Option<String>; 6] = std::array::from_fn(|_| None);
    if category == ArtifactCategory::Prepared {
        identities[..5].clone_from_slice(&response.digests[..5]);
    }
    identities[5] = Some(hex_lower(&framed_hash(
        "lispex.embedding.submission/v1",
        &[request_bytes],
    )?));
    let mut artifact = ConsumerArtifact {
        kind: ArtifactKind::Prepare,
        category,
        evaluator_sha256: EVALUATOR_SHA256.into(),
        exact_limits: exact_limits.to_vec(),
        identities,
        response: response_bytes.to_vec(),
        portable_core: Vec::new(),
    };
    artifact.portable_core = portable_core_for_artifact(&artifact)?;
    encode_artifact(&artifact)
}

/// Binds an evaluation response to its request and prepared input artifact.
pub fn wrap_evaluate_artifact(
    response_bytes: &[u8],
    request_bytes: &[u8],
    prepared_artifact_bytes: &[u8],
    evaluation_limits: [u64; 10],
) -> Result<Vec<u8>, ArtifactError> {
    let prepared = decode_artifact(prepared_artifact_bytes)?;
    if prepared.kind != ArtifactKind::Prepare || prepared.category != ArtifactCategory::Prepared {
        return Err(ArtifactError::new("artifact-prepared-input"));
    }
    let response =
        parse_response(response_bytes).map_err(|_| ArtifactError::new("artifact-response"))?;
    verify_response_contract(&response)
        .map_err(|_| ArtifactError::new("artifact-response-contract"))?;
    if response.operation != Operation::Evaluate {
        return Err(ArtifactError::new("artifact-operation"));
    }
    let category = category_from_response(response.category);
    if matches!(
        category,
        ArtifactCategory::Complete
            | ArtifactCategory::SemanticFailure
            | ArtifactCategory::LimitExhaustion
    ) && (response.digests[0] != prepared.identities[3]
        || response.digests[1] != prepared.identities[4])
    {
        return Err(ArtifactError::new("artifact-prepared-binding"));
    }
    let mut identities = prepared.identities;
    identities[5] = Some(hex_lower(&framed_hash(
        "lispex.embedding.submission/v1",
        &[request_bytes],
    )?));
    let mut exact_limits = prepared.exact_limits;
    exact_limits.extend_from_slice(&evaluation_limits);
    let mut artifact = ConsumerArtifact {
        kind: ArtifactKind::Evaluate,
        category,
        evaluator_sha256: EVALUATOR_SHA256.into(),
        exact_limits,
        identities,
        response: response_bytes.to_vec(),
        portable_core: Vec::new(),
    };
    artifact.portable_core = portable_core_for_artifact(&artifact)?;
    encode_artifact(&artifact)
}

/// Derives the path-neutral semantic core for a bounded-profile artifact.
pub fn portable_core_for_artifact(artifact: &ConsumerArtifact) -> Result<Vec<u8>, ArtifactError> {
    portable_core_for_identity(
        artifact,
        EVALUATOR_SHA256,
        PROFILE_ID,
        FEATURE_SET_SHA256,
        MODEL_ID,
    )
}

pub(super) fn portable_core_for_identity(
    artifact: &ConsumerArtifact,
    evaluator_sha256: &str,
    profile_id: &str,
    feature_set_sha256: &str,
    model_id: &str,
) -> Result<Vec<u8>, ArtifactError> {
    let response =
        parse_response(&artifact.response).map_err(|_| ArtifactError::new("artifact-response"))?;
    verify_response_contract(&response)
        .map_err(|_| ArtifactError::new("artifact-response-contract"))?;
    match (artifact.kind, artifact.category) {
        (ArtifactKind::Prepare, ArtifactCategory::LimitExhaustion) => {
            prepare_exhaustion_core(artifact, &response.code, evaluator_sha256)
        }
        (ArtifactKind::Prepare | ArtifactKind::Evaluate, ArtifactCategory::RequestRefusal) => {
            refusal_core(artifact, &response.code, evaluator_sha256)
        }
        (
            ArtifactKind::Evaluate,
            ArtifactCategory::Complete
            | ArtifactCategory::SemanticFailure
            | ArtifactCategory::LimitExhaustion,
        ) => semantic_evaluation_core(
            artifact,
            &response,
            evaluator_sha256,
            profile_id,
            feature_set_sha256,
            model_id,
        ),
        (ArtifactKind::Prepare, ArtifactCategory::Prepared | ArtifactCategory::EngineFault)
        | (ArtifactKind::Evaluate, ArtifactCategory::EngineFault) => Ok(Vec::new()),
        _ => Err(ArtifactError::new("artifact-kind-category")),
    }
}

fn verify_decoded(artifact: &ConsumerArtifact) -> Result<(), ArtifactError> {
    if artifact.evaluator_sha256 != EVALUATOR_SHA256 {
        return Err(ArtifactError::new("artifact-evaluator"));
    }
    if artifact.exact_limits.len() != artifact.kind.limit_count() {
        return Err(ArtifactError::new("artifact-limit-count"));
    }
    if artifact.response.len() > MAX_FIELD_BYTES || artifact.portable_core.len() > MAX_FIELD_BYTES {
        return Err(ArtifactError::new("artifact-field-size"));
    }
    let response =
        parse_response(&artifact.response).map_err(|_| ArtifactError::new("artifact-response"))?;
    verify_response_contract(&response)
        .map_err(|_| ArtifactError::new("artifact-response-contract"))?;
    let operation_matches = matches!(
        (artifact.kind, response.operation),
        (ArtifactKind::Prepare, Operation::Prepare) | (ArtifactKind::Evaluate, Operation::Evaluate)
    );
    let category_matches = matches!(
        (artifact.category, response.category),
        (ArtifactCategory::Prepared, super::Category::Prepared)
            | (ArtifactCategory::Complete, super::Category::Complete)
            | (
                ArtifactCategory::SemanticFailure,
                super::Category::SemanticFailure
            )
            | (
                ArtifactCategory::LimitExhaustion,
                super::Category::LimitExhaustion
            )
            | (
                ArtifactCategory::RequestRefusal,
                super::Category::RequestRefusal
            )
            | (ArtifactCategory::EngineFault, super::Category::EngineFault)
    );
    if !operation_matches || !category_matches {
        return Err(ArtifactError::new("artifact-category-binding"));
    }
    match artifact.kind {
        ArtifactKind::Prepare => match artifact.category {
            ArtifactCategory::Prepared => {
                if artifact.identities.iter().any(Option::is_none)
                    || artifact.identities[..5] != response.digests[..5]
                {
                    return Err(ArtifactError::new("artifact-identity-binding"));
                }
            }
            ArtifactCategory::LimitExhaustion | ArtifactCategory::RequestRefusal => {
                if artifact.identities[..5].iter().any(Option::is_some)
                    || artifact.identities[5].is_none()
                {
                    return Err(ArtifactError::new("artifact-identity-binding"));
                }
            }
            ArtifactCategory::EngineFault => {}
            _ => return Err(ArtifactError::new("artifact-kind-category")),
        },
        ArtifactKind::Evaluate => {
            if matches!(
                artifact.category,
                ArtifactCategory::Complete
                    | ArtifactCategory::SemanticFailure
                    | ArtifactCategory::LimitExhaustion
            ) && (artifact.identities.iter().any(Option::is_none)
                || artifact.identities[3] != response.digests[0]
                || artifact.identities[4] != response.digests[1])
            {
                return Err(ArtifactError::new("artifact-identity-binding"));
            }
        }
    }
    if artifact.identities[4]
        .as_deref()
        .is_some_and(|identity| identity != FEATURE_SET_SHA256)
    {
        return Err(ArtifactError::new("artifact-feature-set"));
    }
    let expected_core = portable_core_for_artifact(artifact)?;
    if artifact.category.core_required(artifact.kind) {
        validate_value(&artifact.portable_core, true)
            .map_err(|_| ArtifactError::new("artifact-portable-core-codec"))?;
    }
    if expected_core != artifact.portable_core {
        return Err(ArtifactError::new("artifact-portable-core"));
    }
    Ok(())
}

const fn category_from_response(category: super::Category) -> ArtifactCategory {
    match category {
        super::Category::Prepared => ArtifactCategory::Prepared,
        super::Category::Complete => ArtifactCategory::Complete,
        super::Category::SemanticFailure => ArtifactCategory::SemanticFailure,
        super::Category::LimitExhaustion => ArtifactCategory::LimitExhaustion,
        super::Category::RequestRefusal => ArtifactCategory::RequestRefusal,
        super::Category::EngineFault => ArtifactCategory::EngineFault,
    }
}

fn semantic_evaluation_core(
    artifact: &ConsumerArtifact,
    response: &super::Response,
    evaluator_sha256: &str,
    profile_id: &str,
    feature_set_sha256: &str,
    model_id: &str,
) -> Result<Vec<u8>, ArtifactError> {
    if artifact.exact_limits.len() != 14 {
        return Err(ArtifactError::new("artifact-limit-count"));
    }
    let submitted = required_identity(&artifact.identities[0])?;
    let canonical_source = required_identity(&artifact.identities[1])?;
    let semantic_rule = required_identity(&artifact.identities[2])?;
    let feature_set = required_identity(&artifact.identities[4])?;
    if feature_set != feature_set_sha256 {
        return Err(ArtifactError::new("artifact-feature-set"));
    }
    let canonical_input = required_identity(&response.digests[2])?;
    let exact_limits = resource_contract(&artifact.exact_limits)?;
    let resource_bytes = encode_value(&exact_limits)?;
    let resource_digest = framed_hash("lispex.embedding.resource-contract/v1", &[&resource_bytes])?;
    let semantic_rule_raw = digest_bytes(semantic_rule)?;
    let canonical_input_raw = digest_bytes(canonical_input)?;
    let request_digest = framed_hash(
        "lispex.embedding.request/v1",
        &[
            &semantic_rule_raw,
            &canonical_input_raw,
            &resource_digest,
            VALUE_CODEC_ID.as_bytes(),
            TRANSCRIPT_ID.as_bytes(),
            RECEIPT_ID.as_bytes(),
        ],
    )?;
    let evaluator_raw = digest_bytes(evaluator_sha256)?;
    let evaluation_digest = framed_hash(
        "lispex.embedding.evaluation/v1",
        &[&request_digest, &evaluator_raw, ABI_ID.as_bytes()],
    )?;
    let empty_transcript = encode_value(&Value::List(Vec::new()))?;
    let transcript_digest = framed_hash("lispex.embedding.transcript/v1", &[&empty_transcript])?;

    let (diagnostic, outcome, exhausted_axis) = match artifact.category {
        ArtifactCategory::Complete => {
            validate_value(&response.payload, false)
                .map_err(|_| ArtifactError::new("artifact-result-value"))?;
            (
                Value::Nil,
                Value::record([
                    ("kind", Value::Symbol("values".into())),
                    ("values", Value::Encoded(response.payload.clone())),
                ]),
                Value::Nil,
            )
        }
        ArtifactCategory::SemanticFailure => (
            Value::String(response.code.clone()),
            Value::record([
                ("kind", Value::Symbol("runtime-diagnostic".into())),
                ("code", Value::String(response.code.clone())),
            ]),
            Value::Nil,
        ),
        ArtifactCategory::LimitExhaustion => (
            Value::Nil,
            Value::record([("kind", Value::Symbol("resource-exhausted".into()))]),
            Value::Symbol(response.code.clone()),
        ),
        _ => return Err(ArtifactError::new("artifact-core-category")),
    };
    let result = Value::record([
        ("category", Value::Symbol("semantic".into())),
        ("diagnostic", diagnostic),
        ("exact_limits", exact_limits.clone()),
        ("exhausted_axis", exhausted_axis.clone()),
        ("outcome", outcome.clone()),
        ("transcript", Value::Encoded(empty_transcript)),
    ]);
    let result_bytes = encode_value(&result)?;
    let result_digest = framed_hash("lispex.embedding.result/v1", &[&result_bytes])?;

    encode_value(&Value::record([
        ("schema", Value::String(RECEIPT_ID.into())),
        (
            "category",
            Value::Symbol("deterministic-semantic-outcome".into()),
        ),
        ("semantic_profile_id", Value::String(profile_id.into())),
        ("feature_set_sha256", Value::String(feature_set.into())),
        ("model_id", Value::String(model_id.into())),
        ("exact_limits", exact_limits),
        ("submitted_source_sha256", Value::String(submitted.into())),
        (
            "canonical_input_sha256",
            Value::String(canonical_input.into()),
        ),
        (
            "canonical_source_sha256",
            Value::String(canonical_source.into()),
        ),
        ("semantic_rule_sha256", Value::String(semantic_rule.into())),
        ("request_sha256", Value::String(hex_lower(&request_digest))),
        (
            "engine_artifact_sha256",
            Value::String(evaluator_sha256.into()),
        ),
        (
            "evaluation_sha256",
            Value::String(hex_lower(&evaluation_digest)),
        ),
        ("outcome", outcome),
        ("exhausted_axis", exhausted_axis),
        (
            "transcript_sha256",
            Value::String(hex_lower(&transcript_digest)),
        ),
        ("result_sha256", Value::String(hex_lower(&result_digest))),
    ]))
}

fn prepare_exhaustion_core(
    artifact: &ConsumerArtifact,
    code: &str,
    evaluator_sha256: &str,
) -> Result<Vec<u8>, ArtifactError> {
    if artifact.exact_limits.len() != 4 {
        return Err(ArtifactError::new("artifact-limit-count"));
    }
    let submission = required_identity(&artifact.identities[5])?;
    let available = Value::record([
        ("raw_source_bytes", Value::Integer(artifact.exact_limits[0])),
        ("prepare_work", Value::Integer(artifact.exact_limits[1])),
        (
            "prepare_logical_allocation",
            Value::Integer(artifact.exact_limits[2]),
        ),
        ("syntax_depth", Value::Integer(artifact.exact_limits[3])),
    ]);
    refusal_core_value(submission, code, available, evaluator_sha256)
}

fn refusal_core(
    artifact: &ConsumerArtifact,
    code: &str,
    evaluator_sha256: &str,
) -> Result<Vec<u8>, ArtifactError> {
    let submission = required_identity(&artifact.identities[5])?;
    refusal_core_value(submission, code, Value::record([]), evaluator_sha256)
}

fn refusal_core_value(
    submission: &str,
    code: &str,
    available: Value,
    evaluator_sha256: &str,
) -> Result<Vec<u8>, ArtifactError> {
    encode_value(&Value::record([
        ("schema", Value::String(RECEIPT_ID.into())),
        (
            "category",
            Value::Symbol("deterministic-request-refusal".into()),
        ),
        ("submission_sha256", Value::String(submission.into())),
        (
            "engine_artifact_sha256",
            Value::String(evaluator_sha256.into()),
        ),
        ("refusal_code", Value::String(code.into())),
        ("available_field_identities", available),
    ]))
}

fn resource_contract(limits: &[u64]) -> Result<Value, ArtifactError> {
    if limits.len() != 14 {
        return Err(ArtifactError::new("artifact-limit-count"));
    }
    Ok(Value::record([
        ("raw_source_bytes", Value::Integer(limits[0])),
        ("prepare_work", Value::Integer(limits[1])),
        ("prepare_logical_allocation", Value::Integer(limits[2])),
        ("syntax_depth", Value::Integer(limits[3])),
        ("canonical_input_bytes", Value::Integer(limits[4])),
        ("eval_work", Value::Integer(limits[5])),
        ("eval_logical_allocation", Value::Integer(limits[6])),
        ("semantic_frames", Value::Integer(limits[7])),
        ("traversal_depth", Value::Integer(limits[8])),
        ("output_bytes", Value::Integer(limits[9])),
        ("diagnostic_bytes", Value::Integer(limits[10])),
        ("transcript_bytes", Value::Integer(limits[11])),
        ("transcript_events", Value::Integer(limits[12])),
        ("result_bytes", Value::Integer(limits[13])),
    ]))
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Nil,
    Integer(u64),
    Symbol(String),
    String(String),
    List(Vec<Value>),
    Record(Vec<(String, Value)>),
    Encoded(Vec<u8>),
}

impl Value {
    fn record<const N: usize>(entries: [(&str, Value); N]) -> Self {
        Self::Record(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        )
    }
}

fn encode_value(value: &Value) -> Result<Vec<u8>, ArtifactError> {
    let mut output = Vec::new();
    encode_value_into(value, &mut output)?;
    Ok(output)
}

fn encode_value_into(value: &Value, output: &mut Vec<u8>) -> Result<(), ArtifactError> {
    match value {
        Value::Nil => output.push(0),
        Value::Integer(value) => {
            output.push(3);
            push_u64_bytes(output, value.to_string().as_bytes())?;
        }
        Value::Symbol(value) => {
            output.push(7);
            push_u64_bytes(output, value.as_bytes())?;
        }
        Value::String(value) => {
            output.push(8);
            push_u64_bytes(output, value.as_bytes())?;
        }
        Value::List(values) => {
            output.push(9);
            output.extend_from_slice(
                &u64::try_from(values.len())
                    .map_err(|_| ArtifactError::new("value-count"))?
                    .to_be_bytes(),
            );
            for value in values {
                encode_value_into(value, output)?;
            }
        }
        Value::Record(entries) => {
            let mut ordered = entries.iter().collect::<Vec<_>>();
            ordered.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
            if ordered.windows(2).any(|pair| pair[0].0 == pair[1].0) {
                return Err(ArtifactError::new("value-record-duplicate"));
            }
            output.push(13);
            output.extend_from_slice(
                &u64::try_from(ordered.len())
                    .map_err(|_| ArtifactError::new("value-count"))?
                    .to_be_bytes(),
            );
            for (key, value) in ordered {
                if key.is_empty() {
                    return Err(ArtifactError::new("value-record-key"));
                }
                push_u64_bytes(output, key.as_bytes())?;
                encode_value_into(value, output)?;
            }
        }
        Value::Encoded(value) => {
            validate_value(value, true).map_err(|_| ArtifactError::new("value-encoded"))?;
            output.extend_from_slice(value);
        }
    }
    Ok(())
}

fn framed_hash(domain: &str, fields: &[&[u8]]) -> Result<[u8; 32], ArtifactError> {
    if !domain.is_ascii() || domain.as_bytes().contains(&0) {
        return Err(ArtifactError::new("hash-domain"));
    }
    let mut hasher = Sha256::new();
    hasher.update(domain.as_bytes());
    hasher.update([0]);
    hasher.update(
        u32::try_from(fields.len())
            .map_err(|_| ArtifactError::new("hash-field-count"))?
            .to_be_bytes(),
    );
    for field in fields {
        hasher.update(
            u64::try_from(field.len())
                .map_err(|_| ArtifactError::new("hash-field-size"))?
                .to_be_bytes(),
        );
        hasher.update(field);
    }
    Ok(hasher.finalize().into())
}

pub(super) fn submission_sha256(request_bytes: &[u8]) -> Result<String, ArtifactError> {
    Ok(hex_lower(&framed_hash(
        "lispex.embedding.submission/v1",
        &[request_bytes],
    )?))
}

fn digest_bytes(value: &str) -> Result<[u8; 32], ArtifactError> {
    if value.len() != 64 {
        return Err(ArtifactError::new("artifact-digest"));
    }
    let mut output = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_digit(pair[0]).ok_or_else(|| ArtifactError::new("artifact-digest"))?;
        let low = hex_digit(pair[1]).ok_or_else(|| ArtifactError::new("artifact-digest"))?;
        output[index] = (high << 4) | low;
    }
    Ok(output)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn lower_hex(bytes: &[u8], code: &'static str) -> Result<String, ArtifactError> {
    if bytes.len() != 64 || !bytes.iter().all(|byte| hex_digit(*byte).is_some()) {
        return Err(ArtifactError::new(code));
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| ArtifactError::new(code))
}

fn required_identity(identity: &Option<String>) -> Result<&str, ArtifactError> {
    identity
        .as_deref()
        .ok_or_else(|| ArtifactError::new("artifact-identity-missing"))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[(byte >> 4) as usize]));
        output.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    output
}

fn push_field(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), ArtifactError> {
    if bytes.len() > MAX_FIELD_BYTES {
        return Err(ArtifactError::new("artifact-field-size"));
    }
    output.extend_from_slice(
        &u32::try_from(bytes.len())
            .map_err(|_| ArtifactError::new("artifact-field-size"))?
            .to_be_bytes(),
    );
    output.extend_from_slice(bytes);
    Ok(())
}

fn push_u64_bytes(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), ArtifactError> {
    output.extend_from_slice(
        &u64::try_from(bytes.len())
            .map_err(|_| ArtifactError::new("value-field-size"))?
            .to_be_bytes(),
    );
    output.extend_from_slice(bytes);
    Ok(())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn byte(&mut self) -> Result<u8, ArtifactError> {
        let byte = *self
            .bytes
            .get(self.offset)
            .ok_or_else(|| ArtifactError::new("artifact-truncated"))?;
        self.offset += 1;
        Ok(byte)
    }

    fn u32(&mut self) -> Result<u32, ArtifactError> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| ArtifactError::new("artifact-truncated"))?,
        ))
    }

    fn u64(&mut self) -> Result<u64, ArtifactError> {
        Ok(u64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| ArtifactError::new("artifact-truncated"))?,
        ))
    }

    fn field(&mut self) -> Result<&'a [u8], ArtifactError> {
        let length =
            usize::try_from(self.u32()?).map_err(|_| ArtifactError::new("artifact-field-size"))?;
        if length > MAX_FIELD_BYTES {
            return Err(ArtifactError::new("artifact-field-size"));
        }
        self.take(length)
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], ArtifactError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| ArtifactError::new("artifact-length-overflow"))?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| ArtifactError::new("artifact-truncated"))?;
        self.offset = end;
        Ok(bytes)
    }

    const fn done(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{evaluate_request, prepare_request};
    use crate::runtime::{Operation, invoke};
    use crate::{EvaluateLimits, PrepareLimits, SAFETY_FUEL};
    use std::fs;
    use std::path::{Path, PathBuf};

    const VECTOR_ROOT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/lispex-provider-intake/v1/inputs/",
        "products/embed-evaluator/handoffs/lda-c1/v1/vectors"
    );

    fn vector(name: &str) -> Vec<u8> {
        fs::read(Path::new(VECTOR_ROOT).join(name)).expect("provider vector")
    }

    #[test]
    fn predecessor_positive_artifacts_are_not_current_product_artifacts() {
        for artifact_name in [
            "prepared.lpxembed",
            "complete.lpxembed",
            "semantic-fault.lpxembed",
            "evaluation-exhausted.lpxembed",
            "preparation-exhausted.lpxembed",
            "request-refusal.lpxembed",
        ] {
            assert_eq!(
                decode_artifact(&vector(artifact_name))
                    .expect_err("predecessor evaluator identity must be rejected")
                    .code(),
                "artifact-evaluator",
                "{artifact_name}"
            );
        }
    }

    #[test]
    fn current_evaluator_responses_produce_current_consumer_artifacts() {
        let source =
            fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(
                "../../contracts/lispex-provider-intake/v1/inputs/tests/f12/embed-policy.lspx",
            ))
            .expect("source fixture");
        let input = fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../contracts/lispex-provider-intake/v1/inputs/examples/checkable-refund/generated/inputs/day-14-unopened.bin",
        ))
        .expect("input fixture");
        let prepare_limits = PrepareLimits {
            raw_source_bytes: 4096,
            prepare_work: 1_000_000,
            logical_allocation: 1_000_000,
            syntax_depth: 64,
        };
        let evaluate_limits = EvaluateLimits {
            canonical_input_bytes: 4096,
            eval_work: 1_000_000,
            logical_allocation: 1_000_000,
            semantic_frames: 1000,
            traversal_depth: 256,
            output_bytes: 1_000_000,
            diagnostic_bytes: 1_000_000,
            transcript_bytes: 1_000_000,
            transcript_events: 100,
            result_bytes: 1_000_000,
        };
        let prepare_limits_array = [4096, 1_000_000, 1_000_000, 64];
        let evaluate_limits_array = [
            4096, 1_000_000, 1_000_000, 1000, 256, 1_000_000, 1_000_000, 1_000_000, 100, 1_000_000,
        ];
        let runtime = crate::runtime::runtime().expect("runtime");
        let prepare = prepare_request(&source, prepare_limits).expect("prepare request");
        let prepare_response = invoke(runtime, Operation::Prepare, &prepare, SAFETY_FUEL, None)
            .expect("prepare response");
        let prepared = wrap_prepare_artifact(&prepare_response, &prepare, prepare_limits_array)
            .expect("prepared artifact");

        let prepared_envelope = decode_artifact(&prepared).expect("prepared envelope");
        assert_eq!(prepared_envelope.evaluator_sha256, EVALUATOR_SHA256);
        assert_eq!(prepared_envelope.category, ArtifactCategory::Prepared);
        let prepared_response =
            parse_response(&prepared_envelope.response).expect("prepared response");
        let evaluate = evaluate_request(&prepared_response.payload, &input, evaluate_limits)
            .expect("evaluate request");
        let evaluate_response = invoke(runtime, Operation::Evaluate, &evaluate, SAFETY_FUEL, None)
            .expect("evaluate response");
        let complete = wrap_evaluate_artifact(
            &evaluate_response,
            &evaluate,
            &prepared,
            evaluate_limits_array,
        )
        .expect("complete artifact");
        let inspection = inspect_artifact(&complete).expect("complete artifact inspection");
        assert_eq!(inspection.kind, ArtifactKind::Evaluate);
        assert_eq!(inspection.category, ArtifactCategory::Complete);
        assert_eq!(inspection.evaluator_sha256, EVALUATOR_SHA256);
        assert_eq!(inspection.semantic_profile_id.as_deref(), Some(PROFILE_ID));
        assert_eq!(inspection.artifact_bytes, complete.len() as u64);
        assert!(inspection.portable_core_bytes > 0);
        assert!(inspection.portable_core_sha256.is_some());
        assert!(!inspection.authenticated);
        let mut changed = complete;
        *changed.last_mut().expect("nonempty artifact") ^= 1;
        assert!(inspect_artifact(&changed).is_err());

        let fault_source = b"(car 1)\n";
        let fault_prepare = prepare_request(fault_source, prepare_limits).expect("fault prepare");
        let fault_prepare_response = invoke(
            runtime,
            Operation::Prepare,
            &fault_prepare,
            SAFETY_FUEL,
            None,
        )
        .expect("fault prepare response");
        let fault_prepared = wrap_prepare_artifact(
            &fault_prepare_response,
            &fault_prepare,
            prepare_limits_array,
        )
        .expect("fault prepared artifact");
        let fault_envelope = decode_artifact(&fault_prepared).expect("fault envelope");
        let fault_response =
            parse_response(&fault_envelope.response).expect("fault prepared response");
        let fault_request = evaluate_request(&fault_response.payload, &input, evaluate_limits)
            .expect("fault evaluate request");
        let fault_raw = invoke(
            runtime,
            Operation::Evaluate,
            &fault_request,
            SAFETY_FUEL,
            None,
        )
        .expect("fault response");
        let semantic_fault = wrap_evaluate_artifact(
            &fault_raw,
            &fault_request,
            &fault_prepared,
            evaluate_limits_array,
        )
        .expect("semantic fault artifact");
        assert_eq!(
            decode_artifact(&semantic_fault)
                .expect("semantic fault envelope")
                .category,
            ArtifactCategory::SemanticFailure
        );

        let low_evaluate_limits = EvaluateLimits {
            eval_work: 0,
            ..evaluate_limits
        };
        let low_evaluate_limits_array = [
            4096, 0, 1_000_000, 1000, 256, 1_000_000, 1_000_000, 1_000_000, 100, 1_000_000,
        ];
        let exhausted_request =
            evaluate_request(&prepared_response.payload, &input, low_evaluate_limits)
                .expect("exhausted evaluate request");
        let exhausted_response = invoke(
            runtime,
            Operation::Evaluate,
            &exhausted_request,
            SAFETY_FUEL,
            None,
        )
        .expect("exhausted response");
        let evaluation_exhausted = wrap_evaluate_artifact(
            &exhausted_response,
            &exhausted_request,
            &prepared,
            low_evaluate_limits_array,
        )
        .expect("evaluation exhausted artifact");
        assert_eq!(
            decode_artifact(&evaluation_exhausted)
                .expect("evaluation exhausted envelope")
                .category,
            ArtifactCategory::LimitExhaustion
        );

        let low_prepare_limits = PrepareLimits {
            prepare_work: 0,
            ..prepare_limits
        };
        let low_prepare_request =
            prepare_request(&source, low_prepare_limits).expect("low prepare request");
        let low_prepare_response = invoke(
            runtime,
            Operation::Prepare,
            &low_prepare_request,
            SAFETY_FUEL,
            None,
        )
        .expect("low prepare response");
        let preparation_exhausted = wrap_prepare_artifact(
            &low_prepare_response,
            &low_prepare_request,
            [4096, 0, 1_000_000, 64],
        )
        .expect("preparation exhausted artifact");
        assert_eq!(
            decode_artifact(&preparation_exhausted)
                .expect("preparation exhausted envelope")
                .category,
            ArtifactCategory::LimitExhaustion
        );

        let refusal_request =
            prepare_request(b"(", prepare_limits).expect("refusal prepare request");
        let refusal_response = invoke(
            runtime,
            Operation::Prepare,
            &refusal_request,
            SAFETY_FUEL,
            None,
        )
        .expect("refusal response");
        let refusal =
            wrap_prepare_artifact(&refusal_response, &refusal_request, prepare_limits_array)
                .expect("request refusal artifact");
        assert_eq!(
            decode_artifact(&refusal)
                .expect("request refusal envelope")
                .category,
            ArtifactCategory::RequestRefusal
        );

        let evaluate_refusal_request = b"not-an-evaluate-request";
        let evaluate_refusal_response = invoke(
            runtime,
            Operation::Evaluate,
            evaluate_refusal_request,
            SAFETY_FUEL,
            None,
        )
        .expect("evaluate refusal response");
        let evaluate_refusal = wrap_evaluate_artifact(
            &evaluate_refusal_response,
            evaluate_refusal_request,
            &prepared,
            evaluate_limits_array,
        )
        .expect("evaluate refusal artifact");
        let evaluate_refusal_envelope =
            decode_artifact(&evaluate_refusal).expect("evaluate refusal envelope");
        assert_eq!(
            evaluate_refusal_envelope.category,
            ArtifactCategory::RequestRefusal
        );
        assert!(!evaluate_refusal_envelope.portable_core.is_empty());
    }

    #[test]
    fn provider_negative_artifacts_all_fail_closed() {
        let mut paths = fs::read_dir(VECTOR_ROOT)
            .expect("vector directory")
            .map(|entry| entry.expect("vector entry").path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with("negative-") && name.ends_with(".lpxembed")
                    })
            })
            .collect::<Vec<PathBuf>>();
        paths.sort();
        assert_eq!(paths.len(), 13);
        for path in paths {
            let bytes = fs::read(&path).expect("negative vector");
            assert!(
                verify_artifact(&bytes).is_err(),
                "accepted {}",
                path.display()
            );
        }
    }

    #[test]
    fn topaz_mutation_rejects_each_envelope_boundary() {
        let original = vector("complete.lpxembed");
        let mut mutations = Vec::new();
        let mutate = |offset: usize, value: u8| {
            let mut bytes = original.clone();
            bytes[offset] = value;
            bytes
        };
        mutations.push(mutate(0, b'X'));
        mutations.push(mutate(8, 0));
        mutations.push(mutate(9, 0));
        mutations.push(mutate(10, b'0'));
        mutations.push(mutate(74, 13));
        for offset in (75..75 + 14 * 8).step_by(8) {
            mutations.push(mutate(offset + 7, original[offset + 7] ^ 1));
        }
        let first_identity_tag = 75 + 14 * 8;
        mutations.push(mutate(first_identity_tag, 2));
        mutations.push(mutate(first_identity_tag + 1, b'A'));
        mutations.push(mutate(first_identity_tag + 1, b'0'));
        let mut missing_identity = original.clone();
        missing_identity[first_identity_tag] = 0;
        missing_identity.drain(first_identity_tag + 1..first_identity_tag + 65);
        mutations.push(missing_identity);
        mutations.push(original[..original.len() - 1].to_vec());
        let mut trailing = original.clone();
        trailing.push(0);
        mutations.push(trailing);
        for bytes in mutations {
            assert!(verify_artifact(&bytes).is_err());
        }
    }
}
