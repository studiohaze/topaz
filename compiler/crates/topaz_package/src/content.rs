use crate::*;

/// Computes the portable identity of manifest and Topaz source bytes under a package root.
pub fn package_content_hash(root: impl AsRef<Path>) -> Result<String, PackageError> {
    Ok(read_package_content(root.as_ref())?.hash)
}

pub(crate) struct PackageContentFile {
    pub(crate) relative: String,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) struct PackageContent {
    pub(crate) files: Vec<PackageContentFile>,
    pub(crate) hash: String,
}

pub(crate) fn read_package_content(root: &Path) -> Result<PackageContent, PackageError> {
    let root = std::fs::canonicalize(root).map_err(|error| {
        PackageError::new(format!(
            "cannot resolve package root `{}`: {error}",
            root.display()
        ))
    })?;
    let mut files = Vec::new();
    collect_package_files(&root, Path::new(""), &mut files)?;
    if !files.iter().any(|file| file.relative == "topaz.toml") {
        return Err(PackageError::new(format!(
            "package `{}` has no topaz.toml",
            root.to_string_lossy()
        )));
    }
    files.sort_by(|left, right| left.relative.cmp(&right.relative));
    let mut hasher = Sha256::new();
    hasher.update(b"topaz-package-content-v1\n");
    for file in &files {
        hasher.update(file.relative.as_bytes());
        hasher.update([0]);
        hasher.update(file.bytes.len().to_string().as_bytes());
        hasher.update(b"\n");
        hasher.update(&file.bytes);
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    bytes_to_hex_into(&mut hex, &digest);
    Ok(PackageContent {
        files,
        hash: format!("sha256:{hex}"),
    })
}

/// Returns the portable identity of a file in the package content source set.
/// Included source paths must have an exact Unicode representation.
pub fn package_content_relative_path(relative: &Path) -> Result<Option<String>, PackageError> {
    if relative != Path::new("topaz.toml")
        && !relative.as_os_str().as_encoded_bytes().ends_with(b".tpz")
    {
        return Ok(None);
    }
    let exact = relative.to_str().ok_or_else(|| {
        PackageError::new(format!(
            "package content path `{}` cannot be represented as Unicode",
            relative.display()
        ))
    })?;
    Ok(Some(exact.replace('\\', "/")))
}

fn collect_package_files(
    root: &Path,
    rel: &Path,
    out: &mut Vec<PackageContentFile>,
) -> Result<(), PackageError> {
    let dir = root.join(rel);
    let entries = std::fs::read_dir(&dir).map_err(|e| {
        PackageError::new(format!(
            "cannot read package dir `{}`: {e}",
            dir.to_string_lossy()
        ))
    })?;
    let mut entries = entries.collect::<Result<Vec<_>, _>>().map_err(|e| {
        PackageError::new(format!(
            "cannot read package dir `{}`: {e}",
            dir.to_string_lossy()
        ))
    })?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_type = entry.file_type().map_err(|e| {
            PackageError::new(format!(
                "cannot inspect `{}`: {e}",
                entry.path().to_string_lossy()
            ))
        })?;
        if file_type.is_symlink() {
            return Err(PackageError::new(format!(
                "package content hashing rejects symlink `{}`",
                entry.path().to_string_lossy()
            )));
        }
        let child_rel = rel.join(entry.file_name());
        if file_type.is_dir() {
            if ignored_package_dir(&entry.file_name()) {
                continue;
            }
            collect_package_files(root, &child_rel, out)?;
            continue;
        }
        let relative = package_content_relative_path(&child_rel)?;
        if !file_type.is_file() {
            if relative.is_some() {
                return Err(PackageError::new(format!(
                    "package content source `{}` must be a regular file",
                    entry.path().display()
                )));
            }
            continue;
        }
        if let Some(relative) = relative {
            let path = entry.path();
            let bytes = std::fs::read(&path).map_err(|error| {
                PackageError::new(format!("cannot read `{}`: {error}", path.display()))
            })?;
            out.push(PackageContentFile { relative, bytes });
        }
    }
    Ok(())
}

fn ignored_package_dir(name: &std::ffi::OsStr) -> bool {
    matches!(
        name.to_str(),
        Some(".git" | ".topaz" | "target" | "node_modules")
    )
}

pub(crate) fn hex_sha256(data: &[u8]) -> String {
    let digest = sha256(data);
    let mut hex = String::with_capacity(64);
    bytes_to_hex_into(&mut hex, &digest);
    format!("sha256:{hex}")
}
