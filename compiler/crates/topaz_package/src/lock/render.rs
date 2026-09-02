use super::*;
use crate::*;

/// Renders the canonical lock document for a package without a Lispex profile.
pub fn render_lockfile(project: &Project) -> Result<String, PackageError> {
    if project.manifest.lispex.is_some() {
        return Err(PackageError::new(
            "[lispex] lock resolution requires the exact Lispex product resolver",
        ));
    }
    render_lockfile_base(project)
}

/// Renders a canonical package lock after validating the supplied Lispex lock.
pub fn render_lockfile_with_lispex(
    project: &Project,
    lispex: &LispexLock,
) -> Result<String, PackageError> {
    verify_lispex_lock_declarations(Some(lispex), &project.manifest)?;
    let mut out = render_lockfile_base(project)?;
    render_lispex_lock(&mut out, lispex);
    Ok(out)
}

fn render_lockfile_base(project: &Project) -> Result<String, PackageError> {
    let mut out = String::new();
    out.push_str("[[package]]\n");
    push_lock_string(&mut out, "name", &project.manifest.package.name);
    push_lock_string(&mut out, "version", &project.manifest.package.version);
    push_lock_string(&mut out, "source", "root");
    push_lock_string(
        &mut out,
        "manifest_hash",
        &manifest_sha256(&project.manifest_text),
    );

    for (name, dep) in &project.manifest.dependencies {
        if name == "std" {
            continue;
        }
        out.push('\n');
        out.push_str("[[package]]\n");
        push_lock_string(&mut out, "name", name);
        match (&dep.path, &dep.version) {
            (Some(path), _) => {
                let expected_hash = dep.hash.as_deref().ok_or_else(|| {
                    PackageError::new(format!(
                        "[dependencies].{name} with `path` must include a content `hash`"
                    ))
                })?;
                let dep_root = project.root.join(path);
                let dep_project = Project::load(&dep_root)?;
                if dep_project.manifest.package.name != *name {
                    return Err(PackageError::new(format!(
                        "local package `{name}` points to `{}` whose [package].name is `{}`",
                        dep_root.to_string_lossy(),
                        dep_project.manifest.package.name
                    )));
                }
                let actual_hash = package_content_hash(&dep_root)?;
                if actual_hash != expected_hash {
                    return Err(PackageError::new(format!(
                        "local package `{name}` content hash is stale (expected {expected_hash}, got {actual_hash})"
                    )));
                }
                push_lock_string(&mut out, "path", path);
                push_lock_string(&mut out, "hash", expected_hash);
            }
            (None, Some(version)) => {
                let computed_hash;
                let hash = match dep.hash.as_deref() {
                    Some(hash) => {
                        validate_sha256_hash(&format!("[dependencies].{name}.hash"), hash)?;
                        hash
                    }
                    None => {
                        let vendor_root = registry_vendor_root(&project.root, name, version);
                        let vendor_project = Project::load(&vendor_root).map_err(|e| {
                            PackageError::new(format!(
                                "registry package `{name}` version `{version}` needs `hash` in topaz.toml or vendored content at `{}` before `topaz lock`: {e}",
                                vendor_root.to_string_lossy()
                            ))
                        })?;
                        if vendor_project.manifest.package.name != *name
                            || vendor_project.manifest.package.version != *version
                        {
                            return Err(PackageError::new(format!(
                                "vendored registry package `{name}` version `{version}` points to `{}` whose [package] is `{}` version `{}`",
                                vendor_root.to_string_lossy(),
                                vendor_project.manifest.package.name,
                                vendor_project.manifest.package.version
                            )));
                        }
                        computed_hash = package_content_hash(&vendor_root)?;
                        computed_hash.as_str()
                    }
                };
                push_lock_string(&mut out, "version", version);
                push_lock_string(&mut out, "source", "registry");
                push_lock_string(&mut out, "hash", hash);
            }
            (None, None) => {
                return Err(PackageError::new(format!(
                    "[dependencies].{name} must include `version` or `path`"
                )));
            }
        }
    }
    for (name, module) in &project.manifest.externs {
        out.push('\n');
        out.push_str("[[extern]]\n");
        push_lock_string(&mut out, "module", name);
        verify_extern_artifact_bytes(&project.root, name, module)?;
        push_lock_string(&mut out, "hash", &module.hash);
        push_lock_string(&mut out, "abi_hash", &module.abi_hash);
        if let Some(artifact) = &module.artifact {
            push_lock_string(&mut out, "artifact_path", &artifact.path);
        }
        push_lock_string(&mut out, "sandbox", module.sandbox.kind.as_str());
        if let Some(fuel) = module.sandbox.fuel {
            push_lock_u64(&mut out, "fuel", fuel);
        }
        if let Some(memory_bytes) = module.sandbox.memory_bytes {
            push_lock_u64(&mut out, "memory_bytes", memory_bytes);
        }
        let replay_hash = extern_replay_hash(&project.root, name, module)?;
        push_lock_string(&mut out, "replay_hash", &replay_hash);
    }
    Ok(out)
}

fn push_lock_string(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push_str(" = ");
    push_basic_string(out, value);
    out.push('\n');
}

fn push_lock_u64(out: &mut String, key: &str, value: u64) {
    out.push_str(key);
    out.push_str(" = ");
    out.push_str(&value.to_string());
    out.push('\n');
}

fn push_basic_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn render_lispex_lock(out: &mut String, lispex: &LispexLock) {
    out.push('\n');
    out.push_str("[lispex]\n");
    push_lock_string(out, "profile", &lispex.profile);
    if let Some(application) = &lispex.application {
        push_lock_string(out, "application", application);
    }
    if let Some(path) = &lispex.application_quotas {
        push_lock_string(out, "application_quotas", path);
        push_lock_string(
            out,
            "application_quotas_sha256",
            lispex
                .application_quotas_sha256
                .as_deref()
                .expect("application quota path requires a digest"),
        );
    }
    push_lock_string(out, "feature_set_sha256", &lispex.feature_set_sha256);
    push_lock_string(out, "component_id", &lispex.component_id);
    push_lock_string(
        out,
        "component_manifest_sha256",
        &lispex.component_manifest_sha256,
    );
    push_lock_string(out, "evaluator_sha256", &lispex.evaluator_sha256);
    push_lock_string(out, "abi_id", &lispex.abi_id);
    push_lock_string(out, "value_codec_id", &lispex.value_codec_id);
    push_lock_string(out, "meter_model_id", &lispex.meter_model_id);
    push_lock_string(out, "artifact_contract_id", &lispex.artifact_contract_id);
    push_lock_string(out, "transcript_id", &lispex.transcript_id);
    push_lock_string(out, "receipt_core_id", &lispex.receipt_core_id);
    push_lock_string(out, "adapter_id", &lispex.adapter_id);
    push_lock_string(out, "admission_sha256", &lispex.admission_sha256);
    push_lock_string(out, "target", &lispex.target);
    push_lock_string(out, "target_disposition", &lispex.target_disposition);
    push_lock_string(out, "handle_catalog_path", &lispex.handle_catalog_path);
    push_lock_string(out, "handle_catalog_sha256", &lispex.handle_catalog_sha256);
    for rule in &lispex.rules {
        out.push('\n');
        out.push_str("[[lispex.rule]]\n");
        push_lock_string(out, "name", &rule.name);
        push_lock_string(out, "source", &rule.source);
        push_lock_string(out, "source_sha256", &rule.source_sha256);
        push_lock_string(out, "limits", &rule.limits);
        push_lock_string(out, "limits_sha256", &rule.limits_sha256);
        push_lock_string(
            out,
            "preparation_request_sha256",
            &rule.preparation_request_sha256,
        );
        push_lock_string(
            out,
            "preparation_submission_sha256",
            &rule.preparation_submission_sha256,
        );
        push_lock_string(out, "prepared_artifact_path", &rule.prepared_artifact_path);
        push_lock_string(
            out,
            "prepared_artifact_sha256",
            &rule.prepared_artifact_sha256,
        );
    }
}
