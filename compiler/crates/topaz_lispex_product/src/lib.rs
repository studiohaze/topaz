//! Package locking and conditional managed-product payloads for the exact
//! bounded Lispex evaluator.
//!
//! This layer has no discovery surface. It prepares only the component already
//! compiled into `topaz_lispex_embed`, verifies locked bytes without executing
//! the evaluator, and plans payload files only from an explicit reached-rule
//! set supplied by a later checked language join.

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use topaz_lispex_embed::{
    ABI_ID, ADAPTER_ID, ArtifactCategory, ArtifactKind, CAPABILITY_MANIFEST_V1_SHA256,
    CAPABILITY_MANIFEST_V2_SHA256, COMPONENT_ID, COMPONENT_MANIFEST_SHA256, CONTRACT_ID,
    DIAGNOSTIC_PHASE_ALIAS_SHA256, EVALUATOR_SHA256, GUEST_FAULT_BOUNDARY_SHA256,
    INTAKE_DISPOSITION_SHA256, INTERACTIVE_OUTPUT_SHA256, LispexApplicationRuleIdentity,
    METER_MANIFEST_V2_SHA256, METER_MANIFEST_V3_SHA256, MODEL_ID, OBSERVATION_CONTRACT_SHA256,
    OBSERVATION_RESULT_SHA256, PROFILE_ID, PROFILE_TOMBSTONES_SHA256, RESOURCE_PROFILES_SHA256,
    TOPLEVEL_OUTPUT_V2_SHA256, VALUE_CODEC_ID, decode_artifact, preparation_request_sha256,
    preparation_submission_sha256, prepare_consumer_artifact, verify_artifact,
};
use topaz_package::{
    LISPEX_APPLICATION_PROFILE_ID, LISPEX_BOUNDED_PROFILE_ID, LispexLock, LispexLockRule, Project,
    parse_lock_lispex, read_package_file_strict, read_package_text_strict,
    render_lockfile_with_lispex, validate_lispex_application_binding,
};

mod full_product;
pub use full_product::{
    FULL_ADAPTER_ID, FULL_ARTIFACT_CONTRACT_ID, FULL_COMPONENT_ADMISSION_SHA256,
    FULL_COMPONENT_MANIFEST_SHA256, FULL_HANDLE_CATALOG_PATH, FULL_TARGET_DISPOSITION,
    prepare_complete_package,
};

pub const FEATURE_SET_SHA256: &str =
    "c7ac2d3037b43dd90889467aabdcd2d3c061559bde12bb8330d878886c5ab429";
pub const TRANSCRIPT_ID: &str = "lispex.embed-transcript/v1";
pub const RECEIPT_CORE_ID: &str = "lispex.embed-receipt-core/v1";
pub const HANDLE_CATALOG_PATH: &str = ".topaz/lispex/rule-handles.v1.json";
pub const TARGET_DISPOSITION: &str = "native-only";
pub const APPLICATION_API_MODULE: &str = "std.lispex";
pub const APPLICATION_RULES_MODULE: &str = "std.lispex.rules";
pub const RULE_HANDLE_TARGET_PREFIX: &str = "topaz.lispex-rule-handle/v1:";
const APPLICATION_API_SOURCE: &str =
    include_str!("../../../contracts/lispex-application/v1/std.lispex.tpz");
const LICENSE_SHA256: &str = "7a1b2d2865fe1ca0a4ffe11a0ee99026c0ae634b6e9c56dca15056a85a0290c3";
const NOTICE_SHA256: &str = "1add89fe307090d059a05b0430c424148a45b21ff368f645fae008dfd896e1f8";
const THIRD_PARTY_SHA256: &str = "1580fdf02bcfcb6175af1f43c7e5c604e96b13d989d1043adba344d6d16b4806";

const EVALUATOR: &[u8] = include_bytes!(
    "../../../components/lispex-embed-evaluator/1.20.0/payload/lispex-embed-evaluator.wasm"
);
const COMPONENT_MANIFEST: &[u8] =
    include_bytes!("../../../components/lispex-embed-evaluator/1.20.0/payload/manifest.v1.json");
const LISPEX_LICENSE: &[u8] =
    include_bytes!("../../../components/lispex-embed-evaluator/1.20.0/redistribution/LICENSE");
const LISPEX_NOTICE: &[u8] =
    include_bytes!("../../../components/lispex-embed-evaluator/1.20.0/redistribution/NOTICE");
const LISPEX_THIRD_PARTY: &[u8] = include_bytes!(
    "../../../components/lispex-embed-evaluator/1.20.0/redistribution/THIRD-PARTY-NOTICES.txt"
);
const CAPABILITY_MANIFEST_V1: &[u8] = include_bytes!(
    "../../../components/lispex-embed-evaluator/1.20.0/redistribution/contracts/capability-manifest.v1.json"
);
const CAPABILITY_MANIFEST_V2: &[u8] = include_bytes!(
    "../../../components/lispex-embed-evaluator/1.20.0/redistribution/contracts/capability-manifest.v2.json"
);
const METER_MANIFEST_V2: &[u8] = include_bytes!(
    "../../../components/lispex-embed-evaluator/1.20.0/redistribution/contracts/meter-manifest.v2.json"
);
const METER_MANIFEST_V3: &[u8] = include_bytes!(
    "../../../components/lispex-embed-evaluator/1.20.0/redistribution/contracts/meter-manifest.v3.json"
);
const RESOURCE_PROFILES: &[u8] = include_bytes!(
    "../../../components/lispex-embed-evaluator/1.20.0/redistribution/contracts/resource-profiles.v1.json"
);
const OBSERVATION_CONTRACT: &[u8] = include_bytes!(
    "../../../components/lispex-embed-evaluator/1.20.0/redistribution/contracts/lispex-observation-contract.v1.json"
);
const OBSERVATION_RESULT: &[u8] = include_bytes!(
    "../../../components/lispex-embed-evaluator/1.20.0/redistribution/contracts/observation-result.v1.json"
);
const DIAGNOSTIC_PHASE_ALIAS: &[u8] = include_bytes!(
    "../../../components/lispex-embed-evaluator/1.20.0/redistribution/contracts/diagnostic-phase-alias.v1.json"
);
const GUEST_FAULT_BOUNDARY: &[u8] = include_bytes!(
    "../../../components/lispex-embed-evaluator/1.20.0/redistribution/contracts/guest-fault-vs-infrastructure.v1.json"
);
const TOPLEVEL_OUTPUT_V2: &[u8] = include_bytes!(
    "../../../components/lispex-embed-evaluator/1.20.0/redistribution/contracts/toplevel-value-output.v2.json"
);
const INTERACTIVE_OUTPUT: &[u8] = include_bytes!(
    "../../../components/lispex-embed-evaluator/1.20.0/redistribution/contracts/interactive-output.v1.json"
);
const PROFILE_TOMBSTONES: &[u8] = include_bytes!(
    "../../../components/lispex-embed-evaluator/1.20.0/redistribution/contracts/profile-tombstones.v1.json"
);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedPackage {
    pub lock: LispexLock,
    pub lock_text: String,
    pub generated_files: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PayloadFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConditionalPayload {
    pub requires_runtime: bool,
    pub files: Vec<PayloadFile>,
}

/// One checker-reached rule and every immutable input required to construct
/// the interpreter or generated-native application host.  This is data only;
/// it contains no live evaluator instance or ambient lookup path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedApplicationRule {
    pub identity: LispexApplicationRuleIdentity,
    pub preparation_submission_sha256: String,
    pub prepared_artifact: Vec<u8>,
    pub limits: topaz_lispex_embed::EvaluateLimits,
}

fn checked_application_rule_from_lock(
    lock: &LispexLock,
    lock_rule: &LispexLockRule,
    prepared_artifact: Vec<u8>,
    limits: topaz_lispex_embed::EvaluateLimits,
) -> CheckedApplicationRule {
    CheckedApplicationRule {
        identity: LispexApplicationRuleIdentity {
            name: lock_rule.name.clone(),
            profile: lock.profile.clone(),
            component_id: lock.component_id.clone(),
            evaluator_sha256: lock.evaluator_sha256.clone(),
            prepared_artifact_sha256: lock_rule.prepared_artifact_sha256.clone(),
            preparation_request_sha256: lock_rule.preparation_request_sha256.clone(),
        },
        preparation_submission_sha256: lock_rule.preparation_submission_sha256.clone(),
        prepared_artifact,
        limits,
    }
}

/// The single immutable application plan derived after a clean checker run.
/// The same value is the authority for host admission and conditional payload
/// selection; callers must not reconstruct reachability from imports, strings,
/// declarations, or linker retention.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedApplicationPlan {
    pub reachable_rules: BTreeSet<String>,
    pub rules: Vec<CheckedApplicationRule>,
    pub payload: ConditionalPayload,
    pub quotas: topaz_lispex_embed::ApplicationQuotas,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicationModule {
    pub identity: &'static str,
    pub path: &'static str,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductError {
    message: String,
}

impl ProductError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ProductError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ProductError {}

/// Generate the two compiler-owned application modules from one fixed API
/// contract and the exact sorted lock rows. The lock is recovered through the
/// complete package verifier so a caller cannot supply a forged or stale row
/// set. A lock-only package receives no module and therefore cannot import the
/// surface accidentally.
pub fn application_modules(project: &Project) -> Result<Vec<ApplicationModule>, ProductError> {
    let lock = verify_locked_package(project)?;
    if !matches!(
        lock.application.as_deref(),
        Some(LISPEX_APPLICATION_PROFILE_ID | topaz_package::LISPEX_COMPLETE_APPLICATION_PROFILE_ID)
    ) {
        return Ok(Vec::new());
    }
    let mut rules = lock.rules.iter().collect::<Vec<_>>();
    rules.sort_by(|left, right| left.name.cmp(&right.name));
    let mut source = String::from("import std.lispex { PreparedLispexRule }\n\n");
    for rule in rules {
        source.push_str(&format!(
            "export function {}() -> PreparedLispexRule {{\n    __lispexRule(\"{}\")\n}}\n\n",
            rule.name, rule.name
        ));
    }
    Ok(vec![
        ApplicationModule {
            identity: APPLICATION_API_MODULE,
            path: "std/lispex.tpz",
            source: APPLICATION_API_SOURCE.to_string(),
        },
        ApplicationModule {
            identity: APPLICATION_RULES_MODULE,
            path: "std/lispex/rules.tpz",
            source,
        },
    ])
}

pub fn prepare_package(project: &Project) -> Result<PreparedPackage, ProductError> {
    let config = project
        .manifest
        .lispex
        .as_ref()
        .ok_or_else(|| ProductError::new("package has no [lispex] declaration"))?;
    require_bounded_native(project)?;
    verify_component_payload()?;
    let application_quotas = match &config.application_quotas {
        Some(path) => {
            let bytes =
                read_package_file_strict(&project.root, path, "[lispex] application quotas")
                    .map_err(package_error)?;
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| ProductError::new("[lispex] application quotas must be UTF-8 JSON"))?;
            topaz_lispex_embed::ApplicationQuotas::parse_json(text)
                .map_err(|error| ProductError::new(error.to_string()))?;
            Some((path.clone(), digest(&bytes)))
        }
        None => None,
    };
    let mut generated_files = BTreeMap::new();
    let mut rules = Vec::with_capacity(config.rules.len());
    for rule in &config.rules {
        let source = read_package_file_strict(
            &project.root,
            &rule.source,
            &format!("[lispex] rule `{}` source", rule.name),
        )
        .map_err(package_error)?;
        let limits_bytes = read_package_file_strict(
            &project.root,
            &rule.limits,
            &format!("[lispex] rule `{}` limits", rule.name),
        )
        .map_err(package_error)?;
        let limits_text = std::str::from_utf8(&limits_bytes).map_err(|_| {
            ProductError::new(format!(
                "[lispex] rule `{}` limits must be UTF-8 JSON",
                rule.name
            ))
        })?;
        let limits = topaz_lispex_embed::Limits::parse_json(limits_text).map_err(|error| {
            ProductError::new(format!("[lispex] rule `{}` limits: {error}", rule.name))
        })?;
        let artifact = prepare_consumer_artifact(&source, limits.prepare).map_err(|error| {
            ProductError::new(format!(
                "[lispex] rule `{}` preparation failed: {error}",
                rule.name
            ))
        })?;
        let decoded = decode_artifact(&artifact).map_err(|error| {
            ProductError::new(format!(
                "[lispex] rule `{}` prepared artifact is invalid: {error}",
                rule.name
            ))
        })?;
        if decoded.kind != ArtifactKind::Prepare || decoded.category != ArtifactCategory::Prepared {
            return Err(ProductError::new(format!(
                "[lispex] rule `{}` did not produce a prepared artifact",
                rule.name
            )));
        }
        let request_sha256 = preparation_request_sha256(&source, limits.prepare)
            .map_err(|error| ProductError::new(error.to_string()))?;
        let submission_sha256 = preparation_submission_sha256(&source, limits.prepare)
            .map_err(|error| ProductError::new(error.to_string()))?;
        if decoded.identities[4].as_deref() != Some(FEATURE_SET_SHA256)
            || decoded.identities[5].as_deref() != Some(submission_sha256.as_str())
        {
            return Err(ProductError::new(format!(
                "[lispex] rule `{}` prepared identity binding failed",
                rule.name
            )));
        }
        let artifact_path = prepared_artifact_path(&rule.name);
        let artifact_sha256 = digest(&artifact);
        generated_files.insert(artifact_path.clone(), artifact);
        rules.push(LispexLockRule {
            name: rule.name.clone(),
            source: rule.source.clone(),
            source_sha256: digest(&source),
            limits: rule.limits.clone(),
            limits_sha256: digest(&limits_bytes),
            preparation_request_sha256: prefixed(&request_sha256),
            preparation_submission_sha256: prefixed(&submission_sha256),
            prepared_artifact_path: artifact_path,
            prepared_artifact_sha256: artifact_sha256,
        });
    }
    rules.sort_by(|left, right| left.name.cmp(&right.name));
    let mut lock = LispexLock {
        profile: PROFILE_ID.into(),
        application: config.application.clone(),
        application_quotas: application_quotas.as_ref().map(|(path, _)| path.clone()),
        application_quotas_sha256: application_quotas.map(|(_, digest)| digest),
        feature_set_sha256: prefixed(FEATURE_SET_SHA256),
        component_id: COMPONENT_ID.into(),
        component_manifest_sha256: prefixed(COMPONENT_MANIFEST_SHA256),
        evaluator_sha256: prefixed(EVALUATOR_SHA256),
        abi_id: ABI_ID.into(),
        value_codec_id: VALUE_CODEC_ID.into(),
        meter_model_id: MODEL_ID.into(),
        artifact_contract_id: CONTRACT_ID.into(),
        transcript_id: TRANSCRIPT_ID.into(),
        receipt_core_id: RECEIPT_CORE_ID.into(),
        adapter_id: ADAPTER_ID.into(),
        admission_sha256: prefixed(INTAKE_DISPOSITION_SHA256),
        target: project.manifest.build.target.clone(),
        target_disposition: TARGET_DISPOSITION.into(),
        handle_catalog_path: HANDLE_CATALOG_PATH.into(),
        handle_catalog_sha256: String::new(),
        rules,
    };
    let catalog = render_catalog(&lock.rules);
    lock.handle_catalog_sha256 = digest(&catalog);
    generated_files.insert(HANDLE_CATALOG_PATH.into(), catalog);
    let lock_text = render_lockfile_with_lispex(project, &lock).map_err(package_error)?;
    Ok(PreparedPackage {
        lock,
        lock_text,
        generated_files,
    })
}

pub fn write_locked_package(project: &Project) -> Result<LispexLock, ProductError> {
    if project
        .manifest
        .lispex
        .as_ref()
        .is_some_and(|config| config.profile == topaz_package::LISPEX_COMPLETE_PROFILE_ID)
    {
        return full_product::write_complete_locked_package(project);
    }
    let prepared = prepare_package(project)?;
    for (path, bytes) in &prepared.generated_files {
        replace_package_file(&project.root, path, bytes)?;
    }
    replace_package_file(&project.root, "topaz.lock", prepared.lock_text.as_bytes())?;
    verify_locked_package(project)?;
    Ok(prepared.lock)
}

pub fn verify_locked_package(project: &Project) -> Result<LispexLock, ProductError> {
    if project
        .manifest
        .lispex
        .as_ref()
        .is_some_and(|config| config.profile == topaz_package::LISPEX_COMPLETE_PROFILE_ID)
    {
        return full_product::verify_complete_locked_package(project);
    }
    require_bounded_native(project)?;
    project.verify_locked().map_err(package_error)?;
    let lock_text = read_package_text_strict(&project.root, "topaz.lock", "package lockfile")
        .map_err(package_error)?;
    let lock = parse_lock_lispex(&lock_text)
        .map_err(package_error)?
        .ok_or_else(|| ProductError::new("topaz.lock is missing [lispex]"))?;
    verify_fixed_identity(&lock)?;
    let config = project
        .manifest
        .lispex
        .as_ref()
        .ok_or_else(|| ProductError::new("package has no [lispex] declaration"))?;
    if lock.application != config.application
        || lock.application_quotas != config.application_quotas
    {
        return Err(ProductError::new(
            "the locked Lispex application declaration does not match topaz.toml",
        ));
    }
    let declared = config
        .rules
        .iter()
        .map(|rule| (rule.name.as_str(), rule))
        .collect::<BTreeMap<_, _>>();
    for rule in &lock.rules {
        if rule.prepared_artifact_path != prepared_artifact_path(&rule.name) {
            return Err(ProductError::new(format!(
                "[lispex] rule `{}` prepared artifact path is not canonical",
                rule.name
            )));
        }
        let manifest_rule = declared
            .get(rule.name.as_str())
            .ok_or_else(|| ProductError::new(format!("undeclared locked rule `{}`", rule.name)))?;
        let source = read_package_file_strict(
            &project.root,
            &manifest_rule.source,
            &format!("[lispex] rule `{}` source", rule.name),
        )
        .map_err(package_error)?;
        let limits_bytes = read_package_file_strict(
            &project.root,
            &manifest_rule.limits,
            &format!("[lispex] rule `{}` limits", rule.name),
        )
        .map_err(package_error)?;
        let limits_text = std::str::from_utf8(&limits_bytes)
            .map_err(|_| ProductError::new("locked Lispex limits are not UTF-8"))?;
        let limits = topaz_lispex_embed::Limits::parse_json(limits_text)
            .map_err(|error| ProductError::new(error.to_string()))?;
        let request = prefixed(
            &preparation_request_sha256(&source, limits.prepare)
                .map_err(|error| ProductError::new(error.to_string()))?,
        );
        let submission = prefixed(
            &preparation_submission_sha256(&source, limits.prepare)
                .map_err(|error| ProductError::new(error.to_string()))?,
        );
        if request != rule.preparation_request_sha256
            || submission != rule.preparation_submission_sha256
        {
            return Err(ProductError::new(format!(
                "[lispex] rule `{}` preparation request binding is stale",
                rule.name
            )));
        }
        let artifact = read_package_file_strict(
            &project.root,
            &rule.prepared_artifact_path,
            &format!("[lispex] rule `{}` prepared artifact", rule.name),
        )
        .map_err(package_error)?;
        verify_artifact(&artifact).map_err(|error| ProductError::new(error.to_string()))?;
        let decoded =
            decode_artifact(&artifact).map_err(|error| ProductError::new(error.to_string()))?;
        if decoded.kind != ArtifactKind::Prepare
            || decoded.category != ArtifactCategory::Prepared
            || decoded.evaluator_sha256 != EVALUATOR_SHA256
            || decoded.identities[4].as_deref() != Some(FEATURE_SET_SHA256)
            || decoded.identities[5].as_deref()
                != Some(
                    rule.preparation_submission_sha256
                        .trim_start_matches("sha256:"),
                )
        {
            return Err(ProductError::new(format!(
                "[lispex] rule `{}` prepared artifact identity is stale",
                rule.name
            )));
        }
    }
    let expected_catalog = render_catalog(&lock.rules);
    let actual_catalog = read_package_file_strict(
        &project.root,
        &lock.handle_catalog_path,
        "[lispex] handle catalog",
    )
    .map_err(package_error)?;
    if expected_catalog != actual_catalog || digest(&actual_catalog) != lock.handle_catalog_sha256 {
        return Err(ProductError::new(
            "[lispex] handle catalog is not the canonical locked catalog",
        ));
    }
    Ok(lock)
}

pub fn conditional_payload(
    project: &Project,
    reachable_rules: &BTreeSet<String>,
) -> Result<ConditionalPayload, ProductError> {
    if project
        .manifest
        .lispex
        .as_ref()
        .is_some_and(|config| config.profile == topaz_package::LISPEX_COMPLETE_PROFILE_ID)
    {
        return full_product::complete_conditional_payload(project, reachable_rules);
    }
    let lock = verify_locked_package(project)?;
    if reachable_rules.is_empty() {
        return Ok(ConditionalPayload {
            requires_runtime: false,
            files: Vec::new(),
        });
    }
    let by_name = lock
        .rules
        .iter()
        .map(|rule| (rule.name.as_str(), rule))
        .collect::<BTreeMap<_, _>>();
    for name in reachable_rules {
        if !by_name.contains_key(name.as_str()) {
            return Err(ProductError::new(format!(
                "reachable Lispex rule `{name}` is not declared and locked"
            )));
        }
    }
    verify_component_payload()?;
    let selected = lock
        .rules
        .iter()
        .filter(|rule| reachable_rules.contains(&rule.name))
        .cloned()
        .collect::<Vec<_>>();
    let mut files = vec![
        PayloadFile {
            path: "lispex/component/lispex-embed-evaluator.wasm".into(),
            bytes: EVALUATOR.to_vec(),
        },
        PayloadFile {
            path: "lispex/component/manifest.v1.json".into(),
            bytes: COMPONENT_MANIFEST.to_vec(),
        },
        PayloadFile {
            path: "lispex/LICENSE".into(),
            bytes: LISPEX_LICENSE.to_vec(),
        },
        PayloadFile {
            path: "lispex/NOTICE".into(),
            bytes: LISPEX_NOTICE.to_vec(),
        },
        PayloadFile {
            path: "lispex/THIRD-PARTY-NOTICES.txt".into(),
            bytes: LISPEX_THIRD_PARTY.to_vec(),
        },
        PayloadFile {
            path: "lispex/contracts/capability-manifest.v1.json".into(),
            bytes: CAPABILITY_MANIFEST_V1.to_vec(),
        },
        PayloadFile {
            path: "lispex/contracts/capability-manifest.v2.json".into(),
            bytes: CAPABILITY_MANIFEST_V2.to_vec(),
        },
        PayloadFile {
            path: "lispex/contracts/meter-manifest.v2.json".into(),
            bytes: METER_MANIFEST_V2.to_vec(),
        },
        PayloadFile {
            path: "lispex/contracts/meter-manifest.v3.json".into(),
            bytes: METER_MANIFEST_V3.to_vec(),
        },
        PayloadFile {
            path: "lispex/contracts/resource-profiles.v1.json".into(),
            bytes: RESOURCE_PROFILES.to_vec(),
        },
        PayloadFile {
            path: "lispex/contracts/lispex-observation-contract.v1.json".into(),
            bytes: OBSERVATION_CONTRACT.to_vec(),
        },
        PayloadFile {
            path: "lispex/contracts/observation-result.v1.json".into(),
            bytes: OBSERVATION_RESULT.to_vec(),
        },
        PayloadFile {
            path: "lispex/contracts/diagnostic-phase-alias.v1.json".into(),
            bytes: DIAGNOSTIC_PHASE_ALIAS.to_vec(),
        },
        PayloadFile {
            path: "lispex/contracts/guest-fault-vs-infrastructure.v1.json".into(),
            bytes: GUEST_FAULT_BOUNDARY.to_vec(),
        },
        PayloadFile {
            path: "lispex/contracts/toplevel-value-output.v2.json".into(),
            bytes: TOPLEVEL_OUTPUT_V2.to_vec(),
        },
        PayloadFile {
            path: "lispex/contracts/interactive-output.v1.json".into(),
            bytes: INTERACTIVE_OUTPUT.to_vec(),
        },
        PayloadFile {
            path: "lispex/contracts/profile-tombstones.v1.json".into(),
            bytes: PROFILE_TOMBSTONES.to_vec(),
        },
        PayloadFile {
            path: "lispex/rule-handles.v1.json".into(),
            bytes: render_catalog(&selected),
        },
    ];
    for rule in &selected {
        files.push(PayloadFile {
            path: format!(
                "lispex/rules/{}",
                Path::new(&rule.prepared_artifact_path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| ProductError::new("prepared artifact has no file name"))?
            ),
            bytes: read_package_file_strict(
                &project.root,
                &rule.prepared_artifact_path,
                &format!("[lispex] rule `{}` prepared artifact", rule.name),
            )
            .map_err(package_error)?,
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(ConditionalPayload {
        requires_runtime: true,
        files,
    })
}

/// Derive the exact bounded application plan from checker-owned call target
/// identities. Unknown or undeclared rule identities fail closed.
pub fn checked_application_plan<'a>(
    project: &Project,
    call_target_identities: impl IntoIterator<Item = &'a str>,
) -> Result<CheckedApplicationPlan, ProductError> {
    if project
        .manifest
        .lispex
        .as_ref()
        .is_some_and(|config| config.profile == topaz_package::LISPEX_COMPLETE_PROFILE_ID)
    {
        return full_product::checked_complete_application_plan(project, call_target_identities);
    }
    let lock = verify_locked_package(project)?;
    if lock.application.as_deref() != Some(LISPEX_APPLICATION_PROFILE_ID) {
        return Err(ProductError::new(
            "the locked package does not select the first-class Lispex application profile",
        ));
    }
    let quota_path = lock
        .application_quotas
        .as_deref()
        .ok_or_else(|| ProductError::new("the checked Lispex application has no quota document"))?;
    let quota_bytes =
        read_package_file_strict(&project.root, quota_path, "[lispex] application quotas")
            .map_err(package_error)?;
    let quota_text = std::str::from_utf8(&quota_bytes)
        .map_err(|_| ProductError::new("[lispex] application quotas must be UTF-8 JSON"))?;
    let quotas = topaz_lispex_embed::ApplicationQuotas::parse_json(quota_text)
        .map_err(|error| ProductError::new(error.to_string()))?;
    let mut reachable_rules = BTreeSet::new();
    for target in call_target_identities {
        let Some(name) = target.strip_prefix(RULE_HANDLE_TARGET_PREFIX) else {
            continue;
        };
        if name.is_empty() || target != format!("{RULE_HANDLE_TARGET_PREFIX}{name}") {
            return Err(ProductError::new(format!(
                "invalid Lispex rule target identity `{target}`"
            )));
        }
        reachable_rules.insert(name.to_string());
    }

    let payload = conditional_payload(project, &reachable_rules)?;
    let declared = project
        .manifest
        .lispex
        .as_ref()
        .expect("verified Lispex package")
        .rules
        .iter()
        .map(|rule| (rule.name.as_str(), rule))
        .collect::<BTreeMap<_, _>>();
    let locked = lock
        .rules
        .iter()
        .map(|rule| (rule.name.as_str(), rule))
        .collect::<BTreeMap<_, _>>();
    let mut rules = Vec::with_capacity(reachable_rules.len());
    for name in &reachable_rules {
        let lock_rule = locked.get(name.as_str()).ok_or_else(|| {
            ProductError::new(format!(
                "reachable Lispex rule `{name}` is not declared and locked"
            ))
        })?;
        let declared_rule = declared.get(name.as_str()).ok_or_else(|| {
            ProductError::new(format!("reachable Lispex rule `{name}` is not declared"))
        })?;
        if declared_rule.limits != lock_rule.limits {
            return Err(ProductError::new(format!(
                "reachable Lispex rule `{name}` has a stale limits binding"
            )));
        }
        let limits_bytes = read_package_file_strict(
            &project.root,
            &lock_rule.limits,
            &format!("[lispex] rule `{name}` limits"),
        )
        .map_err(package_error)?;
        let limits_text = std::str::from_utf8(&limits_bytes)
            .map_err(|_| ProductError::new("locked Lispex limits are not UTF-8"))?;
        let limits = topaz_lispex_embed::Limits::parse_json(limits_text)
            .map_err(|error| ProductError::new(error.to_string()))?;
        let prepared_artifact = read_package_file_strict(
            &project.root,
            &lock_rule.prepared_artifact_path,
            &format!("[lispex] rule `{name}` prepared artifact"),
        )
        .map_err(package_error)?;
        rules.push(checked_application_rule_from_lock(
            &lock,
            lock_rule,
            prepared_artifact,
            limits.evaluate,
        ));
    }

    Ok(CheckedApplicationPlan {
        reachable_rules,
        rules,
        payload,
        quotas,
    })
}

fn require_bounded_native(project: &Project) -> Result<(), ProductError> {
    validate_lispex_application_binding(&project.manifest).map_err(package_error)?;
    let config = project
        .manifest
        .lispex
        .as_ref()
        .ok_or_else(|| ProductError::new("package has no [lispex] declaration"))?;
    if config.profile != LISPEX_BOUNDED_PROFILE_ID || config.profile != PROFILE_ID {
        return Err(ProductError::new(format!(
            "unsupported Lispex profile `{}`",
            config.profile
        )));
    }
    if project.manifest.build.target != "native" {
        return Err(ProductError::new(format!(
            "[lispex] target `{}` is not admitted; only `native` is available at P1",
            project.manifest.build.target
        )));
    }
    Ok(())
}

fn verify_fixed_identity(lock: &LispexLock) -> Result<(), ProductError> {
    let expected = [
        ("profile", lock.profile.as_str(), PROFILE_ID),
        (
            "feature_set_sha256",
            lock.feature_set_sha256.trim_start_matches("sha256:"),
            FEATURE_SET_SHA256,
        ),
        ("component_id", lock.component_id.as_str(), COMPONENT_ID),
        (
            "component_manifest_sha256",
            lock.component_manifest_sha256.trim_start_matches("sha256:"),
            COMPONENT_MANIFEST_SHA256,
        ),
        (
            "evaluator_sha256",
            lock.evaluator_sha256.trim_start_matches("sha256:"),
            EVALUATOR_SHA256,
        ),
        ("abi_id", lock.abi_id.as_str(), ABI_ID),
        (
            "value_codec_id",
            lock.value_codec_id.as_str(),
            VALUE_CODEC_ID,
        ),
        ("meter_model_id", lock.meter_model_id.as_str(), MODEL_ID),
        (
            "artifact_contract_id",
            lock.artifact_contract_id.as_str(),
            CONTRACT_ID,
        ),
        ("transcript_id", lock.transcript_id.as_str(), TRANSCRIPT_ID),
        (
            "receipt_core_id",
            lock.receipt_core_id.as_str(),
            RECEIPT_CORE_ID,
        ),
        ("adapter_id", lock.adapter_id.as_str(), ADAPTER_ID),
        (
            "admission_sha256",
            lock.admission_sha256.trim_start_matches("sha256:"),
            INTAKE_DISPOSITION_SHA256,
        ),
        ("target", lock.target.as_str(), "native"),
        (
            "target_disposition",
            lock.target_disposition.as_str(),
            TARGET_DISPOSITION,
        ),
        (
            "handle_catalog_path",
            lock.handle_catalog_path.as_str(),
            HANDLE_CATALOG_PATH,
        ),
    ];
    for (field, actual, expected) in expected {
        if actual != expected {
            return Err(ProductError::new(format!(
                "topaz.lock [lispex].{field} is not the fixed bounded identity"
            )));
        }
    }
    if let Some(application) = &lock.application
        && application != topaz_package::LISPEX_APPLICATION_PROFILE_ID
    {
        return Err(ProductError::new(
            "topaz.lock [lispex].application is not the fixed application identity",
        ));
    }
    Ok(())
}

fn verify_component_payload() -> Result<(), ProductError> {
    if bare_digest(EVALUATOR) != EVALUATOR_SHA256 {
        return Err(ProductError::new(
            "compiled Lispex evaluator digest drifted",
        ));
    }
    if bare_digest(COMPONENT_MANIFEST) != COMPONENT_MANIFEST_SHA256 {
        return Err(ProductError::new(
            "compiled Lispex component manifest digest drifted",
        ));
    }
    for (label, bytes, expected) in [
        ("LICENSE", LISPEX_LICENSE, LICENSE_SHA256),
        ("NOTICE", LISPEX_NOTICE, NOTICE_SHA256),
        (
            "THIRD-PARTY-NOTICES.txt",
            LISPEX_THIRD_PARTY,
            THIRD_PARTY_SHA256,
        ),
        (
            "contracts/capability-manifest.v1.json",
            CAPABILITY_MANIFEST_V1,
            CAPABILITY_MANIFEST_V1_SHA256,
        ),
        (
            "contracts/capability-manifest.v2.json",
            CAPABILITY_MANIFEST_V2,
            CAPABILITY_MANIFEST_V2_SHA256,
        ),
        (
            "contracts/meter-manifest.v2.json",
            METER_MANIFEST_V2,
            METER_MANIFEST_V2_SHA256,
        ),
        (
            "contracts/meter-manifest.v3.json",
            METER_MANIFEST_V3,
            METER_MANIFEST_V3_SHA256,
        ),
        (
            "contracts/resource-profiles.v1.json",
            RESOURCE_PROFILES,
            RESOURCE_PROFILES_SHA256,
        ),
        (
            "contracts/lispex-observation-contract.v1.json",
            OBSERVATION_CONTRACT,
            OBSERVATION_CONTRACT_SHA256,
        ),
        (
            "contracts/observation-result.v1.json",
            OBSERVATION_RESULT,
            OBSERVATION_RESULT_SHA256,
        ),
        (
            "contracts/diagnostic-phase-alias.v1.json",
            DIAGNOSTIC_PHASE_ALIAS,
            DIAGNOSTIC_PHASE_ALIAS_SHA256,
        ),
        (
            "contracts/guest-fault-vs-infrastructure.v1.json",
            GUEST_FAULT_BOUNDARY,
            GUEST_FAULT_BOUNDARY_SHA256,
        ),
        (
            "contracts/toplevel-value-output.v2.json",
            TOPLEVEL_OUTPUT_V2,
            TOPLEVEL_OUTPUT_V2_SHA256,
        ),
        (
            "contracts/interactive-output.v1.json",
            INTERACTIVE_OUTPUT,
            INTERACTIVE_OUTPUT_SHA256,
        ),
        (
            "contracts/profile-tombstones.v1.json",
            PROFILE_TOMBSTONES,
            PROFILE_TOMBSTONES_SHA256,
        ),
    ] {
        if bare_digest(bytes) != expected {
            return Err(ProductError::new(format!(
                "compiled Lispex redistribution file `{label}` digest drifted"
            )));
        }
    }
    Ok(())
}

fn prepared_artifact_path(name: &str) -> String {
    format!(
        ".topaz/lispex/rules/{}.lpxembed",
        bare_digest(name.as_bytes())
    )
}

fn render_catalog(rules: &[LispexLockRule]) -> Vec<u8> {
    let mut out = String::from("{\"schema\":\"topaz.lispex-rule-handles/v1\",\"profile_id\":\"");
    push_json_text(&mut out, PROFILE_ID);
    out.push_str("\",\"feature_set_sha256\":\"");
    push_json_text(&mut out, FEATURE_SET_SHA256);
    out.push_str("\",\"component_id\":\"");
    push_json_text(&mut out, COMPONENT_ID);
    out.push_str("\",\"evaluator_sha256\":\"");
    push_json_text(&mut out, EVALUATOR_SHA256);
    out.push_str("\",\"rules\":[");
    for (index, rule) in rules.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":\"");
        push_json_text(&mut out, &rule.name);
        out.push_str("\",\"prepared_artifact_path\":\"");
        push_json_text(&mut out, &rule.prepared_artifact_path);
        out.push_str("\",\"prepared_artifact_sha256\":\"");
        push_json_text(&mut out, &rule.prepared_artifact_sha256);
        out.push_str("\",\"preparation_request_sha256\":\"");
        push_json_text(&mut out, &rule.preparation_request_sha256);
        out.push_str("\"}");
    }
    out.push_str("]}\n");
    out.into_bytes()
}

fn push_json_text(out: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character <= '\u{1f}' => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", character as u32);
            }
            character => out.push(character),
        }
    }
}

fn replace_package_file(root: &Path, relative: &str, bytes: &[u8]) -> Result<(), ProductError> {
    topaz_package::replace_package_file_strict(root, relative, bytes).map_err(package_error)
}

fn bare_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn digest(bytes: &[u8]) -> String {
    prefixed(&bare_digest(bytes))
}

fn prefixed(value: &str) -> String {
    format!("sha256:{value}")
}

fn package_error(error: topaz_package::PackageError) -> ProductError {
    ProductError::new(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_TEMP_PACKAGE: AtomicU64 = AtomicU64::new(0);

    struct TempPackage(PathBuf);

    impl TempPackage {
        fn new(target: &str, with_lispex: bool) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let sequence = NEXT_TEMP_PACKAGE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "topaz-lispex-product-{}-{nonce}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(root.join("src")).expect("source dir");
            fs::create_dir_all(root.join("rules")).expect("rules dir");
            let mut manifest = format!(
                "[package]\nname = \"policy\"\nversion = \"0.1.0\"\nlanguage = \"5.17\"\nentry = \"src/main.tpz\"\n\n[build]\ntarget = \"{target}\"\ndeterministic = true\n\n[dependencies]\nstd = \"5.17\"\n"
            );
            if with_lispex {
                manifest.push_str(&format!(
                    "\n[lispex]\nprofile = \"{PROFILE_ID}\"\n\n[[lispex.rule]]\nname = \"환불\"\nsource = \"rules/refund.lspx\"\nlimits = \"rules/refund.limits.json\"\n"
                ));
            }
            fs::write(root.join("topaz.toml"), manifest).expect("manifest");
            fs::write(root.join("src/main.tpz"), "print(\"ok\")\n").expect("source");
            if with_lispex {
                fs::write(
                    root.join("rules/refund.lspx"),
                    "(if (< 10 15) \"allow\" \"deny\")\n",
                )
                .expect("rule");
                fs::write(root.join("rules/refund.limits.json"), limits_json()).expect("limits");
            }
            Self(root)
        }

        fn project(&self) -> Project {
            Project::load(&self.0).expect("project")
        }
    }

    impl Drop for TempPackage {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn limits_json() -> &'static str {
        "{\n  \"schema\": \"topaz.lispex-embed-limits/v1\",\n  \"prepare\": {\n    \"raw_source_bytes\": 4096,\n    \"prepare_work\": 1000000,\n    \"logical_allocation\": 1000000,\n    \"syntax_depth\": 64\n  },\n  \"evaluate\": {\n    \"canonical_input_bytes\": 4096,\n    \"eval_work\": 1000000,\n    \"logical_allocation\": 1000000,\n    \"semantic_frames\": 1000,\n    \"traversal_depth\": 256,\n    \"output_bytes\": 1000000,\n    \"diagnostic_bytes\": 1000000,\n    \"transcript_bytes\": 1000000,\n    \"transcript_events\": 100,\n    \"result_bytes\": 1000000\n  }\n}\n"
    }

    #[test]
    fn lock_is_exact_and_verifies_without_reprepare() {
        let package = TempPackage::new("native", true);
        let project = package.project();
        let lock = write_locked_package(&project).expect("lock package");
        assert_eq!(lock.profile, PROFILE_ID);
        assert_eq!(lock.rules.len(), 1);
        let verified = verify_locked_package(&package.project()).expect("verify locked package");
        assert_eq!(verified, lock);

        fs::write(package.0.join("rules/refund.lspx"), "(quote changed)\n").expect("change rule");
        let error = verify_locked_package(&package.project()).expect_err("stale source");
        assert!(error.message().contains("source hash is stale"), "{error}");
    }

    #[test]
    fn v517_application_profile_fails_before_lock_or_generated_output() {
        let package = TempPackage::new("native", true);
        let manifest_path = package.0.join("topaz.toml");
        let manifest = fs::read_to_string(&manifest_path)
            .expect("manifest")
            .replace(
                &format!("profile = \"{PROFILE_ID}\""),
                &format!(
                    "profile = \"{PROFILE_ID}\"\napplication = \"{}\"\napplication_quotas = \"rules/application.quotas.json\"",
                    topaz_package::LISPEX_APPLICATION_PROFILE_ID
                ),
            );
        fs::write(&manifest_path, manifest).expect("activate application profile");
        let error = Project::load(&package.0).expect_err("5.17 application refusal");
        assert!(
            error
                .message()
                .contains("requires [package].language `5.18`"),
            "{error}"
        );
        assert!(!package.0.join("topaz.lock").exists());
        assert!(!package.0.join(".topaz").exists());
    }

    #[test]
    fn conditional_payload_is_empty_or_exactly_reachable() {
        let package = TempPackage::new("native", true);
        write_locked_package(&package.project()).expect("lock package");
        let project = package.project();
        let empty = conditional_payload(&project, &BTreeSet::new()).expect("empty payload");
        assert!(!empty.requires_runtime);
        assert!(empty.files.is_empty());

        let reached = BTreeSet::from(["환불".to_string()]);
        let payload = conditional_payload(&project, &reached).expect("reached payload");
        assert!(payload.requires_runtime);
        assert_eq!(payload.files.len(), 19);
        for path in [
            "lispex/contracts/capability-manifest.v1.json",
            "lispex/contracts/capability-manifest.v2.json",
            "lispex/contracts/meter-manifest.v2.json",
            "lispex/contracts/meter-manifest.v3.json",
            "lispex/contracts/resource-profiles.v1.json",
            "lispex/contracts/lispex-observation-contract.v1.json",
            "lispex/contracts/observation-result.v1.json",
            "lispex/contracts/diagnostic-phase-alias.v1.json",
            "lispex/contracts/guest-fault-vs-infrastructure.v1.json",
            "lispex/contracts/toplevel-value-output.v2.json",
            "lispex/contracts/interactive-output.v1.json",
            "lispex/contracts/profile-tombstones.v1.json",
        ] {
            assert!(payload.files.iter().any(|file| file.path == path), "{path}");
        }
        assert!(
            payload
                .files
                .iter()
                .any(|file| file.path.ends_with(".lpxembed"))
        );
        assert!(
            !payload.files.iter().any(|file| {
                file.path.ends_with(".lspx") || file.path.ends_with(".limits.json")
            })
        );
    }

    #[test]
    fn stale_same_identity_lock_cannot_activate_application_modules() {
        let package = TempPackage::new("native", true);
        let mut project = package.project();
        project.manifest_text = project
            .manifest_text
            .replace("language = \"5.17\"", "language = \"5.18\"")
            .replace("std = \"5.17\"", "std = \"5.18\"")
            .replace(
                &format!("profile = \"{PROFILE_ID}\""),
                &format!(
                    "profile = \"{PROFILE_ID}\"\napplication = \"{}\"\napplication_quotas = \"rules/application.quotas.json\"",
                    topaz_package::LISPEX_APPLICATION_PROFILE_ID
                ),
            );
        project.manifest.package.language = topaz_package::LISPEX_APPLICATION_LANGUAGE;
        project
            .manifest
            .dependencies
            .get_mut("std")
            .expect("std dependency")
            .version = Some(topaz_package::LISPEX_APPLICATION_STD_VERSION.to_string());
        let lispex = project
            .manifest
            .lispex
            .as_mut()
            .expect("Lispex declaration");
        lispex.application = Some(topaz_package::LISPEX_APPLICATION_PROFILE_ID.to_string());
        lispex.application_quotas = Some("rules/application.quotas.json".to_string());
        fs::write(
            package.0.join("rules/application.quotas.json"),
            "{\n  \"schema\": \"topaz.lispex-application-quotas/v1\",\n  \"concurrent_evaluations\": 2,\n  \"queued_evaluations\": 2,\n  \"total_evaluations\": 64,\n  \"aggregate_input_bytes\": 262144,\n  \"aggregate_result_bytes\": 262144,\n  \"aggregate_output_bytes\": 262144,\n  \"aggregate_transcript_bytes\": 262144,\n  \"aggregate_safety_fuel\": 64000000000,\n  \"prepared_bytes\": 3000000,\n  \"wall_millis\": 100\n}\n",
        )
        .expect("application quotas");
        write_locked_package(&project).expect("exact application lock");
        let lock_path = package.0.join("topaz.lock");
        let stale = fs::read_to_string(&lock_path)
            .expect("lock")
            .replace("name = \"환불\"", "name = \"위조\"");
        fs::write(&lock_path, stale).expect("forge same-identity rule row");

        let error = application_modules(&project)
            .expect_err("same-identity stale lock must not feed generated modules");
        assert!(
            error.message().contains("missing rule `환불`")
                || error.message().contains("undeclared locked rule `위조`"),
            "{error}"
        );
    }

    #[test]
    fn unsupported_target_fails_before_any_generated_output() {
        let package = TempPackage::new("python", true);
        let project = package.project();
        let error = prepare_package(&project).expect_err("python refusal");
        assert!(error.message().contains("is not admitted"), "{error}");
        assert!(!package.0.join(".topaz/lispex").exists());
        assert!(!package.0.join("topaz.lock").exists());
    }

    #[test]
    fn full_profile_cannot_enter_the_bounded_application_identity() {
        let package = TempPackage::new("native", true);
        let manifest = fs::read_to_string(package.0.join("topaz.toml"))
            .expect("manifest")
            .replace(PROFILE_ID, "lispex/r7rs-rule-current-profile-bounded/1");
        fs::write(package.0.join("topaz.toml"), manifest).expect("replace manifest");
        let error = Project::load(&package.0).expect_err("full profile refusal");
        assert!(
            error
                .message()
                .contains("requires [lispex].application `topaz/lispex-decision-application/2`"),
            "{error}"
        );
    }
}
