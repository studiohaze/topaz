//! Mechanical filesystem operations used by Topaz-authored command policy.
//!
//! Rooted inspection reports canonical path containment. Its alias is a path
//! relative to the canonical root, so symlink aliases share an identity and
//! recorded requests remain relocatable. Distinct hard-link paths remain
//! distinct aliases. This is not an inode identity or an atomic filesystem
//! sandbox against concurrent directory replacement.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use topaz_rt::{CallFuture, RtCx, TpzCall, Value};

use super::{repository_root_text, result_value};

#[derive(Debug)]
pub struct Native;

impl TpzCall for Native {
    fn as_any(&self) -> &dyn std::any::Any { self }
    fn name(&self) -> Option<&str> { Some("native") }
    fn arity(&self) -> usize { 2 }
    fn param_name(&self, index: usize) -> Option<&str> {
        ["operation", "arguments"].get(index).copied()
    }
    fn call(&self, _context: RtCx, arguments: Vec<Value>) -> CallFuture {
        let result = native_arguments(&arguments).map(Value::str);
        Box::pin(async move { Ok(result_value(result)) })
    }
}

fn native_arguments(arguments: &[Value]) -> Result<String, String> {
    let [Value::Str(operation), Value::Array(arguments)] = arguments else {
        return Err("native expects an operation and an array of string arguments".into());
    };
    let arguments = arguments.borrow();
    let arguments = arguments.iter().map(|value| match value {
        Value::Str(value) => Ok(value.to_string()),
        _ => Err("every native operation argument must be a string".to_string()),
    }).collect::<Result<Vec<_>, _>>()?;
    operate(operation, &arguments)
}

fn operate(operation: &str, arguments: &[String]) -> Result<String, String> {
    match (operation, arguments) {
        ("inspect-rooted", [root, relative]) => inspect_rooted(Path::new(root), relative)?.json(),
        ("mkdir", [path]) => make_directory(Path::new(path)),
        ("mkdir-new", [path]) => make_new_directory(Path::new(path)),
        ("rmdir-empty", [path]) => remove_empty_directory(Path::new(path)),
        ("mkdir-parent", [path]) => make_parent(path),
        ("copy-file", [source, destination]) => copy_file(Path::new(source), Path::new(destination)),
        ("file-metadata", [path]) => file_metadata(Path::new(path)),
        ("set-file-mode", [path, mode]) => set_file_mode(Path::new(path), mode),
        ("rename-directory-new", [source, destination]) => rename_directory_new(Path::new(source), Path::new(destination)),
        #[cfg(feature = "native-cli")]
        ("generated-cli-sha256", []) => Ok(env!("TOPAZ_CORE_KIT_GENERATED_SHA256").into()),
        #[cfg(not(feature = "native-cli"))]
        ("generated-cli-sha256", []) => Err("no generated CLI was linked into this build".into()),
        ("build-target", []) => topaz_rt::json_stringify(&Value::record(vec![
            ("target".into(), Value::str(env!("TOPAZ_CORE_KIT_TARGET"))),
            ("executableSuffix".into(), Value::str(std::env::consts::EXE_SUFFIX)),
        ]), false),
        ("inspect-rooted", _) => Err("inspect-rooted requires a root and a relative path".into()),
        ("mkdir", _) => Err("mkdir requires one directory path".into()),
        ("mkdir-new", _) => Err("mkdir-new requires one directory path".into()),
        ("rmdir-empty", _) => Err("rmdir-empty requires one directory path".into()),
        ("mkdir-parent", _) => Err("mkdir-parent requires one output file path".into()),
        ("copy-file", _) => Err("copy-file requires source and destination paths".into()),
        ("file-metadata", _) => Err("file-metadata requires one file path".into()),
        ("set-file-mode", _) => Err("set-file-mode requires one file path and a mode".into()),
        ("rename-directory-new", _) => Err("rename-directory-new requires source and destination directory paths".into()),
        ("build-target", _) => Err("build-target accepts no arguments".into()),
        ("generated-cli-sha256", _) => Err("generated-cli-sha256 accepts no arguments".into()),
        _ => Err(format!("unknown native operation `{operation}`")),
    }
}

#[derive(Debug)]
struct Inspection {
    status: &'static str,
    path: String,
    kind: &'static str,
    alias: String,
    reason: String,
}

impl Inspection {
    fn unavailable(status: &'static str, reason: impl Into<String>) -> Self {
        Self { status, path: String::new(), kind: "other", alias: String::new(), reason: reason.into() }
    }

    fn json(self) -> Result<String, String> {
        topaz_rt::json_stringify(&Value::record(vec![
            ("status".into(), Value::str(self.status)),
            ("path".into(), Value::str(self.path)),
            ("kind".into(), Value::str(self.kind)),
            ("alias".into(), Value::str(self.alias)),
            ("reason".into(), Value::str(self.reason)),
        ]), false)
    }
}

fn logical_relative(path: &str) -> Result<PathBuf, String> {
    if path.is_empty() || path == "." { return Ok(PathBuf::new()); }
    if path.contains('\\') || path.contains(':') || path.bytes().any(|byte| byte < 32 || byte == 127) {
        return Err("inspect-rooted requires a relative slash-delimited logical path".into());
    }
    let mut relative = PathBuf::new();
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err("inspect-rooted rejects absolute, empty and traversal path segments".into());
        }
        relative.push(segment);
    }
    Ok(relative)
}

fn inspect_rooted(root: &Path, logical_path: &str) -> Result<Inspection, String> {
    let relative = logical_relative(logical_path)?;
    let root = match root.canonicalize() {
        Ok(path) => path,
        Err(error) => return Ok(Inspection::unavailable("unreadable", format!("cannot resolve root: {error}"))),
    };
    match fs::metadata(&root) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Ok(Inspection::unavailable("unreadable", "inspection root is not a directory")),
        Err(error) => return Ok(Inspection::unavailable("unreadable", format!("cannot inspect root: {error}"))),
    }
    let mut existing = root.join(relative);
    let mut missing = Vec::new();
    loop {
        match fs::symlink_metadata(&existing) {
            Ok(_) => {
                // Resolve the nearest existing ancestor before appending any
                // missing suffix. A missing child below an escaping link must
                // never be reported as an inside missing file.
                let canonical = match existing.canonicalize() {
                    Ok(path) => path,
                    Err(error) => return Ok(Inspection::unavailable("unreadable", format!("cannot resolve path: {error}"))),
                };
                if !canonical.starts_with(&root) {
                    return Ok(Inspection::unavailable("outside", "path resolves outside the selected root"));
                }
                let metadata = match fs::metadata(&canonical) {
                    Ok(metadata) => metadata,
                    Err(error) => return Ok(Inspection::unavailable("unreadable", format!("cannot inspect path: {error}"))),
                };
                if !missing.is_empty() && !metadata.is_dir() {
                    return Ok(Inspection::unavailable("unreadable", "a path ancestor is not a directory"));
                }
                let present = missing.is_empty();
                let mut resolved = canonical;
                for segment in missing.iter().rev() { resolved.push(segment); }
                let path = match repository_root_text(&resolved) {
                    Ok(path) => path,
                    Err(reason) => return Ok(Inspection::unavailable("unreadable", reason)),
                };
                let alias = if present {
                    let relative = resolved.strip_prefix(&root)
                        .map_err(|_| "resolved path lost its root containment".to_string())?;
                    match repository_root_text(relative) {
                        Ok(path) => path,
                        Err(reason) => return Ok(Inspection::unavailable("unreadable", reason)),
                    }
                } else {
                    String::new()
                };
                let kind = if !present { "other" }
                    else if metadata.is_file() { "file" }
                    else if metadata.is_dir() { "directory" }
                    else { "other" };
                return Ok(Inspection {
                    status: if present { "present" } else { "missing" },
                    path, kind, alias, reason: String::new(),
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if existing == root {
                    return Ok(Inspection::unavailable("unreadable", "inspection root disappeared"));
                }
                let name = existing.file_name()
                    .ok_or_else(|| "cannot inspect a path without a final component".to_string())?
                    .to_os_string();
                missing.push(name);
                if !existing.pop() || !existing.starts_with(&root) {
                    return Ok(Inspection::unavailable("outside", "path ancestor is outside the selected root"));
                }
            }
            Err(error) => return Ok(Inspection::unavailable("unreadable", format!("cannot inspect path: {error}"))),
        }
    }
}

fn make_directory(path: &Path) -> Result<String, String> {
    if path.as_os_str().is_empty() { return Err("mkdir requires a nonempty path".into()); }
    fs::create_dir_all(path).map_err(|error| format!("cannot create directory: {error}"))?;
    let canonical = path.canonicalize().map_err(|error| format!("cannot resolve directory: {error}"))?;
    repository_root_text(&canonical)
}

fn make_new_directory(path: &Path) -> Result<String, String> {
    if path.as_os_str().is_empty() { return Err("mkdir-new requires a nonempty path".into()); }
    // The exclusive OS operation decides ownership. An existence check followed
    // by recursive creation would let two callers claim the same directory.
    fs::create_dir(path).map_err(|error| format!("cannot create new directory: {error}"))?;
    let canonical = path.canonicalize().map_err(|error| format!("cannot resolve new directory: {error}"))?;
    repository_root_text(&canonical)
}

fn remove_empty_directory(path: &Path) -> Result<String, String> {
    if path.as_os_str().is_empty() { return Err("rmdir-empty requires a nonempty path".into()); }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect empty directory: {error}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("rmdir-empty requires a directory without a symlink".into());
    }
    // Never remove contents or infer lock ownership. The Topaz caller selects
    // the directory and decides whether it owns the entry it is releasing.
    fs::remove_dir(path).map_err(|error| format!("cannot remove empty directory: {error}"))?;
    Ok("removed".into())
}

fn rename_directory_new(source: &Path, destination: &Path) -> Result<String, String> {
    if source.as_os_str().is_empty() || destination.as_os_str().is_empty() {
        return Err("rename-directory-new requires nonempty paths".into());
    }
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("cannot inspect rename source: {error}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("rename source must be a real directory".into());
    }
    let source = source.canonicalize()
        .map_err(|error| format!("cannot resolve rename source: {error}"))?;
    let name = destination.file_name().ok_or("rename destination requires a final directory name")?;
    let parent = destination.parent().filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = parent.canonicalize()
        .map_err(|error| format!("cannot resolve rename destination parent: {error}"))?;
    if source.parent() != Some(parent.as_path()) {
        return Err("rename-directory-new requires sibling directories on the same filesystem".into());
    }
    let destination = parent.join(name);
    match fs::symlink_metadata(&destination) {
        Ok(_) => return Err("rename destination already exists".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot inspect rename destination: {error}")),
    }
    let text = repository_root_text(&destination)?;
    // The Topaz caller holds the target's exclusive installation lock. Moving
    // a completed sibling is one filesystem rename, not a copy into an active
    // installation. The existence check is not a defence against an unrelated
    // process replacing directory entries outside that cooperative lock.
    fs::rename(&source, &destination)
        .map_err(|error| format!("cannot rename completed directory: {error}"))?;
    Ok(text)
}

fn make_parent(text: &str) -> Result<String, String> {
    let path = Path::new(text);
    let name = text.rsplit(std::path::is_separator).next().unwrap_or("");
    if name.is_empty() || name == "." || name == ".." || path.file_name().is_none() {
        return Err("mkdir-parent requires a path ending in a file name".into());
    }
    let parent = path.parent().filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    make_directory(parent)
}

fn regular_file_metadata(path: &Path) -> Result<(PathBuf, fs::Metadata), String> {
    if path.as_os_str().is_empty() { return Err("file metadata requires a nonempty path".into()); }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect file metadata: {error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("file metadata requires a regular file without a symlink".into());
    }
    let canonical = path.canonicalize()
        .map_err(|error| format!("cannot resolve file metadata path: {error}"))?;
    Ok((canonical, metadata))
}

fn file_metadata_json(path: &Path, metadata: &fs::Metadata) -> Result<String, String> {
    let bytes = i64::try_from(metadata.len()).map_err(|_| "file size exceeds the native integer range")?;
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        format!("{:04o}", metadata.permissions().mode() & 0o7777)
    };
    #[cfg(not(unix))]
    let mode = String::new();
    topaz_rt::json_stringify(&Value::record(vec![
        ("path".into(), Value::str(repository_root_text(path)?)),
        ("bytes".into(), Value::Int(bytes)),
        ("mode".into(), Value::str(mode)),
    ]), true)
}

fn file_metadata(path: &Path) -> Result<String, String> {
    let (canonical, metadata) = regular_file_metadata(path)?;
    file_metadata_json(&canonical, &metadata)
}

fn set_file_mode(path: &Path, mode: &str) -> Result<String, String> {
    if mode != "0644" && mode != "0755" {
        return Err("set-file-mode accepts only 0644 or 0755".into());
    }
    let (canonical, metadata) = regular_file_metadata(path)?;
    // Admit the reported path and integer size before changing permissions.
    file_metadata_json(&canonical, &metadata)?;
    let permissions = {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::Permissions::from_mode(if mode == "0644" { 0o644 } else { 0o755 })
        }
        #[cfg(windows)]
        {
            if mode == "0755" {
                return Err("set-file-mode cannot apply Unix executable mode 0755 on Windows".into());
            }
            let mut permissions = metadata.permissions();
            permissions.set_readonly(false);
            permissions
        }
        #[cfg(not(any(unix, windows)))]
        {
            return Err("set-file-mode is unsupported on this platform".into());
        }
    };
    fs::set_permissions(&canonical, permissions)
        .map_err(|error| format!("cannot set file mode: {error}"))?;
    file_metadata(&canonical)
}

static COPY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn copy_file(source: &Path, destination: &Path) -> Result<String, String> {
    if source.as_os_str().is_empty() || destination.as_os_str().is_empty() {
        return Err("copy-file requires nonempty source and destination paths".into());
    }
    let source_metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("cannot inspect copy source: {error}"))?;
    if !source_metadata.is_file() || source_metadata.file_type().is_symlink() {
        return Err("copy source must be a regular file without a symlink".into());
    }
    let canonical_source = source.canonicalize()
        .map_err(|error| format!("cannot resolve copy source: {error}"))?;
    match fs::symlink_metadata(destination) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err("copy destination must be a regular file without a symlink".into());
            }
            let canonical_destination = destination.canonicalize()
                .map_err(|error| format!("cannot resolve copy destination: {error}"))?;
            if canonical_source == canonical_destination {
                return Err("copy source and destination identify the same file".into());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if source_metadata.dev() == metadata.dev() && source_metadata.ino() == metadata.ino() {
                    return Err("copy source and destination identify the same file".into());
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot inspect copy destination: {error}")),
    }
    let parent = destination.parent().filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = parent.canonicalize().map_err(|error| format!("cannot resolve copy directory: {error}"))?;
    let name = destination.file_name().ok_or("copy destination requires a file name")?;
    let destination = parent.join(name);
    // Admit the return path before replacing a previous file. Its parent may
    // resolve through a symlink to a path that Topaz cannot represent.
    let destination_text = repository_root_text(&destination)?;

    // Copy into a private sibling before replacing the destination. This
    // preserves source bytes even when destination has a hard link to source
    // on a platform where std does not expose a stable file identity API.
    let temporary = loop {
        let sequence = COPY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".topaz-copy-{}-{sequence}", std::process::id()));
        match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => { drop(file); break path; }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("cannot prepare copy destination: {error}")),
        }
    };
    let copied = fs::copy(&canonical_source, &temporary)
        .map_err(|error| format!("cannot copy file: {error}"))
        .and_then(|_| fs::rename(&temporary, &destination)
            .map_err(|error| format!("cannot replace copy destination: {error}")));
    if let Err(error) = copied {
        return match fs::remove_file(&temporary) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(format!("{error}; cannot remove temporary copy: {cleanup}")),
        };
    }
    Ok(destination_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    static DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let sequence = DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("topaz-platform-{}-{sequence}", std::process::id()));
            fs::create_dir(&path).expect("create test directory");
            Self(path)
        }
        fn path(&self) -> &Path { &self.0 }
    }
    impl Drop for Directory {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }

    fn reported_file_metadata(text: &str) -> (String, i64, String) {
        use topaz_rt::JsonValue;
        let JsonValue::Object(fields) = topaz_rt::json_parse(text).unwrap() else {
            panic!("metadata must be a JSON object");
        };
        assert_eq!(fields.len(), 3);
        let Some(JsonValue::String(path)) = fields.get("path") else { panic!("missing path"); };
        let Some(JsonValue::Number(bytes)) = fields.get("bytes") else { panic!("missing bytes"); };
        let Some(JsonValue::String(mode)) = fields.get("mode") else { panic!("missing mode"); };
        (path.to_string(), bytes.int.expect("byte count must be an integer"), mode.to_string())
    }

    #[test]
    fn file_metadata_reports_canonical_path_bytes_and_native_mode() {
        let root = Directory::new();
        let file = root.path().join("payload file");
        fs::write(&file, [0_u8, 1, 2, 255]).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o754)).unwrap();
        }
        let text = operate("file-metadata", &[file.to_str().unwrap().into()]).unwrap();
        let (path, bytes, mode) = reported_file_metadata(&text);
        assert_eq!(path, repository_root_text(&file.canonicalize().unwrap()).unwrap());
        assert_eq!(bytes, 4);
        #[cfg(unix)]
        assert_eq!(mode, "0754");
        #[cfg(not(unix))]
        assert_eq!(mode, "");
        assert_eq!(fs::read(&file).unwrap(), [0_u8, 1, 2, 255]);
    }

    #[cfg(unix)]
    #[test]
    fn file_modes_survive_copy_and_can_be_set_nonexecutable() {
        use std::os::unix::fs::PermissionsExt;
        let root = Directory::new();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::write(&source, "native bytes").unwrap();
        let executable = operate("set-file-mode", &[source.to_str().unwrap().into(), "0755".into()]).unwrap();
        assert_eq!(reported_file_metadata(&executable).2, "0755");
        copy_file(&source, &destination).unwrap();
        assert_eq!(reported_file_metadata(&file_metadata(&destination).unwrap()).2, "0755");
        let ordinary = set_file_mode(&destination, "0644").unwrap();
        assert_eq!(reported_file_metadata(&ordinary).2, "0644");
        assert_eq!(reported_file_metadata(&file_metadata(&source).unwrap()).2, "0755");
        assert_eq!(fs::read(&source).unwrap(), b"native bytes");
        assert_eq!(fs::read(&destination).unwrap(), b"native bytes");
        // Admission must not mistake special permission bits for ordinary
        // executable mode. Normalizing an owned copy clears those bits.
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o4755)).unwrap();
        assert_eq!(reported_file_metadata(&file_metadata(&destination).unwrap()).2, "4755");
        set_file_mode(&destination, "0755").unwrap();
        assert_eq!(reported_file_metadata(&file_metadata(&destination).unwrap()).2, "0755");
    }

    #[cfg(windows)]
    #[test]
    fn windows_file_mode_clears_readonly_without_claiming_unix_permissions() {
        let root = Directory::new();
        let file = root.path().join("payload");
        fs::write(&file, "native bytes").unwrap();
        let mut permissions = fs::metadata(&file).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&file, permissions).unwrap();
        assert_eq!(reported_file_metadata(&file_metadata(&file).unwrap()).2, "");
        assert!(set_file_mode(&file, "0755").unwrap_err().contains("on Windows"));
        assert!(fs::metadata(&file).unwrap().permissions().readonly());
        let ordinary = set_file_mode(&file, "0644").unwrap();
        assert_eq!(reported_file_metadata(&ordinary).2, "");
        assert!(!fs::metadata(&file).unwrap().permissions().readonly());
        assert_eq!(fs::read(&file).unwrap(), b"native bytes");
    }

    #[test]
    fn file_metadata_and_mode_changes_refuse_invalid_entries() {
        let root = Directory::new();
        let file = root.path().join("payload");
        fs::write(&file, "keep bytes").unwrap();
        let before = file_metadata(&file).unwrap();
        for mode in ["", "644", "755", "0600", "0644 ", "0777"] {
            assert!(set_file_mode(&file, mode).is_err(), "accepted mode {mode:?}");
        }
        assert_eq!(file_metadata(&file).unwrap(), before);
        assert_eq!(fs::read(&file).unwrap(), b"keep bytes");
        let missing = root.path().join("missing");
        for invalid in [Path::new(""), root.path(), missing.as_path()] {
            assert!(file_metadata(invalid).is_err());
            assert!(set_file_mode(invalid, "0644").is_err());
        }
        assert!(!missing.exists());
    }

    #[test]
    fn directory_rename_publishes_complete_sibling_without_replacing_existing() {
        let root = Directory::new();
        let source = root.path().join("stage");
        let destination = root.path().join("installed");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("receipt"), "complete\n").unwrap();
        fs::create_dir(source.join("payload")).unwrap();
        fs::write(source.join("payload").join("program"), [0_u8, 255, 7]).unwrap();
        let moved = rename_directory_new(&source, &destination).unwrap();
        assert_eq!(moved, repository_root_text(&destination.canonicalize().unwrap()).unwrap());
        assert!(!source.exists());
        assert_eq!(fs::read(destination.join("payload").join("program")).unwrap(), [0_u8, 255, 7]);
        fs::create_dir(&source).unwrap();
        fs::write(source.join("other"), "retain stage").unwrap();
        assert!(rename_directory_new(&source, &destination).unwrap_err().contains("already exists"));
        assert_eq!(fs::read(source.join("other")).unwrap(), b"retain stage");
        assert_eq!(fs::read(destination.join("receipt")).unwrap(), b"complete\n");
        let empty = root.path().join("empty");
        fs::create_dir(&empty).unwrap();
        assert!(rename_directory_new(&source, &empty).is_err());
        assert!(empty.is_dir());
    }

    #[test]
    fn directory_rename_refuses_nondirectories_missing_and_different_parents() {
        let root = Directory::new();
        let source = root.path().join("stage");
        let other = root.path().join("other");
        let file = root.path().join("file");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&other).unwrap();
        fs::write(&file, "retain").unwrap();
        assert!(rename_directory_new(&source, &other.join("installed")).is_err());
        assert!(rename_directory_new(&file, &root.path().join("installed")).is_err());
        assert!(rename_directory_new(&root.path().join("missing"), &root.path().join("installed")).is_err());
        assert!(rename_directory_new(&source, &file).is_err());
        assert!(source.is_dir());
        assert_eq!(fs::read(&file).unwrap(), b"retain");
        assert!(!root.path().join("installed").exists());
    }

    #[cfg(unix)]
    #[test]
    fn directory_rename_refuses_link_sources_and_link_destinations() {
        use std::os::unix::fs::symlink;
        let root = Directory::new();
        let source = root.path().join("stage");
        let alias = root.path().join("alias");
        let dangling = root.path().join("dangling");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("file"), "retain").unwrap();
        symlink(&source, &alias).unwrap();
        symlink("missing", &dangling).unwrap();
        assert!(rename_directory_new(&alias, &root.path().join("installed")).is_err());
        assert!(rename_directory_new(&source, &alias).is_err());
        assert!(rename_directory_new(&source, &dangling).is_err());
        assert!(fs::symlink_metadata(&alias).unwrap().file_type().is_symlink());
        assert!(fs::symlink_metadata(&dangling).unwrap().file_type().is_symlink());
        assert_eq!(fs::read(source.join("file")).unwrap(), b"retain");
    }

    #[cfg(unix)]
    #[test]
    fn file_metadata_and_mode_changes_refuse_direct_symlinks() {
        use std::os::unix::fs::symlink;
        let root = Directory::new();
        let file = root.path().join("payload");
        fs::write(&file, "keep bytes").unwrap();
        let before = file_metadata(&file).unwrap();
        let alias = root.path().join("alias");
        symlink(&file, &alias).unwrap();
        let dangling = root.path().join("dangling");
        symlink("missing", &dangling).unwrap();
        for path in [&alias, &dangling] {
            assert!(file_metadata(path).is_err());
            assert!(set_file_mode(path, "0644").is_err());
            assert!(set_file_mode(path, "0755").is_err());
            assert!(fs::symlink_metadata(path).unwrap().file_type().is_symlink());
        }
        assert_eq!(file_metadata(&file).unwrap(), before);
        assert_eq!(fs::read(&file).unwrap(), b"keep bytes");
        assert!(!root.path().join("missing").exists());
    }

    #[test]
    fn rooted_inspection_reports_real_files_and_missing_paths() {
        let root = Directory::new();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/main.tpz"), "hello").unwrap();
        let found = inspect_rooted(root.path(), "src/main.tpz").unwrap();
        assert_eq!((found.status, found.kind, found.alias.as_str()), ("present", "file", "src/main.tpz"));
        assert_eq!(found.path, repository_root_text(&root.path().join("src/main.tpz").canonicalize().unwrap()).unwrap());
        let missing = inspect_rooted(root.path(), "src/missing/child.tpz").unwrap();
        assert_eq!((missing.status, missing.kind), ("missing", "other"));
        assert!(missing.alias.is_empty());
        assert!(missing.path.ends_with("/src/missing/child.tpz"));
        assert_eq!(inspect_rooted(root.path(), "").unwrap().kind, "directory");
        assert_eq!(inspect_rooted(root.path(), ".").unwrap().kind, "directory");
    }

    #[test]
    fn rooted_inspection_rejects_traversal_and_platform_path_syntax() {
        let root = Directory::new();
        for path in ["../outside", "src/../outside", "/etc/passwd", "src//main.tpz", "./src", "C:/source", "src\\main.tpz", "bad\0path"] {
            assert!(inspect_rooted(root.path(), path).is_err(), "accepted {path:?}");
        }
        let file = root.path().join("file");
        fs::write(&file, "file").unwrap();
        assert_eq!(inspect_rooted(&file, "child").unwrap().status, "unreadable");
        assert_eq!(inspect_rooted(&root.path().join("absent"), "child").unwrap().status, "unreadable");
    }

    #[cfg(unix)]
    #[test]
    fn rooted_inspection_follows_inside_links_and_detects_missing_escape() {
        use std::os::unix::fs::symlink;
        let parent = Directory::new();
        let root = parent.path().join("project");
        let outside = parent.path().join("project-neighbor");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(root.join("main.tpz"), "inside").unwrap();
        symlink("main.tpz", root.join("alias.tpz")).unwrap();
        symlink(&outside, root.join("escape")).unwrap();
        symlink("absent", root.join("dangling")).unwrap();
        let alias = inspect_rooted(&root, "alias.tpz").unwrap();
        assert_eq!((alias.status, alias.alias.as_str()), ("present", "main.tpz"));
        assert_eq!(inspect_rooted(&root, "escape").unwrap().status, "outside");
        let missing = inspect_rooted(&root, "escape/missing/child.tpz").unwrap();
        assert_eq!(missing.status, "outside");
        assert!(missing.path.is_empty());
        assert_eq!(inspect_rooted(&root, "dangling").unwrap().status, "unreadable");
    }

    #[cfg(unix)]
    #[test]
    fn rooted_aliases_are_canonical_paths_not_hard_link_identities() {
        let root = Directory::new();
        fs::write(root.path().join("original.tpz"), "source").unwrap();
        fs::hard_link(root.path().join("original.tpz"), root.path().join("other.tpz")).unwrap();
        let original = inspect_rooted(root.path(), "original.tpz").unwrap();
        let other = inspect_rooted(root.path(), "other.tpz").unwrap();
        assert_eq!(original.alias, "original.tpz");
        assert_eq!(other.alias, "other.tpz");
    }

    #[test]
    fn copying_replaces_regular_files_and_preserves_source() {
        let root = Directory::new();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::write(&source, "new bytes").unwrap();
        fs::write(&destination, "old bytes").unwrap();
        copy_file(&source, &destination).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"new bytes");
        assert_eq!(fs::read(&source).unwrap(), b"new bytes");
        assert!(copy_file(&source, &source).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"new bytes");
        assert!(copy_file(&source, root.path()).is_err());
        assert!(copy_file(root.path(), &destination).is_err());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn copy_preserves_destination_when_resolved_parent_is_not_utf8() {
        use std::ffi::OsStr;
        use std::os::unix::{ffi::OsStrExt, fs::symlink};
        let root = Directory::new();
        let directory = root.path().join(OsStr::from_bytes(b"parent-\xff"));
        let alias = root.path().join("alias");
        let source = root.path().join("source");
        fs::create_dir(&directory).unwrap();
        symlink(directory, &alias).unwrap();
        let destination = alias.join("destination");
        fs::write(&source, b"new").unwrap();
        fs::write(&destination, b"original").unwrap();
        assert!(copy_file(&source, &destination).unwrap_err().contains("UTF-8"));
        assert_eq!(fs::read(&destination).unwrap(), b"original");
        assert_eq!(fs::read_dir(alias).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn copying_rejects_links_and_preserves_executable_permissions() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = Directory::new();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::write(&source, "executable").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o751)).unwrap();
        copy_file(&source, &destination).unwrap();
        assert_eq!(fs::metadata(&destination).unwrap().permissions().mode() & 0o777, 0o751);
        fs::remove_file(&destination).unwrap();
        symlink(&source, &destination).unwrap();
        assert!(copy_file(&source, &destination).is_err());
        assert!(copy_file(&destination, &root.path().join("other")).is_err());
        fs::remove_file(&destination).unwrap();
        fs::hard_link(&source, &destination).unwrap();
        assert!(copy_file(&source, &destination).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"executable");
    }

    #[test]
    fn exclusive_directory_creation_preserves_existing_entries() {
        let root = Directory::new();
        let directory = root.path().join("new-directory");
        let created = operate("mkdir-new", &[directory.to_str().unwrap().into()]).unwrap();
        assert_eq!(created, repository_root_text(&directory.canonicalize().unwrap()).unwrap());
        assert!(make_new_directory(&directory).is_err());

        let retained = directory.join("retained");
        fs::write(&retained, "keep directory contents").unwrap();
        assert!(make_new_directory(&directory).is_err());
        assert_eq!(fs::read(&retained).unwrap(), b"keep directory contents");

        let file = root.path().join("existing-file");
        fs::write(&file, "keep existing file").unwrap();
        assert!(make_new_directory(&file).is_err());
        assert_eq!(fs::read(&file).unwrap(), b"keep existing file");

        assert!(make_new_directory(&root.path().join("absent/child")).is_err());
        assert!(!root.path().join("absent").exists());
        assert!(make_new_directory(Path::new("")).is_err());
    }

    #[test]
    fn empty_directory_removal_preserves_files_and_nonempty_directories() {
        let root = Directory::new();
        let empty = root.path().join("empty");
        fs::create_dir(&empty).unwrap();
        assert_eq!(operate("rmdir-empty", &[empty.to_str().unwrap().into()]).unwrap(), "removed");
        assert!(!empty.exists());
        assert!(remove_empty_directory(&empty).is_err());

        let nonempty = root.path().join("nonempty");
        fs::create_dir(&nonempty).unwrap();
        let retained = nonempty.join("retained");
        fs::write(&retained, "keep contents").unwrap();
        assert!(remove_empty_directory(&nonempty).is_err());
        assert!(nonempty.is_dir());
        assert_eq!(fs::read(&retained).unwrap(), b"keep contents");
        assert!(remove_empty_directory(&retained).is_err());
        assert_eq!(fs::read(&retained).unwrap(), b"keep contents");
        assert!(remove_empty_directory(Path::new("")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn exclusive_directory_operations_refuse_symlinks() {
        use std::os::unix::fs::symlink;
        let root = Directory::new();
        let directory = root.path().join("target");
        fs::create_dir(&directory).unwrap();
        let alias = root.path().join("alias");
        symlink(&directory, &alias).unwrap();
        assert!(make_new_directory(&alias).is_err());
        assert!(remove_empty_directory(&alias).is_err());
        assert!(fs::symlink_metadata(&alias).unwrap().file_type().is_symlink());
        assert!(directory.is_dir());

        let dangling = root.path().join("dangling");
        symlink("absent", &dangling).unwrap();
        assert!(make_new_directory(&dangling).is_err());
        assert!(remove_empty_directory(&dangling).is_err());
        assert!(fs::symlink_metadata(&dangling).unwrap().file_type().is_symlink());
        assert!(!root.path().join("absent").exists());
    }

    #[test]
    fn directory_creation_and_operation_arity_are_explicit() {
        let root = Directory::new();
        let nested = root.path().join("new/nested");
        let path = make_directory(&nested).unwrap();
        assert_eq!(path, repository_root_text(&nested.canonicalize().unwrap()).unwrap());
        assert!(make_directory(Path::new("")).is_err());
        let output = root.path().join("parent/child/output.rs");
        let parent = make_parent(output.to_str().unwrap()).unwrap();
        assert_eq!(parent, repository_root_text(&output.parent().unwrap().canonicalize().unwrap()).unwrap());
        assert!(!output.exists());
        for invalid in ["", "/", "file/", "file/.", "file/.."] { assert!(make_parent(invalid).is_err()); }
        #[cfg(windows)]
        assert!(make_parent("file\\").is_err());
        assert!(operate("build-target", &["extra".into()]).is_err());
        assert!(operate("copy-file", &[]).is_err());
        for operation in ["mkdir-new", "rmdir-empty", "file-metadata"] {
            assert!(operate(operation, &[]).is_err());
            assert!(operate(operation, &["one".into(), "two".into()]).is_err());
        }
        assert!(operate("set-file-mode", &[]).is_err());
        assert!(operate("set-file-mode", &["one".into()]).is_err());
        assert!(operate("set-file-mode", &["one".into(), "0644".into(), "extra".into()]).is_err());
        assert!(operate("rename-directory-new", &[]).is_err());
        assert!(operate("rename-directory-new", &["one".into()]).is_err());
        assert!(operate("rename-directory-new", &["one".into(), "two".into(), "extra".into()]).is_err());
        assert!(operate("unknown", &[]).is_err());
    }
}
