//! The native CLI/runtime host (CDR-006 §3): the `std`-backed `Host`
//! implementation used by `topaz run` and by every `topaz build`-emitted
//! binary. It lives in its own leaf crate — NOT in `topaz_rt` — so the
//! emitted-program runtime core stays WASM-clean: the `std::fs`/`std::io`/clock
//! effects are confined here, behind the `Host` effect boundary. A `topaz
//! build` output therefore depends on `topaz_rt` + `topaz_host_native` only,
//! never on the interpreter (`topaz_interp` keeps the corpus `TestHost`).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

// The `Host` effect trait and the opaque `ResourceId` handle live in the shared
// core (CDR-006 §3): they are the bottom-layer ABI the callables compile
// against. This crate is one host IMPLEMENTATION (the native, std-backed leaf).
pub use topaz_value::{ExternReplayStore, Host, HostDirEntry, ResourceId, Value};

/// CLI host: std I/O and a monotonic clock.
#[derive(Debug)]
pub struct NativeHost {
    start: std::time::Instant,
    files: RefCell<NativeFiles>,
    fs_caps: Option<FsCaps>,
    /// §22 `input()` cache: piped stdin, read once on first call (`""` when
    /// stdin is a terminal, so an interactive run never blocks). Cached so
    /// repeated `input()` calls return the same string within a run.
    stdin: RefCell<Option<String>>,
    extern_replay: ExternReplayStore,
}

#[derive(Debug, Default)]
struct NativeFiles {
    next: u64,
    open: BTreeMap<u64, NativeFile>,
}

#[derive(Debug)]
struct NativeFile {
    path: PathBuf,
    read_allowed: bool,
    write_allowed: bool,
    /// One-shot read guard mirrors the §22.3 surface (read returns
    /// the whole contents; repeated reads re-read the file).
    closed: bool,
}

#[derive(Debug)]
struct FsCaps {
    base: PathBuf,
    read_roots: Vec<PathBuf>,
    write_roots: Vec<PathBuf>,
}

impl NativeHost {
    pub fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
            files: RefCell::new(NativeFiles::default()),
            fs_caps: None,
            stdin: RefCell::new(None),
            extern_replay: ExternReplayStore::empty(),
        }
    }

    pub fn with_fs_capabilities(
        base: impl AsRef<Path>,
        read_roots: &[String],
        write_roots: &[String],
    ) -> Self {
        let base = normalize_path(base.as_ref());
        Self {
            start: std::time::Instant::now(),
            files: RefCell::new(NativeFiles::default()),
            fs_caps: Some(FsCaps {
                read_roots: read_roots
                    .iter()
                    .map(|root| normalize_under(&base, root))
                    .collect(),
                write_roots: write_roots
                    .iter()
                    .map(|root| normalize_under(&base, root))
                    .collect(),
                base,
            }),
            stdin: RefCell::new(None),
            extern_replay: ExternReplayStore::empty(),
        }
    }

    pub fn with_extern_replay(mut self, replay: ExternReplayStore) -> Self {
        self.extern_replay = replay;
        self
    }

    fn resolve_open(&self, path: &str) -> Result<(PathBuf, bool, bool), String> {
        let Some(caps) = &self.fs_caps else {
            return Ok((PathBuf::from(path), true, true));
        };
        let target = normalize_under(&caps.base, path);
        let read_allowed = allowed(&target, &caps.read_roots);
        let write_allowed = allowed(&target, &caps.write_roots);
        if !read_allowed && !write_allowed {
            return Err(format!(
                "cannot open `{path}`: not permitted by package fs capabilities"
            ));
        }
        if target.exists() && escapes_capability_roots(&target, caps, read_allowed, write_allowed) {
            return Err(format!(
                "cannot open `{path}`: resolves outside package fs capabilities"
            ));
        }
        Ok((target, read_allowed, write_allowed))
    }
}

impl Default for NativeHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Host for NativeHost {
    fn print(&self, line: &str) {
        println!("{line}");
    }

    fn open(&self, path: &str) -> Result<ResourceId, String> {
        let (resolved_path, read_allowed, write_allowed) = self.resolve_open(path)?;
        // Existence check at open: §22.3 `open` is the fallible
        // step; reads/writes report their own failures.
        if !resolved_path.exists() {
            return Err(format!("cannot open `{path}`: not found"));
        }
        let mut files = self.files.borrow_mut();
        files.next += 1;
        let id = files.next;
        files.open.insert(
            id,
            NativeFile {
                path: resolved_path,
                read_allowed,
                write_allowed,
                closed: false,
            },
        );
        Ok(ResourceId(id))
    }

    fn read(&self, handle: ResourceId) -> Result<String, String> {
        let files = self.files.borrow();
        let file = files.open.get(&handle.0).ok_or("file is not open")?;
        if file.closed {
            return Err("file is closed".to_string());
        }
        if !file.read_allowed {
            return Err("file is not readable by package fs capabilities".to_string());
        }
        std::fs::read_to_string(&file.path).map_err(|e| e.to_string())
    }

    fn write(&self, handle: ResourceId, s: &str) -> Result<(), String> {
        let files = self.files.borrow();
        let file = files.open.get(&handle.0).ok_or("file is not open")?;
        if file.closed {
            return Err("file is closed".to_string());
        }
        if !file.write_allowed {
            return Err("file is not writable by package fs capabilities".to_string());
        }
        std::fs::write(&file.path, s).map_err(|e| e.to_string())
    }

    fn close(&self, handle: ResourceId) {
        if let Some(file) = self.files.borrow_mut().open.get_mut(&handle.0) {
            file.closed = true;
        }
    }

    fn read_bytes(&self, path: &str) -> Result<Vec<u8>, String> {
        let (resolved_path, read_allowed, _) = self.resolve_open(path)?;
        if !read_allowed {
            return Err(format!(
                "cannot read `{path}`: not permitted by package fs capabilities"
            ));
        }
        std::fs::read(&resolved_path).map_err(|e| e.to_string())
    }

    fn write_bytes(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let (resolved_path, _, write_allowed) = self.resolve_open(path)?;
        if !write_allowed {
            return Err(format!(
                "cannot write `{path}`: not permitted by package fs capabilities"
            ));
        }
        std::fs::write(&resolved_path, bytes).map_err(|e| e.to_string())
    }

    fn list_dir(&self, path: &str) -> Result<Vec<HostDirEntry>, String> {
        let (resolved_path, read_allowed, _) = self.resolve_open(path)?;
        if !read_allowed {
            return Err(format!(
                "cannot list `{path}`: not permitted by package fs capabilities"
            ));
        }
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&resolved_path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name().into_string().map_err(|_| {
                format!("cannot list `{path}`: directory entry name is not valid Unicode")
            })?;
            let meta = std::fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
            let file_type = meta.file_type();
            let kind = if file_type.is_file() {
                "file"
            } else if file_type.is_dir() {
                "directory"
            } else if file_type.is_symlink() {
                "symlink"
            } else {
                "other"
            };
            let size_bytes = if file_type.is_file() {
                i64::try_from(meta.len()).ok()
            } else {
                None
            };
            entries.push(HostDirEntry {
                name,
                kind: kind.to_string(),
                size_bytes,
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    fn now_millis(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    fn defer_error(&self, rendered: &str) {
        eprintln!("deferred action error: {rendered}");
    }

    fn input(&self) -> String {
        use std::io::{IsTerminal, Read};
        let mut cache = self.stdin.borrow_mut();
        if let Some(s) = cache.as_ref() {
            return s.clone();
        }
        // Read piped stdin once; a terminal yields `""` so an interactive run
        // (no piped input) never blocks waiting on the keyboard.
        let s = if std::io::stdin().is_terminal() {
            String::new()
        } else {
            let mut buf = String::new();
            let _ = std::io::stdin().read_to_string(&mut buf);
            buf
        };
        *cache = Some(s.clone());
        s
    }

    fn extern_call(&self, module: &str, function: &str, args: &[Value]) -> Result<Value, String> {
        self.extern_replay
            .call_replay_sandbox(module, function, args)
    }

    fn lispex_application(
        &self,
        _request: topaz_value::LispexApplicationRequest,
    ) -> topaz_value::LispexApplicationResponse {
        topaz_value::LispexApplicationResponse::OperationalFault {
            code: "target-unavailable".into(),
            detail: None,
        }
    }
}

fn normalize_under(base: &Path, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        normalize_path(path)
    } else {
        normalize_path(&base.join(path))
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(part) => out.push(part),
            Component::RootDir | Component::Prefix(_) => out.push(component.as_os_str()),
        }
    }
    out
}

fn allowed(target: &Path, roots: &[PathBuf]) -> bool {
    roots
        .iter()
        .any(|root| target == root || target.starts_with(root))
}

fn escapes_capability_roots(
    target: &Path,
    caps: &FsCaps,
    read_allowed: bool,
    write_allowed: bool,
) -> bool {
    let Ok(real_target) = std::fs::canonicalize(target) else {
        return false;
    };
    let real_base = std::fs::canonicalize(&caps.base).unwrap_or_else(|_| caps.base.clone());
    let roots = caps
        .read_roots
        .iter()
        .filter(move |_| read_allowed)
        .chain(caps.write_roots.iter().filter(move |_| write_allowed));
    !roots
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .filter(|root| *root == real_base || root.starts_with(&real_base))
        .any(|root| real_target == root || real_target.starts_with(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "topaz_native_host_caps_{}_{}",
            std::process::id(),
            tag
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root");
        root
    }

    #[test]
    fn package_fs_caps_gate_read_and_write_roots() {
        let root = temp_root("rw");
        std::fs::create_dir_all(root.join("data")).expect("data dir");
        std::fs::create_dir_all(root.join("out")).expect("out dir");
        std::fs::write(root.join("data/in.txt"), "in").expect("seed read");
        std::fs::write(root.join("out/result.txt"), "old").expect("seed write");
        std::fs::write(root.join("secret.txt"), "secret").expect("seed secret");

        let host = NativeHost::with_fs_capabilities(
            &root,
            &[String::from("data")],
            &[String::from("out")],
        );
        let input = host.open("data/in.txt").expect("read handle");
        assert_eq!(host.read(input).as_deref(), Ok("in"));
        assert!(host.write(input, "nope").is_err());

        let output = host.open("out/result.txt").expect("write handle");
        assert!(host.read(output).is_err());
        host.write(output, "new").expect("write allowed");
        assert_eq!(
            std::fs::read_to_string(root.join("out/result.txt")).expect("result"),
            "new"
        );

        let denied = host.open("secret.txt").expect_err("outside caps");
        assert!(denied.contains("not permitted"), "{denied}");

        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn package_fs_caps_reject_symlink_roots_that_escape_base() {
        let root = temp_root("symlink-root");
        let outside = temp_root("symlink-outside");
        std::fs::write(outside.join("secret.txt"), "secret").expect("outside secret");
        std::os::unix::fs::symlink(&outside, root.join("outlink")).expect("symlink");

        let host = NativeHost::with_fs_capabilities(&root, &[], &[String::from("outlink")]);
        let denied = host
            .open("outlink/secret.txt")
            .expect_err("symlink root escapes package base");
        assert!(denied.contains("resolves outside"), "{denied}");

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }
}
