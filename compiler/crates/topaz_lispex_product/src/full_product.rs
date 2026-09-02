use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use topaz_lispex_embed::{
    ABI_ID, FULL_COMPONENT_ID, FULL_EVALUATOR_SHA256, FULL_FEATURE_SET_SHA256, FULL_MODEL_ID,
    FULL_PROFILE_ID, FullArtifactCategory, FullArtifactKind, VALUE_CODEC_ID, decode_full_artifact,
    preparation_request_sha256, preparation_submission_sha256, prepare_full_consumer_artifact,
    verify_full_artifact,
};
use topaz_package::{
    LISPEX_COMPLETE_APPLICATION_PROFILE_ID, LISPEX_COMPLETE_PROFILE_ID, LispexLock, LispexLockRule,
    Project, parse_lock_lispex, read_package_file_strict, read_package_text_strict,
    render_lockfile_with_lispex, validate_lispex_application_binding,
};

use super::{
    CheckedApplicationPlan, ConditionalPayload, PayloadFile, PreparedPackage, ProductError,
    RECEIPT_CORE_ID, RULE_HANDLE_TARGET_PREFIX, TRANSCRIPT_ID, checked_application_rule_from_lock,
    digest, package_error, prefixed, push_json_text, replace_package_file,
};

pub const FULL_COMPONENT_MANIFEST_SHA256: &str =
    "741275df24d81b7eb002657c285fb142a6f089ec5ebc744fbb5a88c749fc1b09";
pub const FULL_COMPONENT_ADMISSION_SHA256: &str =
    "b251132c50ad58422378ea436a35bca7a035072ff239998c6d9fe1348a3ef171";
pub const FULL_ARTIFACT_CONTRACT_ID: &str = "topaz-lispex-full-consumer-artifact/v1";
pub const FULL_ADAPTER_ID: &str = "topaz.lispex-full-embed-adapter/5.20";
pub const FULL_HANDLE_CATALOG_PATH: &str = ".topaz/lispex/full-rule-handles.v1.json";
pub const FULL_TARGET_DISPOSITION: &str = "native-only";

const FULL_COMPONENT_MANIFEST: &[u8] = include_bytes!(
    "../../../contracts/lispex-full-provider-intake/v1/inputs/embedding-readiness/release-dag/components/lispex-full-embed-evaluator-1.v1.json"
);
// The 1.15.8 redistribution revises packaging only; evaluator bytes remain at 1.15.7.
const FULL_EVALUATOR: &[u8] = include_bytes!(
    "../../../contracts/lispex-full-provider-intake/v1/inputs/products/full-embed-evaluator/v1.15.7/lispex-full-embed-evaluator.wasm"
);
const FULL_REDISTRIBUTION: &[u8] = include_bytes!(
    "../../../contracts/lispex-full-provider-intake/v1/inputs/products/full-embed-evaluator/v1.15.8/lispex-full-embed-evaluator-redistribution.zip"
);

/// Prepare the exact complete-current-profile package closure selected by
/// Topaz 5.20. The profile and application identities remain disjoint from the
/// immutable bounded compatibility product.
pub fn prepare_complete_package(project: &Project) -> Result<PreparedPackage, ProductError> {
    validate_lispex_application_binding(&project.manifest).map_err(package_error)?;
    let config = project
        .manifest
        .lispex
        .as_ref()
        .ok_or_else(|| ProductError::new("package has no [lispex] declaration"))?;
    if config.profile != LISPEX_COMPLETE_PROFILE_ID || config.profile != FULL_PROFILE_ID {
        return Err(ProductError::new(format!(
            "unsupported complete Lispex profile `{}`",
            config.profile
        )));
    }
    if config.application.as_deref() != Some(LISPEX_COMPLETE_APPLICATION_PROFILE_ID) {
        return Err(ProductError::new(
            "the complete Lispex profile needs its exact application identity",
        ));
    }
    if project.manifest.build.target != "native" {
        return Err(ProductError::new(format!(
            "[lispex] complete-profile target `{}` is not yet implemented; expected `native`",
            project.manifest.build.target
        )));
    }
    if super::bare_digest(FULL_COMPONENT_MANIFEST) != FULL_COMPONENT_MANIFEST_SHA256 {
        return Err(ProductError::new(
            "retained full Lispex component manifest digest drifted",
        ));
    }

    let quota_path = config
        .application_quotas
        .as_ref()
        .ok_or_else(|| ProductError::new("the complete Lispex application needs quotas"))?;
    let quota_bytes = read_package_file_strict(
        &project.root,
        quota_path,
        "[lispex] complete application quotas",
    )
    .map_err(package_error)?;
    let quota_text = std::str::from_utf8(&quota_bytes)
        .map_err(|_| ProductError::new("[lispex] application quotas must be UTF-8 JSON"))?;
    topaz_lispex_embed::ApplicationQuotas::parse_json(quota_text)
        .map_err(|error| ProductError::new(error.to_string()))?;

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
        let artifact =
            prepare_full_consumer_artifact(&source, limits.prepare).map_err(|error| {
                ProductError::new(format!(
                    "[lispex] rule `{}` full preparation failed: {error}",
                    rule.name
                ))
            })?;
        let decoded = decode_full_artifact(&artifact).map_err(|error| {
            ProductError::new(format!(
                "[lispex] rule `{}` full prepared artifact is invalid: {error}",
                rule.name
            ))
        })?;
        let request_sha256 = preparation_request_sha256(&source, limits.prepare)
            .map_err(|error| ProductError::new(error.to_string()))?;
        let submission_sha256 = preparation_submission_sha256(&source, limits.prepare)
            .map_err(|error| ProductError::new(error.to_string()))?;
        if decoded.kind != FullArtifactKind::Prepare
            || decoded.category != FullArtifactCategory::Prepared
            || decoded.evaluator_sha256 != FULL_EVALUATOR_SHA256
            || decoded.identities[4].as_deref() != Some(FULL_FEATURE_SET_SHA256)
            || decoded.identities[5].as_deref() != Some(submission_sha256.as_str())
        {
            return Err(ProductError::new(format!(
                "[lispex] rule `{}` full prepared identity binding failed",
                rule.name
            )));
        }
        let artifact_path = format!(
            ".topaz/lispex/rules/{}.lpxfull",
            super::bare_digest(rule.name.as_bytes())
        );
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
        profile: FULL_PROFILE_ID.into(),
        application: config.application.clone(),
        application_quotas: Some(quota_path.clone()),
        application_quotas_sha256: Some(digest(&quota_bytes)),
        feature_set_sha256: prefixed(FULL_FEATURE_SET_SHA256),
        component_id: FULL_COMPONENT_ID.into(),
        component_manifest_sha256: prefixed(FULL_COMPONENT_MANIFEST_SHA256),
        evaluator_sha256: prefixed(FULL_EVALUATOR_SHA256),
        abi_id: ABI_ID.into(),
        value_codec_id: VALUE_CODEC_ID.into(),
        meter_model_id: FULL_MODEL_ID.into(),
        artifact_contract_id: FULL_ARTIFACT_CONTRACT_ID.into(),
        transcript_id: TRANSCRIPT_ID.into(),
        receipt_core_id: RECEIPT_CORE_ID.into(),
        adapter_id: FULL_ADAPTER_ID.into(),
        admission_sha256: prefixed(FULL_COMPONENT_ADMISSION_SHA256),
        target: project.manifest.build.target.clone(),
        target_disposition: FULL_TARGET_DISPOSITION.into(),
        handle_catalog_path: FULL_HANDLE_CATALOG_PATH.into(),
        handle_catalog_sha256: String::new(),
        rules,
    };
    let catalog = render_full_catalog(&lock.rules);
    lock.handle_catalog_sha256 = digest(&catalog);
    generated_files.insert(FULL_HANDLE_CATALOG_PATH.into(), catalog);
    let lock_text = render_lockfile_with_lispex(project, &lock).map_err(package_error)?;
    Ok(PreparedPackage {
        lock,
        lock_text,
        generated_files,
    })
}

pub(super) fn write_complete_locked_package(project: &Project) -> Result<LispexLock, ProductError> {
    let prepared = prepare_complete_package(project)?;
    for (path, bytes) in &prepared.generated_files {
        replace_package_file(&project.root, path, bytes)?;
    }
    replace_package_file(&project.root, "topaz.lock", prepared.lock_text.as_bytes())?;
    verify_complete_locked_package(project)?;
    Ok(prepared.lock)
}

pub(super) fn verify_complete_locked_package(
    project: &Project,
) -> Result<LispexLock, ProductError> {
    validate_lispex_application_binding(&project.manifest).map_err(package_error)?;
    project.verify_locked().map_err(package_error)?;
    verify_full_component_payload()?;
    let lock_text = read_package_text_strict(&project.root, "topaz.lock", "package lockfile")
        .map_err(package_error)?;
    let lock = parse_lock_lispex(&lock_text)
        .map_err(package_error)?
        .ok_or_else(|| ProductError::new("topaz.lock is missing [lispex]"))?;
    verify_complete_identity(&lock)?;
    let config = project
        .manifest
        .lispex
        .as_ref()
        .ok_or_else(|| ProductError::new("package has no [lispex] declaration"))?;
    if lock.application != config.application
        || lock.application_quotas != config.application_quotas
    {
        return Err(ProductError::new(
            "the locked complete Lispex application declaration does not match topaz.toml",
        ));
    }
    let quota_path = config
        .application_quotas
        .as_ref()
        .ok_or_else(|| ProductError::new("the complete Lispex application needs quotas"))?;
    let quota_bytes = read_package_file_strict(
        &project.root,
        quota_path,
        "[lispex] complete application quotas",
    )
    .map_err(package_error)?;
    if lock.application_quotas_sha256.as_deref() != Some(digest(&quota_bytes).as_str()) {
        return Err(ProductError::new(
            "the locked complete Lispex application quota digest is stale",
        ));
    }
    let declared = config
        .rules
        .iter()
        .map(|rule| (rule.name.as_str(), rule))
        .collect::<BTreeMap<_, _>>();
    if declared.len() != lock.rules.len() {
        return Err(ProductError::new(
            "the locked complete Lispex rule set does not match topaz.toml",
        ));
    }
    for rule in &lock.rules {
        let expected_path = full_prepared_artifact_path(&rule.name);
        if rule.prepared_artifact_path != expected_path {
            return Err(ProductError::new(format!(
                "[lispex] rule `{}` full prepared artifact path is not canonical",
                rule.name
            )));
        }
        let declared_rule = declared.get(rule.name.as_str()).ok_or_else(|| {
            ProductError::new(format!("undeclared locked complete rule `{}`", rule.name))
        })?;
        let source = read_package_file_strict(
            &project.root,
            &declared_rule.source,
            &format!("[lispex] rule `{}` source", rule.name),
        )
        .map_err(package_error)?;
        let limits_bytes = read_package_file_strict(
            &project.root,
            &declared_rule.limits,
            &format!("[lispex] rule `{}` limits", rule.name),
        )
        .map_err(package_error)?;
        if rule.source != declared_rule.source
            || rule.source_sha256 != digest(&source)
            || rule.limits != declared_rule.limits
            || rule.limits_sha256 != digest(&limits_bytes)
        {
            return Err(ProductError::new(format!(
                "[lispex] rule `{}` full source or limits binding is stale",
                rule.name
            )));
        }
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
                "[lispex] rule `{}` full preparation request binding is stale",
                rule.name
            )));
        }
        let artifact = read_package_file_strict(
            &project.root,
            &rule.prepared_artifact_path,
            &format!("[lispex] rule `{}` full prepared artifact", rule.name),
        )
        .map_err(package_error)?;
        if digest(&artifact) != rule.prepared_artifact_sha256 {
            return Err(ProductError::new(format!(
                "[lispex] rule `{}` full prepared artifact digest is stale",
                rule.name
            )));
        }
        verify_full_artifact(&artifact).map_err(|error| ProductError::new(error.to_string()))?;
        let decoded = decode_full_artifact(&artifact)
            .map_err(|error| ProductError::new(error.to_string()))?;
        if decoded.kind != FullArtifactKind::Prepare
            || decoded.category != FullArtifactCategory::Prepared
            || decoded.evaluator_sha256 != FULL_EVALUATOR_SHA256
            || decoded.identities[4].as_deref() != Some(FULL_FEATURE_SET_SHA256)
            || decoded.identities[5].as_deref()
                != Some(
                    rule.preparation_submission_sha256
                        .trim_start_matches("sha256:"),
                )
        {
            return Err(ProductError::new(format!(
                "[lispex] rule `{}` full prepared artifact identity is stale",
                rule.name
            )));
        }
    }
    let expected_catalog = render_full_catalog(&lock.rules);
    let actual_catalog = read_package_file_strict(
        &project.root,
        &lock.handle_catalog_path,
        "[lispex] full handle catalog",
    )
    .map_err(package_error)?;
    if expected_catalog != actual_catalog || digest(&actual_catalog) != lock.handle_catalog_sha256 {
        return Err(ProductError::new(
            "[lispex] full handle catalog is not the canonical locked catalog",
        ));
    }
    Ok(lock)
}

pub(super) fn complete_conditional_payload(
    project: &Project,
    reachable_rules: &BTreeSet<String>,
) -> Result<ConditionalPayload, ProductError> {
    let lock = verify_complete_locked_package(project)?;
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
                "reachable complete Lispex rule `{name}` is not declared and locked"
            )));
        }
    }
    let selected = lock
        .rules
        .iter()
        .filter(|rule| reachable_rules.contains(&rule.name))
        .cloned()
        .collect::<Vec<_>>();
    let mut files = vec![
        PayloadFile {
            path: "lispex/component/lispex-full-embed-evaluator.wasm".into(),
            bytes: FULL_EVALUATOR.to_vec(),
        },
        PayloadFile {
            path: "lispex/component/full-component-manifest.v1.json".into(),
            bytes: FULL_COMPONENT_MANIFEST.to_vec(),
        },
        PayloadFile {
            path: "lispex/lispex-full-embed-evaluator-redistribution.zip".into(),
            bytes: FULL_REDISTRIBUTION.to_vec(),
        },
        PayloadFile {
            path: "lispex/full-rule-handles.v1.json".into(),
            bytes: render_full_catalog(&selected),
        },
    ];
    for rule in &selected {
        files.push(PayloadFile {
            path: format!(
                "lispex/rules/{}",
                Path::new(&rule.prepared_artifact_path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| ProductError::new("full prepared artifact has no file name"))?
            ),
            bytes: read_package_file_strict(
                &project.root,
                &rule.prepared_artifact_path,
                &format!("[lispex] rule `{}` full prepared artifact", rule.name),
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

pub(super) fn checked_complete_application_plan<'a>(
    project: &Project,
    call_target_identities: impl IntoIterator<Item = &'a str>,
) -> Result<CheckedApplicationPlan, ProductError> {
    let lock = verify_complete_locked_package(project)?;
    let quota_path = lock
        .application_quotas
        .as_deref()
        .ok_or_else(|| ProductError::new("the complete Lispex application has no quotas"))?;
    let quota_bytes = read_package_file_strict(
        &project.root,
        quota_path,
        "[lispex] complete application quotas",
    )
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
                "invalid complete Lispex rule target identity `{target}`"
            )));
        }
        reachable_rules.insert(name.to_string());
    }
    let payload = complete_conditional_payload(project, &reachable_rules)?;
    let declared = project
        .manifest
        .lispex
        .as_ref()
        .expect("verified complete Lispex package")
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
                "reachable complete Lispex rule `{name}` is not declared and locked"
            ))
        })?;
        let declared_rule = declared.get(name.as_str()).ok_or_else(|| {
            ProductError::new(format!(
                "reachable complete Lispex rule `{name}` is not declared"
            ))
        })?;
        if declared_rule.limits != lock_rule.limits {
            return Err(ProductError::new(format!(
                "reachable complete Lispex rule `{name}` has a stale limits binding"
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
            &format!("[lispex] rule `{name}` full prepared artifact"),
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

fn verify_complete_identity(lock: &LispexLock) -> Result<(), ProductError> {
    let expected = [
        ("profile", lock.profile.as_str(), FULL_PROFILE_ID),
        (
            "feature_set_sha256",
            lock.feature_set_sha256.trim_start_matches("sha256:"),
            FULL_FEATURE_SET_SHA256,
        ),
        (
            "component_id",
            lock.component_id.as_str(),
            FULL_COMPONENT_ID,
        ),
        (
            "component_manifest_sha256",
            lock.component_manifest_sha256.trim_start_matches("sha256:"),
            FULL_COMPONENT_MANIFEST_SHA256,
        ),
        (
            "evaluator_sha256",
            lock.evaluator_sha256.trim_start_matches("sha256:"),
            FULL_EVALUATOR_SHA256,
        ),
        ("abi_id", lock.abi_id.as_str(), ABI_ID),
        (
            "value_codec_id",
            lock.value_codec_id.as_str(),
            VALUE_CODEC_ID,
        ),
        (
            "meter_model_id",
            lock.meter_model_id.as_str(),
            FULL_MODEL_ID,
        ),
        (
            "artifact_contract_id",
            lock.artifact_contract_id.as_str(),
            FULL_ARTIFACT_CONTRACT_ID,
        ),
        ("transcript_id", lock.transcript_id.as_str(), TRANSCRIPT_ID),
        (
            "receipt_core_id",
            lock.receipt_core_id.as_str(),
            RECEIPT_CORE_ID,
        ),
        ("adapter_id", lock.adapter_id.as_str(), FULL_ADAPTER_ID),
        (
            "admission_sha256",
            lock.admission_sha256.trim_start_matches("sha256:"),
            FULL_COMPONENT_ADMISSION_SHA256,
        ),
        ("target", lock.target.as_str(), "native"),
        (
            "target_disposition",
            lock.target_disposition.as_str(),
            FULL_TARGET_DISPOSITION,
        ),
        (
            "handle_catalog_path",
            lock.handle_catalog_path.as_str(),
            FULL_HANDLE_CATALOG_PATH,
        ),
    ];
    for (field, actual, expected) in expected {
        if actual != expected {
            return Err(ProductError::new(format!(
                "topaz.lock [lispex].{field} is not the fixed complete-profile identity"
            )));
        }
    }
    if lock.application.as_deref() != Some(LISPEX_COMPLETE_APPLICATION_PROFILE_ID) {
        return Err(ProductError::new(
            "topaz.lock [lispex].application is not the complete application identity",
        ));
    }
    Ok(())
}

fn verify_full_component_payload() -> Result<(), ProductError> {
    if super::bare_digest(FULL_EVALUATOR) != FULL_EVALUATOR_SHA256 {
        return Err(ProductError::new(
            "retained full Lispex evaluator digest drifted",
        ));
    }
    if super::bare_digest(FULL_COMPONENT_MANIFEST) != FULL_COMPONENT_MANIFEST_SHA256 {
        return Err(ProductError::new(
            "retained full Lispex component manifest digest drifted",
        ));
    }
    Ok(())
}

fn full_prepared_artifact_path(name: &str) -> String {
    format!(
        ".topaz/lispex/rules/{}.lpxfull",
        super::bare_digest(name.as_bytes())
    )
}

fn render_full_catalog(rules: &[LispexLockRule]) -> Vec<u8> {
    let mut out =
        String::from("{\"schema\":\"topaz.lispex-full-rule-handles/v1\",\"profile_id\":\"");
    push_json_text(&mut out, FULL_PROFILE_ID);
    out.push_str("\",\"feature_set_sha256\":\"");
    push_json_text(&mut out, FULL_FEATURE_SET_SHA256);
    out.push_str("\",\"component_id\":\"");
    push_json_text(&mut out, FULL_COMPONENT_ID);
    out.push_str("\",\"evaluator_sha256\":\"");
    push_json_text(&mut out, FULL_EVALUATOR_SHA256);
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

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use topaz_package::{
        LISPEX_COMPLETE_APPLICATION_LANGUAGE, LISPEX_COMPLETE_APPLICATION_STD_VERSION,
        parse_manifest,
    };

    use super::*;

    #[test]
    fn complete_package_prepares_exact_full_profile_lock_for_candidate_source_activation() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("topaz-full-package-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(root.join("rules")).expect("temporary package");
        std::fs::write(
            root.join("rules/refund.lspx"),
            b"(if (< 10 15) \"allow\" \"deny\")\n",
        )
        .expect("full rule source");
        std::fs::write(
            root.join("rules/refund.limits.json"),
            b"{\"schema\":\"topaz.lispex-embed-limits/v1\",\"prepare\":{\"raw_source_bytes\":4096,\"prepare_work\":1000000,\"logical_allocation\":1000000,\"syntax_depth\":64},\"evaluate\":{\"canonical_input_bytes\":4096,\"eval_work\":1000000,\"logical_allocation\":1000000,\"semantic_frames\":1000,\"traversal_depth\":256,\"output_bytes\":1000000,\"diagnostic_bytes\":1000000,\"transcript_bytes\":1000000,\"transcript_events\":100,\"result_bytes\":1000000}}",
        )
        .expect("full rule limits");
        std::fs::write(
            root.join("rules/application.quotas.json"),
            b"{\"schema\":\"topaz.lispex-application-quotas/v1\",\"concurrent_evaluations\":2,\"queued_evaluations\":2,\"total_evaluations\":16,\"aggregate_input_bytes\":65536,\"aggregate_result_bytes\":16000000,\"aggregate_output_bytes\":16000000,\"aggregate_transcript_bytes\":16000000,\"aggregate_safety_fuel\":16000000000,\"prepared_bytes\":1000000,\"wall_millis\":5000}",
        )
        .expect("full application quotas");

        let bounded_manifest = format!(
            "[package]\nname = \"full_app\"\nversion = \"0.1.0\"\nlanguage = \"5.18\"\nentry = \"main.tpz\"\n\n[build]\ntarget = \"native\"\ndeterministic = true\n\n[dependencies]\nstd = \"5.18\"\n\n[lispex]\nprofile = \"{}\"\napplication = \"{}\"\napplication_quotas = \"rules/application.quotas.json\"\n\n[[lispex.rule]]\nname = \"refund\"\nsource = \"rules/refund.lspx\"\nlimits = \"rules/refund.limits.json\"\n",
            topaz_package::LISPEX_BOUNDED_PROFILE_ID,
            topaz_package::LISPEX_APPLICATION_PROFILE_ID,
        );
        let mut manifest = parse_manifest(&bounded_manifest).expect("bounded manifest template");
        manifest.package.language = LISPEX_COMPLETE_APPLICATION_LANGUAGE;
        manifest
            .dependencies
            .get_mut("std")
            .expect("std dependency")
            .version = Some(LISPEX_COMPLETE_APPLICATION_STD_VERSION.into());
        let lispex = manifest.lispex.as_mut().expect("Lispex manifest");
        lispex.profile = LISPEX_COMPLETE_PROFILE_ID.into();
        lispex.application = Some(LISPEX_COMPLETE_APPLICATION_PROFILE_ID.into());
        let future_manifest = bounded_manifest
            .replace("5.18", "5.20")
            .replace(
                topaz_package::LISPEX_BOUNDED_PROFILE_ID,
                LISPEX_COMPLETE_PROFILE_ID,
            )
            .replace(
                topaz_package::LISPEX_APPLICATION_PROFILE_ID,
                LISPEX_COMPLETE_APPLICATION_PROFILE_ID,
            );
        let project = Project {
            root: root.clone(),
            manifest_text: future_manifest,
            manifest,
        };

        let lock =
            super::super::write_locked_package(&project).expect("write complete package closure");
        let verified =
            super::super::verify_locked_package(&project).expect("verify complete package closure");
        assert_eq!(verified, lock);
        assert_eq!(lock.profile, FULL_PROFILE_ID);
        assert_eq!(lock.component_id, FULL_COMPONENT_ID);
        assert_eq!(lock.evaluator_sha256, prefixed(FULL_EVALUATOR_SHA256));
        assert_eq!(lock.target_disposition, FULL_TARGET_DISPOSITION);
        assert_eq!(lock.rules.len(), 1);
        let artifact = std::fs::read(root.join(&lock.rules[0].prepared_artifact_path))
            .expect("full prepared artifact");
        let decoded = decode_full_artifact(&artifact).expect("decode full prepared artifact");
        assert_eq!(decoded.kind, FullArtifactKind::Prepare);
        assert_eq!(decoded.category, FullArtifactCategory::Prepared);
        let lock_text = std::fs::read_to_string(root.join("topaz.lock")).expect("full lock text");
        assert!(lock_text.contains(FULL_COMPONENT_ID));
        assert!(lock_text.contains(FULL_HANDLE_CATALOG_PATH));
        let modules = super::super::application_modules(&project).expect("full generated modules");
        assert_eq!(modules.len(), 2);
        let plan = super::super::checked_application_plan(
            &project,
            ["topaz.lispex-rule-handle/v1:refund"],
        )
        .expect("full checked application plan");
        assert_eq!(plan.rules.len(), 1);
        assert_eq!(plan.rules[0].identity.profile, FULL_PROFILE_ID);
        assert!(plan.payload.requires_runtime);
        assert!(plan.payload.files.iter().any(|file| {
            file.path == "lispex/component/lispex-full-embed-evaluator.wasm"
                && super::super::bare_digest(&file.bytes) == FULL_EVALUATOR_SHA256
        }));
        assert!(LISPEX_COMPLETE_APPLICATION_LANGUAGE.is_selectable());

        std::fs::remove_dir_all(&root).expect("remove temporary package");
    }

    #[test]
    fn complete_application_declines_non_native_products_before_output() {
        let template = format!(
            "[package]\nname = \"full_decline\"\nversion = \"0.1.0\"\nlanguage = \"5.18\"\nentry = \"main.tpz\"\n\n[build]\ntarget = \"native\"\ndeterministic = true\n\n[dependencies]\nstd = \"5.18\"\n\n[lispex]\nprofile = \"{}\"\napplication = \"{}\"\napplication_quotas = \"rules/application.quotas.json\"\n\n[[lispex.rule]]\nname = \"rule\"\nsource = \"rules/rule.lspx\"\nlimits = \"rules/rule.limits.json\"\n",
            topaz_package::LISPEX_BOUNDED_PROFILE_ID,
            topaz_package::LISPEX_APPLICATION_PROFILE_ID,
        );
        for target in ["python", "web", "web-worker", "web-app"] {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "topaz-full-decline-{target}-{}-{nonce}",
                std::process::id()
            ));
            std::fs::create_dir_all(&root).expect("temporary package");
            let mut manifest = parse_manifest(&template).expect("bounded manifest template");
            manifest.package.language = LISPEX_COMPLETE_APPLICATION_LANGUAGE;
            manifest.build.target = target.into();
            manifest
                .dependencies
                .get_mut("std")
                .expect("std dependency")
                .version = Some(LISPEX_COMPLETE_APPLICATION_STD_VERSION.into());
            let lispex = manifest.lispex.as_mut().expect("Lispex manifest");
            lispex.profile = LISPEX_COMPLETE_PROFILE_ID.into();
            lispex.application = Some(LISPEX_COMPLETE_APPLICATION_PROFILE_ID.into());
            let project = Project {
                root: root.clone(),
                manifest_text: template.clone(),
                manifest,
            };
            let error = prepare_complete_package(&project).expect_err("non-native refusal");
            assert!(
                error
                    .message()
                    .contains("is not yet implemented; expected `native`"),
                "{target}: {error}"
            );
            assert!(!root.join(".topaz").exists());
            assert!(!root.join("topaz.lock").exists());
            std::fs::remove_dir_all(&root).expect("remove temporary package");
        }
    }
}
