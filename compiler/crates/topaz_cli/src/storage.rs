use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use topaz_value::{JsonValue, json_parse};

const MARKER: &str = ".topaz-workspace.json";
const SCHEMA: &str = "topaz.storage.workspace.v1";
const MAX_ACTIVE_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// CLI-owned isolated Cargo workspace and its source, home, and target paths.
pub struct Workspace {
    root: PathBuf,
    pub source: PathBuf,
    pub cwd: PathBuf,
    pub cargo_home: PathBuf,
    pub target: PathBuf,
}

impl Workspace {
    /// Allocates a unique owned workspace after reclaiming inactive predecessors.
    pub fn create() -> Result<Self, String> {
        let base = builds_root();
        if base.to_str().is_none() {
            return Err(
                "Topaz build storage path cannot be represented as Unicode; Cargo requires Unicode package paths"
                    .to_string(),
            );
        }
        fs::create_dir_all(&base)
            .map_err(|e| format!("cannot create Topaz storage `{}`: {e}", base.display()))?;
        reclaim_inactive(&base, false)?;
        for attempt in 0..32_u64 {
            let stamp = now_nanos();
            let root = base.join(format!("build-{}-{stamp}-{attempt}", std::process::id()));
            match create_private_dir(&root) {
                Ok(()) => {
                    let marker = format!(
                        "{{\n  \"schema\": \"{SCHEMA}\",\n  \"pid\": {},\n  \"createdEpochSeconds\": {}\n}}\n",
                        std::process::id(),
                        now_seconds()
                    );
                    if let Err(e) = write_new(&root.join(MARKER), marker.as_bytes()) {
                        let _ = fs::remove_dir_all(&root);
                        return Err(format!("cannot mark Topaz workspace: {e}"));
                    }
                    // macOS exposes the temporary directory through `/var` while
                    // rustc canonicalizes it to `/private/var`. Windows returns a
                    // verbatim `\\?\` path that Rust accepts but some downstream
                    // native tools do not. Keep one canonical,
                    // command-safe spelling throughout the workspace.
                    let root = fs::canonicalize(&root).map(command_path).unwrap_or(root);
                    let source = root.join("source");
                    let cwd = root.join("cwd");
                    let cargo_home = root.join("cargo-home");
                    let target = root.join("target");
                    for dir in [&source, &cwd, &cargo_home, &target] {
                        if let Err(e) = create_private_dir(dir) {
                            let _ = fs::remove_dir_all(&root);
                            return Err(format!(
                                "cannot create Topaz workspace `{}`: {e}",
                                dir.display()
                            ));
                        }
                    }
                    return Ok(Self {
                        root,
                        source,
                        cwd,
                        cargo_home,
                        target,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("cannot create Topaz workspace: {e}")),
            }
        }
        Err("could not allocate a unique Topaz workspace".into())
    }

    /// Removes the files rooted at this CLI-owned workspace.
    pub fn cleanup(&self) {
        let _ = fs::remove_dir_all(&self.root);
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// `std::fs::canonicalize` returns verbatim paths (`\\?\C:\...`) on Windows.
/// Rust accepts those, but MinGW `ld` does not understand response-file paths
/// like `@\\?\C:\...\linker-arguments`. Keep the canonical resolution, then
/// pass ordinary drive/UNC syntax to every downstream native tool.
pub(super) fn command_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.as_os_str().to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Prints owned workspace age and byte usage without mutating storage.
pub fn status() -> Result<(), String> {
    let base = builds_root();
    let entries = owned_entries(&base)?;
    let mut total = 0_u64;
    if entries.is_empty() {
        println!("Topaz storage: 0 owned workspaces (0 B)");
        println!("Root: {}", base.display());
        return Ok(());
    }
    println!("Topaz-owned build workspaces:");
    for entry in entries {
        let size = directory_size(&entry.path)?;
        total = total.saturating_add(size);
        let state = if marker_active(&entry.marker) {
            "active"
        } else {
            "safe to clean: owner marker valid and process inactive"
        };
        println!(
            "{}\t{}\tcreated={}\t{}",
            human_bytes(size),
            entry.path.display(),
            entry.marker.created,
            state
        );
    }
    println!("Total: {}", human_bytes(total));
    Ok(())
}

/// Reclaims inactive CLI-owned workspaces and reports recovered bytes.
pub fn clean() -> Result<(), String> {
    let base = builds_root();
    let (removed, bytes) = reclaim_inactive(&base, true)?;
    println!(
        "Topaz storage clean: removed {removed} inactive workspace(s), reclaimed {}",
        human_bytes(bytes)
    );
    Ok(())
}

fn builds_root() -> PathBuf {
    if let Some(path) = std::env::var_os("TOPAZ_STORAGE_DIR") {
        return PathBuf::from(path).join("builds");
    }
    if let Some(path) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(path).join("topaz").join("builds");
    }
    #[cfg(target_os = "windows")]
    if let Some(path) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(path).join("Topaz").join("builds");
    }
    std::env::temp_dir().join("topaz-storage").join("builds")
}

#[derive(Clone)]
struct Marker {
    pid: u32,
    created: u64,
}

struct OwnedEntry {
    path: PathBuf,
    marker: Marker,
}

fn owned_entries(base: &Path) -> Result<Vec<OwnedEntry>, String> {
    if !base.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(base)
        .map_err(|e| format!("cannot inspect Topaz storage `{}`: {e}", base.display()))?
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("cannot inspect Topaz storage: {e}")),
        };
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("cannot inspect `{}`: {e}", entry.path().display())),
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        let marker_path = path.join(MARKER);
        let Ok(text) = fs::read_to_string(&marker_path) else {
            continue;
        };
        let Some(marker) = parse_marker(&text) else {
            continue;
        };
        out.push(OwnedEntry { path, marker });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn parse_marker(text: &str) -> Option<Marker> {
    let JsonValue::Object(object) = json_parse(text).ok()? else {
        return None;
    };
    match object.get("schema")? {
        JsonValue::String(value) if value.as_ref() == SCHEMA => {}
        _ => return None,
    }
    let pid = match object.get("pid")? {
        JsonValue::Number(value) => u32::try_from(value.int?).ok()?,
        _ => return None,
    };
    let created = match object.get("createdEpochSeconds")? {
        JsonValue::Number(value) => u64::try_from(value.int?).ok()?,
        _ => return None,
    };
    Some(Marker { pid, created })
}

fn marker_active(marker: &Marker) -> bool {
    let age = now_seconds().saturating_sub(marker.created);
    age < MAX_ACTIVE_AGE.as_secs() && process_is_alive(marker.pid)
}

fn reclaim_inactive(base: &Path, report: bool) -> Result<(u64, u64), String> {
    let entries = owned_entries(base)?;
    let mut removed = 0_u64;
    let mut bytes = 0_u64;
    for entry in entries {
        if marker_active(&entry.marker) {
            continue;
        }
        let size = directory_size(&entry.path)?;
        match fs::remove_dir_all(&entry.path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("cannot remove `{}`: {e}", entry.path.display())),
        }
        removed += 1;
        bytes = bytes.saturating_add(size);
        if report {
            println!("removed {} ({})", entry.path.display(), human_bytes(size));
        }
    }
    Ok((removed, bytes))
}

fn directory_size(root: &Path) -> Result<u64, String> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(format!("cannot inspect `{}`: {e}", root.display())),
    };
    if metadata.file_type().is_symlink() {
        return Ok(0);
    }
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    let mut total = 0_u64;
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(format!("cannot inspect `{}`: {e}", root.display())),
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("cannot inspect `{}`: {e}", root.display())),
        };
        total = total.saturating_add(directory_size(&entry.path())?);
    }
    Ok(total)
}

fn process_is_alive(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    #[cfg(unix)]
    {
        return std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
    }
    #[cfg(windows)]
    {
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output();
        return output.is_ok_and(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\""))
        });
    }
    #[allow(unreachable_code)]
    true
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(unix)]
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    fs::create_dir(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "topaz-storage-test-{name}-{}-{}",
            std::process::id(),
            now_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn invalid_marker_is_not_owned() {
        assert!(parse_marker("{}").is_none());
        assert!(parse_marker("not json").is_none());
    }

    #[test]
    fn marker_round_trips() {
        let marker = format!(
            "{{\"schema\":\"{SCHEMA}\",\"pid\":{},\"createdEpochSeconds\":1}}",
            std::process::id()
        );
        let parsed = parse_marker(&marker).unwrap();
        assert_eq!(parsed.pid, std::process::id());
        assert_eq!(parsed.created, 1);
    }

    #[test]
    fn byte_units_are_stable() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
    }

    #[cfg(windows)]
    #[test]
    fn canonical_workspace_paths_are_command_safe_on_windows() {
        assert_eq!(
            command_path(PathBuf::from(r"\\?\C:\Users\runner\build")),
            PathBuf::from(r"C:\Users\runner\build")
        );
        assert_eq!(
            command_path(PathBuf::from(r"\\?\UNC\server\share\build")),
            PathBuf::from(r"\\server\share\build")
        );
    }

    #[test]
    fn reclaim_removes_only_marked_inactive_workspaces() {
        let root = temp_root("reclaim");
        let owned = root.join("owned");
        let unknown = root.join("unknown");
        fs::create_dir_all(&owned).unwrap();
        fs::create_dir_all(&unknown).unwrap();
        fs::write(unknown.join("user.txt"), "keep").unwrap();
        fs::write(
            owned.join(MARKER),
            format!("{{\"schema\":\"{SCHEMA}\",\"pid\":4294967295,\"createdEpochSeconds\":1}}"),
        )
        .unwrap();
        fs::write(owned.join("leftover"), "bytes").unwrap();
        let (removed, _) = reclaim_inactive(&root, false).unwrap();
        assert_eq!(removed, 1);
        assert!(!owned.exists());
        assert_eq!(
            fs::read_to_string(unknown.join("user.txt")).unwrap(),
            "keep"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reclaim_preserves_current_active_workspace() {
        let root = temp_root("active");
        let owned = root.join("owned");
        fs::create_dir_all(&owned).unwrap();
        fs::write(
            owned.join(MARKER),
            format!(
                "{{\"schema\":\"{SCHEMA}\",\"pid\":{},\"createdEpochSeconds\":{}}}",
                std::process::id(),
                now_seconds()
            ),
        )
        .unwrap();
        let (removed, _) = reclaim_inactive(&root, false).unwrap();
        assert_eq!(removed, 0);
        assert!(owned.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
