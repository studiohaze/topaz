use super::*;
use crate::content::hex_sha256;
use crate::*;

/// Verifies the on-disk lock against the manifest and referenced package artifacts.
pub fn check_lock(
    root: impl AsRef<Path>,
    manifest_text: &str,
    manifest: &PackageManifest,
) -> Result<(), PackageError> {
    let root = root.as_ref();
    let lock_text = read_package_text_strict(root, "topaz.lock", "package lockfile")?;
    let lock = parse_lock_document(&lock_text)?;
    verify_package_lock_declarations(&lock.packages, manifest_text, manifest)?;
    verify_extern_lock_declarations(&lock.externs, manifest)?;
    verify_extern_artifact_lock_bytes(root, &lock.externs, manifest)?;
    verify_extern_replay_bytes(root, &lock.externs, manifest)?;
    verify_lispex_lock_declarations(lock.lispex.as_ref(), manifest)?;
    verify_lispex_lock_bytes(root, lock.lispex.as_ref(), manifest)?;
    verify_registry_dependency_content(root, manifest, &lock.packages)?;
    verify_path_dependency_content(root, manifest)
}

/// Checks supplied lock text against an already parsed manifest.
pub fn verify_lock_text(
    lock_text: &str,
    manifest_text: &str,
    manifest: &PackageManifest,
) -> Result<(), PackageError> {
    let lock = parse_lock_document(lock_text)?;
    verify_package_lock_declarations(&lock.packages, manifest_text, manifest)?;
    verify_extern_lock_declarations(&lock.externs, manifest)?;
    verify_lispex_lock_declarations(lock.lispex.as_ref(), manifest)
}

fn verify_package_lock_declarations(
    packages: &[LockPackage],
    manifest_text: &str,
    manifest: &PackageManifest,
) -> Result<(), PackageError> {
    let expected_hash = manifest_sha256(manifest_text);
    let mut root_seen = false;
    let mut locked_dependencies = BTreeSet::new();
    for package in packages {
        if package.source.as_deref() == Some("root") {
            if root_seen {
                return Err(PackageError::new(
                    "topaz.lock: duplicate package with source = \"root\"",
                ));
            }
            root_seen = true;
            if package.path.is_some() || package.hash.is_some() {
                return Err(PackageError::new(
                    "topaz.lock root package permits only `name`, `version`, `source`, and `manifest_hash`",
                ));
            }
            if package.name != manifest.package.name
                || package.version.as_deref() != Some(manifest.package.version.as_str())
            {
                return Err(PackageError::new(format!(
                    "topaz.lock: root package must be `{}` version `{}`",
                    manifest.package.name, manifest.package.version
                )));
            }
            let actual = package.manifest_hash.as_deref().ok_or_else(|| {
                PackageError::new("topaz.lock root package: missing `manifest_hash`")
            })?;
            if actual != expected_hash {
                return Err(PackageError::new(format!(
                    "topaz.lock: root manifest_hash is stale (expected {expected_hash}, got {actual})"
                )));
            }
            continue;
        }

        if package.source.as_deref() == Some("registry") {
            if package.path.is_some() || package.manifest_hash.is_some() {
                return Err(PackageError::new(format!(
                    "topaz.lock registry package `{}` permits only `name`, `version`, `source`, and `hash`",
                    package.name
                )));
            }
            let version = package.version.as_deref().ok_or_else(|| {
                PackageError::new(format!(
                    "topaz.lock: registry package `{}` is missing `version`",
                    package.name
                ))
            })?;
            let hash = package.hash.as_deref().ok_or_else(|| {
                PackageError::new(format!(
                    "topaz.lock: registry package `{}` version `{version}` is missing `hash`",
                    package.name
                ))
            })?;
            let Some(dep) = manifest.dependencies.get(&package.name) else {
                return Err(PackageError::new(format!(
                    "topaz.lock: registry package `{}` is not declared in topaz.toml",
                    package.name
                )));
            };
            if package.name == "std"
                || dep.path.is_some()
                || dep.version.as_deref() != Some(version)
            {
                return Err(PackageError::new(format!(
                    "topaz.lock: registry package `{}` version `{version}` does not match topaz.toml",
                    package.name
                )));
            }
            if !locked_dependencies.insert(package.name.as_str()) {
                return Err(PackageError::new(format!(
                    "topaz.lock: duplicate package `{}`",
                    package.name
                )));
            }
            if let Some(expected_hash) = dep.hash.as_deref()
                && hash != expected_hash
            {
                return Err(PackageError::new(format!(
                    "topaz.lock: registry package `{}` hash does not match topaz.toml (manifest {expected_hash}, lock {hash})",
                    package.name
                )));
            }
            continue;
        }

        let path = package.path.as_deref().ok_or_else(|| {
            PackageError::new(format!(
                "topaz.lock package `{}` must describe a root, local, or registry package",
                package.name
            ))
        })?;
        if package.version.is_some() || package.manifest_hash.is_some() {
            return Err(PackageError::new(format!(
                "topaz.lock local package `{}` permits only `name`, `path`, and `hash`",
                package.name
            )));
        }
        let actual_hash = package.hash.as_deref().ok_or_else(|| {
            PackageError::new(format!(
                "topaz.lock: local package `{}` path `{path}` is missing `hash`",
                package.name
            ))
        })?;
        let Some(dep) = manifest.dependencies.get(&package.name) else {
            return Err(PackageError::new(format!(
                "topaz.lock: local package `{}` is not declared in topaz.toml",
                package.name
            )));
        };
        if package.name == "std" || dep.path.as_deref() != Some(path) {
            return Err(PackageError::new(format!(
                "topaz.lock: local package `{}` path `{path}` does not match topaz.toml",
                package.name
            )));
        }
        if !locked_dependencies.insert(package.name.as_str()) {
            return Err(PackageError::new(format!(
                "topaz.lock: duplicate package `{}`",
                package.name
            )));
        }
        let expected_hash = dep.hash.as_deref().ok_or_else(|| {
            PackageError::new(format!(
                "[dependencies].{} with `path` must include a content `hash`",
                package.name
            ))
        })?;
        if actual_hash != expected_hash {
            return Err(PackageError::new(format!(
                "topaz.lock: local package `{}` hash does not match topaz.toml (manifest {expected_hash}, lock {actual_hash})",
                package.name
            )));
        }
    }

    if !root_seen {
        return Err(PackageError::new(format!(
            "topaz.lock: missing root package `{}` version `{}` with source = \"root\"",
            manifest.package.name, manifest.package.version
        )));
    }
    for name in manifest.dependencies.keys() {
        if name == "std" || locked_dependencies.contains(name.as_str()) {
            continue;
        }
        return Err(PackageError::new(format!(
            "topaz.lock: missing package `{name}` declared in topaz.toml"
        )));
    }
    Ok(())
}

fn verify_extern_lock_declarations(
    externs: &[LockExtern],
    manifest: &PackageManifest,
) -> Result<(), PackageError> {
    let locked = externs
        .iter()
        .map(|item| (item.module.as_str(), item))
        .collect::<BTreeMap<_, _>>();
    for module in locked.keys() {
        if !manifest.externs.contains_key(*module) {
            return Err(PackageError::new(format!(
                "topaz.lock: extern module `{module}` is not declared in topaz.toml"
            )));
        }
    }
    for (name, module) in &manifest.externs {
        let Some(locked) = locked.get(name.as_str()) else {
            return Err(PackageError::new(format!(
                "topaz.lock: missing extern module `{name}`"
            )));
        };
        if locked.hash != module.hash {
            return Err(PackageError::new(format!(
                "topaz.lock: extern module `{name}` hash does not match topaz.toml (manifest {}, lock {})",
                module.hash, locked.hash
            )));
        }
        if locked.abi_hash != module.abi_hash {
            return Err(PackageError::new(format!(
                "topaz.lock: extern module `{name}` abi_hash does not match topaz.toml (manifest {}, lock {})",
                module.abi_hash, locked.abi_hash
            )));
        }
        let manifest_artifact_path = module
            .artifact
            .as_ref()
            .map(|artifact| artifact.path.as_str());
        if locked.artifact_path.as_deref() != manifest_artifact_path {
            let manifest_value = manifest_artifact_path.unwrap_or("<none>");
            let lock_value = locked.artifact_path.as_deref().unwrap_or("<none>");
            return Err(PackageError::new(format!(
                "topaz.lock: extern module `{name}` artifact_path does not match topaz.toml (manifest {manifest_value}, lock {lock_value})"
            )));
        }
        if locked.sandbox != module.sandbox.kind {
            return Err(PackageError::new(format!(
                "topaz.lock: extern module `{name}` sandbox does not match topaz.toml (manifest {}, lock {})",
                module.sandbox.kind.as_str(),
                locked.sandbox.as_str()
            )));
        }
        if locked.fuel != module.sandbox.fuel {
            return Err(PackageError::new(format!(
                "topaz.lock: extern module `{name}` fuel does not match topaz.toml (manifest {}, lock {})",
                optional_u64_text(module.sandbox.fuel),
                optional_u64_text(locked.fuel)
            )));
        }
        if locked.memory_bytes != module.sandbox.memory_bytes {
            return Err(PackageError::new(format!(
                "topaz.lock: extern module `{name}` memory_bytes does not match topaz.toml (manifest {}, lock {})",
                optional_u64_text(module.sandbox.memory_bytes),
                optional_u64_text(locked.memory_bytes)
            )));
        }
    }
    Ok(())
}

fn optional_u64_text(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "<none>".to_string())
}

fn verify_extern_replay_bytes(
    root: &Path,
    externs: &[LockExtern],
    manifest: &PackageManifest,
) -> Result<(), PackageError> {
    let locked = externs
        .iter()
        .map(|item| (item.module.as_str(), item))
        .collect::<BTreeMap<_, _>>();
    for (name, module) in &manifest.externs {
        let Some(locked) = locked.get(name.as_str()) else {
            return Err(PackageError::new(format!(
                "topaz.lock: missing extern module `{name}`"
            )));
        };
        let actual_hash = extern_replay_hash(root, name, module)?;
        if actual_hash != locked.replay_hash {
            return Err(PackageError::new(format!(
                "topaz.lock: extern module `{name}` replay_hash is stale (expected {actual_hash}, got {})",
                locked.replay_hash
            )));
        }
    }
    Ok(())
}

pub(super) fn verify_extern_artifact_bytes(
    root: &Path,
    name: &str,
    module: &ExternModule,
) -> Result<(), PackageError> {
    if module.artifact.is_none() {
        return Ok(());
    }
    let actual_hash = extern_artifact_hash(root, name, module)?;
    if actual_hash != module.hash {
        return Err(PackageError::new(format!(
            "extern module `{name}` artifact hash is stale (expected {actual_hash}, got {})",
            module.hash
        )));
    }
    Ok(())
}

fn verify_extern_artifact_lock_bytes(
    root: &Path,
    externs: &[LockExtern],
    manifest: &PackageManifest,
) -> Result<(), PackageError> {
    let locked = externs
        .iter()
        .map(|item| (item.module.as_str(), item))
        .collect::<BTreeMap<_, _>>();
    for (name, module) in &manifest.externs {
        let Some(locked) = locked.get(name.as_str()) else {
            return Err(PackageError::new(format!(
                "topaz.lock: missing extern module `{name}`"
            )));
        };
        if module.artifact.is_some() {
            let actual_hash = extern_artifact_hash(root, name, module)?;
            if actual_hash != locked.hash {
                return Err(PackageError::new(format!(
                    "topaz.lock: extern module `{name}` artifact hash is stale (expected {actual_hash}, got {})",
                    locked.hash
                )));
            }
        }
    }
    Ok(())
}

/// Matches optional Lispex lock declarations to the manifest's selected profile.
pub fn verify_lispex_lock_declarations(
    lock: Option<&LispexLock>,
    manifest: &PackageManifest,
) -> Result<(), PackageError> {
    match (&manifest.lispex, lock) {
        (None, None) => return Ok(()),
        (None, Some(_)) => {
            return Err(PackageError::new(
                "topaz.lock contains [lispex] but topaz.toml does not",
            ));
        }
        (Some(_), None) => {
            return Err(PackageError::new(
                "topaz.lock is missing [lispex] and [[lispex.rule]] rows",
            ));
        }
        (Some(_), Some(_)) => {}
    }
    let config = manifest.lispex.as_ref().expect("matched manifest Lispex");
    let lock = lock.expect("matched lock Lispex");
    if lock.profile != config.profile {
        return Err(PackageError::new(format!(
            "topaz.lock [lispex].profile is stale (manifest `{}`, lock `{}`)",
            config.profile, lock.profile
        )));
    }
    if lock.application != config.application {
        return Err(PackageError::new(format!(
            "topaz.lock [lispex].application is stale (manifest {:?}, lock {:?})",
            config.application, lock.application
        )));
    }
    if lock.application_quotas != config.application_quotas {
        return Err(PackageError::new(format!(
            "topaz.lock [lispex].application_quotas is stale (manifest {:?}, lock {:?})",
            config.application_quotas, lock.application_quotas
        )));
    }
    if lock.target != manifest.build.target || lock.target_disposition != "native-only" {
        return Err(PackageError::new(format!(
            "topaz.lock [lispex] target disposition is stale or unsupported (target `{}`, disposition `{}`)",
            lock.target, lock.target_disposition
        )));
    }
    if config.rules.len() != lock.rules.len() {
        return Err(PackageError::new(format!(
            "topaz.lock [lispex] rule count is stale (manifest {}, lock {})",
            config.rules.len(),
            lock.rules.len()
        )));
    }
    let locked = lock
        .rules
        .iter()
        .map(|rule| (rule.name.as_str(), rule))
        .collect::<BTreeMap<_, _>>();
    for rule in &config.rules {
        let Some(locked) = locked.get(rule.name.as_str()) else {
            return Err(PackageError::new(format!(
                "topaz.lock [lispex] is missing rule `{}`",
                rule.name
            )));
        };
        if locked.source != rule.source || locked.limits != rule.limits {
            return Err(PackageError::new(format!(
                "topaz.lock [lispex] rule `{}` paths do not match topaz.toml",
                rule.name
            )));
        }
    }
    Ok(())
}

fn verify_lispex_lock_bytes(
    root: &Path,
    lock: Option<&LispexLock>,
    manifest: &PackageManifest,
) -> Result<(), PackageError> {
    let Some(config) = &manifest.lispex else {
        return Ok(());
    };
    let lock = lock.ok_or_else(|| PackageError::new("topaz.lock is missing [lispex]"))?;
    if let Some(path) = &config.application_quotas {
        let bytes = read_package_file_strict(root, path, "[lispex] application quotas")?;
        let actual = hex_sha256(&bytes);
        if lock.application_quotas_sha256.as_deref() != Some(actual.as_str()) {
            return Err(PackageError::new(format!(
                "topaz.lock [lispex] application quota hash is stale (expected {actual}, got {:?})",
                lock.application_quotas_sha256
            )));
        }
    }
    let locked = lock
        .rules
        .iter()
        .map(|rule| (rule.name.as_str(), rule))
        .collect::<BTreeMap<_, _>>();
    for rule in &config.rules {
        let locked = locked.get(rule.name.as_str()).ok_or_else(|| {
            PackageError::new(format!("topaz.lock is missing rule `{}`", rule.name))
        })?;
        for (kind, path, expected) in [
            ("source", &rule.source, &locked.source_sha256),
            ("limits", &rule.limits, &locked.limits_sha256),
            (
                "prepared artifact",
                &locked.prepared_artifact_path,
                &locked.prepared_artifact_sha256,
            ),
        ] {
            let bytes = read_package_file_strict(
                root,
                path,
                &format!("[lispex] rule `{}` {kind}", rule.name),
            )?;
            let actual = hex_sha256(&bytes);
            if actual != expected.as_str() {
                return Err(PackageError::new(format!(
                    "topaz.lock [lispex] rule `{}` {kind} hash is stale (expected {expected}, got {actual})",
                    rule.name
                )));
            }
        }
    }
    let catalog = read_package_file_strict(
        root,
        &lock.handle_catalog_path,
        "topaz.lock [lispex] handle catalog",
    )?;
    let actual = hex_sha256(&catalog);
    if actual != lock.handle_catalog_sha256 {
        return Err(PackageError::new(format!(
            "topaz.lock [lispex] handle catalog hash is stale (expected {}, got {actual})",
            lock.handle_catalog_sha256,
        )));
    }
    Ok(())
}
