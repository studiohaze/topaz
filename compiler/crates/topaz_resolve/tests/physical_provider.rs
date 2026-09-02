//! Real-filesystem provider physicality (CDR-002 Phase B): link
//! containment and duplicate physical identity, exercised against
//! actual directory links (NTFS junctions / POSIX symlinks) in a
//! temporary directory — the in-memory fixtures model these facts,
//! this suite proves `PhysicalProvider` reports them.

use std::fs;
use std::path::{Path, PathBuf};

use topaz_resolve::{
    DirectoryRead, FileProvider, InMemoryProvider, PhysicalProvider, SourceRead, resolve,
};

struct UnreadableDirectoryProvider {
    inner: InMemoryProvider,
}

impl FileProvider for UnreadableDirectoryProvider {
    fn read(&self, path: &str) -> SourceRead {
        self.inner.read(path)
    }

    fn read_directory(&self, dir: &str) -> DirectoryRead {
        if dir == "root" {
            DirectoryRead::Unreadable {
                reason_code: "permission-denied".to_string(),
            }
        } else {
            self.inner.read_directory(dir)
        }
    }

    fn physical_id(&self, path: &str) -> Option<String> {
        self.inner.physical_id(path)
    }
}

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("topaz_resolve_phys_{}_{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Directory link: junction on Windows (no privilege needed),
/// symlink elsewhere.
#[cfg(windows)]
fn link_dir(link: &Path, target: &Path) {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("cmd")
        .raw_arg(format!(
            "/C mklink /J \"{}\" \"{}\"",
            link.display(),
            target.display()
        ))
        .output()
        .expect("mklink spawn");
    assert!(
        out.status.success(),
        "junction creation failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[cfg(unix)]
fn link_dir(link: &Path, target: &Path) {
    std::os::unix::fs::symlink(target, link).expect("symlink");
}

fn write(path: &Path, contents: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
    fs::write(path, contents).expect("write");
}

fn primary_diagnostic(base: &Path, entry: &str, root: Option<&str>) -> Option<(String, String)> {
    let provider = PhysicalProvider::new(base);
    let output = resolve(&provider, entry, root);
    output
        .diagnostics
        .first()
        .map(|d| (d.code.as_str().to_string(), d.message.clone()))
}

fn primary_code(base: &Path, entry: &str, root: Option<&str>) -> Option<String> {
    primary_diagnostic(base, entry, root).map(|(code, _)| code)
}

#[test]
fn duplicate_physical_identity_through_a_link() {
    let base = temp_root("dup_id");
    write(&base.join("real/lib.tpz"), "export let v = 1\n");
    link_dir(&base.join("alias"), &base.join("real"));

    let provider = PhysicalProvider::new(&base);
    let direct = provider.physical_id("real/lib.tpz").expect("direct id");
    let aliased = provider.physical_id("alias/lib.tpz").expect("aliased id");
    assert_eq!(direct, aliased, "one file, one physical identity");

    let other = provider.physical_id("real").expect("dir id");
    assert_ne!(direct, other, "distinct objects keep distinct identities");

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn import_through_link_escaping_root_is_rejected() {
    let base = temp_root("import_escape");
    write(&base.join("outside/evil.tpz"), "export let v = 1\n");
    write(&base.join("unit/main.tpz"), "import link.evil\nlet x = 1\n");
    link_dir(&base.join("unit/link"), &base.join("outside"));

    let code = primary_code(&base, "unit/main.tpz", Some("unit"));
    assert_eq!(code.as_deref(), Some("TPZ3005"));

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn entry_through_link_escaping_root_is_rejected() {
    let base = temp_root("entry_escape");
    write(&base.join("outside/main.tpz"), "let x = 1\n");
    fs::create_dir_all(base.join("unit")).expect("root dir");
    link_dir(&base.join("unit/link"), &base.join("outside"));

    let code = primary_code(&base, "unit/link/main.tpz", Some("unit"));
    assert_eq!(code.as_deref(), Some("TPZ3005"));

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn in_root_link_stays_resolvable() {
    let base = temp_root("in_root_link");
    write(&base.join("unit/real/lib.tpz"), "export let v = 1\n");
    write(
        &base.join("unit/main.tpz"),
        "import real.lib\nlet x = lib.v\n",
    );
    link_dir(&base.join("unit/alias"), &base.join("unit/real"));

    let code = primary_code(&base, "unit/main.tpz", Some("unit"));
    assert_eq!(code, None, "a link inside the root is not an escape");

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn in_root_alias_cannot_give_one_file_two_module_identities() {
    let base = temp_root("duplicate_module_identity");
    write(&base.join("unit/real/lib.tpz"), "export let v = 1\n");
    write(
        &base.join("unit/main.tpz"),
        "import real.lib\nimport alias.lib\nlet x = 1\n",
    );
    link_dir(&base.join("unit/alias"), &base.join("unit/real"));

    let provider = PhysicalProvider::new(&base);
    let output = resolve(&provider, "unit/main.tpz", Some("unit"));
    let collision = output
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_str() == "TPZ3004")
        .expect("two module identities for one physical file must be rejected");
    assert!(collision.message.contains("real.lib"));
    assert!(collision.message.contains("alias.lib"));

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn case_mismatched_segment_is_unresolved_on_disk() {
    let base = temp_root("case_mismatch");
    write(&base.join("utils/strings.tpz"), "export let v = 1\n");
    write(&base.join("main.tpz"), "import Utils.strings\nlet x = 1\n");

    // Exact-scalar mapping: `Utils` is not a directory entry even
    // when a case-insensitive filesystem would open it.
    let code = primary_code(&base, "main.tpz", None);
    assert_eq!(code.as_deref(), Some("TPZ3001"));

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn invalid_utf8_entry_reports_a_source_load_failure() {
    let base = temp_root("invalid_utf8_entry");
    write(&base.join("main.tpz"), [0xff]);

    let diagnostic = primary_diagnostic(&base, "main.tpz", None).expect("load diagnostic");
    assert_eq!(diagnostic.0, "TPZ3003");
    assert!(diagnostic.1.contains("source is not valid UTF-8"));

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn invalid_utf8_import_reports_a_source_load_failure() {
    let base = temp_root("invalid_utf8_import");
    write(&base.join("main.tpz"), "import broken\nlet x = 1\n");
    write(&base.join("broken.tpz"), [0xff]);

    let diagnostic = primary_diagnostic(&base, "main.tpz", None).expect("load diagnostic");
    assert_eq!(diagnostic.0, "TPZ3003");
    assert!(diagnostic.1.contains("module `broken`"));
    assert!(diagnostic.1.contains("source is not valid UTF-8"));

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn unreadable_import_directory_reports_a_source_load_failure() {
    let mut inner = InMemoryProvider::new();
    inner.add_file("root/main.tpz", "import lib\nlet x = 1\n");
    inner.add_file("root/lib.tpz", "export let v = 1\n");
    let provider = UnreadableDirectoryProvider { inner };

    let output = resolve(&provider, "root/main.tpz", Some("root"));
    let diagnostic = output.diagnostics.first().expect("load diagnostic");
    assert_eq!(diagnostic.code.as_str(), "TPZ3003");
    assert_eq!(
        diagnostic.message,
        "cannot inspect module path for `lib`: permission-denied"
    );
    assert_eq!(
        provider.inner.reads(),
        std::collections::BTreeSet::from(["root/main.tpz".to_string()])
    );
}

#[test]
fn clean_unit_resolves_from_disk() {
    let base = temp_root("clean_unit");
    write(&base.join("utils/strings.tpz"), "export let v = 1\n");
    write(
        &base.join("main.tpz"),
        "import utils.strings\nlet x = strings.v\n",
    );

    let code = primary_code(&base, "main.tpz", None);
    assert_eq!(code, None);

    let _ = fs::remove_dir_all(&base);
}
