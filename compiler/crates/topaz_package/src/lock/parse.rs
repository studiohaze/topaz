use super::*;
use crate::manifest::*;
use crate::*;

pub(crate) fn parse_lock_document(lock_text: &str) -> Result<ParsedLock, PackageError> {
    let doc = toml_parse_document(lock_text)
        .map_err(|e| PackageError::new(format!("topaz.lock parse error: {e}")))?;
    let root = expect_table(&doc, "topaz.lock")?;
    reject_unknown_keys(root, "topaz.lock", &["package", "extern", "lispex"])?;
    Ok(ParsedLock {
        packages: parse_lock_packages(root)?,
        externs: parse_lock_externs(root)?,
        lispex: parse_lock_lispex_table(root)?,
    })
}

fn parse_lock_packages(
    root: &BTreeMap<Rc<str>, TomlValue>,
) -> Result<Vec<LockPackage>, PackageError> {
    let packages = match root.get("package") {
        Some(TomlValue::Array(items)) => items,
        Some(_) => {
            return Err(PackageError::new(
                "topaz.lock: `package` must be an array of tables",
            ));
        }
        None => {
            return Err(PackageError::new(
                "topaz.lock: missing `[[package]]` entries",
            ));
        }
    };
    let mut out = Vec::new();
    for item in packages.iter() {
        let table = expect_table(item, "topaz.lock [[package]]")?;
        reject_unknown_keys(
            table,
            "topaz.lock [[package]]",
            &["name", "version", "source", "path", "hash", "manifest_hash"],
        )?;
        let name = required_string(table, "name", "topaz.lock [[package]]")?;
        validate_package_name(&name)?;
        let version = optional_string(table, "version", "topaz.lock [[package]]")?;
        if let Some(version) = &version {
            validate_package_version("topaz.lock [[package]].version", version)?;
        }
        let source = optional_string(table, "source", "topaz.lock [[package]]")?;
        if let Some(source) = &source {
            validate_lock_package_source(source)?;
        }
        let path = optional_string(table, "path", "topaz.lock [[package]]")?;
        let hash = optional_string(table, "hash", "topaz.lock [[package]]")?;
        let manifest_hash = optional_string(table, "manifest_hash", "topaz.lock [[package]]")?;
        if let Some(hash) = &hash {
            validate_sha256_hash("topaz.lock [[package]].hash", hash)?;
        }
        if let Some(hash) = &manifest_hash {
            validate_sha256_hash("topaz.lock [[package]].manifest_hash", hash)?;
        }
        out.push(LockPackage {
            name,
            version,
            source,
            path,
            hash,
            manifest_hash,
        });
    }
    Ok(out)
}

fn validate_lock_package_source(source: &str) -> Result<(), PackageError> {
    match source {
        "root" | "registry" => Ok(()),
        _ => Err(PackageError::new(format!(
            "topaz.lock [[package]].source `{source}` is not supported in v5.4; \
             use `registry` for local-registry vendored packages"
        ))),
    }
}

fn parse_lock_externs(
    root: &BTreeMap<Rc<str>, TomlValue>,
) -> Result<Vec<LockExtern>, PackageError> {
    let Some(externs) = root.get("extern") else {
        return Ok(Vec::new());
    };
    let TomlValue::Array(items) = externs else {
        return Err(PackageError::new(
            "topaz.lock: `extern` must be an array of tables",
        ));
    };
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(items.len());
    for item in items.iter() {
        let table = expect_table(item, "topaz.lock [[extern]]")?;
        reject_unknown_keys(
            table,
            "topaz.lock [[extern]]",
            &[
                "module",
                "hash",
                "abi_hash",
                "artifact_path",
                "sandbox",
                "fuel",
                "memory_bytes",
                "replay_hash",
            ],
        )?;
        let module = required_string(table, "module", "topaz.lock [[extern]]")?;
        validate_extern_module_name("topaz.lock [[extern]].module", &module)?;
        if !seen.insert(module.clone()) {
            return Err(PackageError::new(format!(
                "topaz.lock: duplicate extern module `{module}`"
            )));
        }
        let hash = required_string(table, "hash", "topaz.lock [[extern]]")?;
        validate_sha256_hash("topaz.lock [[extern]].hash", &hash)?;
        let abi_hash = required_string(table, "abi_hash", "topaz.lock [[extern]]")?;
        validate_sha256_hash("topaz.lock [[extern]].abi_hash", &abi_hash)?;
        let artifact_path = optional_string(table, "artifact_path", "topaz.lock [[extern]]")?
            .map(|path| normalize_project_path("topaz.lock [[extern]].artifact_path", &path))
            .transpose()?;
        let sandbox = parse_extern_sandbox_kind_field(
            "topaz.lock [[extern]].sandbox",
            &required_string(table, "sandbox", "topaz.lock [[extern]]")?,
        )?;
        let fuel = optional_u64(table, "fuel", "topaz.lock [[extern]]")?;
        let memory_bytes = optional_u64(table, "memory_bytes", "topaz.lock [[extern]]")?;
        let replay_hash = required_string(table, "replay_hash", "topaz.lock [[extern]]")?;
        validate_sha256_hash("topaz.lock [[extern]].replay_hash", &replay_hash)?;
        out.push(LockExtern {
            module,
            hash,
            abi_hash,
            artifact_path,
            sandbox,
            fuel,
            memory_bytes,
            replay_hash,
        });
    }
    Ok(out)
}

/// Decodes the optional Lispex product section from an admitted lock document.
pub fn parse_lock_lispex(lock_text: &str) -> Result<Option<LispexLock>, PackageError> {
    Ok(parse_lock_document(lock_text)?.lispex)
}

fn parse_lock_lispex_table(
    root: &BTreeMap<Rc<str>, TomlValue>,
) -> Result<Option<LispexLock>, PackageError> {
    let Some(table) = optional_table(root, "lispex")? else {
        return Ok(None);
    };
    reject_unknown_keys(
        table,
        "topaz.lock [lispex]",
        &[
            "profile",
            "application",
            "application_quotas",
            "application_quotas_sha256",
            "feature_set_sha256",
            "component_id",
            "component_manifest_sha256",
            "evaluator_sha256",
            "abi_id",
            "value_codec_id",
            "meter_model_id",
            "artifact_contract_id",
            "transcript_id",
            "receipt_core_id",
            "adapter_id",
            "admission_sha256",
            "target",
            "target_disposition",
            "handle_catalog_path",
            "handle_catalog_sha256",
            "rule",
        ],
    )?;
    let hash = |key: &str| -> Result<String, PackageError> {
        let value = required_string(table, key, "topaz.lock [lispex]")?;
        validate_sha256_hash(&format!("topaz.lock [lispex].{key}"), &value)?;
        Ok(value)
    };
    let profile = required_string(table, "profile", "topaz.lock [lispex]")?;
    let application = optional_string(table, "application", "topaz.lock [lispex]")?;
    if let Some(application) = &application
        && !matches!(
            application.as_str(),
            LISPEX_APPLICATION_PROFILE_ID | LISPEX_COMPLETE_APPLICATION_PROFILE_ID
        )
    {
        return Err(PackageError::new(format!(
            "topaz.lock [lispex].application `{application}` is unsupported"
        )));
    }
    let application_quotas = optional_string(table, "application_quotas", "topaz.lock [lispex]")?
        .map(|path| normalize_project_path("topaz.lock [lispex].application_quotas", &path))
        .transpose()?;
    let application_quotas_sha256 =
        optional_string(table, "application_quotas_sha256", "topaz.lock [lispex]")?;
    if application.is_some() != application_quotas.is_some()
        || application_quotas.is_some() != application_quotas_sha256.is_some()
    {
        return Err(PackageError::new(
            "topaz.lock [lispex] application, application_quotas, and application_quotas_sha256 must appear together",
        ));
    }
    if let Some(value) = &application_quotas_sha256 {
        validate_sha256_hash("topaz.lock [lispex].application_quotas_sha256", value)?;
    }
    let component_id = required_nonempty_lock_string(table, "component_id")?;
    let abi_id = required_nonempty_lock_string(table, "abi_id")?;
    let value_codec_id = required_nonempty_lock_string(table, "value_codec_id")?;
    let meter_model_id = required_nonempty_lock_string(table, "meter_model_id")?;
    let artifact_contract_id = required_nonempty_lock_string(table, "artifact_contract_id")?;
    let transcript_id = required_nonempty_lock_string(table, "transcript_id")?;
    let receipt_core_id = required_nonempty_lock_string(table, "receipt_core_id")?;
    let adapter_id = required_nonempty_lock_string(table, "adapter_id")?;
    let target = required_nonempty_lock_string(table, "target")?;
    let target_disposition = required_nonempty_lock_string(table, "target_disposition")?;
    let handle_catalog_path = normalize_project_path(
        "topaz.lock [lispex].handle_catalog_path",
        &required_string(table, "handle_catalog_path", "topaz.lock [lispex]")?,
    )?;
    let rules_value = table
        .get("rule")
        .ok_or_else(|| PackageError::new("topaz.lock [lispex] requires [[lispex.rule]] rows"))?;
    let TomlValue::Array(items) = rules_value else {
        return Err(PackageError::new(
            "topaz.lock [lispex].rule must be an array of tables",
        ));
    };
    let mut seen = BTreeSet::new();
    let mut rules = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let context = format!("topaz.lock [[lispex.rule]][{index}]");
        let rule = expect_table(item, &context)?;
        reject_unknown_keys(
            rule,
            &context,
            &[
                "name",
                "source",
                "source_sha256",
                "limits",
                "limits_sha256",
                "preparation_request_sha256",
                "preparation_submission_sha256",
                "prepared_artifact_path",
                "prepared_artifact_sha256",
            ],
        )?;
        let name = required_string(rule, "name", &context)?;
        validate_lispex_rule_name(&format!("{context}.name"), &name)?;
        if !seen.insert(name.clone()) {
            return Err(PackageError::new(format!(
                "topaz.lock [lispex]: duplicate rule name `{name}`"
            )));
        }
        if application.is_some() && LISPEX_APPLICATION_EXPORTS.contains(&name.as_str()) {
            return Err(PackageError::new(format!(
                "{context}.name `{name}` conflicts with a reserved std.lispex export"
            )));
        }
        let source = normalize_project_path(
            &format!("{context}.source"),
            &required_string(rule, "source", &context)?,
        )?;
        let limits = normalize_project_path(
            &format!("{context}.limits"),
            &required_string(rule, "limits", &context)?,
        )?;
        let prepared_artifact_path = normalize_project_path(
            &format!("{context}.prepared_artifact_path"),
            &required_string(rule, "prepared_artifact_path", &context)?,
        )?;
        let source_sha256 = required_sha256_lock_string(rule, "source_sha256", &context)?;
        let limits_sha256 = required_sha256_lock_string(rule, "limits_sha256", &context)?;
        let preparation_request_sha256 =
            required_sha256_lock_string(rule, "preparation_request_sha256", &context)?;
        let preparation_submission_sha256 =
            required_sha256_lock_string(rule, "preparation_submission_sha256", &context)?;
        let prepared_artifact_sha256 =
            required_sha256_lock_string(rule, "prepared_artifact_sha256", &context)?;
        rules.push(LispexLockRule {
            name,
            source,
            source_sha256,
            limits,
            limits_sha256,
            preparation_request_sha256,
            preparation_submission_sha256,
            prepared_artifact_path,
            prepared_artifact_sha256,
        });
    }
    if rules.is_empty() {
        return Err(PackageError::new(
            "topaz.lock [lispex] requires at least one [[lispex.rule]] row",
        ));
    }
    Ok(Some(LispexLock {
        profile,
        application,
        application_quotas,
        application_quotas_sha256,
        feature_set_sha256: hash("feature_set_sha256")?,
        component_id,
        component_manifest_sha256: hash("component_manifest_sha256")?,
        evaluator_sha256: hash("evaluator_sha256")?,
        abi_id,
        value_codec_id,
        meter_model_id,
        artifact_contract_id,
        transcript_id,
        receipt_core_id,
        adapter_id,
        admission_sha256: hash("admission_sha256")?,
        target,
        target_disposition,
        handle_catalog_path,
        handle_catalog_sha256: hash("handle_catalog_sha256")?,
        rules,
    }))
}

fn required_nonempty_lock_string(
    table: &BTreeMap<Rc<str>, TomlValue>,
    key: &str,
) -> Result<String, PackageError> {
    let value = required_string(table, key, "topaz.lock [lispex]")?;
    if value.trim().is_empty() {
        return Err(PackageError::new(format!(
            "topaz.lock [lispex].{key} must be non-empty"
        )));
    }
    Ok(value)
}

fn required_sha256_lock_string(
    table: &BTreeMap<Rc<str>, TomlValue>,
    key: &str,
    context: &str,
) -> Result<String, PackageError> {
    let value = required_string(table, key, context)?;
    validate_sha256_hash(&format!("{context}.{key}"), &value)?;
    Ok(value)
}
