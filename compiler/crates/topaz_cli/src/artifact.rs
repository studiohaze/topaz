use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use topaz_syntax::LangVersion;
use topaz_value::{JsonValue, json_parse};

pub const MANIFEST_NAME: &str = "topaz-artifact.json";
pub const LICENSE_NAME: &str = "LICENSE";
pub const NOTICE_NAME: &str = "NOTICE";
pub const OUTPUT_NOTICE_NAME: &str = "GENERATED-OUTPUT-NOTICE.txt";

const APACHE_LICENSE: &str = include_str!("../../../licenses/APACHE-2.0-PUBLIC-ARTIFACTS.txt");
const TOPAZ_NOTICE: &str = include_str!("../../../NOTICE");
const OUTPUT_NOTICE: &str = include_str!("../../../licenses/GENERATED-OUTPUT-NOTICE.txt");

pub fn license_text() -> &'static str {
    APACHE_LICENSE
}

pub fn notice_text() -> &'static str {
    TOPAZ_NOTICE
}

pub fn output_notice_text() -> &'static str {
    OUTPUT_NOTICE
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Determines both the output manifest label and which stale files may be reclaimed.
pub enum Target {
    Native,
    Python,
    Web,
    WebWorker,
    WebApp,
    HttpService,
}

impl Target {
    pub fn label(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Python => "python",
            Self::Web => "web",
            Self::WebWorker => "web-worker",
            Self::WebApp => "web-app",
            Self::HttpService => "http-service",
        }
    }
}

#[derive(Debug)]
/// Output bytes remain in memory until the destination preflight succeeds.
pub struct File {
    pub path: String,
    pub bytes: Vec<u8>,
    pub executable: bool,
}

impl File {
    pub fn text(path: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            bytes: text.into().into_bytes(),
            executable: false,
        }
    }

    pub fn binary(path: impl Into<String>, bytes: Vec<u8>, executable: bool) -> Self {
        Self {
            path: path.into(),
            bytes,
            executable,
        }
    }
}

#[derive(Debug)]
/// Destination rejects a plan whose target differs from its preflight target.
pub struct Plan {
    pub target: Target,
    pub language_version: LangVersion,
    pub entry: String,
    pub runtime_requirements: Vec<String>,
    pub invocation: String,
    pub compiler: Option<CompilerProvenance>,
    pub files: Vec<File>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Self-compiler identities recorded beside generated target source.
pub struct CompilerProvenance {
    pub selector: String,
    pub producer: String,
    pub selection_origin: String,
    pub compiler_source_set_id: String,
    pub target_source_set_id: String,
    pub compile_product_id: String,
    pub generated_source_sha256: String,
    pub target_compiler_fallback: bool,
}

#[derive(Debug)]
/// Tracks only files named by a prior Topaz manifest; unrelated files are never cleanup candidates.
pub struct Destination {
    root: PathBuf,
    target: Target,
    previous_files: Vec<String>,
    legacy_cleanup: Vec<PathBuf>,
}

impl Destination {
    /// Admits an output root and inventories files owned by its prior manifest.
    pub fn open(root: &Path, target: Target) -> Result<Self, String> {
        if root.as_os_str().is_empty() {
            return Err("output directory is empty".into());
        }
        fs::create_dir_all(root).map_err(|e| format!("cannot create `{}`: {e}", root.display()))?;
        let manifest_path = root.join(MANIFEST_NAME);
        let mut previous_files = Vec::new();
        let mut legacy_cleanup = Vec::new();
        if manifest_path.exists() {
            let parsed = parse_manifest(&manifest_path)?;
            if parsed.target != target.label() {
                return Err(format!(
                    "output directory `{}` contains target `{}`; choose another directory for `{}`",
                    root.display(),
                    parsed.target,
                    target.label()
                ));
            }
            previous_files = parsed.managed_files;
        } else {
            let entries = fs::read_dir(root)
                .map_err(|e| format!("cannot inspect `{}`: {e}", root.display()))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("cannot inspect `{}`: {e}", root.display()))?;
            if !entries.is_empty() {
                let legacy = legacy_topaz_output(root, target)?;
                if legacy.is_empty() {
                    eprintln!(
                        "topaz: warning: `{}` is a nonempty pre-v5.6.1 output directory; unknown files will be preserved",
                        root.display()
                    );
                } else {
                    eprintln!(
                        "topaz: migrating recognized pre-v5.6.1 {} output in `{}`",
                        target.label(),
                        root.display()
                    );
                    legacy_cleanup = legacy;
                }
            }
        }
        Ok(Self {
            root: root.to_path_buf(),
            target,
            previous_files,
            legacy_cleanup,
        })
    }

    /// Writes a complete artifact plan and removes only previously owned stale files.
    pub fn commit(self, mut plan: Plan) -> Result<(), String> {
        if plan.target != self.target {
            return Err("artifact plan target differs from destination preflight".into());
        }
        if !plan.language_version.is_selectable() {
            return Err(format!(
                "artifact plan language mode `topaz-{}` is known but not selectable",
                plan.language_version.as_str()
            ));
        }
        append_distribution_files(&mut plan.files);
        validate_files(&plan.files)?;
        let managed_files: Vec<String> = plan.files.iter().map(|f| f.path.clone()).collect();
        let manifest = render_manifest(&plan);

        for old in &self.previous_files {
            for current in &managed_files {
                if old != current && paths_overlap(old, current) {
                    return Err(format!(
                        "previous managed path `{old}` overlaps new managed path `{current}`"
                    ));
                }
            }
        }

        let mut staged = Vec::new();
        let mut stage_guard = StageGuard::default();
        let mut created_parents: BTreeSet<PathBuf> = BTreeSet::new();
        for (index, file) in plan.files.iter().enumerate() {
            let final_path = self.root.join(&file.path);
            ensure_plain_managed_parent(&self.root, Path::new(&file.path))?;
            if let Ok(metadata) = fs::symlink_metadata(&final_path) {
                if metadata.file_type().is_symlink() {
                    return Err(format!(
                        "managed output `{}` is a symlink; refusing to follow it",
                        final_path.display()
                    ));
                }
                if !self.owns_path(&final_path) {
                    return Err(format!(
                        "output path `{}` already exists but is not owned by Topaz",
                        final_path.display()
                    ));
                }
                if metadata.is_file()
                    && fs::read(&final_path)
                        .is_ok_and(|bytes| bytes.as_slice() == file.bytes.as_slice())
                {
                    set_executable(&final_path, file.executable)?;
                    continue;
                }
            }
            if let Some(parent) = final_path.parent() {
                if parent != self.root
                    && parent.exists()
                    && !created_parents
                        .iter()
                        .any(|created| created.starts_with(parent))
                    && !self.owns_parent(parent)
                {
                    return Err(format!(
                        "output directory `{}` already exists but is not owned by Topaz",
                        parent.display()
                    ));
                }
                if !parent.exists() {
                    fs::create_dir_all(parent)
                        .map_err(|e| format!("cannot create `{}`: {e}", parent.display()))?;
                    created_parents.insert(parent.to_path_buf());
                }
            }
            ensure_plain_managed_parent(&self.root, Path::new(&file.path))?;
            let stage = stage_path(&self.root, index);
            let mut handle = match OpenOptions::new().write(true).create_new(true).open(&stage) {
                Ok(handle) => handle,
                Err(e) => {
                    return Err(format!("cannot stage `{}`: {e}", final_path.display()));
                }
            };
            if let Err(e) = handle
                .write_all(&file.bytes)
                .and_then(|_| handle.sync_all())
            {
                let _ = fs::remove_file(&stage);
                return Err(format!("cannot stage `{}`: {e}", final_path.display()));
            }
            stage_guard.paths.push(stage.clone());
            staged.push((stage, final_path, file.executable));
        }

        let manifest_path = self.root.join(MANIFEST_NAME);
        let stale_exists = self
            .previous_files
            .iter()
            .any(|old| !managed_files.iter().any(|current| current == old));
        if staged.is_empty()
            && self.legacy_cleanup.is_empty()
            && !stale_exists
            && fs::read_to_string(&manifest_path).is_ok_and(|current| current == manifest)
        {
            return Ok(());
        }
        let manifest_stage = stage_path(&self.root, plan.files.len());
        let mut manifest_handle = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&manifest_stage)
            .map_err(|e| format!("cannot stage `{}`: {e}", manifest_path.display()))?;
        stage_guard.paths.push(manifest_stage.clone());
        if let Err(e) = manifest_handle
            .write_all(manifest.as_bytes())
            .and_then(|_| manifest_handle.sync_all())
        {
            let _ = fs::remove_file(&manifest_stage);
            return Err(format!("cannot stage `{}`: {e}", manifest_path.display()));
        }

        let current: BTreeSet<&str> = managed_files.iter().map(String::as_str).collect();
        for old in &self.legacy_cleanup {
            let relative = old.strip_prefix(&self.root).map_err(|_| {
                format!("legacy managed output `{}` escaped its root", old.display())
            })?;
            ensure_plain_managed_parent(&self.root, relative)?;
        }
        for (_, final_path, _) in &staged {
            let relative = final_path.strip_prefix(&self.root).map_err(|_| {
                format!("managed output `{}` escaped its root", final_path.display())
            })?;
            ensure_plain_managed_parent(&self.root, relative)?;
        }
        for old in &self.previous_files {
            if !current.contains(old.as_str()) {
                ensure_plain_managed_parent(&self.root, Path::new(old))?;
            }
        }

        if manifest_path.exists() {
            fs::remove_file(&manifest_path)
                .map_err(|e| format!("cannot replace `{}`: {e}", manifest_path.display()))?;
        }
        for old in &self.legacy_cleanup {
            let relative = old.strip_prefix(&self.root).map_err(|_| {
                format!("legacy managed output `{}` escaped its root", old.display())
            })?;
            ensure_plain_managed_parent(&self.root, relative)?;
            if old.exists() {
                remove_managed_path(old)?;
            }
        }
        for (stage, final_path, executable) in &staged {
            let relative = final_path.strip_prefix(&self.root).map_err(|_| {
                format!("managed output `{}` escaped its root", final_path.display())
            })?;
            ensure_plain_managed_parent(&self.root, relative)?;
            if let Some(parent) = final_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot recreate `{}`: {e}", parent.display()))?;
            }
            ensure_plain_managed_parent(&self.root, relative)?;
            if final_path.exists() {
                remove_managed_path(final_path)?;
            }
            if let Err(e) = fs::rename(stage, final_path) {
                let _ = fs::remove_file(&manifest_stage);
                return Err(format!("cannot install `{}`: {e}", final_path.display()));
            }
            set_executable(final_path, *executable)?;
        }

        for old in &self.previous_files {
            if !current.contains(old.as_str()) {
                ensure_plain_managed_parent(&self.root, Path::new(old))?;
                remove_managed_path(&self.root.join(old))?;
            }
        }
        fs::rename(&manifest_stage, &manifest_path)
            .map_err(|e| format!("cannot install `{}`: {e}", manifest_path.display()))?;
        Ok(())
    }

    fn owns_path(&self, path: &Path) -> bool {
        self.previous_files
            .iter()
            .any(|old| self.root.join(old) == path)
            || self
                .legacy_cleanup
                .iter()
                .any(|old| path == old || (old.is_dir() && path.starts_with(old)))
    }

    fn owns_parent(&self, parent: &Path) -> bool {
        self.previous_files
            .iter()
            .any(|old| self.root.join(old).starts_with(parent))
            || self
                .legacy_cleanup
                .iter()
                .any(|old| parent == old || parent.starts_with(old) || old.starts_with(parent))
    }
}

#[derive(Default)]
struct StageGuard {
    paths: Vec<PathBuf>,
}

impl Drop for StageGuard {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = fs::remove_file(path);
        }
    }
}

fn append_distribution_files(files: &mut Vec<File>) {
    let paths: BTreeSet<&str> = files.iter().map(|f| f.path.as_str()).collect();
    let mut added = Vec::new();
    if !paths.contains(LICENSE_NAME) {
        added.push(File::text(LICENSE_NAME, APACHE_LICENSE));
    }
    if !paths.contains(NOTICE_NAME) {
        added.push(File::text(NOTICE_NAME, TOPAZ_NOTICE));
    }
    if !paths.contains(OUTPUT_NOTICE_NAME) {
        added.push(File::text(OUTPUT_NOTICE_NAME, OUTPUT_NOTICE));
    }
    files.extend(added);
}

fn validate_files(files: &[File]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for file in files {
        validate_relative(&file.path)?;
        if file.path == MANIFEST_NAME {
            return Err(format!(
                "{MANIFEST_NAME} is written by the artifact committer"
            ));
        }
        if !seen.insert(file.path.as_str()) {
            return Err(format!("duplicate managed file `{}`", file.path));
        }
    }
    for left in files {
        for right in files {
            if left.path != right.path && paths_overlap(&left.path, &right.path) {
                return Err(format!(
                    "managed paths overlap: `{}` and `{}`",
                    left.path, right.path
                ));
            }
        }
    }
    Ok(())
}

fn paths_overlap(left: &str, right: &str) -> bool {
    let left = Path::new(left);
    let right = Path::new(right);
    left.starts_with(right) || right.starts_with(left)
}

fn validate_relative(raw: &str) -> Result<(), String> {
    if raw.is_empty() || raw.contains('\\') {
        return Err(format!("invalid managed path `{raw}`"));
    }
    let path = Path::new(raw);
    if path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(format!("managed path must be repository-relative: `{raw}`"));
    }
    Ok(())
}

fn ensure_plain_managed_parent(root: &Path, relative: &Path) -> Result<(), String> {
    let mut current = root.to_path_buf();
    let Some(parent) = relative.parent() else {
        return Ok(());
    };
    for component in parent.components() {
        let Component::Normal(segment) = component else {
            return Err(format!(
                "managed output `{}` is not repository-relative",
                relative.display()
            ));
        };
        current.push(segment);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(format!(
                    "managed output parent `{}` is not a plain directory",
                    current.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(format!("cannot inspect `{}`: {error}", current.display()));
            }
        }
    }
    Ok(())
}

fn stage_path(root: &Path, index: usize) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    root.join(format!(
        ".topaz-stage-{}-{nanos}-{index}",
        std::process::id()
    ))
}

fn remove_managed_path(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || meta.is_file() => fs::remove_file(path),
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => return Err(format!("unsupported managed path `{}`", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("cannot inspect `{}`: {e}", path.display())),
    }
    .map_err(|e| format!("cannot remove managed path `{}`: {e}", path.display()))
}

#[cfg(unix)]
fn set_executable(path: &Path, executable: bool) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    if executable {
        let mut permissions = fs::metadata(path)
            .map_err(|e| format!("cannot inspect `{}`: {e}", path.display()))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)
            .map_err(|e| format!("cannot mark `{}` executable: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path, _executable: bool) -> Result<(), String> {
    Ok(())
}

struct ParsedManifest {
    target: String,
    managed_files: Vec<String>,
}

fn parse_manifest(path: &Path) -> Result<ParsedManifest, String> {
    let text =
        fs::read_to_string(path).map_err(|e| format!("cannot read `{}`: {e}", path.display()))?;
    let root = json_parse(&text).map_err(|e| format!("invalid `{}`: {e:?}", path.display()))?;
    let JsonValue::Object(object) = root else {
        return Err(format!(
            "invalid `{}`: root must be an object",
            path.display()
        ));
    };
    let required_root_fields = [
        "schema",
        "toolchainVersion",
        "languageMode",
        "target",
        "entry",
        "runtimeRequirements",
        "managedFiles",
        "invocation",
    ];
    if !matches!(object.len(), 8 | 9)
        || required_root_fields
            .iter()
            .any(|name| !object.contains_key(*name))
        || (object.len() == 9 && !object.contains_key("compiler"))
    {
        return Err(format!(
            "invalid `{}`: unknown or missing field",
            path.display()
        ));
    }
    let get_string = |name: &str| -> Result<String, String> {
        match object.get(name) {
            Some(JsonValue::String(value)) => Ok(value.to_string()),
            _ => Err(format!(
                "invalid `{}`: `{name}` must be a string",
                path.display()
            )),
        }
    };
    if get_string("schema")? != "topaz.artifact.v1" {
        return Err(format!("invalid `{}`: unsupported schema", path.display()));
    }
    let language_mode = get_string("languageMode")?;
    let valid_language_mode = language_mode
        .strip_prefix("topaz-")
        .and_then(LangVersion::parse_selectable)
        .is_some();
    if get_string("toolchainVersion")?.is_empty()
        || !valid_language_mode
        || get_string("entry")?.is_empty()
        || get_string("invocation")?.is_empty()
    {
        return Err(format!(
            "invalid `{}`: invalid artifact identity",
            path.display()
        ));
    }
    if let Some(compiler) = object.get("compiler") {
        validate_compiler_provenance(path, compiler)?;
    }
    let target = get_string("target")?;
    if ![
        "native",
        "python",
        "web",
        "web-worker",
        "web-app",
        "http-service",
    ]
    .contains(&target.as_str())
    {
        return Err(format!("invalid `{}`: unknown target", path.display()));
    }
    let Some(JsonValue::Array(requirements)) = object.get("runtimeRequirements") else {
        return Err(format!(
            "invalid `{}`: runtimeRequirements must be an array",
            path.display()
        ));
    };
    if requirements
        .iter()
        .any(|value| !matches!(value, JsonValue::String(_)))
    {
        return Err(format!(
            "invalid `{}`: runtime requirement must be a string",
            path.display()
        ));
    }
    let Some(JsonValue::Array(files)) = object.get("managedFiles") else {
        return Err(format!(
            "invalid `{}`: `managedFiles` must be an array",
            path.display()
        ));
    };
    let mut managed_files = Vec::new();
    let mut seen = BTreeSet::new();
    let root = path
        .parent()
        .ok_or_else(|| "artifact manifest has no parent".to_string())?;
    for value in files.iter() {
        let JsonValue::Object(fields) = value else {
            return Err(format!(
                "invalid `{}`: managed file must be an object",
                path.display()
            ));
        };
        let expected_file_fields = ["path", "sha256", "bytes", "executable"];
        if fields.len() != expected_file_fields.len()
            || expected_file_fields
                .iter()
                .any(|name| !fields.contains_key(*name))
        {
            return Err(format!(
                "invalid `{}`: managed file has unknown or missing field",
                path.display()
            ));
        }
        let field_string = |name: &str| -> Result<String, String> {
            match fields.get(name) {
                Some(JsonValue::String(value)) => Ok(value.to_string()),
                _ => Err(format!(
                    "invalid `{}`: managed file `{name}` must be a string",
                    path.display()
                )),
            }
        };
        let managed_path = field_string("path")?;
        validate_relative(&managed_path)?;
        ensure_plain_managed_parent(root, Path::new(&managed_path))?;
        if !seen.insert(managed_path.clone()) {
            return Err(format!(
                "invalid `{}`: duplicate managed file",
                path.display()
            ));
        }
        let expected_hash = field_string("sha256")?;
        if expected_hash.len() != 64
            || !expected_hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(format!("invalid `{}`: malformed sha256", path.display()));
        }
        let expected_bytes =
            match fields.get("bytes") {
                Some(JsonValue::Number(value)) => u64::try_from(value.int.ok_or_else(|| {
                    format!("invalid `{}`: bytes must be integral", path.display())
                })?)
                .map_err(|_| format!("invalid `{}`: bytes out of range", path.display()))?,
                _ => {
                    return Err(format!(
                        "invalid `{}`: bytes must be a number",
                        path.display()
                    ));
                }
            };
        #[cfg_attr(not(unix), allow(unused_variables))]
        let expected_executable = match fields.get("executable") {
            Some(JsonValue::Bool(value)) => *value,
            _ => {
                return Err(format!(
                    "invalid `{}`: executable must be a boolean",
                    path.display()
                ));
            }
        };
        let bytes = fs::read(root.join(&managed_path)).map_err(|e| {
            format!(
                "managed artifact `{}` does not match its manifest: {e}",
                managed_path
            )
        })?;
        if bytes.len() as u64 != expected_bytes || sha256_hex(&bytes) != expected_hash {
            return Err(format!(
                "managed artifact `{managed_path}` does not match its recorded identity"
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let actual_executable = fs::metadata(root.join(&managed_path))
                .map_err(|e| format!("cannot inspect managed artifact `{managed_path}`: {e}"))?
                .permissions()
                .mode()
                & 0o111
                != 0;
            if expected_executable && !actual_executable {
                return Err(format!(
                    "managed artifact `{managed_path}` is not executable as required by its manifest"
                ));
            }
        }
        managed_files.push(managed_path);
    }
    Ok(ParsedManifest {
        target,
        managed_files,
    })
}

fn validate_compiler_provenance(path: &Path, value: &JsonValue) -> Result<(), String> {
    let JsonValue::Object(fields) = value else {
        return Err(format!(
            "invalid `{}`: compiler provenance must be an object",
            path.display()
        ));
    };
    let predecessor = [
        "selector",
        "producer",
        "compilerSourceSetId",
        "targetSourceSetId",
        "compileProductId",
        "generatedSourceSha256",
        "targetCompilerFallback",
    ];
    let current = [
        "selector",
        "producer",
        "selectionOrigin",
        "compilerSourceSetId",
        "targetSourceSetId",
        "compileProductId",
        "generatedSourceSha256",
        "targetCompilerFallback",
    ];
    let exact_predecessor = fields.len() == predecessor.len()
        && predecessor.iter().all(|name| fields.contains_key(*name));
    let exact_current =
        fields.len() == current.len() && current.iter().all(|name| fields.contains_key(*name));
    if !exact_predecessor && !exact_current {
        return Err(format!(
            "invalid `{}`: compiler provenance has unknown or missing field",
            path.display()
        ));
    }
    let string = |name: &str| -> Result<&str, String> {
        match fields.get(name) {
            Some(JsonValue::String(value)) => Ok(value),
            _ => Err(format!(
                "invalid `{}`: compiler provenance `{name}` must be a string",
                path.display()
            )),
        }
    };
    if !matches!(string("selector")?, "rust" | "self")
        || string("producer")?.is_empty()
        || fields.get("selectionOrigin").is_some_and(|_| {
            !matches!(
                string("selectionOrigin"),
                Ok("explicit" | "current-default" | "compatibility")
            )
        })
        || !valid_sha256_identity(string("compilerSourceSetId")?)
        || !valid_sha256_identity(string("targetSourceSetId")?)
        || !valid_sha256_identity(string("compileProductId")?)
        || !valid_sha256_identity(string("generatedSourceSha256")?)
        || !matches!(
            fields.get("targetCompilerFallback"),
            Some(JsonValue::Bool(false))
        )
    {
        return Err(format!(
            "invalid `{}`: compiler provenance identity is invalid",
            path.display()
        ));
    }
    Ok(())
}

fn valid_sha256_identity(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn render_manifest(plan: &Plan) -> String {
    let requirements = plan
        .runtime_requirements
        .iter()
        .map(|value| json_string(value))
        .collect::<Vec<_>>()
        .join(", ");
    let files = plan
        .files
        .iter()
        .map(|file| {
            format!(
                "{{\"path\": {}, \"sha256\": {}, \"bytes\": {}, \"executable\": {}}}",
                json_string(&file.path),
                json_string(&sha256_hex(&file.bytes)),
                file.bytes.len(),
                file.executable
            )
        })
        .collect::<Vec<_>>()
        .join(",\n    ");
    let compiler = plan.compiler.as_ref().map_or_else(String::new, |compiler| {
        format!(
            ",\n  \"compiler\": {{\"selector\": {}, \"producer\": {}, \"selectionOrigin\": {}, \"compilerSourceSetId\": {}, \"targetSourceSetId\": {}, \"compileProductId\": {}, \"generatedSourceSha256\": {}, \"targetCompilerFallback\": {}}}",
            json_string(&compiler.selector),
            json_string(&compiler.producer),
            json_string(&compiler.selection_origin),
            json_string(&compiler.compiler_source_set_id),
            json_string(&compiler.target_source_set_id),
            json_string(&compiler.compile_product_id),
            json_string(&compiler.generated_source_sha256),
            compiler.target_compiler_fallback,
        )
    });
    format!(
        "{{\n  \"schema\": \"topaz.artifact.v1\",\n  \"toolchainVersion\": {},\n  \"languageMode\": \"topaz-{}\",\n  \"target\": {},\n  \"entry\": {},\n  \"runtimeRequirements\": [{}],\n  \"managedFiles\": [\n    {}\n  ],\n  \"invocation\": {}{}\n}}\n",
        json_string(env!("CARGO_PKG_VERSION")),
        plan.language_version.as_str(),
        json_string(plan.target.label()),
        json_string(&plan.entry),
        requirements,
        files,
        json_string(&plan.invocation),
        compiler,
    )
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = topaz_value::value::sha256(bytes);
    let mut out = String::with_capacity(64);
    topaz_value::bytes_to_hex_into(&mut out, &digest);
    out
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c <= '\u{1f}' => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn legacy_topaz_output(root: &Path, target: Target) -> Result<Vec<PathBuf>, String> {
    match target {
        Target::Native => legacy_native(root),
        Target::Python => legacy_python(root),
        Target::Web | Target::WebWorker | Target::WebApp => legacy_web(root),
        Target::HttpService => Ok(Vec::new()),
    }
}

fn legacy_native(root: &Path) -> Result<Vec<PathBuf>, String> {
    let cargo = root.join("Cargo.toml");
    if !cargo.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&cargo)
        .map_err(|e| format!("cannot inspect legacy `{}`: {e}", cargo.display()))?;
    if !text.contains("name = \"topaz-emitted\"") || !text.contains("vendor/crates/topaz_rt") {
        return Ok(Vec::new());
    }
    Ok([
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "src",
        "vendor",
        "target",
    ]
    .into_iter()
    .map(|p| root.join(p))
    .collect())
}

fn legacy_python(root: &Path) -> Result<Vec<PathBuf>, String> {
    let program = root.join("program.py");
    let runtime = root.join("topaz_py_rt.py");
    if !program.exists() || !runtime.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&program)
        .map_err(|e| format!("cannot inspect legacy `{}`: {e}", program.display()))?;
    if !text.starts_with("# Topaz Python backend parity artifact.")
        && !text.starts_with("# Generated Topaz Python application artifact.")
    {
        return Ok(Vec::new());
    }
    let mut paths = vec![program, runtime];
    let cache = root.join("__pycache__");
    if fs::symlink_metadata(&cache)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
    {
        for entry in fs::read_dir(&cache)
            .map_err(|e| format!("cannot inspect legacy `{}`: {e}", cache.display()))?
        {
            let entry = entry.map_err(|e| format!("cannot inspect legacy cache: {e}"))?;
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };
            if (name.starts_with("program.") || name.starts_with("topaz_py_rt."))
                && name.ends_with(".pyc")
                && entry.file_type().is_ok_and(|kind| kind.is_file())
            {
                paths.push(entry.path());
            }
        }
    }
    Ok(paths)
}

const LEGACY_WEB_LOADER_SIGNATURES: &[&str] = &[
    "export async function instantiateTopaz(",
    "wasm.topaz_call_export_json",
    "wasm.topaz_export_names_json",
];

fn legacy_web(root: &Path) -> Result<Vec<PathBuf>, String> {
    let loader = root.join("topaz-web.js");
    let wasm = root.join("topaz-web.wasm");
    if !loader.exists() || !wasm.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&loader)
        .map_err(|e| format!("cannot inspect legacy `{}`: {e}", loader.display()))?;
    if !LEGACY_WEB_LOADER_SIGNATURES
        .iter()
        .all(|signature| text.contains(signature))
    {
        return Ok(Vec::new());
    }
    Ok([
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "src",
        "vendor",
        "target",
        "topaz-web.js",
        "topaz-web.d.ts",
        "topaz-web.wasm",
        "topaz-web-worker.js",
        "topaz-web-worker-client.js",
    ]
    .into_iter()
    .map(|p| root.join(p))
    .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "topaz-artifact-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn plan(target: Target) -> Plan {
        Plan {
            target,
            language_version: LangVersion::CURRENT,
            entry: "main.tpz".into(),
            runtime_requirements: vec![],
            invocation: "./program".into(),
            compiler: None,
            files: vec![File::text("program.txt", "ok")],
        }
    }

    fn provenance(selector: &str) -> CompilerProvenance {
        CompilerProvenance {
            selector: selector.to_string(),
            producer: if selector == "self" {
                "topaz-stage2".to_string()
            } else {
                "rust-stage0".to_string()
            },
            selection_origin: "explicit".to_string(),
            compiler_source_set_id: format!("sha256:{}", "1".repeat(64)),
            target_source_set_id: format!("sha256:{}", "2".repeat(64)),
            compile_product_id: format!("sha256:{}", "3".repeat(64)),
            generated_source_sha256: format!("sha256:{}", "4".repeat(64)),
            target_compiler_fallback: false,
        }
    }

    #[test]
    fn compiler_provenance_is_validated_and_foreign_engine_manifest_is_replaced() {
        let root = temp_dir("compiler-provenance");
        let mut self_plan = plan(Target::Native);
        self_plan.compiler = Some(provenance("self"));
        Destination::open(&root, Target::Native)
            .unwrap()
            .commit(self_plan)
            .unwrap();
        let first = fs::read_to_string(root.join(MANIFEST_NAME)).unwrap();
        assert!(first.contains("\"selector\": \"self\""), "{first}");
        assert!(
            first.contains("\"selectionOrigin\": \"explicit\""),
            "{first}"
        );
        assert!(
            first.contains("\"targetCompilerFallback\": false"),
            "{first}"
        );
        let predecessor = first.replace(", \"selectionOrigin\": \"explicit\"", "");
        fs::write(root.join(MANIFEST_NAME), predecessor).unwrap();

        let mut rust_plan = plan(Target::Native);
        rust_plan.compiler = Some(provenance("rust"));
        Destination::open(&root, Target::Native)
            .unwrap()
            .commit(rust_plan)
            .unwrap();
        let second = fs::read_to_string(root.join(MANIFEST_NAME)).unwrap();
        assert!(second.contains("\"selector\": \"rust\""), "{second}");
        assert!(!second.contains("\"selector\": \"self\""), "{second}");

        let invalid = second.replace(
            "\"targetCompilerFallback\": false",
            "\"targetCompilerFallback\": true",
        );
        fs::write(root.join(MANIFEST_NAME), invalid).unwrap();
        let error = Destination::open(&root, Target::Native).unwrap_err();
        assert!(error.contains("compiler provenance identity"), "{error}");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_is_written_last_and_target_switch_is_rejected() {
        let root = temp_dir("switch");
        Destination::open(&root, Target::Native)
            .unwrap()
            .commit(plan(Target::Native))
            .unwrap();
        assert!(root.join(MANIFEST_NAME).is_file());
        assert!(Destination::open(&root, Target::Python).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn managed_http_service_artifact_can_be_reopened_for_rebuild() {
        let root = temp_dir("http-service-rebuild");
        Destination::open(&root, Target::HttpService)
            .unwrap()
            .commit(plan(Target::HttpService))
            .unwrap();
        Destination::open(&root, Target::HttpService)
            .expect("same-target service rebuild remains managed");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_records_the_plan_language_version() {
        let root = temp_dir("language-version");
        let mut selected = plan(Target::Python);
        selected.language_version = LangVersion::V5_7;
        Destination::open(&root, Target::Python)
            .unwrap()
            .commit(selected)
            .unwrap();
        let manifest = fs::read_to_string(root.join(MANIFEST_NAME)).unwrap();
        assert!(manifest.contains("\"languageMode\": \"topaz-5.7\""));
        Destination::open(&root, Target::Python).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_records_compatible_v510_language_version() {
        let root = temp_dir("current-language-version");
        let mut selected = plan(Target::Python);
        selected.language_version = LangVersion::V5_10;
        Destination::open(&root, Target::Python)
            .unwrap()
            .commit(selected)
            .unwrap();
        let manifest = fs::read_to_string(root.join(MANIFEST_NAME)).unwrap();
        assert!(manifest.contains("\"languageMode\": \"topaz-5.10\""));
        Destination::open(&root, Target::Python).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn managed_artifact_reader_accepts_compatible_v510_identity() {
        let root = temp_dir("current-v510-language-version");
        let mut selected = plan(Target::Python);
        selected.language_version = LangVersion::V5_10;
        Destination::open(&root, Target::Python)
            .unwrap()
            .commit(selected)
            .unwrap();
        Destination::open(&root, Target::Python).expect("current identity reopens");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_records_compatible_v511_language_version() {
        let root = temp_dir("compatible-v511-language-version");
        let mut selected = plan(Target::Python);
        selected.language_version = LangVersion::V5_11;
        Destination::open(&root, Target::Python)
            .unwrap()
            .commit(selected)
            .unwrap();
        let manifest = fs::read_to_string(root.join(MANIFEST_NAME)).unwrap();
        assert!(manifest.contains("\"languageMode\": \"topaz-5.11\""));
        Destination::open(&root, Target::Python).expect("compatible identity reopens");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_records_compatible_v512_language_version() {
        let root = temp_dir("compatible-v512-language-version");
        let mut selected = plan(Target::Python);
        selected.language_version = LangVersion::V5_12;
        Destination::open(&root, Target::Python)
            .unwrap()
            .commit(selected)
            .unwrap();
        let manifest = fs::read_to_string(root.join(MANIFEST_NAME)).unwrap();
        assert!(manifest.contains("\"languageMode\": \"topaz-5.12\""));
        Destination::open(&root, Target::Python).expect("compatible identity reopens");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_records_compatible_v513_language_version() {
        let root = temp_dir("compatible-v513-language-version");
        let mut selected = plan(Target::Python);
        selected.language_version = LangVersion::V5_13;
        Destination::open(&root, Target::Python)
            .unwrap()
            .commit(selected)
            .unwrap();
        let manifest = fs::read_to_string(root.join(MANIFEST_NAME)).unwrap();
        assert!(manifest.contains("\"languageMode\": \"topaz-5.13\""));
        Destination::open(&root, Target::Python).expect("compatible identity reopens");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_records_compatible_v517_language_version() {
        let root = temp_dir("compatible-v517-language-version");
        let mut selected = plan(Target::Python);
        selected.language_version = LangVersion::V5_17;
        Destination::open(&root, Target::Python)
            .unwrap()
            .commit(selected)
            .unwrap();
        let manifest = fs::read_to_string(root.join(MANIFEST_NAME)).unwrap();
        assert!(manifest.contains("\"languageMode\": \"topaz-5.17\""));
        Destination::open(&root, Target::Python).expect("compatible identity reopens");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn compatible_v518_plan_writes_an_artifact() {
        let root = temp_dir("compatible-v518-language-version");
        let mut selected = plan(Target::Python);
        selected.language_version = LangVersion::V5_18;
        Destination::open(&root, Target::Python)
            .unwrap()
            .commit(selected)
            .expect("compatible 5.18 artifact");
        let manifest = fs::read_to_string(root.join(MANIFEST_NAME)).unwrap();
        assert!(manifest.contains("\"languageMode\": \"topaz-5.18\""));
        assert!(root.join("program.txt").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn current_v520_plan_writes_an_artifact() {
        let root = temp_dir("current-v520-language-version");
        let mut selected = plan(Target::Python);
        selected.language_version = LangVersion::V5_20;
        Destination::open(&root, Target::Python)
            .unwrap()
            .commit(selected)
            .expect("current 5.20 artifact");
        let manifest = fs::read_to_string(root.join(MANIFEST_NAME)).unwrap();
        assert!(manifest.contains("\"languageMode\": \"topaz-5.20\""));
        assert!(root.join("program.txt").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn same_target_rebuild_preserves_unknown_files_and_removes_managed_stale() {
        let root = temp_dir("rebuild");
        let mut first = plan(Target::Native);
        first.files.push(File::text("stale.txt", "old"));
        Destination::open(&root, Target::Native)
            .unwrap()
            .commit(first)
            .unwrap();
        fs::write(root.join("user.txt"), "mine").unwrap();
        Destination::open(&root, Target::Native)
            .unwrap()
            .commit(plan(Target::Native))
            .unwrap();
        assert!(!root.join("stale.txt").exists());
        assert_eq!(fs::read_to_string(root.join("user.txt")).unwrap(), "mine");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_manifest_fails_closed() {
        let root = temp_dir("malformed");
        fs::write(root.join(MANIFEST_NAME), "{\"target\":\"native\"}").unwrap();
        assert!(Destination::open(&root, Target::Native).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_plan_leaves_no_completion_manifest() {
        let root = temp_dir("invalid-plan");
        let mut invalid = plan(Target::Python);
        invalid.files.push(File::text("../escape", "bad"));
        assert!(
            Destination::open(&root, Target::Python)
                .unwrap()
                .commit(invalid)
                .is_err()
        );
        assert!(!root.join(MANIFEST_NAME).exists());
        assert!(!root.parent().unwrap().join("escape").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_python_cleanup_preserves_unowned_cache_files() {
        let root = temp_dir("legacy-python");
        fs::write(
            root.join("program.py"),
            "# Topaz Python backend parity artifact.\n",
        )
        .unwrap();
        fs::write(root.join("topaz_py_rt.py"), "# runtime\n").unwrap();
        fs::create_dir(root.join("__pycache__")).unwrap();
        fs::write(root.join("__pycache__/program.cpython-313.pyc"), "owned").unwrap();
        fs::write(root.join("__pycache__/user.cpython-313.pyc"), "user").unwrap();
        let cleanup = legacy_python(&root).unwrap();
        assert!(cleanup.contains(&root.join("__pycache__/program.cpython-313.pyc")));
        assert!(!cleanup.contains(&root.join("__pycache__/user.cpython-313.pyc")));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn legacy_python_cleanup_preserves_a_linked_cache_and_outside_content() {
        use std::os::unix::fs::symlink;

        let root = temp_dir("legacy-python-linked-cache");
        let outside = temp_dir("legacy-python-linked-cache-outside");
        fs::write(
            root.join("program.py"),
            "# Topaz Python backend parity artifact.\n",
        )
        .unwrap();
        fs::write(root.join("topaz_py_rt.py"), "# runtime\n").unwrap();
        fs::write(outside.join("program.cpython-313.pyc"), "outside").unwrap();
        symlink(&outside, root.join("__pycache__")).unwrap();

        Destination::open(&root, Target::Python)
            .unwrap()
            .commit(plan(Target::Python))
            .unwrap();

        assert!(root.join("__pycache__").is_symlink());
        assert_eq!(
            fs::read_to_string(outside.join("program.cpython-313.pyc")).unwrap(),
            "outside"
        );
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn legacy_web_migration_preserves_lookalike_user_bundle() {
        let root = temp_dir("legacy-web-lookalike");
        fs::write(
            root.join("topaz-web.js"),
            "// User-maintained Topaz integration, not a generated loader.\n",
        )
        .unwrap();
        fs::write(root.join("topaz-web.wasm"), "user wasm").unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"user-web-project\"\n",
        )
        .unwrap();

        Destination::open(&root, Target::Web)
            .unwrap()
            .commit(plan(Target::Web))
            .unwrap();

        assert_eq!(
            fs::read_to_string(root.join("topaz-web.js")).unwrap(),
            "// User-maintained Topaz integration, not a generated loader.\n"
        );
        assert_eq!(fs::read(root.join("topaz-web.wasm")).unwrap(), b"user wasm");
        assert_eq!(
            fs::read_to_string(root.join("Cargo.toml")).unwrap(),
            "[package]\nname = \"user-web-project\"\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_web_migration_accepts_generated_loader_identity() {
        let root = temp_dir("legacy-web-generated");
        fs::write(
            root.join("topaz-web.js"),
            "export async function instantiateTopaz() {\n\
             wasm.topaz_call_export_json();\n\
             wasm.topaz_export_names_json();\n\
             }\n",
        )
        .unwrap();
        fs::write(root.join("topaz-web.wasm"), "generated wasm").unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"topaz-emitted\"\n",
        )
        .unwrap();

        Destination::open(&root, Target::Web)
            .unwrap()
            .commit(plan(Target::Web))
            .unwrap();

        assert!(!root.join("topaz-web.js").exists());
        assert!(!root.join("topaz-web.wasm").exists());
        assert!(!root.join("Cargo.toml").exists());
        assert!(root.join(MANIFEST_NAME).is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recorded_artifact_mutation_is_rejected() {
        let root = temp_dir("identity-mutation");
        Destination::open(&root, Target::Python)
            .unwrap()
            .commit(plan(Target::Python))
            .unwrap();
        fs::write(root.join("program.txt"), "mutated").unwrap();
        let error = Destination::open(&root, Target::Python).unwrap_err();
        assert!(error.contains("recorded identity"), "{error}");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn synthesized_executable_bit_on_non_runnable_file_is_tolerated() {
        use std::os::unix::fs::PermissionsExt;

        let root = temp_dir("synthetic-executable");
        Destination::open(&root, Target::Web)
            .unwrap()
            .commit(plan(Target::Web))
            .unwrap();
        let path = root.join("program.txt");
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(permissions.mode() | 0o111);
        fs::set_permissions(&path, permissions).unwrap();

        Destination::open(&root, Target::Web).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn required_executable_bit_is_still_enforced() {
        use std::os::unix::fs::PermissionsExt;

        let root = temp_dir("required-executable");
        let mut native = plan(Target::Native);
        native.files = vec![File::binary("program", b"native".to_vec(), true)];
        Destination::open(&root, Target::Native)
            .unwrap()
            .commit(native)
            .unwrap();
        let path = root.join("program");
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(permissions.mode() & !0o111);
        fs::set_permissions(&path, permissions).unwrap();

        let error = Destination::open(&root, Target::Native).unwrap_err();
        assert!(error.contains("not executable as required"), "{error}");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unowned_collision_is_preserved_and_rejected() {
        let root = temp_dir("unowned-collision");
        fs::write(root.join("program.txt"), "user").unwrap();
        let error = Destination::open(&root, Target::Python)
            .unwrap()
            .commit(plan(Target::Python))
            .unwrap_err();
        assert!(error.contains("not owned by Topaz"), "{error}");
        assert_eq!(
            fs::read_to_string(root.join("program.txt")).unwrap(),
            "user"
        );
        assert!(!root.join(MANIFEST_NAME).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn managed_parent_link_is_rejected_without_touching_outside_content() {
        use std::os::unix::fs::symlink;

        let root = temp_dir("managed-parent-link");
        let outside = temp_dir("managed-parent-link-outside");
        let mut nested = plan(Target::Python);
        nested.files = vec![File::text("nested/program.txt", "managed\n")];
        Destination::open(&root, Target::Python)
            .unwrap()
            .commit(nested)
            .unwrap();

        fs::remove_file(root.join("nested/program.txt")).unwrap();
        fs::remove_dir(root.join("nested")).unwrap();
        fs::write(outside.join("program.txt"), "managed\n").unwrap();
        symlink(&outside, root.join("nested")).unwrap();

        let error = Destination::open(&root, Target::Python).unwrap_err();
        assert!(error.contains("not a plain directory"), "{error}");
        assert_eq!(
            fs::read_to_string(outside.join("program.txt")).unwrap(),
            "managed\n"
        );
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn nested_managed_files_may_share_a_new_ancestor_directory() {
        let root = temp_dir("nested-managed-files");
        let mut native = plan(Target::Native);
        native.files = vec![
            File::binary("lispex/component/runtime.bin", b"runtime".to_vec(), false),
            File::text("lispex/RUNTIME-NOTICE.txt", "notice\n"),
        ];
        Destination::open(&root, Target::Native)
            .unwrap()
            .commit(native)
            .unwrap();
        assert_eq!(
            fs::read(root.join("lispex/component/runtime.bin")).unwrap(),
            b"runtime"
        );
        assert_eq!(
            fs::read_to_string(root.join("lispex/RUNTIME-NOTICE.txt")).unwrap(),
            "notice\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_native_cleanup_runs_before_final_install() {
        let root = temp_dir("legacy-native");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"topaz-emitted\"\n# vendor/crates/topaz_rt\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("target/debug/deps")).unwrap();
        fs::write(root.join("target/debug/deps/old.rlib"), "large").unwrap();
        let final_path = "target/debug/program";
        let mut native = plan(Target::Native);
        native.files = vec![File::binary(final_path, b"new".to_vec(), true)];
        Destination::open(&root, Target::Native)
            .unwrap()
            .commit(native)
            .unwrap();
        assert_eq!(fs::read(root.join(final_path)).unwrap(), b"new");
        assert!(!root.join("target/debug/deps").exists());
        assert!(root.join(MANIFEST_NAME).is_file());
        fs::remove_dir_all(root).unwrap();
    }
}
