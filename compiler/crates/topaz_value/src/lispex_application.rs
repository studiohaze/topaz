//! Engine-neutral request and value boundary for the profile-gated
//! `std.lispex` application surface.
//!
//! This module knows the Topaz runtime representation but contains no Lispex
//! evaluator.  The interpreter and emitted Rust call the same leaf, and the
//! host either answers from an exact admitted package payload or denies the
//! capability.

use std::rc::Rc;

use topaz_diag::Span;

use crate::{Host, RtError, Value, codes, fault, nominal_declaration_identity, value::exact_args};

pub const PREPARED_RULE_CARRIER_ID: &str = "__PreparedLispexRuleCarrier";
pub const PREPARED_RULE_VARIANT: &str = "__PreparedLispexRuleValue";
pub const LISPEX_VALUE_CARRIER_ID: &str = "__LispexValueCarrier";
pub const LISPEX_VALUE_VARIANT: &str = "__LispexValueValue";

#[derive(Clone, Debug)]
pub struct LispexApplicationOpaqueValue {
    payload: LispexApplicationOpaquePayload,
}

#[derive(Clone, Debug)]
enum LispexApplicationOpaquePayload {
    Rule {
        identity: LispexApplicationRuleIdentity,
        prepared_artifact: Vec<u8>,
    },
    Value(Vec<u8>),
    ConsumerArtifact(Vec<u8>),
}

impl LispexApplicationOpaqueValue {
    pub(crate) fn kind_name(&self) -> &'static str {
        match &self.payload {
            LispexApplicationOpaquePayload::Rule { .. } => "PreparedLispexRule",
            LispexApplicationOpaquePayload::Value(_) => "LispexValue",
            LispexApplicationOpaquePayload::ConsumerArtifact(_) => "LispexConsumerArtifact",
        }
    }
}

const LIMIT_FIELDS: [&str; 10] = [
    "canonicalInputBytes",
    "evalWork",
    "logicalAllocation",
    "semanticFrames",
    "traversalDepth",
    "outputBytes",
    "diagnosticBytes",
    "transcriptBytes",
    "transcriptEvents",
    "resultBytes",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LispexApplicationOperation {
    Rule,
    ValueFromCanonical,
    CanonicalBytes,
    DefaultLimits,
    InspectRule,
    Evaluate,
    EvaluateWithEvidence,
    ConsumerArtifactFromBytes,
    ConsumerArtifactBytes,
    PortableCoreBytes,
    InspectConsumerArtifact,
    VerifyConsumerArtifact,
    FreshReplay,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LispexApplicationRuleIdentity {
    pub name: String,
    pub profile: String,
    pub component_id: String,
    pub evaluator_sha256: String,
    pub prepared_artifact_sha256: String,
    pub preparation_request_sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LispexEvaluationLimits {
    pub canonical_input_bytes: u64,
    pub eval_work: u64,
    pub logical_allocation: u64,
    pub semantic_frames: u64,
    pub traversal_depth: u64,
    pub output_bytes: u64,
    pub diagnostic_bytes: u64,
    pub transcript_bytes: u64,
    pub transcript_events: u64,
    pub result_bytes: u64,
}

impl LispexEvaluationLimits {
    #[must_use]
    pub const fn values(self) -> [u64; 10] {
        [
            self.canonical_input_bytes,
            self.eval_work,
            self.logical_allocation,
            self.semantic_frames,
            self.traversal_depth,
            self.output_bytes,
            self.diagnostic_bytes,
            self.transcript_bytes,
            self.transcript_events,
            self.result_bytes,
        ]
    }

    #[must_use]
    pub const fn from_values(values: [u64; 10]) -> Self {
        Self {
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
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LispexApplicationRequest {
    Rule {
        target_identity: String,
    },
    ValueFromCanonical {
        bytes: Vec<u8>,
    },
    CanonicalBytes {
        bytes: Vec<u8>,
    },
    DefaultLimits {
        rule: LispexApplicationRuleIdentity,
        prepared_artifact: Vec<u8>,
    },
    InspectRule {
        rule: LispexApplicationRuleIdentity,
        prepared_artifact: Vec<u8>,
    },
    Evaluate {
        rule: LispexApplicationRuleIdentity,
        prepared_artifact: Vec<u8>,
        input: Vec<u8>,
        limits: LispexEvaluationLimits,
    },
    EvaluateWithEvidence {
        rule: LispexApplicationRuleIdentity,
        prepared_artifact: Vec<u8>,
        input: Vec<u8>,
        limits: LispexEvaluationLimits,
    },
    ConsumerArtifactFromBytes {
        bytes: Vec<u8>,
    },
    ConsumerArtifactBytes {
        artifact: Vec<u8>,
    },
    PortableCoreBytes {
        artifact: Vec<u8>,
    },
    InspectConsumerArtifact {
        artifact: Vec<u8>,
    },
    VerifyConsumerArtifact {
        artifact: Vec<u8>,
    },
    FreshReplay {
        rule: LispexApplicationRuleIdentity,
        prepared_artifact: Vec<u8>,
        input: Vec<u8>,
        artifact: Vec<u8>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LispexApplicationSettlementCategory {
    Complete,
    SemanticFailure,
    LimitExhaustion,
    RequestRefusal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LispexApplicationSettlement {
    pub category: LispexApplicationSettlementCategory,
    pub code: String,
    pub result: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LispexConsumerArtifactInspection {
    pub kind: String,
    pub category: String,
    pub evaluator_sha256: String,
    pub semantic_profile_id: Option<String>,
    pub artifact_sha256: String,
    pub artifact_bytes: u64,
    pub portable_core_sha256: Option<String>,
    pub portable_core_bytes: u64,
    pub authenticated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LispexApplicationResponse {
    Rule {
        identity: LispexApplicationRuleIdentity,
        prepared_artifact: Vec<u8>,
    },
    CanonicalValue(Vec<u8>),
    ValueRefusal(String),
    Limits(LispexEvaluationLimits),
    Identity(LispexApplicationRuleIdentity),
    Settlement(LispexApplicationSettlement),
    EvidenceSettlement {
        settlement: LispexApplicationSettlement,
        artifact: Option<Vec<u8>>,
    },
    ConsumerArtifact(Vec<u8>),
    ConsumerArtifactBytes(Vec<u8>),
    ConsumerArtifactInspection(LispexConsumerArtifactInspection),
    EvidenceRefusal(String),
    ReplayFault {
        code: String,
        operational_code: Option<String>,
        detail: Option<String>,
    },
    OperationalFault {
        code: String,
        detail: Option<String>,
    },
}

pub fn builtin_lispex_application(
    host: &dyn Host,
    operation: LispexApplicationOperation,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RtError> {
    let request = request_for(operation, args, span)?;
    let response = host.lispex_application(request);
    response_value(operation, response, span)
}

/// Project the immutable nominal wrappers returned by the fixed `std.lispex`
/// host boundary into their Topaz 5.20 defining-module identity. Older language
/// modes keep the source-level identities produced by the shared host leaf.
pub fn project_lispex_application_host_value(value: Value) -> Value {
    fn identity(name: &str) -> Rc<str> {
        Rc::from(format!("std.lispex::{name}"))
    }

    match value {
        Value::Some(value) => Value::Some(Rc::new(project_lispex_application_host_value(
            Rc::unwrap_or_clone(value),
        ))),
        Value::Ok(value) => Value::Ok(Rc::new(project_lispex_application_host_value(
            Rc::unwrap_or_clone(value),
        ))),
        Value::Err(value) => Value::Err(Rc::new(project_lispex_application_host_value(
            Rc::unwrap_or_clone(value),
        ))),
        Value::Enum {
            enum_id,
            declaration_identity,
            method_identity,
            variant,
            variant_index,
            payloads,
        } => {
            let canonical = declaration_identity.unwrap_or_else(|| identity(&enum_id));
            Value::Enum {
                enum_id,
                declaration_identity: Some(canonical.clone()),
                method_identity: Some(method_identity.unwrap_or(canonical)),
                variant,
                variant_index,
                payloads: payloads
                    .iter()
                    .cloned()
                    .map(project_lispex_application_host_value)
                    .collect(),
            }
        }
        Value::NominalRecord {
            record_id,
            declaration_identity,
            method_identity,
            fields,
        } => {
            let canonical = declaration_identity.unwrap_or_else(|| identity(&record_id));
            Value::NominalRecord {
                record_id,
                declaration_identity: Some(canonical.clone()),
                method_identity: Some(method_identity.unwrap_or(canonical)),
                fields: fields
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.clone(),
                            project_lispex_application_host_value(value.clone()),
                        )
                    })
                    .collect(),
            }
        }
        Value::Newtype {
            newtype_id,
            declaration_identity,
            method_identity,
            inner,
        } => {
            let canonical = declaration_identity.unwrap_or_else(|| identity(&newtype_id));
            Value::Newtype {
                newtype_id,
                declaration_identity: Some(canonical.clone()),
                method_identity: Some(method_identity.unwrap_or(canonical)),
                inner: Rc::new(project_lispex_application_host_value(Rc::unwrap_or_clone(
                    inner,
                ))),
            }
        }
        value => value,
    }
}

fn request_for(
    operation: LispexApplicationOperation,
    args: Vec<Value>,
    span: Span,
) -> Result<LispexApplicationRequest, RtError> {
    Ok(match operation {
        LispexApplicationOperation::Rule => {
            let [name] = exact_args(args, span)?;
            let Value::Str(name) = name else {
                return Err(type_fault("Lispex rule name", span));
            };
            LispexApplicationRequest::Rule {
                target_identity: format!("topaz.lispex-rule-handle/v1:{name}"),
            }
        }
        LispexApplicationOperation::ValueFromCanonical => {
            let [value] = exact_args(args, span)?;
            LispexApplicationRequest::ValueFromCanonical {
                bytes: bytes_value(value, "canonical Lispex value", span)?,
            }
        }
        LispexApplicationOperation::CanonicalBytes => {
            let [value] = exact_args(args, span)?;
            LispexApplicationRequest::CanonicalBytes {
                bytes: lispex_value(value, span)?,
            }
        }
        LispexApplicationOperation::DefaultLimits => {
            let [value] = exact_args(args, span)?;
            let (rule, prepared_artifact) = prepared_rule(value, span)?;
            LispexApplicationRequest::DefaultLimits {
                rule,
                prepared_artifact,
            }
        }
        LispexApplicationOperation::InspectRule => {
            let [value] = exact_args(args, span)?;
            let (rule, prepared_artifact) = prepared_rule(value, span)?;
            LispexApplicationRequest::InspectRule {
                rule,
                prepared_artifact,
            }
        }
        LispexApplicationOperation::Evaluate => {
            let [rule, input, limits] = exact_args(args, span)?;
            let limits = limits_value(limits, span)?;
            let input = lispex_value(input, span)?;
            let (rule, prepared_artifact) = prepared_rule(rule, span)?;
            LispexApplicationRequest::Evaluate {
                rule,
                prepared_artifact,
                input,
                limits,
            }
        }
        LispexApplicationOperation::EvaluateWithEvidence => {
            let [rule, input, limits] = exact_args(args, span)?;
            let limits = limits_value(limits, span)?;
            let input = lispex_value(input, span)?;
            let (rule, prepared_artifact) = prepared_rule(rule, span)?;
            LispexApplicationRequest::EvaluateWithEvidence {
                rule,
                prepared_artifact,
                input,
                limits,
            }
        }
        LispexApplicationOperation::ConsumerArtifactFromBytes => {
            let [value] = exact_args(args, span)?;
            LispexApplicationRequest::ConsumerArtifactFromBytes {
                bytes: bytes_value(value, "serialized Lispex consumer artifact", span)?,
            }
        }
        LispexApplicationOperation::ConsumerArtifactBytes => {
            let [value] = exact_args(args, span)?;
            LispexApplicationRequest::ConsumerArtifactBytes {
                artifact: consumer_artifact(value, span)?,
            }
        }
        LispexApplicationOperation::PortableCoreBytes => {
            let [value] = exact_args(args, span)?;
            LispexApplicationRequest::PortableCoreBytes {
                artifact: consumer_artifact(value, span)?,
            }
        }
        LispexApplicationOperation::InspectConsumerArtifact => {
            let [value] = exact_args(args, span)?;
            LispexApplicationRequest::InspectConsumerArtifact {
                artifact: consumer_artifact(value, span)?,
            }
        }
        LispexApplicationOperation::VerifyConsumerArtifact => {
            let [value] = exact_args(args, span)?;
            LispexApplicationRequest::VerifyConsumerArtifact {
                artifact: consumer_artifact(value, span)?,
            }
        }
        LispexApplicationOperation::FreshReplay => {
            let [rule, input, artifact] = exact_args(args, span)?;
            let artifact = consumer_artifact(artifact, span)?;
            let input = lispex_value(input, span)?;
            let (rule, prepared_artifact) = prepared_rule(rule, span)?;
            LispexApplicationRequest::FreshReplay {
                rule,
                prepared_artifact,
                input,
                artifact,
            }
        }
    })
}

fn response_value(
    operation: LispexApplicationOperation,
    response: LispexApplicationResponse,
    span: Span,
) -> Result<Value, RtError> {
    match (operation, response) {
        (
            LispexApplicationOperation::Rule,
            LispexApplicationResponse::Rule {
                identity,
                prepared_artifact,
            },
        ) => Ok(prepared_rule_value(identity, prepared_artifact)),
        (
            LispexApplicationOperation::ValueFromCanonical,
            LispexApplicationResponse::CanonicalValue(bytes),
        ) => Ok(Value::Ok(Rc::new(lispex_value_carrier(bytes)))),
        (
            LispexApplicationOperation::ValueFromCanonical,
            LispexApplicationResponse::ValueRefusal(code),
        ) => Ok(Value::Err(Rc::new(enum_value(
            "LispexValueError",
            "InvalidCanonicalValue",
            0,
            vec![Value::str(code)],
        )))),
        (
            LispexApplicationOperation::CanonicalBytes,
            LispexApplicationResponse::CanonicalValue(bytes),
        ) => Ok(Value::Bytes(Rc::from(bytes))),
        (LispexApplicationOperation::DefaultLimits, LispexApplicationResponse::Limits(limits)) => {
            limits_to_value(limits, span)
        }
        (
            LispexApplicationOperation::InspectRule,
            LispexApplicationResponse::Identity(identity),
        ) => Ok(identity_value(identity)),
        (
            LispexApplicationOperation::Evaluate,
            LispexApplicationResponse::Settlement(settlement),
        ) => settlement_value(settlement, span).map(|value| Value::Ok(Rc::new(value))),
        (
            LispexApplicationOperation::Evaluate,
            LispexApplicationResponse::OperationalFault { code, detail },
        ) => operational_fault_value(&code, detail, span).map(|value| Value::Err(Rc::new(value))),
        (
            LispexApplicationOperation::EvaluateWithEvidence,
            LispexApplicationResponse::EvidenceSettlement {
                settlement,
                artifact,
            },
        ) => evidence_settlement_value(settlement, artifact, span)
            .map(|value| Value::Ok(Rc::new(value))),
        (
            LispexApplicationOperation::EvaluateWithEvidence,
            LispexApplicationResponse::OperationalFault { code, detail },
        ) => operational_fault_value(&code, detail, span).map(|value| Value::Err(Rc::new(value))),
        (
            LispexApplicationOperation::ConsumerArtifactFromBytes,
            LispexApplicationResponse::ConsumerArtifact(bytes),
        ) => Ok(Value::Ok(Rc::new(consumer_artifact_carrier(bytes)))),
        (
            LispexApplicationOperation::ConsumerArtifactFromBytes,
            LispexApplicationResponse::EvidenceRefusal(code),
        ) => Ok(Value::Err(Rc::new(evidence_error_value(&code)))),
        (
            LispexApplicationOperation::ConsumerArtifactBytes,
            LispexApplicationResponse::ConsumerArtifactBytes(bytes),
        ) => Ok(Value::Bytes(Rc::from(bytes))),
        (
            LispexApplicationOperation::PortableCoreBytes,
            LispexApplicationResponse::ConsumerArtifactBytes(bytes),
        ) => Ok(Value::Ok(Rc::new(Value::Bytes(Rc::from(bytes))))),
        (
            LispexApplicationOperation::PortableCoreBytes,
            LispexApplicationResponse::EvidenceRefusal(code),
        ) => Ok(Value::Err(Rc::new(evidence_error_value(&code)))),
        (
            LispexApplicationOperation::InspectConsumerArtifact,
            LispexApplicationResponse::ConsumerArtifactInspection(inspection),
        )
        | (
            LispexApplicationOperation::VerifyConsumerArtifact,
            LispexApplicationResponse::ConsumerArtifactInspection(inspection),
        ) => Ok(Value::Ok(Rc::new(inspection_value(inspection, span)?))),
        (
            LispexApplicationOperation::InspectConsumerArtifact
            | LispexApplicationOperation::VerifyConsumerArtifact,
            LispexApplicationResponse::EvidenceRefusal(code),
        ) => Ok(Value::Err(Rc::new(evidence_error_value(&code)))),
        (
            LispexApplicationOperation::FreshReplay,
            LispexApplicationResponse::Settlement(settlement),
        ) => settlement_value(settlement, span).map(|value| Value::Ok(Rc::new(value))),
        (
            LispexApplicationOperation::FreshReplay,
            LispexApplicationResponse::EvidenceRefusal(code),
        ) => Ok(Value::Err(Rc::new(replay_error_value(&code, None)))),
        (
            LispexApplicationOperation::FreshReplay,
            LispexApplicationResponse::ReplayFault {
                code,
                operational_code,
                detail,
            },
        ) => {
            let operational = match operational_code {
                Some(operational_code) => {
                    Some(operational_fault_value(&operational_code, detail, span)?)
                }
                None => None,
            };
            Ok(Value::Err(Rc::new(replay_error_value(&code, operational))))
        }
        (_, LispexApplicationResponse::OperationalFault { code, detail }) => Err(fault(
            codes::GUARD_UNIMPLEMENTED,
            match detail {
                Some(detail) => format!("Lispex application host refused `{code}`: {detail}"),
                None => format!("Lispex application host refused `{code}`"),
            },
            span,
        )),
        (_, _) => Err(fault(
            codes::GUARD_UNIMPLEMENTED,
            "the Lispex application host returned a response for another operation",
            span,
        )),
    }
}

fn prepared_rule_value(
    identity: LispexApplicationRuleIdentity,
    prepared_artifact: Vec<u8>,
) -> Value {
    Value::LispexApplicationOpaque(Rc::new(LispexApplicationOpaqueValue {
        payload: LispexApplicationOpaquePayload::Rule {
            identity,
            prepared_artifact,
        },
    }))
}

fn prepared_rule(
    value: Value,
    span: Span,
) -> Result<(LispexApplicationRuleIdentity, Vec<u8>), RtError> {
    match value {
        Value::LispexApplicationOpaque(value) => match &value.payload {
            LispexApplicationOpaquePayload::Rule {
                identity,
                prepared_artifact,
            } => Ok((identity.clone(), prepared_artifact.clone())),
            LispexApplicationOpaquePayload::Value(_)
            | LispexApplicationOpaquePayload::ConsumerArtifact(_) => {
                Err(type_fault("PreparedLispexRule", span))
            }
        },
        _ => Err(type_fault("PreparedLispexRule", span)),
    }
}

fn lispex_value_carrier(bytes: Vec<u8>) -> Value {
    Value::LispexApplicationOpaque(Rc::new(LispexApplicationOpaqueValue {
        payload: LispexApplicationOpaquePayload::Value(bytes),
    }))
}

fn lispex_value(value: Value, span: Span) -> Result<Vec<u8>, RtError> {
    match value {
        Value::LispexApplicationOpaque(value) => match &value.payload {
            LispexApplicationOpaquePayload::Value(bytes) => Ok(bytes.clone()),
            LispexApplicationOpaquePayload::Rule { .. }
            | LispexApplicationOpaquePayload::ConsumerArtifact(_) => {
                Err(type_fault("LispexValue", span))
            }
        },
        _ => Err(type_fault("LispexValue", span)),
    }
}

fn consumer_artifact_carrier(bytes: Vec<u8>) -> Value {
    Value::LispexApplicationOpaque(Rc::new(LispexApplicationOpaqueValue {
        payload: LispexApplicationOpaquePayload::ConsumerArtifact(bytes),
    }))
}

fn consumer_artifact(value: Value, span: Span) -> Result<Vec<u8>, RtError> {
    match value {
        Value::LispexApplicationOpaque(value) => match &value.payload {
            LispexApplicationOpaquePayload::ConsumerArtifact(bytes) => Ok(bytes.clone()),
            LispexApplicationOpaquePayload::Rule { .. }
            | LispexApplicationOpaquePayload::Value(_) => {
                Err(type_fault("LispexConsumerArtifact", span))
            }
        },
        _ => Err(type_fault("LispexConsumerArtifact", span)),
    }
}

fn limits_value(value: Value, span: Span) -> Result<LispexEvaluationLimits, RtError> {
    let Value::NominalRecord {
        record_id,
        declaration_identity,
        fields,
        ..
    } = value
    else {
        return Err(type_fault("LispexLimits", span));
    };
    let identity = nominal_declaration_identity(&record_id, declaration_identity.as_deref());
    if !matches!(identity, "LispexLimits" | "std.lispex::LispexLimits")
        || fields.len() != LIMIT_FIELDS.len()
    {
        return Err(type_fault("LispexLimits", span));
    }
    let mut values = [0_u64; 10];
    for (index, (expected, (actual, value))) in LIMIT_FIELDS.iter().zip(fields.iter()).enumerate() {
        if actual.as_ref() != *expected {
            return Err(type_fault("LispexLimits", span));
        }
        let Value::Int(value) = value else {
            return Err(type_fault("LispexLimits", span));
        };
        values[index] = u64::try_from(*value).map_err(|_| type_fault("LispexLimits", span))?;
    }
    Ok(LispexEvaluationLimits::from_values(values))
}

fn limits_to_value(limits: LispexEvaluationLimits, span: Span) -> Result<Value, RtError> {
    let mut fields = Vec::with_capacity(10);
    for (name, value) in LIMIT_FIELDS.into_iter().zip(limits.values()) {
        let value = i64::try_from(value).map_err(|_| {
            fault(
                codes::GUARD_UNIMPLEMENTED,
                "a Lispex limit exceeds the Topaz int range",
                span,
            )
        })?;
        fields.push((Rc::from(name), Value::Int(value)));
    }
    Ok(Value::nominal_record("LispexLimits", fields))
}

fn identity_value(identity: LispexApplicationRuleIdentity) -> Value {
    Value::nominal_record(
        "LispexRuleIdentity",
        [
            (Rc::from("name"), Value::str(identity.name)),
            (Rc::from("profile"), Value::str(identity.profile)),
            (Rc::from("componentId"), Value::str(identity.component_id)),
            (
                Rc::from("evaluatorSha256"),
                Value::str(identity.evaluator_sha256),
            ),
            (
                Rc::from("preparedArtifactSha256"),
                Value::str(identity.prepared_artifact_sha256),
            ),
            (
                Rc::from("preparationRequestSha256"),
                Value::str(identity.preparation_request_sha256),
            ),
        ],
    )
}

fn settlement_value(settlement: LispexApplicationSettlement, span: Span) -> Result<Value, RtError> {
    let value = match settlement.category {
        LispexApplicationSettlementCategory::Complete => enum_value(
            "LispexSettlement",
            "Complete",
            0,
            vec![lispex_value_carrier(settlement.result.ok_or_else(
                || {
                    fault(
                        codes::GUARD_UNIMPLEMENTED,
                        "a complete Lispex settlement has no canonical result",
                        span,
                    )
                },
            )?)],
        ),
        LispexApplicationSettlementCategory::SemanticFailure => enum_value(
            "LispexSettlement",
            "SemanticFailure",
            1,
            vec![Value::str(settlement.code)],
        ),
        LispexApplicationSettlementCategory::LimitExhaustion => enum_value(
            "LispexSettlement",
            "LimitExhaustion",
            2,
            vec![Value::str(settlement.code)],
        ),
        LispexApplicationSettlementCategory::RequestRefusal => enum_value(
            "LispexSettlement",
            "RequestRefusal",
            3,
            vec![Value::str(settlement.code)],
        ),
    };
    Ok(value)
}

fn evidence_settlement_value(
    settlement: LispexApplicationSettlement,
    artifact: Option<Vec<u8>>,
    span: Span,
) -> Result<Value, RtError> {
    let settlement = settlement_value(settlement, span)?;
    Ok(match artifact {
        Some(artifact) => enum_value(
            "LispexEvidenceOutcome",
            "Portable",
            0,
            vec![Value::nominal_record(
                "LispexConsumerEvidence",
                [
                    (Rc::from("settlement"), settlement),
                    (Rc::from("artifact"), consumer_artifact_carrier(artifact)),
                ],
            )],
        ),
        None => enum_value("LispexEvidenceOutcome", "Unrecorded", 1, vec![settlement]),
    })
}

fn inspection_value(
    inspection: LispexConsumerArtifactInspection,
    span: Span,
) -> Result<Value, RtError> {
    let integer = |value: u64| {
        i64::try_from(value).map(Value::Int).map_err(|_| {
            fault(
                codes::GUARD_UNIMPLEMENTED,
                "a Lispex consumer artifact length exceeds the Topaz int range",
                span,
            )
        })
    };
    let optional_string = |value: Option<String>| match value {
        Some(value) => Value::Some(Rc::new(Value::str(value))),
        None => Value::None,
    };
    Ok(Value::nominal_record(
        "LispexConsumerArtifactInspection",
        [
            (Rc::from("kind"), Value::str(inspection.kind)),
            (Rc::from("category"), Value::str(inspection.category)),
            (
                Rc::from("evaluatorSha256"),
                Value::str(inspection.evaluator_sha256),
            ),
            (
                Rc::from("semanticProfileId"),
                optional_string(inspection.semantic_profile_id),
            ),
            (
                Rc::from("artifactSha256"),
                Value::str(inspection.artifact_sha256),
            ),
            (
                Rc::from("artifactBytes"),
                integer(inspection.artifact_bytes)?,
            ),
            (
                Rc::from("portableCoreSha256"),
                optional_string(inspection.portable_core_sha256),
            ),
            (
                Rc::from("portableCoreBytes"),
                integer(inspection.portable_core_bytes)?,
            ),
            (
                Rc::from("authenticated"),
                Value::Bool(inspection.authenticated),
            ),
            (Rc::from("issuer"), Value::None),
        ],
    ))
}

fn evidence_error_value(code: &str) -> Value {
    match code {
        "no-portable-core" => enum_value("LispexEvidenceError", "NoPortableCore", 1, vec![]),
        _ => enum_value(
            "LispexEvidenceError",
            "InvalidConsumerArtifact",
            0,
            vec![Value::str(code)],
        ),
    }
}

fn replay_error_value(code: &str, operational: Option<Value>) -> Value {
    match operational {
        Some(operational) => enum_value(
            "LispexReplayError",
            "OperationalFault",
            3,
            vec![operational],
        ),
        None if code == "replay-mismatch" => {
            enum_value("LispexReplayError", "ReplayMismatch", 2, vec![])
        }
        None if code == "context-mismatch" => enum_value(
            "LispexReplayError",
            "ContextMismatch",
            1,
            vec![Value::str(code)],
        ),
        None => enum_value(
            "LispexReplayError",
            "InvalidConsumerArtifact",
            0,
            vec![Value::str(code)],
        ),
    }
}

fn operational_fault_value(
    code: &str,
    detail: Option<String>,
    span: Span,
) -> Result<Value, RtError> {
    let (variant, index, payloads) = match code {
        "cancelled" => ("Cancelled", 0, vec![]),
        "deadline-exceeded" => ("DeadlineExceeded", 1, vec![]),
        "queue-full" => ("QueueFull", 2, vec![]),
        "total-evaluations-exceeded" => ("TotalEvaluationsExceeded", 3, vec![]),
        "aggregate-input-exceeded" => ("InputQuotaExceeded", 4, vec![]),
        "aggregate-result-exceeded" => ("ResultQuotaExceeded", 5, vec![]),
        "aggregate-output-exceeded" => ("OutputQuotaExceeded", 6, vec![]),
        "aggregate-transcript-exceeded" => ("TranscriptQuotaExceeded", 7, vec![]),
        "aggregate-safety-fuel-exceeded" => ("SafetyFuelQuotaExceeded", 8, vec![]),
        "prepared-bytes-exceeded" => ("PreparedBytesQuotaExceeded", 9, vec![]),
        "cancellation-token-already-used" => ("CancellationTokenAlreadyUsed", 10, vec![]),
        "component-mismatch" => ("ComponentMismatch", 11, vec![]),
        "admission-mismatch" => ("AdmissionMismatch", 12, vec![]),
        "safety-preemption" => ("SafetyPreemption", 13, vec![]),
        "target-unavailable" => ("TargetUnavailable", 14, vec![]),
        "engine-failure" => (
            "EngineFailure",
            15,
            vec![Value::str(
                detail.unwrap_or_else(|| "engine failure".to_string()),
            )],
        ),
        _ => {
            return Err(fault(
                codes::GUARD_UNIMPLEMENTED,
                format!("unknown Lispex operational fault `{code}`"),
                span,
            ));
        }
    };
    Ok(enum_value(
        "LispexOperationalFault",
        variant,
        index,
        payloads,
    ))
}

fn enum_value(enum_id: &str, variant: &str, variant_index: u32, payloads: Vec<Value>) -> Value {
    Value::Enum {
        enum_id: Rc::from(enum_id),
        declaration_identity: None,
        method_identity: None,
        variant: Rc::from(variant),
        variant_index,
        payloads: Rc::from(payloads),
    }
}

fn bytes_value(value: Value, expected: &str, span: Span) -> Result<Vec<u8>, RtError> {
    match value {
        Value::Bytes(bytes) => Ok(bytes.as_ref().to_vec()),
        _ => Err(type_fault(expected, span)),
    }
}

fn type_fault(expected: &str, span: Span) -> RtError {
    fault(
        codes::GUARD_TYPE,
        format!("expected `{expected}` at the Lispex application boundary"),
        span,
    )
}
