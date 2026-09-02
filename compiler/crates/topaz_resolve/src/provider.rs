//! Filesystem providers (CDR-002 §5): the resolver consumes logical
//! *and physical* filesystem facts through this trait so directory
//! fixtures run hermetically. Paths are `/`-separated and relative
//! to the provider's base; the resolver never touches `std::fs`.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Complete outcome of loading one logical Topaz source file. Missing input is
/// distinct from an existing source that cannot cross the UTF-8 loader boundary.
pub enum SourceRead {
    Present(String),
    Missing,
    Unreadable { reason_code: String },
    InvalidUtf8,
}

/// Complete outcome of listing one logical source directory. A missing path is
/// distinct from a directory whose entries cannot cross the loader boundary.
pub enum DirectoryRead {
    Present(Vec<(String, bool)>),
    Missing,
    Unreadable { reason_code: String },
}

/// Loads a physical source path with deterministic failure categories shared by
/// direct CLI resolution and kernel host-fact collection.
pub fn read_source_path(path: &Path) -> SourceRead {
    match fs::read(path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(source) => SourceRead::Present(source),
            Err(_) => SourceRead::InvalidUtf8,
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => SourceRead::Missing,
        Err(error) => SourceRead::Unreadable {
            reason_code: match error.kind() {
                std::io::ErrorKind::PermissionDenied => "permission-denied",
                std::io::ErrorKind::IsADirectory => "is-directory",
                _ => "io-error",
            }
            .to_string(),
        },
    }
}

/// Component-preserving physical identity for comparisons within one compiler
/// process. Component separators remain visible for containment, while exact
/// platform `OsStr` encodings are hex-encoded instead of replaced as Unicode.
pub fn physical_path_identity(path: &Path) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut identity = String::new();
    for component in path.components() {
        if !identity.is_empty() {
            identity.push('/');
        }
        let bytes = match component {
            Component::Prefix(prefix) => {
                identity.push_str("prefix:");
                prefix.as_os_str().as_encoded_bytes()
            }
            Component::RootDir => {
                identity.push_str("root");
                continue;
            }
            Component::CurDir => {
                identity.push_str("current");
                continue;
            }
            Component::ParentDir => {
                identity.push_str("parent");
                continue;
            }
            Component::Normal(segment) => {
                identity.push_str("normal:");
                segment.as_encoded_bytes()
            }
        };
        for byte in bytes {
            identity.push(HEX[(byte >> 4) as usize] as char);
            identity.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    identity
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedStdModule {
    pub path: String,
    pub source: String,
}

pub trait FileProvider {
    /// Complete source-load outcome for `path`.
    fn read(&self, path: &str) -> SourceRead;
    /// Whether `path` is a generated manifest extern module. The default keeps
    /// ordinary providers unaware of package-only virtual modules.
    fn is_extern_file(&self, _path: &str) -> bool {
        false
    }
    /// Exact compiler-owned `std.*` source supplied by a package capability.
    /// Ordinary providers return none, so a physical file cannot impersonate
    /// one of these modules.
    fn generated_std_module(&self, _identity: &str) -> Option<GeneratedStdModule> {
        None
    }
    /// Whether the dotted module identity falls under a manifest extern
    /// namespace but is not itself declared.
    fn is_extern_namespace(&self, _identity: &str) -> bool {
        false
    }
    /// Replay fixture load/validation error for a declared extern module, if any.
    fn extern_replay_error(&self, _identity: &str) -> Option<String> {
        None
    }
    /// Complete directory-load outcome. Present entries are `(name, is_dir)`
    /// pairs sorted by name (used by the collision keys; CDR-002 Phase B).
    fn read_directory(&self, dir: &str) -> DirectoryRead;
    /// Physical identity of a path, for containment and
    /// duplicate-identity checks. In-memory fixtures derive it from
    /// virtual link records; the physical provider canonicalizes.
    fn physical_id(&self, path: &str) -> Option<String>;
}

/// Hermetic provider for fixtures: a map of `path -> contents` plus
/// virtual symlink records (`link dir -> target dir`) consumed by
/// the resolver's containment checks.
#[derive(Debug, Default)]
pub struct InMemoryProvider {
    files: BTreeMap<String, String>,
    generated_std_modules: BTreeMap<String, GeneratedStdModule>,
    links: BTreeMap<String, String>,
    /// Every path handed to [`FileProvider::read`], for harness
    /// negative-space assertions ("this file was never read").
    reads: RefCell<BTreeSet<String>>,
}

impl InMemoryProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_file(&mut self, path: impl Into<String>, contents: impl Into<String>) {
        self.files.insert(path.into(), contents.into());
    }

    pub fn add_link(&mut self, from: impl Into<String>, to: impl Into<String>) {
        self.links.insert(from.into(), to.into());
    }

    pub fn add_generated_std_module(
        &mut self,
        identity: impl Into<String>,
        path: impl Into<String>,
        source: impl Into<String>,
    ) {
        self.generated_std_modules.insert(
            identity.into(),
            GeneratedStdModule {
                path: path.into(),
                source: source.into(),
            },
        );
    }

    /// Paths read so far (harness observability).
    pub fn reads(&self) -> BTreeSet<String> {
        self.reads.borrow().clone()
    }

    /// Resolves virtual links in `path` (one level, leftmost-first).
    fn resolve_links(&self, path: &str) -> String {
        for (from, to) in &self.links {
            if let Some(rest) = path.strip_prefix(from.as_str())
                && (rest.is_empty() || rest.starts_with('/'))
            {
                return format!("{to}{rest}");
            }
        }
        path.to_string()
    }
}

impl FileProvider for InMemoryProvider {
    fn read(&self, path: &str) -> SourceRead {
        self.reads.borrow_mut().insert(path.to_string());
        let physical = self.resolve_links(path);
        self.files
            .get(&physical)
            .cloned()
            .map_or(SourceRead::Missing, SourceRead::Present)
    }

    fn generated_std_module(&self, identity: &str) -> Option<GeneratedStdModule> {
        self.generated_std_modules.get(identity).cloned()
    }

    fn read_directory(&self, dir: &str) -> DirectoryRead {
        let physical = self.resolve_links(dir);
        if self.files.contains_key(&physical) {
            return DirectoryRead::Unreadable {
                reason_code: "not-directory".to_string(),
            };
        }
        let prefix = if physical.is_empty() {
            String::new()
        } else {
            format!("{physical}/")
        };
        let mut out: Vec<(String, bool)> = Vec::new();
        for path in self.files.keys() {
            let Some(rest) = path.strip_prefix(&prefix) else {
                continue;
            };
            match rest.split_once('/') {
                Some((head, _)) => {
                    let entry = (head.to_string(), true);
                    if !out.contains(&entry) {
                        out.push(entry);
                    }
                }
                None => out.push((rest.to_string(), false)),
            }
        }
        out.sort();
        out.dedup();
        if physical.is_empty() || !out.is_empty() {
            DirectoryRead::Present(out)
        } else {
            DirectoryRead::Missing
        }
    }

    fn physical_id(&self, path: &str) -> Option<String> {
        // The virtual canonicalization itself, whether or not the
        // target exists: containment checks must see where a link
        // points even when nothing is stored there.
        Some(self.resolve_links(path))
    }
}

/// Real-filesystem provider for the CLI.
#[derive(Debug)]
pub struct PhysicalProvider {
    base: PathBuf,
}

impl PhysicalProvider {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }

    fn join(&self, path: &str) -> PathBuf {
        let mut p = self.base.clone();
        for seg in path.split('/').filter(|s| !s.is_empty()) {
            p.push(seg);
        }
        p
    }
}

impl FileProvider for PhysicalProvider {
    fn read(&self, path: &str) -> SourceRead {
        read_source_path(&self.join(path))
    }

    fn read_directory(&self, dir: &str) -> DirectoryRead {
        let entries = match fs::read_dir(self.join(dir)) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return DirectoryRead::Missing;
            }
            Err(error) => {
                return DirectoryRead::Unreadable {
                    reason_code: directory_error_reason(&error),
                };
            }
        };
        let mut out = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    return DirectoryRead::Unreadable {
                        reason_code: "entry-io-error".to_string(),
                    };
                }
            };
            let name = match entry.file_name().into_string() {
                Ok(name) => name,
                Err(_) => continue,
            };
            let is_dir = match fs::metadata(entry.path()) {
                Ok(metadata) => metadata.is_dir(),
                Err(_) => {
                    return DirectoryRead::Unreadable {
                        reason_code: "entry-type-error".to_string(),
                    };
                }
            };
            out.push((name, is_dir));
        }
        out.sort();
        DirectoryRead::Present(out)
    }

    fn physical_id(&self, path: &str) -> Option<String> {
        fs::canonicalize(self.join(path))
            .ok()
            .map(|path| physical_path_identity(&path))
    }
}

fn directory_error_reason(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => "permission-denied",
        std::io::ErrorKind::NotADirectory => "not-directory",
        _ => "io-error",
    }
    .to_string()
}

/// Normalizes a `/`-relative path: strips `./`, collapses empty
/// segments. (`..` is not interpreted — module paths cannot address
/// upward, and entry/root paths are expected pre-normalized.)
pub fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
        .split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .collect::<Vec<_>>()
        .join("/")
}
