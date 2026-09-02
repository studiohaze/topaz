use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use topaz_value::{
    Host, HostDirEntry, LispexApplicationRequest, LispexApplicationResponse,
    LispexApplicationRuleIdentity, LispexApplicationSettlement,
    LispexApplicationSettlementCategory, LispexConsumerArtifactInspection, LispexEvaluationLimits,
    ResourceId, Value,
};

use super::{
    ApplicationError, ApplicationQuotas, ApplicationRuntime, ArtifactCategory, ArtifactKind,
    COMPONENT_ID, CancellationToken, EVALUATOR_SHA256, EvaluateLimits, LispexValue, PROFILE_ID,
    PreparedRule, ReusableRuntime, RunError, SettledCategory, decode_artifact, inspect_artifact,
    verify_artifact, wrap_evaluate_artifact,
};
#[cfg(feature = "full-profile-contract")]
use super::{
    FULL_COMPONENT_ID, FULL_EVALUATOR_SHA256, FULL_FEATURE_SET_SHA256, FULL_PROFILE_ID,
    FullArtifactCategory, FullArtifactKind, decode_full_artifact, inspect_full_artifact,
    verify_full_artifact, wrap_full_evaluate_artifact,
};

/// One exact package-locked rule admitted to the first-class application host.
#[derive(Clone, Debug)]
pub struct AdmittedApplicationRule {
    identity: LispexApplicationRuleIdentity,
    prepared_artifact: Vec<u8>,
    limits: EvaluateLimits,
    prepared: PreparedRule,
}

impl AdmittedApplicationRule {
    /// Verifies a locked prepared artifact and binds it to the declared rule identity.
    pub fn from_locked_artifact(
        identity: LispexApplicationRuleIdentity,
        preparation_submission_sha256: &str,
        artifact: &[u8],
        limits: EvaluateLimits,
    ) -> Result<Self, String> {
        require_digest(
            &identity.prepared_artifact_sha256,
            &sha256_hex(artifact),
            "prepared artifact",
        )?;
        let prepared = match identity.profile.as_str() {
            PROFILE_ID => {
                if identity.component_id != COMPONENT_ID {
                    return Err(
                        "the prepared rule component does not match the admitted component".into(),
                    );
                }
                require_digest(&identity.evaluator_sha256, EVALUATOR_SHA256, "evaluator")?;
                ReusableRuntime::embedded()
                    .map_err(|error| error.to_string())?
                    .load_prepared_consumer_artifact(
                        artifact,
                        &identity.preparation_request_sha256,
                        preparation_submission_sha256,
                    )
                    .map_err(|error| error.to_string())?
            }
            #[cfg(feature = "full-profile-contract")]
            FULL_PROFILE_ID => {
                if identity.component_id != FULL_COMPONENT_ID {
                    return Err(
                        "the full prepared rule component does not match the admitted component"
                            .into(),
                    );
                }
                require_digest(
                    &identity.evaluator_sha256,
                    FULL_EVALUATOR_SHA256,
                    "evaluator",
                )?;
                let decoded = decode_full_artifact(artifact).map_err(|error| error.to_string())?;
                if decoded.kind != FullArtifactKind::Prepare
                    || decoded.identities[4].as_deref() != Some(FULL_FEATURE_SET_SHA256)
                    || decoded.identities[5].as_deref()
                        != Some(
                            preparation_submission_sha256
                                .strip_prefix("sha256:")
                                .ok_or("the preparation submission lacks sha256 identity")?,
                        )
                {
                    return Err("the full prepared artifact identity is stale".into());
                }
                ReusableRuntime::full_profile()
                    .map_err(|error| error.to_string())?
                    .load_full_prepared_consumer_artifact(
                        artifact,
                        &identity.preparation_request_sha256,
                    )
                    .map_err(|error| error.to_string())?
            }
            _ => {
                return Err("the prepared rule profile does not match an admitted profile".into());
            }
        };
        Ok(Self {
            identity,
            prepared_artifact: artifact.to_vec(),
            limits,
            prepared,
        })
    }

    #[must_use]
    pub fn identity(&self) -> &LispexApplicationRuleIdentity {
        &self.identity
    }
}

/// A deny-by-default Host decorator that admits only the exact rule closure
/// supplied by a checked package target.
pub struct LispexApplicationHost<H> {
    inner: H,
    runtime: ApplicationRuntime,
    profile: String,
    rules: BTreeMap<String, AdmittedApplicationRule>,
}

impl<H> LispexApplicationHost<H> {
    pub fn new(
        inner: H,
        rules: impl IntoIterator<Item = AdmittedApplicationRule>,
        quotas: ApplicationQuotas,
    ) -> Result<Self, String> {
        let mut catalog = BTreeMap::new();
        let mut profile = None;
        for rule in rules {
            match &profile {
                Some(profile) if profile != &rule.identity.profile => {
                    return Err("a Lispex application host cannot mix profiles".into());
                }
                None => profile = Some(rule.identity.profile.clone()),
                _ => {}
            }
            let target_identity = format!("topaz.lispex-rule-handle/v1:{}", rule.identity.name);
            if catalog.insert(target_identity.clone(), rule).is_some() {
                return Err(format!(
                    "duplicate admitted Lispex rule target `{target_identity}`"
                ));
            }
        }
        if catalog.is_empty() {
            return Err("an admitted Lispex application host needs a reached rule".into());
        }
        let profile = profile.expect("nonempty application catalog");
        let runtime = match profile.as_str() {
            PROFILE_ID => ApplicationRuntime::new(quotas),
            #[cfg(feature = "full-profile-contract")]
            FULL_PROFILE_ID => ApplicationRuntime::full_profile(quotas),
            _ => return Err("the application host profile is not implemented".into()),
        }
        .map_err(|error| error.to_string())?;
        Ok(Self {
            inner,
            runtime,
            profile,
            rules: catalog,
        })
    }

    #[must_use]
    pub fn inner(&self) -> &H {
        &self.inner
    }

    fn exact_rule(
        &self,
        identity: &LispexApplicationRuleIdentity,
        prepared_artifact: &[u8],
    ) -> Result<&AdmittedApplicationRule, ApplicationOperationalFailure> {
        let target_identity = format!("topaz.lispex-rule-handle/v1:{}", identity.name);
        let Some(rule) = self.rules.get(&target_identity) else {
            return Err(ApplicationOperationalFailure::new(
                "admission-mismatch",
                None,
            ));
        };
        if &rule.identity != identity
            || rule.prepared_artifact != prepared_artifact
            || identity.prepared_artifact_sha256
                != format!("sha256:{}", sha256_hex(prepared_artifact))
            || !verify_prepared_for_profile(&self.profile, prepared_artifact)
        {
            return Err(ApplicationOperationalFailure::new(
                "admission-mismatch",
                None,
            ));
        }
        Ok(rule)
    }
}

impl<H: Host> Host for LispexApplicationHost<H> {
    fn print(&self, line: &str) {
        self.inner.print(line);
    }

    fn open(&self, path: &str) -> Result<ResourceId, String> {
        self.inner.open(path)
    }

    fn read(&self, handle: ResourceId) -> Result<String, String> {
        self.inner.read(handle)
    }

    fn write(&self, handle: ResourceId, value: &str) -> Result<(), String> {
        self.inner.write(handle, value)
    }

    fn close(&self, handle: ResourceId) {
        self.inner.close(handle);
    }

    fn now_millis(&self) -> u64 {
        self.inner.now_millis()
    }

    fn defer_error(&self, rendered: &str) {
        self.inner.defer_error(rendered);
    }

    fn input(&self) -> String {
        self.inner.input()
    }

    fn read_bytes(&self, path: &str) -> Result<Vec<u8>, String> {
        self.inner.read_bytes(path)
    }

    fn write_bytes(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.inner.write_bytes(path, bytes)
    }

    fn list_dir(&self, path: &str) -> Result<Vec<HostDirEntry>, String> {
        self.inner.list_dir(path)
    }

    fn extern_call(&self, module: &str, function: &str, args: &[Value]) -> Result<Value, String> {
        self.inner.extern_call(module, function, args)
    }

    fn lispex_application(&self, request: LispexApplicationRequest) -> LispexApplicationResponse {
        match request {
            LispexApplicationRequest::Rule { target_identity } => {
                match self.rules.get(&target_identity) {
                    Some(rule) => LispexApplicationResponse::Rule {
                        identity: rule.identity.clone(),
                        prepared_artifact: rule.prepared_artifact.clone(),
                    },
                    None => operational("admission-mismatch", None),
                }
            }
            LispexApplicationRequest::ValueFromCanonical { bytes } => {
                match LispexValue::from_canonical(bytes) {
                    Ok(value) => {
                        LispexApplicationResponse::CanonicalValue(value.into_canonical_bytes())
                    }
                    Err(error) => LispexApplicationResponse::ValueRefusal(error.code().into()),
                }
            }
            LispexApplicationRequest::CanonicalBytes { bytes } => {
                match LispexValue::from_canonical(bytes) {
                    Ok(value) => {
                        LispexApplicationResponse::CanonicalValue(value.into_canonical_bytes())
                    }
                    Err(error) => LispexApplicationResponse::ValueRefusal(error.code().into()),
                }
            }
            LispexApplicationRequest::DefaultLimits {
                rule,
                prepared_artifact,
            } => match self.exact_rule(&rule, &prepared_artifact) {
                Ok(rule) => LispexApplicationResponse::Limits(to_boundary_limits(rule.limits)),
                Err(failure) => failure.into_response(),
            },
            LispexApplicationRequest::InspectRule {
                rule,
                prepared_artifact,
            } => match self.exact_rule(&rule, &prepared_artifact) {
                Ok(rule) => LispexApplicationResponse::Identity(rule.identity.clone()),
                Err(failure) => failure.into_response(),
            },
            LispexApplicationRequest::Evaluate {
                rule,
                prepared_artifact,
                input,
                limits,
            } => match self.exact_rule(&rule, &prepared_artifact) {
                Err(failure) => failure.into_response(),
                Ok(rule) => match evaluate_rule(&self.runtime, rule, input, limits) {
                    Ok((settlement, _)) => LispexApplicationResponse::Settlement(settlement),
                    Err(failure) => failure.into_response(),
                },
            },
            LispexApplicationRequest::EvaluateWithEvidence {
                rule,
                prepared_artifact,
                input,
                limits,
            } => match self.exact_rule(&rule, &prepared_artifact) {
                Err(failure) => failure.into_response(),
                Ok(rule) => match evaluate_rule(&self.runtime, rule, input, limits) {
                    Ok((settlement, None)) => LispexApplicationResponse::EvidenceSettlement {
                        settlement,
                        artifact: None,
                    },
                    Ok((settlement, Some(record))) => match wrap_evaluate_for_profile(
                        &self.profile,
                        &record,
                        &rule.prepared_artifact,
                        limits.values(),
                    ) {
                        Ok(artifact) => LispexApplicationResponse::EvidenceSettlement {
                            settlement,
                            artifact: Some(artifact),
                        },
                        Err(error) => operational("engine-failure", Some(error.to_string())),
                    },
                    Err(failure) => failure.into_response(),
                },
            },
            LispexApplicationRequest::ConsumerArtifactFromBytes { bytes } => {
                match verify_consumer_for_profile(&self.profile, &bytes) {
                    Ok(()) => LispexApplicationResponse::ConsumerArtifact(bytes),
                    Err(code) => LispexApplicationResponse::EvidenceRefusal(code.into()),
                }
            }
            LispexApplicationRequest::ConsumerArtifactBytes { artifact } => {
                match verify_consumer_for_profile(&self.profile, &artifact) {
                    Ok(()) => LispexApplicationResponse::ConsumerArtifactBytes(artifact),
                    Err(code) => LispexApplicationResponse::EvidenceRefusal(code.into()),
                }
            }
            LispexApplicationRequest::PortableCoreBytes { artifact } => {
                match portable_core_for_profile(&self.profile, &artifact) {
                    Ok(portable_core) => {
                        LispexApplicationResponse::ConsumerArtifactBytes(portable_core)
                    }
                    Err(code) => LispexApplicationResponse::EvidenceRefusal(code.into()),
                }
            }
            LispexApplicationRequest::InspectConsumerArtifact { artifact }
            | LispexApplicationRequest::VerifyConsumerArtifact { artifact } => {
                match inspect_consumer_for_profile(&self.profile, &artifact) {
                    Ok(inspection) => LispexApplicationResponse::ConsumerArtifactInspection(
                        to_boundary_inspection(inspection),
                    ),
                    Err(code) => LispexApplicationResponse::EvidenceRefusal(code.into()),
                }
            }
            LispexApplicationRequest::FreshReplay {
                rule,
                prepared_artifact,
                input,
                artifact,
            } => match self.exact_rule(&rule, &prepared_artifact) {
                Err(_) => LispexApplicationResponse::EvidenceRefusal("context-mismatch".into()),
                Ok(rule) => {
                    fresh_replay_for_profile(&self.profile, &self.runtime, rule, input, artifact)
                }
            },
        }
    }
}

fn verify_prepared_for_profile(profile: &str, artifact: &[u8]) -> bool {
    match profile {
        PROFILE_ID => verify_artifact(artifact).is_ok(),
        #[cfg(feature = "full-profile-contract")]
        FULL_PROFILE_ID => verify_full_artifact(artifact).is_ok(),
        _ => false,
    }
}

fn verify_consumer_for_profile(profile: &str, artifact: &[u8]) -> Result<(), &'static str> {
    match profile {
        PROFILE_ID => verify_artifact(artifact).map_err(|error| error.code()),
        #[cfg(feature = "full-profile-contract")]
        FULL_PROFILE_ID => verify_full_artifact(artifact).map_err(|error| error.code()),
        _ => Err("profile-mismatch"),
    }
}

fn portable_core_for_profile(profile: &str, artifact: &[u8]) -> Result<Vec<u8>, &'static str> {
    let portable_core = match profile {
        PROFILE_ID => {
            decode_artifact(artifact)
                .map_err(|error| error.code())?
                .portable_core
        }
        #[cfg(feature = "full-profile-contract")]
        FULL_PROFILE_ID => {
            decode_full_artifact(artifact)
                .map_err(|error| error.code())?
                .portable_core
        }
        _ => return Err("profile-mismatch"),
    };
    if portable_core.is_empty() {
        Err("no-portable-core")
    } else {
        Ok(portable_core)
    }
}

fn inspect_consumer_for_profile(
    profile: &str,
    artifact: &[u8],
) -> Result<super::ConsumerArtifactInspection, &'static str> {
    match profile {
        PROFILE_ID => inspect_artifact(artifact).map_err(|error| error.code()),
        #[cfg(feature = "full-profile-contract")]
        FULL_PROFILE_ID => inspect_full_artifact(artifact).map_err(|error| error.code()),
        _ => Err("profile-mismatch"),
    }
}

fn wrap_evaluate_for_profile(
    profile: &str,
    record: &super::RawEvaluation,
    prepared_artifact: &[u8],
    limits: [u64; 10],
) -> Result<Vec<u8>, String> {
    match profile {
        PROFILE_ID => wrap_evaluate_artifact(
            &record.response_bytes,
            &record.request_bytes,
            prepared_artifact,
            limits,
        )
        .map_err(|error| error.to_string()),
        #[cfg(feature = "full-profile-contract")]
        FULL_PROFILE_ID => wrap_full_evaluate_artifact(
            &record.response_bytes,
            &record.request_bytes,
            prepared_artifact,
            limits,
        )
        .map_err(|error| error.to_string()),
        _ => Err("the application host profile is not implemented".into()),
    }
}

fn evaluate_rule(
    runtime: &ApplicationRuntime,
    rule: &AdmittedApplicationRule,
    input: Vec<u8>,
    limits: LispexEvaluationLimits,
) -> Result<
    (LispexApplicationSettlement, Option<super::RawEvaluation>),
    ApplicationOperationalFailure,
> {
    if !within_limits(limits, rule.limits) {
        return Ok((
            settlement(
                LispexApplicationSettlementCategory::RequestRefusal,
                "limits-exceed-locked-maximum".into(),
                None,
            ),
            None,
        ));
    }
    let input = match LispexValue::from_canonical(input) {
        Ok(input) => input,
        Err(error) => {
            return Ok((
                settlement(
                    LispexApplicationSettlementCategory::RequestRefusal,
                    error.code().into(),
                    None,
                ),
                None,
            ));
        }
    };
    match runtime.evaluate(
        &rule.prepared,
        input.canonical_bytes(),
        from_boundary_limits(limits),
        &CancellationToken::new(),
    ) {
        Ok(record) => {
            let result = match record.result.as_ref() {
                Some(bytes) => match LispexValue::from_guest_result(bytes.clone()) {
                    Ok(value) => Some(value.into_canonical_bytes()),
                    Err(error) => {
                        return Err(ApplicationOperationalFailure::new(
                            "engine-failure",
                            Some(error.to_string()),
                        ));
                    }
                },
                None => None,
            };
            let category = match record.category {
                SettledCategory::Complete => LispexApplicationSettlementCategory::Complete,
                SettledCategory::SemanticFailure => {
                    LispexApplicationSettlementCategory::SemanticFailure
                }
                SettledCategory::LimitExhaustion => {
                    LispexApplicationSettlementCategory::LimitExhaustion
                }
            };
            let code = record.code.clone();
            Ok((settlement(category, code, result), Some(record)))
        }
        Err(ApplicationError::Operational(fault)) => {
            Err(ApplicationOperationalFailure::new(fault.code(), None))
        }
        Err(ApplicationError::Runtime(RunError::RequestRefusal(code))) => Ok((
            settlement(
                LispexApplicationSettlementCategory::RequestRefusal,
                code,
                None,
            ),
            None,
        )),
        Err(ApplicationError::Runtime(RunError::InputRefusal(code))) => Ok((
            settlement(
                LispexApplicationSettlementCategory::RequestRefusal,
                code.into(),
                None,
            ),
            None,
        )),
        Err(ApplicationError::Runtime(RunError::SelectionRefusal(_))) => Err(
            ApplicationOperationalFailure::new("component-mismatch", None),
        ),
        Err(ApplicationError::Runtime(RunError::ContractViolation(_))) => Err(
            ApplicationOperationalFailure::new("admission-mismatch", None),
        ),
        Err(ApplicationError::Runtime(RunError::EngineFault("safety-fuel-exhausted"))) => Err(
            ApplicationOperationalFailure::new("safety-preemption", None),
        ),
        Err(ApplicationError::Runtime(error)) => Err(ApplicationOperationalFailure::new(
            "engine-failure",
            Some(error.to_string()),
        )),
        Err(ApplicationError::Configuration(reason)) => Err(ApplicationOperationalFailure::new(
            "engine-failure",
            Some(reason.into()),
        )),
    }
}

fn fresh_replay_for_profile(
    profile: &str,
    runtime: &ApplicationRuntime,
    rule: &AdmittedApplicationRule,
    input: Vec<u8>,
    artifact_bytes: Vec<u8>,
) -> LispexApplicationResponse {
    match profile {
        PROFILE_ID => fresh_replay(runtime, rule, input, artifact_bytes),
        #[cfg(feature = "full-profile-contract")]
        FULL_PROFILE_ID => fresh_replay_full(runtime, rule, input, artifact_bytes),
        _ => LispexApplicationResponse::EvidenceRefusal("profile-mismatch".into()),
    }
}

fn fresh_replay(
    runtime: &ApplicationRuntime,
    rule: &AdmittedApplicationRule,
    input: Vec<u8>,
    artifact_bytes: Vec<u8>,
) -> LispexApplicationResponse {
    let artifact = match decode_artifact(&artifact_bytes) {
        Ok(artifact) => artifact,
        Err(error) => return LispexApplicationResponse::EvidenceRefusal(error.code().into()),
    };
    if artifact.kind != ArtifactKind::Evaluate
        || !matches!(
            artifact.category,
            ArtifactCategory::Complete
                | ArtifactCategory::SemanticFailure
                | ArtifactCategory::LimitExhaustion
        )
        || artifact.exact_limits.len() != 14
    {
        return LispexApplicationResponse::EvidenceRefusal("context-mismatch".into());
    }
    let prepared = match decode_artifact(&rule.prepared_artifact) {
        Ok(prepared) => prepared,
        Err(error) => {
            return LispexApplicationResponse::ReplayFault {
                code: "operational-fault".into(),
                operational_code: Some("admission-mismatch".into()),
                detail: Some(error.to_string()),
            };
        }
    };
    if artifact.evaluator_sha256 != rule.identity.evaluator_sha256.trim_start_matches("sha256:")
        || artifact.exact_limits[..4] != prepared.exact_limits
        || artifact.identities[..5] != prepared.identities[..5]
    {
        return LispexApplicationResponse::EvidenceRefusal("context-mismatch".into());
    }
    let limits = LispexEvaluationLimits::from_values(
        artifact.exact_limits[4..]
            .try_into()
            .expect("the exact artifact limit count was checked"),
    );
    match evaluate_rule(runtime, rule, input, limits) {
        Ok((_, None)) => LispexApplicationResponse::EvidenceRefusal("replay-mismatch".into()),
        Ok((settlement, Some(record))) => {
            if record.fresh_instances != 1 {
                return LispexApplicationResponse::ReplayFault {
                    code: "operational-fault".into(),
                    operational_code: Some("engine-failure".into()),
                    detail: Some("fresh replay reused evaluator state".into()),
                };
            }
            match wrap_evaluate_artifact(
                &record.response_bytes,
                &record.request_bytes,
                &rule.prepared_artifact,
                limits.values(),
            ) {
                Ok(replayed) if replayed == artifact_bytes => {
                    LispexApplicationResponse::Settlement(settlement)
                }
                Ok(_) => LispexApplicationResponse::EvidenceRefusal("replay-mismatch".into()),
                Err(error) => LispexApplicationResponse::ReplayFault {
                    code: "operational-fault".into(),
                    operational_code: Some("engine-failure".into()),
                    detail: Some(error.to_string()),
                },
            }
        }
        Err(ApplicationOperationalFailure { code, detail }) => {
            LispexApplicationResponse::ReplayFault {
                code: "operational-fault".into(),
                operational_code: Some(code),
                detail,
            }
        }
    }
}

#[cfg(feature = "full-profile-contract")]
fn fresh_replay_full(
    runtime: &ApplicationRuntime,
    rule: &AdmittedApplicationRule,
    input: Vec<u8>,
    artifact_bytes: Vec<u8>,
) -> LispexApplicationResponse {
    let artifact = match decode_full_artifact(&artifact_bytes) {
        Ok(artifact) => artifact,
        Err(error) => return LispexApplicationResponse::EvidenceRefusal(error.code().into()),
    };
    if artifact.kind != FullArtifactKind::Evaluate
        || !matches!(
            artifact.category,
            FullArtifactCategory::Complete
                | FullArtifactCategory::SemanticFailure
                | FullArtifactCategory::LimitExhaustion
        )
        || artifact.exact_limits.len() != 14
    {
        return LispexApplicationResponse::EvidenceRefusal("context-mismatch".into());
    }
    let prepared = match decode_full_artifact(&rule.prepared_artifact) {
        Ok(prepared) => prepared,
        Err(error) => {
            return LispexApplicationResponse::ReplayFault {
                code: "operational-fault".into(),
                operational_code: Some("admission-mismatch".into()),
                detail: Some(error.to_string()),
            };
        }
    };
    if artifact.evaluator_sha256 != rule.identity.evaluator_sha256.trim_start_matches("sha256:")
        || artifact.exact_limits[..4] != prepared.exact_limits
        || artifact.identities[..5] != prepared.identities[..5]
    {
        return LispexApplicationResponse::EvidenceRefusal("context-mismatch".into());
    }
    let limits = LispexEvaluationLimits::from_values(
        artifact.exact_limits[4..]
            .try_into()
            .expect("the exact full artifact limit count was checked"),
    );
    match evaluate_rule(runtime, rule, input, limits) {
        Ok((_, None)) => LispexApplicationResponse::EvidenceRefusal("replay-mismatch".into()),
        Ok((settlement, Some(record))) => {
            if record.fresh_instances != 1 {
                return LispexApplicationResponse::ReplayFault {
                    code: "operational-fault".into(),
                    operational_code: Some("engine-failure".into()),
                    detail: Some("fresh replay reused full evaluator state".into()),
                };
            }
            match wrap_full_evaluate_artifact(
                &record.response_bytes,
                &record.request_bytes,
                &rule.prepared_artifact,
                limits.values(),
            ) {
                Ok(replayed) if replayed == artifact_bytes => {
                    LispexApplicationResponse::Settlement(settlement)
                }
                Ok(_) => LispexApplicationResponse::EvidenceRefusal("replay-mismatch".into()),
                Err(error) => LispexApplicationResponse::ReplayFault {
                    code: "operational-fault".into(),
                    operational_code: Some("engine-failure".into()),
                    detail: Some(error.to_string()),
                },
            }
        }
        Err(ApplicationOperationalFailure { code, detail }) => {
            LispexApplicationResponse::ReplayFault {
                code: "operational-fault".into(),
                operational_code: Some(code),
                detail,
            }
        }
    }
}

fn settlement(
    category: LispexApplicationSettlementCategory,
    code: String,
    result: Option<Vec<u8>>,
) -> LispexApplicationSettlement {
    LispexApplicationSettlement {
        category,
        code,
        result,
    }
}

fn to_boundary_inspection(
    inspection: super::ConsumerArtifactInspection,
) -> LispexConsumerArtifactInspection {
    LispexConsumerArtifactInspection {
        kind: match inspection.kind {
            ArtifactKind::Prepare => "prepare",
            ArtifactKind::Evaluate => "evaluate",
        }
        .into(),
        category: match inspection.category {
            ArtifactCategory::Prepared => "prepared",
            ArtifactCategory::Complete => "complete",
            ArtifactCategory::SemanticFailure => "semantic-failure",
            ArtifactCategory::LimitExhaustion => "limit-exhaustion",
            ArtifactCategory::RequestRefusal => "request-refusal",
            ArtifactCategory::EngineFault => "engine-fault",
        }
        .into(),
        evaluator_sha256: format!("sha256:{}", inspection.evaluator_sha256),
        semantic_profile_id: inspection.semantic_profile_id,
        artifact_sha256: format!("sha256:{}", inspection.artifact_sha256),
        artifact_bytes: inspection.artifact_bytes,
        portable_core_sha256: inspection
            .portable_core_sha256
            .map(|digest| format!("sha256:{digest}")),
        portable_core_bytes: inspection.portable_core_bytes,
        authenticated: inspection.authenticated,
    }
}

fn operational(code: &str, detail: Option<String>) -> LispexApplicationResponse {
    LispexApplicationResponse::OperationalFault {
        code: code.into(),
        detail,
    }
}

struct ApplicationOperationalFailure {
    code: String,
    detail: Option<String>,
}

impl ApplicationOperationalFailure {
    fn new(code: &str, detail: Option<String>) -> Self {
        Self {
            code: code.into(),
            detail,
        }
    }

    fn into_response(self) -> LispexApplicationResponse {
        LispexApplicationResponse::OperationalFault {
            code: self.code,
            detail: self.detail,
        }
    }
}

fn to_boundary_limits(limits: EvaluateLimits) -> LispexEvaluationLimits {
    LispexEvaluationLimits {
        canonical_input_bytes: limits.canonical_input_bytes,
        eval_work: limits.eval_work,
        logical_allocation: limits.logical_allocation,
        semantic_frames: limits.semantic_frames,
        traversal_depth: limits.traversal_depth,
        output_bytes: limits.output_bytes,
        diagnostic_bytes: limits.diagnostic_bytes,
        transcript_bytes: limits.transcript_bytes,
        transcript_events: limits.transcript_events,
        result_bytes: limits.result_bytes,
    }
}

fn from_boundary_limits(limits: LispexEvaluationLimits) -> EvaluateLimits {
    EvaluateLimits {
        canonical_input_bytes: limits.canonical_input_bytes,
        eval_work: limits.eval_work,
        logical_allocation: limits.logical_allocation,
        semantic_frames: limits.semantic_frames,
        traversal_depth: limits.traversal_depth,
        output_bytes: limits.output_bytes,
        diagnostic_bytes: limits.diagnostic_bytes,
        transcript_bytes: limits.transcript_bytes,
        transcript_events: limits.transcript_events,
        result_bytes: limits.result_bytes,
    }
}

fn within_limits(requested: LispexEvaluationLimits, maximum: EvaluateLimits) -> bool {
    requested
        .values()
        .into_iter()
        .zip(to_boundary_limits(maximum).values())
        .all(|(requested, maximum)| requested <= maximum)
}

fn require_digest(actual: &str, expected: &str, label: &str) -> Result<(), String> {
    if actual == format!("sha256:{expected}") {
        Ok(())
    } else {
        Err(format!(
            "the {label} digest does not match the admitted bytes"
        ))
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut text = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(text, "{byte:02x}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    struct InnerHost;

    impl Host for InnerHost {
        fn print(&self, _line: &str) {}

        fn open(&self, _path: &str) -> Result<ResourceId, String> {
            Err("unavailable".into())
        }

        fn read(&self, _handle: ResourceId) -> Result<String, String> {
            Err("unavailable".into())
        }

        fn write(&self, _handle: ResourceId, _value: &str) -> Result<(), String> {
            Err("unavailable".into())
        }

        fn close(&self, _handle: ResourceId) {}

        fn now_millis(&self) -> u64 {
            0
        }

        fn defer_error(&self, _rendered: &str) {}

        fn lispex_application(
            &self,
            _request: LispexApplicationRequest,
        ) -> LispexApplicationResponse {
            operational("target-unavailable", None)
        }
    }

    #[test]
    fn requested_limits_are_compared_field_by_field() {
        let maximum = super::EvaluateLimits {
            canonical_input_bytes: 1,
            eval_work: 2,
            logical_allocation: 3,
            semantic_frames: 4,
            traversal_depth: 5,
            output_bytes: 6,
            diagnostic_bytes: 7,
            transcript_bytes: 8,
            transcript_events: 9,
            result_bytes: 10,
        };
        let exact = to_boundary_limits(maximum);
        assert!(within_limits(exact, maximum));
        let mut too_large = exact;
        too_large.eval_work += 1;
        assert!(!within_limits(too_large, maximum));

        let mut lexicographically_smaller_but_invalid = exact;
        lexicographically_smaller_but_invalid.canonical_input_bytes = 0;
        lexicographically_smaller_but_invalid.eval_work += 1;
        assert!(!within_limits(
            lexicographically_smaller_but_invalid,
            maximum
        ));
    }

    #[test]
    fn exact_host_revalidates_handles_and_returns_atomic_settlement() {
        let source = b"(if (< 10 15) \"allow\" \"deny\")\n";
        let limits = super::super::Limits::MAXIMUM;
        let artifact = super::super::prepare_consumer_artifact(source, limits.prepare)
            .expect("prepared consumer artifact");
        let request = super::super::preparation_request_sha256(source, limits.prepare)
            .expect("preparation request digest");
        let submission = super::super::preparation_submission_sha256(source, limits.prepare)
            .expect("preparation submission digest");
        let rule = AdmittedApplicationRule::from_locked_artifact(
            LispexApplicationRuleIdentity {
                name: "identity".into(),
                profile: PROFILE_ID.into(),
                component_id: COMPONENT_ID.into(),
                evaluator_sha256: format!("sha256:{EVALUATOR_SHA256}"),
                prepared_artifact_sha256: format!("sha256:{}", sha256_hex(&artifact)),
                preparation_request_sha256: format!("sha256:{request}"),
            },
            &format!("sha256:{submission}"),
            &artifact,
            limits.evaluate,
        )
        .expect("exact admitted rule");
        let host = LispexApplicationHost::new(
            InnerHost,
            [rule],
            ApplicationQuotas {
                concurrent_evaluations: 1,
                queued_evaluations: 0,
                total_evaluations: 2,
                aggregate_input_bytes: 2,
                aggregate_result_bytes: limits.evaluate.result_bytes * 2,
                aggregate_output_bytes: limits.evaluate.output_bytes * 2,
                aggregate_transcript_bytes: limits.evaluate.transcript_bytes * 2,
                aggregate_safety_fuel: super::super::SAFETY_FUEL * 2,
                prepared_bytes: 1_000_000,
                wall_millis: 5_000,
            },
        )
        .expect("application host");
        let LispexApplicationResponse::Rule {
            identity,
            prepared_artifact,
        } = host.lispex_application(LispexApplicationRequest::Rule {
            target_identity: "topaz.lispex-rule-handle/v1:identity".into(),
        })
        else {
            panic!("exact rule was not admitted")
        };
        let LispexApplicationResponse::Limits(boundary_limits) =
            host.lispex_application(LispexApplicationRequest::DefaultLimits {
                rule: identity.clone(),
                prepared_artifact: prepared_artifact.clone(),
            })
        else {
            panic!("exact limits were not returned")
        };
        let response = host.lispex_application(LispexApplicationRequest::Evaluate {
            rule: identity.clone(),
            prepared_artifact: prepared_artifact.clone(),
            input: vec![0],
            limits: boundary_limits,
        });
        let LispexApplicationResponse::Settlement(settlement) = response else {
            panic!("evaluation did not settle: {response:?}")
        };
        assert_eq!(
            settlement.category,
            LispexApplicationSettlementCategory::Complete
        );
        let result = settlement.result.expect("complete result bytes");
        LispexValue::from_guest_result(result).expect("complete result is canonical");

        let mut tampered = prepared_artifact;
        tampered[0] ^= 1;
        assert!(matches!(
            host.lispex_application(LispexApplicationRequest::InspectRule {
                rule: identity,
                prepared_artifact: tampered,
            }),
            LispexApplicationResponse::OperationalFault { code, .. }
                if code == "admission-mismatch"
        ));
    }

    #[cfg(feature = "full-profile-contract")]
    #[test]
    fn full_profile_host_evaluates_exact_rules_and_rejects_mixed_profiles() {
        let source = b"(if (< 10 15) \"allow\" \"deny\")\n";
        let limits = super::super::Limits::MAXIMUM;
        let artifact = super::super::prepare_full_consumer_artifact(source, limits.prepare)
            .expect("full prepared consumer artifact");
        let request = super::super::preparation_request_sha256(source, limits.prepare)
            .expect("full preparation request digest");
        let submission = super::super::preparation_submission_sha256(source, limits.prepare)
            .expect("full preparation submission digest");
        let full_rule = AdmittedApplicationRule::from_locked_artifact(
            LispexApplicationRuleIdentity {
                name: "complete-profile".into(),
                profile: FULL_PROFILE_ID.into(),
                component_id: FULL_COMPONENT_ID.into(),
                evaluator_sha256: format!("sha256:{FULL_EVALUATOR_SHA256}"),
                prepared_artifact_sha256: format!("sha256:{}", sha256_hex(&artifact)),
                preparation_request_sha256: format!("sha256:{request}"),
            },
            &format!("sha256:{submission}"),
            &artifact,
            limits.evaluate,
        )
        .expect("exact full admitted rule");
        let full_rule_for_mixed_host = full_rule.clone();
        let host = LispexApplicationHost::new(
            InnerHost,
            [full_rule],
            ApplicationQuotas {
                concurrent_evaluations: 1,
                queued_evaluations: 0,
                total_evaluations: 3,
                aggregate_input_bytes: 3,
                aggregate_result_bytes: limits.evaluate.result_bytes * 3,
                aggregate_output_bytes: limits.evaluate.output_bytes * 3,
                aggregate_transcript_bytes: limits.evaluate.transcript_bytes * 3,
                aggregate_safety_fuel: super::super::SAFETY_FUEL * 3,
                prepared_bytes: 1_000_000,
                wall_millis: 60_000,
            },
        )
        .expect("full application host");
        let LispexApplicationResponse::Rule {
            identity,
            prepared_artifact,
        } = host.lispex_application(LispexApplicationRequest::Rule {
            target_identity: "topaz.lispex-rule-handle/v1:complete-profile".into(),
        })
        else {
            panic!("exact full rule was not admitted")
        };
        let LispexApplicationResponse::Limits(boundary_limits) =
            host.lispex_application(LispexApplicationRequest::DefaultLimits {
                rule: identity.clone(),
                prepared_artifact: prepared_artifact.clone(),
            })
        else {
            panic!("full rule limits were not returned")
        };
        let response = host.lispex_application(LispexApplicationRequest::Evaluate {
            rule: identity.clone(),
            prepared_artifact: prepared_artifact.clone(),
            input: vec![0],
            limits: boundary_limits,
        });
        let LispexApplicationResponse::Settlement(settlement) = response else {
            panic!("full evaluation did not settle: {response:?}")
        };
        assert_eq!(
            settlement.category,
            LispexApplicationSettlementCategory::Complete
        );
        LispexValue::from_guest_result(settlement.result.expect("full complete result bytes"))
            .expect("full complete result is canonical");
        let evidence = host.lispex_application(LispexApplicationRequest::EvaluateWithEvidence {
            rule: identity.clone(),
            prepared_artifact: prepared_artifact.clone(),
            input: vec![0],
            limits: boundary_limits,
        });
        let LispexApplicationResponse::EvidenceSettlement {
            settlement,
            artifact: Some(evidence_artifact),
        } = evidence
        else {
            panic!("full evaluation did not retain evidence: {evidence:?}")
        };
        assert_eq!(
            settlement.category,
            LispexApplicationSettlementCategory::Complete
        );
        let LispexApplicationResponse::ConsumerArtifactInspection(inspection) = host
            .lispex_application(LispexApplicationRequest::VerifyConsumerArtifact {
                artifact: evidence_artifact.clone(),
            })
        else {
            panic!("full artifact verification did not return inspection")
        };
        assert_eq!(inspection.kind, "evaluate");
        assert_eq!(inspection.category, "complete");
        assert_eq!(
            inspection.semantic_profile_id.as_deref(),
            Some(FULL_PROFILE_ID)
        );
        assert!(inspection.portable_core_bytes > 0);
        assert!(!inspection.authenticated);
        assert!(matches!(
            host.lispex_application(LispexApplicationRequest::PortableCoreBytes {
                artifact: evidence_artifact.clone(),
            }),
            LispexApplicationResponse::ConsumerArtifactBytes(bytes) if !bytes.is_empty()
        ));
        assert!(matches!(
            host.lispex_application(LispexApplicationRequest::FreshReplay {
                rule: identity,
                prepared_artifact,
                input: vec![0],
                artifact: evidence_artifact,
            }),
            LispexApplicationResponse::Settlement(LispexApplicationSettlement {
                category: LispexApplicationSettlementCategory::Complete,
                ..
            })
        ));

        let bounded_artifact = super::super::prepare_consumer_artifact(source, limits.prepare)
            .expect("bounded prepared consumer artifact");
        let bounded_rule = AdmittedApplicationRule::from_locked_artifact(
            LispexApplicationRuleIdentity {
                name: "bounded-profile".into(),
                profile: PROFILE_ID.into(),
                component_id: COMPONENT_ID.into(),
                evaluator_sha256: format!("sha256:{EVALUATOR_SHA256}"),
                prepared_artifact_sha256: format!("sha256:{}", sha256_hex(&bounded_artifact)),
                preparation_request_sha256: format!("sha256:{request}"),
            },
            &format!("sha256:{submission}"),
            &bounded_artifact,
            limits.evaluate,
        )
        .expect("exact bounded admitted rule");
        assert!(matches!(
            LispexApplicationHost::new(
                InnerHost,
                [full_rule_for_mixed_host, bounded_rule],
                ApplicationQuotas {
                    concurrent_evaluations: 1,
                    queued_evaluations: 0,
                    total_evaluations: 1,
                    aggregate_input_bytes: 1,
                    aggregate_result_bytes: limits.evaluate.result_bytes,
                    aggregate_output_bytes: limits.evaluate.output_bytes,
                    aggregate_transcript_bytes: limits.evaluate.transcript_bytes,
                    aggregate_safety_fuel: super::super::SAFETY_FUEL,
                    prepared_bytes: 2_000_000,
                    wall_millis: 60_000,
                },
            ),
            Err(reason) if reason == "a Lispex application host cannot mix profiles"
        ));
    }

    #[test]
    fn application_evidence_round_trips_and_replays_in_a_fresh_instance() {
        let source = b"(if (< 10 15) \"allow\" \"deny\")\n";
        let limits = super::super::Limits::MAXIMUM;
        let prepared_artifact = super::super::prepare_consumer_artifact(source, limits.prepare)
            .expect("prepared consumer artifact");
        let request = super::super::preparation_request_sha256(source, limits.prepare)
            .expect("preparation request digest");
        let submission = super::super::preparation_submission_sha256(source, limits.prepare)
            .expect("preparation submission digest");
        let rule = AdmittedApplicationRule::from_locked_artifact(
            LispexApplicationRuleIdentity {
                name: "evidence".into(),
                profile: PROFILE_ID.into(),
                component_id: COMPONENT_ID.into(),
                evaluator_sha256: format!("sha256:{EVALUATOR_SHA256}"),
                prepared_artifact_sha256: format!("sha256:{}", sha256_hex(&prepared_artifact)),
                preparation_request_sha256: format!("sha256:{request}"),
            },
            &format!("sha256:{submission}"),
            &prepared_artifact,
            limits.evaluate,
        )
        .expect("exact admitted rule");
        let host = LispexApplicationHost::new(
            InnerHost,
            [rule],
            ApplicationQuotas {
                concurrent_evaluations: 1,
                queued_evaluations: 0,
                total_evaluations: 3,
                aggregate_input_bytes: 3,
                aggregate_result_bytes: limits.evaluate.result_bytes * 3,
                aggregate_output_bytes: limits.evaluate.output_bytes * 3,
                aggregate_transcript_bytes: limits.evaluate.transcript_bytes * 3,
                aggregate_safety_fuel: super::super::SAFETY_FUEL * 3,
                prepared_bytes: 1_000_000,
                wall_millis: 5_000,
            },
        )
        .expect("application host");
        let LispexApplicationResponse::Rule {
            identity,
            prepared_artifact,
        } = host.lispex_application(LispexApplicationRequest::Rule {
            target_identity: "topaz.lispex-rule-handle/v1:evidence".into(),
        })
        else {
            panic!("exact rule was not admitted")
        };
        let boundary_limits = to_boundary_limits(limits.evaluate);
        let mut excessive_limits = boundary_limits;
        excessive_limits.eval_work += 1;
        assert!(matches!(
            host.lispex_application(LispexApplicationRequest::EvaluateWithEvidence {
                rule: identity.clone(),
                prepared_artifact: prepared_artifact.clone(),
                input: vec![0],
                limits: excessive_limits,
            }),
            LispexApplicationResponse::EvidenceSettlement {
                settlement: LispexApplicationSettlement {
                    category: LispexApplicationSettlementCategory::RequestRefusal,
                    ..
                },
                artifact: None,
            }
        ));
        let response = host.lispex_application(LispexApplicationRequest::EvaluateWithEvidence {
            rule: identity.clone(),
            prepared_artifact: prepared_artifact.clone(),
            input: vec![0],
            limits: boundary_limits,
        });
        let LispexApplicationResponse::EvidenceSettlement {
            settlement,
            artifact: Some(artifact),
        } = response
        else {
            panic!("evaluation did not retain evidence: {response:?}")
        };
        assert_eq!(
            settlement.category,
            LispexApplicationSettlementCategory::Complete
        );

        let LispexApplicationResponse::ConsumerArtifact(round_trip) =
            host.lispex_application(LispexApplicationRequest::ConsumerArtifactFromBytes {
                bytes: artifact.clone(),
            })
        else {
            panic!("serialized artifact did not re-enter")
        };
        assert_eq!(round_trip, artifact);
        let LispexApplicationResponse::ConsumerArtifactInspection(inspection) = host
            .lispex_application(LispexApplicationRequest::VerifyConsumerArtifact {
                artifact: artifact.clone(),
            })
        else {
            panic!("artifact verification did not return inspection")
        };
        assert_eq!(inspection.kind, "evaluate");
        assert_eq!(inspection.category, "complete");
        assert_eq!(inspection.semantic_profile_id.as_deref(), Some(PROFILE_ID));
        assert!(inspection.portable_core_bytes > 0);
        assert!(!inspection.authenticated);
        assert!(matches!(
            host.lispex_application(LispexApplicationRequest::PortableCoreBytes {
                artifact: artifact.clone(),
            }),
            LispexApplicationResponse::ConsumerArtifactBytes(bytes) if !bytes.is_empty()
        ));

        let replay = host.lispex_application(LispexApplicationRequest::FreshReplay {
            rule: identity.clone(),
            prepared_artifact: prepared_artifact.clone(),
            input: vec![0],
            artifact: artifact.clone(),
        });
        assert!(matches!(
            replay,
            LispexApplicationResponse::Settlement(LispexApplicationSettlement {
                category: LispexApplicationSettlementCategory::Complete,
                ..
            })
        ));

        assert!(matches!(
            host.lispex_application(LispexApplicationRequest::FreshReplay {
                rule: identity,
                prepared_artifact,
                input: vec![1],
                artifact: artifact.clone(),
            }),
            LispexApplicationResponse::EvidenceRefusal(code) if code == "replay-mismatch"
        ));
        let mut tampered = artifact;
        *tampered.last_mut().expect("nonempty artifact") ^= 1;
        assert!(matches!(
            host.lispex_application(LispexApplicationRequest::ConsumerArtifactFromBytes {
                bytes: tampered,
            }),
            LispexApplicationResponse::EvidenceRefusal(_)
        ));
    }
}
