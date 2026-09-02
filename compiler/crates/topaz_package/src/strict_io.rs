use crate::content::hex_sha256;
use crate::manifest::normalize_project_path;
use crate::*;

/// Reads a canonical package-relative file without following path aliases.
pub fn read_package_file_strict(
    root: &Path,
    relative: &str,
    field: &str,
) -> Result<Vec<u8>, PackageError> {
    let normalized = normalize_project_path(field, relative)?;
    if normalized != relative {
        return Err(PackageError::new(format!(
            "{field} `{relative}` is not a canonical package path"
        )));
    }
    let root = std::fs::canonicalize(root).map_err(|error| {
        PackageError::new(format!(
            "cannot resolve package root `{}`: {error}",
            root.display()
        ))
    })?;
    let mut current = root.clone();
    for component in Path::new(relative).components() {
        let std::path::Component::Normal(segment) = component else {
            return Err(PackageError::new(format!(
                "{field} `{relative}` is not a contained package path"
            )));
        };
        current.push(segment);
        let metadata = std::fs::symlink_metadata(&current).map_err(|error| {
            PackageError::new(format!("cannot inspect {field} `{relative}`: {error}"))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(PackageError::new(format!(
                "{field} `{relative}` must not contain a symlink"
            )));
        }
    }
    let resolved = std::fs::canonicalize(&current).map_err(|error| {
        PackageError::new(format!("cannot resolve {field} `{relative}`: {error}"))
    })?;
    if !resolved.starts_with(&root) {
        return Err(PackageError::new(format!(
            "{field} `{relative}` escapes the package root"
        )));
    }
    let metadata = std::fs::metadata(&resolved).map_err(|error| {
        PackageError::new(format!("cannot inspect {field} `{relative}`: {error}"))
    })?;
    if !metadata.is_file() {
        return Err(PackageError::new(format!(
            "{field} `{relative}` must be a regular file"
        )));
    }
    std::fs::read(&resolved)
        .map_err(|error| PackageError::new(format!("cannot read {field} `{relative}`: {error}")))
}

/// Reads a strict package file and admits its bytes as UTF-8 text.
pub fn read_package_text_strict(
    root: &Path,
    relative: &str,
    field: &str,
) -> Result<String, PackageError> {
    let bytes = read_package_file_strict(root, relative, field)?;
    String::from_utf8(bytes).map_err(|error| {
        PackageError::new(format!(
            "{field} `{relative}` must contain valid UTF-8: {error}"
        ))
    })
}

/// Atomically replaces a canonical package-relative file without traversing aliases.
pub fn replace_package_file_strict(
    root: &Path,
    relative: &str,
    bytes: &[u8],
) -> Result<(), PackageError> {
    let normalized = normalize_project_path("generated package file", relative)?;
    if normalized != relative {
        return Err(PackageError::new(format!(
            "generated package file `{relative}` is not a canonical package path"
        )));
    }
    let root = std::fs::canonicalize(root).map_err(|error| {
        PackageError::new(format!(
            "cannot resolve package root `{}`: {error}",
            root.display()
        ))
    })?;
    let destination = root.join(relative);
    let parent = destination
        .parent()
        .ok_or_else(|| PackageError::new("generated package file has no parent"))?;
    let relative_parent = parent
        .strip_prefix(&root)
        .map_err(|_| PackageError::new("generated package path escapes package root"))?;
    ensure_plain_package_directory(&root, relative_parent, "generated package directory")?;

    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("generated");
    let temporary = parent.join(format!(".{file_name}.topaz-package-{}", std::process::id()));
    if std::fs::symlink_metadata(&temporary).is_ok() {
        return Err(PackageError::new(format!(
            "stale package staging file `{}`",
            temporary.display()
        )));
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| {
            PackageError::new(format!("cannot stage `{}`: {error}", destination.display()))
        })?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(&temporary);
        return Err(PackageError::new(format!(
            "cannot stage `{}`: {error}",
            destination.display()
        )));
    }
    drop(file);

    let destination_metadata = match std::fs::symlink_metadata(&destination) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            return Err(PackageError::new(error.to_string()));
        }
    };
    if destination_metadata
        .as_ref()
        .is_some_and(|metadata| !metadata.is_file() || metadata.file_type().is_symlink())
    {
        let _ = std::fs::remove_file(&temporary);
        return Err(PackageError::new(format!(
            "refusing to replace non-regular package path `{}`",
            destination.display()
        )));
    }
    let backup = parent.join(format!(
        ".{file_name}.topaz-package-backup-{}",
        std::process::id()
    ));
    if std::fs::symlink_metadata(&backup).is_ok() {
        let _ = std::fs::remove_file(&temporary);
        return Err(PackageError::new(format!(
            "stale package backup file `{}`",
            backup.display()
        )));
    }
    if destination_metadata.is_some() {
        std::fs::rename(&destination, &backup).map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            PackageError::new(format!(
                "cannot preserve `{}` before replacement: {error}",
                destination.display()
            ))
        })?;
    }
    if let Err(error) = std::fs::rename(&temporary, &destination) {
        if destination_metadata.is_some() {
            let _ = std::fs::rename(&backup, &destination);
        }
        let _ = std::fs::remove_file(&temporary);
        return Err(PackageError::new(format!(
            "cannot publish `{}`: {error}",
            destination.display()
        )));
    }
    if destination_metadata.is_some() {
        std::fs::remove_file(&backup).map_err(|error| {
            PackageError::new(format!(
                "published `{}` but cannot remove backup `{}`: {error}",
                destination.display(),
                backup.display()
            ))
        })?;
    }
    Ok(())
}

pub(crate) fn ensure_plain_package_directory(
    root: &Path,
    relative: &Path,
    field: &str,
) -> Result<PathBuf, PackageError> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(segment) = component else {
            return Err(PackageError::new(format!(
                "{field} is not a contained package path"
            )));
        };
        current.push(segment);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(PackageError::new(format!(
                    "{field} `{}` is not a plain directory",
                    current.display()
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current).map_err(|error| {
                    PackageError::new(format!("cannot create `{}`: {error}", current.display()))
                })?;
            }
            Err(error) => return Err(PackageError::new(error.to_string())),
        }
    }
    Ok(current)
}

/// Loads the replay fixture declared for an extern module through strict package I/O.
pub fn read_extern_replay_fixture(
    root: &Path,
    name: &str,
    module: &ExternModule,
) -> Result<Vec<u8>, PackageError> {
    let fixture = &module.replay.fixture;
    let field = format!("extern module `{name}` replay fixture");
    read_package_file_strict(root, fixture, &field).map_err(|error| {
        PackageError::new(format!(
            "cannot read extern module `{name}` replay fixture `{fixture}`: {error}"
        ))
    })
}

pub(crate) fn extern_replay_hash(
    root: &Path,
    name: &str,
    module: &ExternModule,
) -> Result<String, PackageError> {
    Ok(hex_sha256(&read_extern_replay_fixture(root, name, module)?))
}

pub(crate) fn extern_artifact_hash(
    root: &Path,
    name: &str,
    module: &ExternModule,
) -> Result<String, PackageError> {
    let artifact = module.artifact.as_ref().ok_or_else(|| {
        PackageError::new(format!(
            "extern module `{name}` has no artifact path to verify"
        ))
    })?;
    let field = format!("extern module `{name}` artifact");
    let bytes = read_package_file_strict(root, &artifact.path, &field).map_err(|error| {
        PackageError::new(format!(
            "cannot read extern module `{name}` artifact `{}`: {error}",
            artifact.path
        ))
    })?;
    Ok(hex_sha256(&bytes))
}
