use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PackageRegistryCommand {
    Fetch,
    Vendor,
}

impl PackageRegistryCommand {
    pub(super) fn name(self) -> &'static str {
        match self {
            PackageRegistryCommand::Fetch => "fetch",
            PackageRegistryCommand::Vendor => "vendor",
        }
    }

    pub(super) fn done_verb(self) -> &'static str {
        match self {
            PackageRegistryCommand::Fetch => "fetched",
            PackageRegistryCommand::Vendor => "vendored",
        }
    }
}

pub(super) fn fetch_package(
    root: Option<&str>,
    version_arg: Option<LangVersion>,
    from: Option<&str>,
) -> ExitCode {
    if from.is_none() {
        eprintln!(
            "topaz: `fetch` requires `--from <local-registry>` in v5.4's deterministic \
             local-registry mode"
        );
        return ExitCode::FAILURE;
    }
    registry_package(root, version_arg, from, PackageRegistryCommand::Fetch)
}

pub(super) fn vendor_package(
    root: Option<&str>,
    version_arg: Option<LangVersion>,
    from: Option<&str>,
) -> ExitCode {
    registry_package(root, version_arg, from, PackageRegistryCommand::Vendor)
}

pub(super) fn registry_package(
    root: Option<&str>,
    version_arg: Option<LangVersion>,
    from: Option<&str>,
    command: PackageRegistryCommand,
) -> ExitCode {
    if version_arg.is_some() {
        eprintln!(
            "topaz: `{}` uses topaz.toml [package].language; drop --language-version",
            command.name()
        );
        return ExitCode::FAILURE;
    }
    let root = root.unwrap_or(".");
    let project = match topaz_package::Project::load(root) {
        Ok(project) => project,
        Err(e) => {
            eprintln!("topaz: {e}");
            return ExitCode::FAILURE;
        }
    };
    let registry_root = from.map(PathBuf::from);
    let mut vendored = 0usize;
    let mut replacements = Vec::new();
    for (name, dep) in &project.manifest.dependencies {
        if name == "std" || dep.path.is_some() {
            continue;
        }
        let Some(version) = &dep.version else {
            continue;
        };
        let vendor_root = topaz_package::registry_vendor_root(&project.root, name, version);
        if let Some(registry_root) = &registry_root {
            let source_root = registry_root.join(name).join(version);
            let replacement = match topaz_package::replace_registry_vendor_package(
                &project.root,
                &source_root,
                name,
                version,
                dep.hash.as_deref(),
            ) {
                Ok(replacement) => replacement,
                Err(error) => {
                    eprintln!("topaz: {error}");
                    return ExitCode::FAILURE;
                }
            };
            replacements.push(replacement);
        } else {
            let vendor_project = match topaz_package::Project::load(&vendor_root) {
                Ok(project) => project,
                Err(e) => {
                    eprintln!(
                        "topaz: registry package `{name}` version `{version}` needs vendored content \
                         at `{}` or `vendor --from <local-registry>`: {e}",
                        vendor_root.to_string_lossy(),
                    );
                    return ExitCode::FAILURE;
                }
            };
            if vendor_project.manifest.package.name != *name
                || vendor_project.manifest.package.version != *version
            {
                eprintln!(
                    "topaz: vendored registry package `{name}` version `{version}` points to `{}` \
                     whose [package] is `{}` version `{}`",
                    vendor_root.to_string_lossy(),
                    vendor_project.manifest.package.name,
                    vendor_project.manifest.package.version
                );
                return ExitCode::FAILURE;
            }
            let hash = match topaz_package::package_content_hash(&vendor_root) {
                Ok(hash) => hash,
                Err(e) => {
                    eprintln!("topaz: {e}");
                    return ExitCode::FAILURE;
                }
            };
            if let Some(expected_hash) = dep.hash.as_deref()
                && hash != expected_hash
            {
                eprintln!(
                    "topaz: vendored registry package `{name}` version `{version}` content hash is \
                     stale (expected {expected_hash}, got {hash})"
                );
                return ExitCode::FAILURE;
            }
        }
        vendored += 1;
    }
    if project.manifest.lispex.is_some() {
        if let Err(error) = topaz_lispex_product::write_locked_package(&project) {
            eprintln!("topaz: {error}");
            return ExitCode::FAILURE;
        }
    } else if let Err(e) = project.write_lockfile() {
        eprintln!("topaz: {e}");
        return ExitCode::FAILURE;
    }
    for replacement in replacements {
        if let Err(error) = replacement.commit() {
            eprintln!("topaz: warning: {error}");
        }
    }
    eprintln!(
        "topaz: {} {vendored} registry package(s); wrote `{}`",
        command.done_verb(),
        project.root.join("topaz.lock").to_string_lossy()
    );
    ExitCode::SUCCESS
}
