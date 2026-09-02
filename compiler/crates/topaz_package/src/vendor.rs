use crate::content::PackageContentFile;
use crate::lock::LockPackage;
use crate::strict_io::ensure_plain_package_directory;
use crate::*;

/// The deterministic offline registry-vendor layout for v5.4:
/// `<package-root>/vendor/<name>/<version>/topaz.toml`.
pub fn registry_vendor_root(root: impl AsRef<Path>, name: &str, version: &str) -> PathBuf {
    root.as_ref().join("vendor").join(name).join(version)
}

/// Owns one published vendor directory until its caller has written the lock
/// metadata that names the new content.
#[derive(Debug)]
#[must_use = "registry vendor replacements roll back unless committed"]
pub struct RegistryVendorReplacement {
    destination: PathBuf,
    backup: Option<PathBuf>,
    active: bool,
}

impl RegistryVendorReplacement {
    /// Commit the published vendor directory after the caller has written the
    /// lockfile that names it. A cleanup error leaves the published directory
    /// active and is safe to report as a non-fatal cleanup warning.
    pub fn commit(mut self) -> Result<(), PackageError> {
        self.active = false;
        if let Some(backup) = self.backup.take() {
            std::fs::remove_dir_all(&backup).map_err(|error| {
                PackageError::new(format!(
                    "cannot remove committed registry vendor backup `{}`: {error}",
                    backup.display()
                ))
            })?;
        }
        Ok(())
    }

    fn rollback(&mut self) -> Result<(), PackageError> {
        if !self.active {
            return Ok(());
        }
        match std::fs::remove_dir_all(&self.destination) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(PackageError::new(format!(
                    "cannot remove uncommitted registry vendor package `{}`: {error}",
                    self.destination.display()
                )));
            }
        }
        if let Some(backup) = &self.backup {
            std::fs::rename(backup, &self.destination).map_err(|error| {
                PackageError::new(format!(
                    "cannot restore registry vendor package `{}`: {error}",
                    self.destination.display()
                ))
            })?;
        }
        self.active = false;
        Ok(())
    }
}

impl Drop for RegistryVendorReplacement {
    fn drop(&mut self) {
        let _ = self.rollback();
    }
}

/// Admit one explicit local-registry package as an exact source snapshot and
/// replace its deterministic vendor directory without following package-local
/// symbolic links. Candidate identity and an optional manifest hash are checked
/// before the existing vendored package is moved.
pub fn replace_registry_vendor_package(
    package_root: &Path,
    source_root: &Path,
    name: &str,
    version: &str,
    expected_hash: Option<&str>,
) -> Result<RegistryVendorReplacement, PackageError> {
    validate_package_name(name)?;
    validate_package_version("registry package version", version)?;
    let content = read_package_content(source_root)?;
    let manifest_file = content
        .files
        .iter()
        .find(|file| file.relative == "topaz.toml")
        .ok_or_else(|| PackageError::new("registry package has no admitted topaz.toml"))?;
    let manifest_text = std::str::from_utf8(&manifest_file.bytes).map_err(|error| {
        PackageError::new(format!(
            "registry package `{name}` version `{version}` topaz.toml must contain valid UTF-8: {error}"
        ))
    })?;
    let manifest = parse_manifest(manifest_text)?;
    if manifest.package.name != name || manifest.package.version != version {
        return Err(PackageError::new(format!(
            "registry package `{name}` version `{version}` selected from `{}` has [package] `{}` version `{}`",
            source_root.display(),
            manifest.package.name,
            manifest.package.version
        )));
    }
    if let Some(expected_hash) = expected_hash
        && content.hash != expected_hash
    {
        return Err(PackageError::new(format!(
            "registry package `{name}` version `{version}` content hash is stale (expected {expected_hash}, got {})",
            content.hash
        )));
    }

    let package_root = std::fs::canonicalize(package_root).map_err(|error| {
        PackageError::new(format!(
            "cannot resolve package root `{}`: {error}",
            package_root.display()
        ))
    })?;
    let relative_parent = Path::new("vendor").join(name);
    let parent = ensure_plain_package_directory(
        &package_root,
        &relative_parent,
        "registry vendor directory",
    )?;
    let destination = parent.join(version);
    let temporary = parent.join(format!(".{version}.topaz-vendor-{}", std::process::id()));
    if std::fs::symlink_metadata(&temporary).is_ok() {
        return Err(PackageError::new(format!(
            "stale registry vendor staging directory `{}`",
            temporary.display()
        )));
    }
    std::fs::create_dir(&temporary).map_err(|error| {
        PackageError::new(format!(
            "cannot create registry vendor staging directory `{}`: {error}",
            temporary.display()
        ))
    })?;
    if let Err(error) = write_package_content_snapshot(&temporary, &content.files) {
        let _ = std::fs::remove_dir_all(&temporary);
        return Err(error);
    }

    let destination_metadata = match std::fs::symlink_metadata(&destination) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&temporary);
            return Err(PackageError::new(error.to_string()));
        }
    };
    if destination_metadata
        .as_ref()
        .is_some_and(|metadata| !metadata.is_dir() || metadata.file_type().is_symlink())
    {
        let _ = std::fs::remove_dir_all(&temporary);
        return Err(PackageError::new(format!(
            "refusing to replace non-directory registry vendor path `{}`",
            destination.display()
        )));
    }
    let backup = parent.join(format!(
        ".{version}.topaz-vendor-backup-{}",
        std::process::id()
    ));
    if std::fs::symlink_metadata(&backup).is_ok() {
        let _ = std::fs::remove_dir_all(&temporary);
        return Err(PackageError::new(format!(
            "stale registry vendor backup directory `{}`",
            backup.display()
        )));
    }
    if destination_metadata.is_some() {
        std::fs::rename(&destination, &backup).map_err(|error| {
            let _ = std::fs::remove_dir_all(&temporary);
            PackageError::new(format!(
                "cannot preserve registry vendor package `{}` before replacement: {error}",
                destination.display()
            ))
        })?;
    }
    if let Err(error) = std::fs::rename(&temporary, &destination) {
        if destination_metadata.is_some() {
            let _ = std::fs::rename(&backup, &destination);
        }
        let _ = std::fs::remove_dir_all(&temporary);
        return Err(PackageError::new(format!(
            "cannot publish registry vendor package `{}`: {error}",
            destination.display()
        )));
    }
    Ok(RegistryVendorReplacement {
        destination,
        backup: destination_metadata.is_some().then_some(backup),
        active: true,
    })
}

fn write_package_content_snapshot(
    root: &Path,
    files: &[PackageContentFile],
) -> Result<(), PackageError> {
    for file in files {
        let destination = root.join(&file.relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                PackageError::new(format!(
                    "cannot create registry vendor staging directory `{}`: {error}",
                    parent.display()
                ))
            })?;
        }
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(|error| {
                PackageError::new(format!(
                    "cannot stage registry vendor file `{}`: {error}",
                    destination.display()
                ))
            })?;
        output.write_all(&file.bytes).map_err(|error| {
            PackageError::new(format!(
                "cannot stage registry vendor file `{}`: {error}",
                destination.display()
            ))
        })?;
    }
    Ok(())
}

pub(crate) fn verify_registry_dependency_content(
    root: impl AsRef<Path>,
    manifest: &PackageManifest,
    packages: &[LockPackage],
) -> Result<(), PackageError> {
    let root = root.as_ref();
    for (name, dep) in &manifest.dependencies {
        if name == "std" || dep.path.is_some() {
            continue;
        }
        let Some(version) = &dep.version else {
            continue;
        };
        let package = packages
            .iter()
            .find(|p| {
                p.name == *name
                    && p.version.as_deref() == Some(version.as_str())
                    && p.source.as_deref() == Some("registry")
            })
            .ok_or_else(|| {
                PackageError::new(format!(
                    "topaz.lock: missing registry package `{name}` version `{version}`"
                ))
            })?;
        let expected_hash = package.hash.as_deref().ok_or_else(|| {
            PackageError::new(format!(
                "topaz.lock: registry package `{name}` version `{version}` is missing `hash`"
            ))
        })?;
        let vendor_root = registry_vendor_root(root, name, version);
        let vendor_project = Project::load(&vendor_root)?;
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
        let actual_hash = package_content_hash(&vendor_root)?;
        if actual_hash != expected_hash {
            return Err(PackageError::new(format!(
                "vendored registry package `{name}` version `{version}` content hash is stale (expected {expected_hash}, got {actual_hash})"
            )));
        }
    }
    Ok(())
}

pub(crate) fn verify_path_dependency_content(
    root: &Path,
    manifest: &PackageManifest,
) -> Result<(), PackageError> {
    for (name, dep) in &manifest.dependencies {
        let Some(path) = &dep.path else {
            continue;
        };
        let expected_hash = dep.hash.as_deref().ok_or_else(|| {
            PackageError::new(format!(
                "[dependencies].{name} with `path` must include a content `hash`"
            ))
        })?;
        let dep_root = root.join(path);
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
    }
    Ok(())
}
