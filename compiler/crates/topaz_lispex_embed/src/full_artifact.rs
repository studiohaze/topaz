use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use super::artifact::{portable_core_for_identity, submission_sha256};
use super::{
    ArtifactCategory, ArtifactKind, ConsumerArtifact, ConsumerArtifactInspection, validate_value,
};
use crate::protocol::{parse_response, verify_response_contract};
use crate::runtime::{Category, Operation};

pub const FULL_COMPONENT_ID: &str = "lispex-evaluator/rust-vm-current-profile/1";
pub const FULL_LANGUAGE_PROFILE_ID: &str = "lispex-profile-1.5";
pub const FULL_EVALUATOR_SHA256: &str =
    "dd4cde2976d825ae99b542d308d87489ac96848e0710329dbb9173664d8c5ad8";
pub const FULL_PROFILE_ID: &str = "lispex/r7rs-rule-current-profile-bounded/1";
pub const FULL_FEATURE_SET_SHA256: &str =
    "6ff16159ab9c6758c1485b67c928a9b3b4a896e4f2ecf96e4cf3cbaf5ac22fae";
pub const FULL_MODEL_ID: &str = "lispex-full-vm-meter/1";
#[cfg(feature = "workspace-component")]
pub(super) const FULL_EVALUATOR_BYTES: &[u8] = include_bytes!(
    "../../../contracts/lispex-full-provider-intake/v1/inputs/products/full-embed-evaluator/v1.15.7/lispex-full-embed-evaluator.wasm"
);
#[cfg(all(
    not(feature = "workspace-component"),
    feature = "managed-product-component"
))]
pub(super) const FULL_EVALUATOR_BYTES: &[u8] =
    include_bytes!("../../../../lispex/component/lispex-full-embed-evaluator.wasm");

/// Closed executable denominator retained from the provider's final semantic
/// surface authority. These are product-contract counts, not document or
/// milestone completion scores.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FullProfileDenominator {
    pub reader_comments: u16,
    pub reader_datums: u16,
    pub core_forms: u16,
    pub derived_forms: u16,
    pub bytecode_opcodes: u16,
    pub primitive_rows: u16,
    pub guest_calling_rows: u16,
    pub diagnostic_rows: u16,
    pub fixed_rows: u16,
    pub precharged_rows: u16,
    pub incremental_rows: u16,
    pub deferred_rows: u16,
}

pub const FULL_PROFILE_DENOMINATOR: FullProfileDenominator = FullProfileDenominator {
    reader_comments: 2,
    reader_datums: 15,
    core_forms: 13,
    derived_forms: 12,
    bytecode_opcodes: 22,
    primitive_rows: 205,
    guest_calling_rows: 18,
    diagnostic_rows: 24,
    fixed_rows: 70,
    precharged_rows: 19,
    incremental_rows: 98,
    deferred_rows: 0,
};

const MAGIC: &[u8; 8] = b"LPXFAR01";
const MAX_FIELD_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Fixes the limit vector shape expected by the complete-profile wire.
pub enum FullArtifactKind {
    Prepare,
    Evaluate,
}

impl FullArtifactKind {
    fn from_tag(tag: u8) -> Result<Self, FullArtifactError> {
        match tag {
            1 => Ok(Self::Prepare),
            2 => Ok(Self::Evaluate),
            _ => Err(FullArtifactError::new("artifact-kind")),
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
/// Preserves the difference between language outcomes and infrastructure failures.
pub enum FullArtifactCategory {
    Prepared,
    Complete,
    SemanticFailure,
    LimitExhaustion,
    RequestRefusal,
    EngineFault,
}

impl FullArtifactCategory {
    fn from_tag(tag: u8) -> Result<Self, FullArtifactError> {
        match tag {
            0 => Ok(Self::Prepared),
            1 => Ok(Self::Complete),
            2 => Ok(Self::SemanticFailure),
            3 => Ok(Self::LimitExhaustion),
            4 => Ok(Self::RequestRefusal),
            5 => Ok(Self::EngineFault),
            _ => Err(FullArtifactError::new("artifact-category")),
        }
    }

    const fn core_required(self, kind: FullArtifactKind) -> bool {
        matches!(
            (kind, self),
            (
                FullArtifactKind::Prepare,
                Self::LimitExhaustion | Self::RequestRefusal
            ) | (
                FullArtifactKind::Evaluate,
                Self::Complete
                    | Self::SemanticFailure
                    | Self::LimitExhaustion
                    | Self::RequestRefusal
            )
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Retains replay bytes under an envelope digest rather than trusting current evaluator output.
pub struct FullConsumerArtifact {
    pub kind: FullArtifactKind,
    pub category: FullArtifactCategory,
    pub evaluator_sha256: String,
    pub exact_limits: Vec<u64>,
    pub identities: [Option<String>; 6],
    pub request_sha256: [u8; 32],
    pub replay_request: Vec<u8>,
    pub response: Vec<u8>,
    pub portable_core: Vec<u8>,
    pub envelope_sha256: [u8; 32],
}

#[derive(Clone, PartialEq, Eq)]
/// Owns verified artifact bytes so evaluation cannot substitute payload or identities.
pub struct FullProfileRuleHandle {
    artifact: Arc<[u8]>,
    artifact_sha256: String,
    request_sha256: [u8; 32],
    prepared_payload_sha256: String,
    identities: [String; 6],
}

impl fmt::Debug for FullProfileRuleHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FullProfileRuleHandle")
            .field("component_id", &FULL_COMPONENT_ID)
            .field("profile_id", &FULL_PROFILE_ID)
            .field("feature_set_sha256", &FULL_FEATURE_SET_SHA256)
            .field("artifact_sha256", &self.artifact_sha256)
            .field("artifact_bytes", &self.artifact.len())
            .field("prepared_payload_sha256", &self.prepared_payload_sha256)
            .finish_non_exhaustive()
    }
}

impl FullProfileRuleHandle {
    #[must_use]
    pub const fn component_id(&self) -> &'static str {
        FULL_COMPONENT_ID
    }

    #[must_use]
    pub const fn evaluator_sha256(&self) -> &'static str {
        FULL_EVALUATOR_SHA256
    }

    #[must_use]
    pub const fn profile_id(&self) -> &'static str {
        FULL_PROFILE_ID
    }

    #[must_use]
    pub const fn feature_set_sha256(&self) -> &'static str {
        FULL_FEATURE_SET_SHA256
    }

    #[must_use]
    pub const fn meter_model_id(&self) -> &'static str {
        FULL_MODEL_ID
    }

    #[must_use]
    pub fn artifact_sha256(&self) -> &str {
        &self.artifact_sha256
    }

    #[must_use]
    pub fn request_sha256(&self) -> &[u8; 32] {
        &self.request_sha256
    }

    #[must_use]
    pub fn prepared_payload_sha256(&self) -> &str {
        &self.prepared_payload_sha256
    }

    #[must_use]
    pub fn artifact_bytes(&self) -> &[u8] {
        &self.artifact
    }

    #[must_use]
    pub fn identity(&self, slot: usize) -> Option<&str> {
        self.identities.get(slot).map(String::as_str)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Closed codes keep binary admission independent of diagnostic prose.
pub struct FullArtifactError {
    code: &'static str,
}

impl FullArtifactError {
    const fn new(code: &'static str) -> Self {
        Self { code }
    }

    #[must_use]
    pub const fn code(self) -> &'static str {
        self.code
    }
}

impl fmt::Display for FullArtifactError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "full Lispex artifact refused: {}", self.code)
    }
}

impl std::error::Error for FullArtifactError {}

/// Admits envelope digest, replay request, response, limits, and identities.
pub fn decode_full_artifact(bytes: &[u8]) -> Result<FullConsumerArtifact, FullArtifactError> {
    if bytes.len() < 32 {
        return Err(FullArtifactError::new("artifact-truncated"));
    }
    let body_len = bytes.len() - 32;
    let actual_envelope: [u8; 32] = Sha256::digest(&bytes[..body_len]).into();
    let expected_envelope: [u8; 32] = bytes[body_len..]
        .try_into()
        .map_err(|_| FullArtifactError::new("artifact-envelope-digest"))?;
    if actual_envelope != expected_envelope {
        return Err(FullArtifactError::new("artifact-envelope-digest"));
    }

    let mut cursor = Cursor::new(&bytes[..body_len]);
    if cursor.take(8)? != MAGIC {
        return Err(FullArtifactError::new("artifact-magic"));
    }
    let kind = FullArtifactKind::from_tag(cursor.byte()?)?;
    let category = FullArtifactCategory::from_tag(cursor.byte()?)?;
    let evaluator_sha256 = lower_hex(cursor.take(64)?, "artifact-evaluator")?;
    let limit_count = usize::from(cursor.byte()?);
    if limit_count != kind.limit_count() {
        return Err(FullArtifactError::new("artifact-limit-count"));
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
            _ => return Err(FullArtifactError::new("artifact-identity-tag")),
        };
    }
    let request_sha256 = cursor
        .take(32)?
        .try_into()
        .map_err(|_| FullArtifactError::new("artifact-request-digest"))?;
    let replay_request = cursor.field()?.to_vec();
    let response = cursor.field()?.to_vec();
    let portable_core = cursor.field()?.to_vec();
    if !cursor.done() {
        return Err(FullArtifactError::new("artifact-trailing"));
    }
    let artifact = FullConsumerArtifact {
        kind,
        category,
        evaluator_sha256,
        exact_limits,
        identities,
        request_sha256,
        replay_request,
        response,
        portable_core,
        envelope_sha256: expected_envelope,
    };
    verify_full_decoded(&artifact)?;
    Ok(artifact)
}

/// Requires a complete-profile artifact to pass strict binary admission.
pub fn verify_full_artifact(bytes: &[u8]) -> Result<(), FullArtifactError> {
    decode_full_artifact(bytes).map(|_| ())
}

/// Projects non-authoritative metadata from an admitted complete-profile artifact.
pub fn inspect_full_artifact(
    bytes: &[u8],
) -> Result<ConsumerArtifactInspection, FullArtifactError> {
    verify_full_artifact(bytes)?;
    let artifact = decode_full_artifact(bytes)?;
    let semantic_profile_id = matches!(
        (artifact.kind, artifact.category),
        (FullArtifactKind::Prepare, FullArtifactCategory::Prepared)
            | (
                FullArtifactKind::Evaluate,
                FullArtifactCategory::Complete
                    | FullArtifactCategory::SemanticFailure
                    | FullArtifactCategory::LimitExhaustion
            )
    )
    .then(|| FULL_PROFILE_ID.to_string());
    let portable_core_sha256 = (!artifact.portable_core.is_empty())
        .then(|| hex_lower(&Sha256::digest(&artifact.portable_core)));
    Ok(ConsumerArtifactInspection {
        kind: bounded_kind(artifact.kind),
        category: bounded_category(artifact.category),
        evaluator_sha256: artifact.evaluator_sha256,
        semantic_profile_id,
        artifact_sha256: hex_lower(&Sha256::digest(bytes)),
        artifact_bytes: u64::try_from(bytes.len())
            .map_err(|_| FullArtifactError::new("artifact-field-size"))?,
        portable_core_sha256,
        portable_core_bytes: u64::try_from(artifact.portable_core.len())
            .map_err(|_| FullArtifactError::new("artifact-field-size"))?,
        authenticated: false,
    })
}

/// Wrap one exact successful full-profile preparation response in the
/// provider-defined LPXFAR01 consumer envelope. Refusals need a portable core
/// and are deliberately left to the provider artifact path; package locking
/// accepts only a prepared rule.
pub fn wrap_full_prepare_artifact(
    response_bytes: &[u8],
    request_bytes: &[u8],
    exact_limits: [u64; 4],
) -> Result<Vec<u8>, FullArtifactError> {
    let response =
        parse_response(response_bytes).map_err(|_| FullArtifactError::new("artifact-response"))?;
    verify_response_contract(&response)
        .map_err(|_| FullArtifactError::new("artifact-response-contract"))?;
    if response.operation != Operation::Prepare || response.category != Category::Prepared {
        return Err(FullArtifactError::new("artifact-needs-prepared"));
    }
    let mut identities = response.digests;
    identities[5] = Some(
        super::artifact::submission_sha256(request_bytes)
            .map_err(|_| FullArtifactError::new("artifact-submission"))?,
    );
    if identities.iter().any(Option::is_none) {
        return Err(FullArtifactError::new("artifact-identity-binding"));
    }

    let mut output = Vec::new();
    output.extend_from_slice(MAGIC);
    output.push(1);
    output.push(FullArtifactCategory::Prepared as u8);
    output.extend_from_slice(FULL_EVALUATOR_SHA256.as_bytes());
    output.push(4);
    for limit in exact_limits {
        output.extend_from_slice(&limit.to_be_bytes());
    }
    for identity in identities {
        output.push(1);
        output.extend_from_slice(
            identity
                .as_deref()
                .expect("full prepared identity must be bound")
                .as_bytes(),
        );
    }
    output.extend_from_slice(&Sha256::digest(request_bytes));
    push_full_field(&mut output, &[])?;
    push_full_field(&mut output, response_bytes)?;
    push_full_field(&mut output, &[])?;
    let envelope: [u8; 32] = Sha256::digest(&output).into();
    output.extend_from_slice(&envelope);
    verify_full_artifact(&output)?;
    Ok(output)
}

/// Preserves request and response bytes so the evaluation can be replayed independently.
pub fn wrap_full_evaluate_artifact(
    response_bytes: &[u8],
    request_bytes: &[u8],
    prepared_artifact_bytes: &[u8],
    evaluation_limits: [u64; 10],
) -> Result<Vec<u8>, FullArtifactError> {
    let prepared = decode_full_artifact(prepared_artifact_bytes)?;
    if prepared.kind != FullArtifactKind::Prepare
        || prepared.category != FullArtifactCategory::Prepared
    {
        return Err(FullArtifactError::new("artifact-prepared-input"));
    }
    let response =
        parse_response(response_bytes).map_err(|_| FullArtifactError::new("artifact-response"))?;
    verify_response_contract(&response)
        .map_err(|_| FullArtifactError::new("artifact-response-contract"))?;
    if response.operation != Operation::Evaluate {
        return Err(FullArtifactError::new("artifact-operation"));
    }
    let category = full_category(response.category);
    if matches!(
        category,
        FullArtifactCategory::Complete
            | FullArtifactCategory::SemanticFailure
            | FullArtifactCategory::LimitExhaustion
    ) && (response.digests[0] != prepared.identities[3]
        || response.digests[1] != prepared.identities[4])
    {
        return Err(FullArtifactError::new("artifact-prepared-binding"));
    }
    let mut identities = prepared.identities;
    identities[5] = Some(
        submission_sha256(request_bytes)
            .map_err(|_| FullArtifactError::new("artifact-submission"))?,
    );
    let mut exact_limits = prepared.exact_limits;
    exact_limits.extend_from_slice(&evaluation_limits);
    let proxy = ConsumerArtifact {
        kind: ArtifactKind::Evaluate,
        category: bounded_category(category),
        evaluator_sha256: FULL_EVALUATOR_SHA256.into(),
        exact_limits: exact_limits.clone(),
        identities: identities.clone(),
        response: response_bytes.to_vec(),
        portable_core: Vec::new(),
    };
    let portable_core = portable_core_for_identity(
        &proxy,
        FULL_EVALUATOR_SHA256,
        FULL_PROFILE_ID,
        FULL_FEATURE_SET_SHA256,
        FULL_MODEL_ID,
    )
    .map_err(|_| FullArtifactError::new("artifact-portable-core"))?;

    let mut output = Vec::new();
    output.extend_from_slice(MAGIC);
    output.push(2);
    output.push(category as u8);
    output.extend_from_slice(FULL_EVALUATOR_SHA256.as_bytes());
    output.push(14);
    for limit in exact_limits {
        output.extend_from_slice(&limit.to_be_bytes());
    }
    for identity in identities {
        match identity {
            Some(identity) => {
                output.push(1);
                output.extend_from_slice(identity.as_bytes());
            }
            None => output.push(0),
        }
    }
    output.extend_from_slice(&Sha256::digest(request_bytes));
    push_full_field(&mut output, request_bytes)?;
    push_full_field(&mut output, response_bytes)?;
    push_full_field(&mut output, &portable_core)?;
    let envelope: [u8; 32] = Sha256::digest(&output).into();
    output.extend_from_slice(&envelope);
    verify_full_artifact(&output)?;
    Ok(output)
}

fn push_full_field(output: &mut Vec<u8>, field: &[u8]) -> Result<(), FullArtifactError> {
    let length =
        u32::try_from(field.len()).map_err(|_| FullArtifactError::new("artifact-field-size"))?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(field);
    Ok(())
}

/// Loads an immutable rule handle from a prepared complete-profile artifact.
pub fn load_full_profile_rule_handle(
    bytes: &[u8],
) -> Result<FullProfileRuleHandle, FullArtifactError> {
    let artifact = decode_full_artifact(bytes)?;
    if artifact.kind != FullArtifactKind::Prepare
        || artifact.category != FullArtifactCategory::Prepared
    {
        return Err(FullArtifactError::new("rule-handle-needs-prepared"));
    }
    let response = parse_response(&artifact.response)
        .map_err(|_| FullArtifactError::new("artifact-response"))?;
    let identities: [String; 6] = artifact
        .identities
        .clone()
        .map(|identity| identity.expect("prepared artifact identity must be bound"));
    Ok(FullProfileRuleHandle {
        artifact: Arc::from(bytes),
        artifact_sha256: hex_lower(&Sha256::digest(bytes)),
        request_sha256: artifact.request_sha256,
        prepared_payload_sha256: hex_lower(&Sha256::digest(&response.payload)),
        identities,
    })
}

fn verify_full_decoded(artifact: &FullConsumerArtifact) -> Result<(), FullArtifactError> {
    if artifact.evaluator_sha256 != FULL_EVALUATOR_SHA256 {
        return Err(FullArtifactError::new("artifact-evaluator"));
    }
    if artifact.exact_limits.len() != artifact.kind.limit_count() {
        return Err(FullArtifactError::new("artifact-limit-count"));
    }
    if artifact.replay_request.len() > MAX_FIELD_BYTES
        || artifact.response.len() > MAX_FIELD_BYTES
        || artifact.portable_core.len() > MAX_FIELD_BYTES
    {
        return Err(FullArtifactError::new("artifact-field-size"));
    }
    match artifact.kind {
        FullArtifactKind::Prepare if !artifact.replay_request.is_empty() => {
            return Err(FullArtifactError::new("artifact-prepare-replay"));
        }
        FullArtifactKind::Evaluate => {
            if artifact.replay_request.is_empty()
                || Sha256::digest(&artifact.replay_request).as_slice() != artifact.request_sha256
            {
                return Err(FullArtifactError::new("artifact-request-binding"));
            }
        }
        FullArtifactKind::Prepare => {}
    }
    let response = parse_response(&artifact.response)
        .map_err(|_| FullArtifactError::new("artifact-response"))?;
    verify_response_contract(&response)
        .map_err(|_| FullArtifactError::new("artifact-response-contract"))?;
    let operation_matches = matches!(
        (artifact.kind, response.operation),
        (FullArtifactKind::Prepare, Operation::Prepare)
            | (FullArtifactKind::Evaluate, Operation::Evaluate)
    );
    let category_matches = artifact.category as u8 == response.category as u8;
    if !operation_matches || !category_matches {
        return Err(FullArtifactError::new("artifact-category-binding"));
    }
    if artifact.category.core_required(artifact.kind) == artifact.portable_core.is_empty() {
        return Err(FullArtifactError::new("artifact-portable-core-eligibility"));
    }
    if !artifact.portable_core.is_empty() {
        validate_value(&artifact.portable_core, true)
            .map_err(|_| FullArtifactError::new("artifact-portable-core-codec"))?;
        verify_portable_core(artifact)?;
        let proxy = ConsumerArtifact {
            kind: bounded_kind(artifact.kind),
            category: bounded_category(artifact.category),
            evaluator_sha256: artifact.evaluator_sha256.clone(),
            exact_limits: artifact.exact_limits.clone(),
            identities: artifact.identities.clone(),
            response: artifact.response.clone(),
            portable_core: Vec::new(),
        };
        let expected = portable_core_for_identity(
            &proxy,
            FULL_EVALUATOR_SHA256,
            FULL_PROFILE_ID,
            FULL_FEATURE_SET_SHA256,
            FULL_MODEL_ID,
        )
        .map_err(|_| FullArtifactError::new("artifact-portable-core"))?;
        if expected != artifact.portable_core {
            return Err(FullArtifactError::new("artifact-portable-core"));
        }
    }
    match artifact.kind {
        FullArtifactKind::Prepare => match artifact.category {
            FullArtifactCategory::Prepared => {
                if artifact.identities.iter().any(Option::is_none)
                    || artifact.identities[..5] != response.digests[..5]
                {
                    return Err(FullArtifactError::new("artifact-identity-binding"));
                }
            }
            FullArtifactCategory::LimitExhaustion | FullArtifactCategory::RequestRefusal => {
                if artifact.identities[..5].iter().any(Option::is_some)
                    || artifact.identities[5].is_none()
                {
                    return Err(FullArtifactError::new("artifact-identity-binding"));
                }
            }
            FullArtifactCategory::EngineFault => {}
            _ => return Err(FullArtifactError::new("artifact-kind-category")),
        },
        FullArtifactKind::Evaluate => {
            if matches!(
                artifact.category,
                FullArtifactCategory::Complete
                    | FullArtifactCategory::SemanticFailure
                    | FullArtifactCategory::LimitExhaustion
            ) && (artifact.identities.iter().any(Option::is_none)
                || artifact.identities[3] != response.digests[0]
                || artifact.identities[4] != response.digests[1])
            {
                return Err(FullArtifactError::new("artifact-identity-binding"));
            }
        }
    }
    if artifact.identities[4]
        .as_deref()
        .is_some_and(|identity| identity != FULL_FEATURE_SET_SHA256)
    {
        return Err(FullArtifactError::new("artifact-feature-set"));
    }
    Ok(())
}

const fn full_category(category: Category) -> FullArtifactCategory {
    match category {
        Category::Prepared => FullArtifactCategory::Prepared,
        Category::Complete => FullArtifactCategory::Complete,
        Category::SemanticFailure => FullArtifactCategory::SemanticFailure,
        Category::LimitExhaustion => FullArtifactCategory::LimitExhaustion,
        Category::RequestRefusal => FullArtifactCategory::RequestRefusal,
        Category::EngineFault => FullArtifactCategory::EngineFault,
    }
}

const fn bounded_kind(kind: FullArtifactKind) -> ArtifactKind {
    match kind {
        FullArtifactKind::Prepare => ArtifactKind::Prepare,
        FullArtifactKind::Evaluate => ArtifactKind::Evaluate,
    }
}

const fn bounded_category(category: FullArtifactCategory) -> ArtifactCategory {
    match category {
        FullArtifactCategory::Prepared => ArtifactCategory::Prepared,
        FullArtifactCategory::Complete => ArtifactCategory::Complete,
        FullArtifactCategory::SemanticFailure => ArtifactCategory::SemanticFailure,
        FullArtifactCategory::LimitExhaustion => ArtifactCategory::LimitExhaustion,
        FullArtifactCategory::RequestRefusal => ArtifactCategory::RequestRefusal,
        FullArtifactCategory::EngineFault => ArtifactCategory::EngineFault,
    }
}

fn verify_portable_core(artifact: &FullConsumerArtifact) -> Result<(), FullArtifactError> {
    let fields = record_fields(&artifact.portable_core)?;
    if text_field(required(&fields, "schema")?)? != "lispex.embed-receipt-core/v1"
        || text_field(required(&fields, "engine_artifact_sha256")?)? != FULL_EVALUATOR_SHA256
    {
        return Err(FullArtifactError::new("artifact-portable-core-identity"));
    }
    let category = text_field(required(&fields, "category")?)?;
    if fields.contains_key("refusal_code") {
        if category != "deterministic-request-refusal" {
            return Err(FullArtifactError::new("artifact-portable-core-category"));
        }
        let available = required(&fields, "available_field_identities")?;
        let available = record_fields(available)?;
        if !available.is_empty() {
            compare_limits(
                &available,
                &artifact.exact_limits,
                &[
                    "raw_source_bytes",
                    "prepare_work",
                    "prepare_logical_allocation",
                    "syntax_depth",
                ],
            )?;
        }
    } else {
        if category != "deterministic-semantic-outcome"
            || text_field(required(&fields, "semantic_profile_id")?)? != FULL_PROFILE_ID
            || text_field(required(&fields, "feature_set_sha256")?)? != FULL_FEATURE_SET_SHA256
            || text_field(required(&fields, "model_id")?)? != FULL_MODEL_ID
        {
            return Err(FullArtifactError::new("artifact-portable-core-identity"));
        }
        let limits = record_fields(required(&fields, "exact_limits")?)?;
        compare_limits(
            &limits,
            &artifact.exact_limits,
            &[
                "raw_source_bytes",
                "prepare_work",
                "prepare_logical_allocation",
                "syntax_depth",
                "canonical_input_bytes",
                "eval_work",
                "eval_logical_allocation",
                "semantic_frames",
                "traversal_depth",
                "output_bytes",
                "diagnostic_bytes",
                "transcript_bytes",
                "transcript_events",
                "result_bytes",
            ],
        )?;
    }
    Ok(())
}

fn required<'a>(
    fields: &'a BTreeMap<&'a str, &'a [u8]>,
    key: &str,
) -> Result<&'a [u8], FullArtifactError> {
    fields
        .get(key)
        .copied()
        .ok_or_else(|| FullArtifactError::new("artifact-portable-core-field"))
}

fn compare_limits(
    fields: &BTreeMap<&str, &[u8]>,
    limits: &[u64],
    names: &[&str],
) -> Result<(), FullArtifactError> {
    if fields.len() != names.len() || limits.len() != names.len() {
        return Err(FullArtifactError::new("artifact-portable-core-limits"));
    }
    for (index, name) in names.iter().enumerate() {
        if integer_field(required(fields, name)?)? != limits[index] {
            return Err(FullArtifactError::new("artifact-portable-core-limits"));
        }
    }
    Ok(())
}

fn text_field(bytes: &[u8]) -> Result<&str, FullArtifactError> {
    if !matches!(bytes.first(), Some(7 | 8)) {
        return Err(FullArtifactError::new("artifact-portable-core-text"));
    }
    let mut cursor = Cursor::new(&bytes[1..]);
    let length = usize::try_from(cursor.u64()?)
        .map_err(|_| FullArtifactError::new("artifact-portable-core-text"))?;
    let text = std::str::from_utf8(cursor.take(length)?)
        .map_err(|_| FullArtifactError::new("artifact-portable-core-text"))?;
    if !cursor.done() {
        return Err(FullArtifactError::new("artifact-portable-core-text"));
    }
    Ok(text)
}

fn integer_field(bytes: &[u8]) -> Result<u64, FullArtifactError> {
    if bytes.first() != Some(&3) {
        return Err(FullArtifactError::new("artifact-portable-core-integer"));
    }
    let mut cursor = Cursor::new(&bytes[1..]);
    let length = usize::try_from(cursor.u64()?)
        .map_err(|_| FullArtifactError::new("artifact-portable-core-integer"))?;
    let value = std::str::from_utf8(cursor.take(length)?)
        .map_err(|_| FullArtifactError::new("artifact-portable-core-integer"))?
        .parse::<u64>()
        .map_err(|_| FullArtifactError::new("artifact-portable-core-integer"))?;
    if !cursor.done() {
        return Err(FullArtifactError::new("artifact-portable-core-integer"));
    }
    Ok(value)
}

fn record_fields(bytes: &[u8]) -> Result<BTreeMap<&str, &[u8]>, FullArtifactError> {
    let mut cursor = Cursor::new(bytes);
    if cursor.byte()? != 13 {
        return Err(FullArtifactError::new("artifact-portable-core-record"));
    }
    let count = usize::try_from(cursor.u64()?)
        .map_err(|_| FullArtifactError::new("artifact-portable-core-record"))?;
    let mut fields = BTreeMap::new();
    for _ in 0..count {
        let key_length = usize::try_from(cursor.u64()?)
            .map_err(|_| FullArtifactError::new("artifact-portable-core-record"))?;
        let key = std::str::from_utf8(cursor.take(key_length)?)
            .map_err(|_| FullArtifactError::new("artifact-portable-core-record"))?;
        let start = cursor.offset;
        skip_value(&mut cursor, 1)?;
        if fields.insert(key, &bytes[start..cursor.offset]).is_some() {
            return Err(FullArtifactError::new("artifact-portable-core-record"));
        }
    }
    if !cursor.done() {
        return Err(FullArtifactError::new("artifact-portable-core-record"));
    }
    Ok(fields)
}

fn skip_value(cursor: &mut Cursor<'_>, depth: usize) -> Result<(), FullArtifactError> {
    if depth > 256 {
        return Err(FullArtifactError::new("artifact-portable-core-depth"));
    }
    match cursor.byte()? {
        0..=2 => {}
        3 | 7 | 8 | 12 => {
            let length = usize::try_from(cursor.u64()?)
                .map_err(|_| FullArtifactError::new("artifact-portable-core-value"))?;
            cursor.take(length)?;
        }
        4 => {
            for _ in 0..2 {
                let length = usize::try_from(cursor.u64()?)
                    .map_err(|_| FullArtifactError::new("artifact-portable-core-value"))?;
                cursor.take(length)?;
            }
        }
        5 => {
            cursor.take(8)?;
        }
        6 => {
            cursor.take(4)?;
        }
        9 | 11 => {
            let count = usize::try_from(cursor.u64()?)
                .map_err(|_| FullArtifactError::new("artifact-portable-core-value"))?;
            for _ in 0..count {
                skip_value(cursor, depth + 1)?;
            }
        }
        10 => {
            let count = usize::try_from(cursor.u64()?)
                .map_err(|_| FullArtifactError::new("artifact-portable-core-value"))?;
            for _ in 0..=count {
                skip_value(cursor, depth + 1)?;
            }
        }
        13 => {
            let count = usize::try_from(cursor.u64()?)
                .map_err(|_| FullArtifactError::new("artifact-portable-core-value"))?;
            for _ in 0..count {
                let length = usize::try_from(cursor.u64()?)
                    .map_err(|_| FullArtifactError::new("artifact-portable-core-value"))?;
                cursor.take(length)?;
                skip_value(cursor, depth + 1)?;
            }
        }
        _ => return Err(FullArtifactError::new("artifact-portable-core-value")),
    }
    Ok(())
}

fn lower_hex(bytes: &[u8], code: &'static str) -> Result<String, FullArtifactError> {
    if bytes.len() != 64
        || !bytes
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(FullArtifactError::new(code));
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| FullArtifactError::new(code))
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

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn byte(&mut self) -> Result<u8, FullArtifactError> {
        let byte = *self
            .bytes
            .get(self.offset)
            .ok_or_else(|| FullArtifactError::new("artifact-truncated"))?;
        self.offset += 1;
        Ok(byte)
    }

    fn u32(&mut self) -> Result<u32, FullArtifactError> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().map_err(
            |_| FullArtifactError::new("artifact-truncated"),
        )?))
    }

    fn u64(&mut self) -> Result<u64, FullArtifactError> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().map_err(
            |_| FullArtifactError::new("artifact-truncated"),
        )?))
    }

    fn field(&mut self) -> Result<&'a [u8], FullArtifactError> {
        let length = usize::try_from(self.u32()?)
            .map_err(|_| FullArtifactError::new("artifact-field-size"))?;
        if length > MAX_FIELD_BYTES {
            return Err(FullArtifactError::new("artifact-field-size"));
        }
        self.take(length)
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], FullArtifactError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| FullArtifactError::new("artifact-length-overflow"))?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| FullArtifactError::new("artifact-truncated"))?;
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
    use std::fs;
    use std::path::{Path, PathBuf};

    const VECTOR_ROOT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/lispex-full-provider-intake/v1/inputs/",
        "products/full-embed-evaluator/v1.15.8/vectors"
    );

    fn vector(name: &str) -> Vec<u8> {
        fs::read(Path::new(VECTOR_ROOT).join(name)).expect("full provider vector")
    }

    #[test]
    fn full_provider_artifacts_are_profile_bound_and_fail_closed() {
        for name in [
            "prepared.lpxfull",
            "complete.lpxfull",
            "semantic-fault.lpxfull",
            "evaluation-exhausted.lpxfull",
            "preparation-exhausted.lpxfull",
            "request-refusal.lpxfull",
        ] {
            verify_full_artifact(&vector(name)).expect(name);
        }
        let handle = load_full_profile_rule_handle(&vector("prepared.lpxfull"))
            .expect("full prepared handle");
        assert_eq!(handle.component_id(), FULL_COMPONENT_ID);
        assert_eq!(handle.profile_id(), FULL_PROFILE_ID);
        assert_eq!(handle.feature_set_sha256(), FULL_FEATURE_SET_SHA256);
        assert_eq!(handle.meter_model_id(), FULL_MODEL_ID);
        assert_eq!(handle.identity(4), Some(FULL_FEATURE_SET_SHA256));

        let mut negatives = fs::read_dir(VECTOR_ROOT)
            .expect("full vector directory")
            .map(|entry| entry.expect("vector entry").path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("negative-") && name.ends_with(".lpxfull"))
            })
            .collect::<Vec<PathBuf>>();
        negatives.sort();
        assert_eq!(negatives.len(), 14);
        for path in negatives {
            assert!(
                verify_full_artifact(&fs::read(&path).expect("negative vector")).is_err(),
                "accepted {}",
                path.display()
            );
        }
    }

    #[test]
    fn bounded_artifact_cannot_become_a_full_profile_handle() {
        let bounded = fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../contracts/lispex-provider-intake/v1/inputs/products/embed-evaluator/handoffs/lda-c1/v1/vectors/prepared.lpxembed",
        ))
        .expect("bounded provider vector");
        assert!(load_full_profile_rule_handle(&bounded).is_err());
    }
}
