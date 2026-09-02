use crate::*;

#[derive(Debug, Clone, PartialEq, Eq)]
/// Compiler routes consume this typed form; [`Project`] retains the exact TOML beside it.
pub struct PackageManifest {
    pub package: PackageSection,
    pub build: BuildConfig,
    pub dependencies: BTreeMap<String, Dependency>,
    pub capabilities: Capabilities,
    pub externs: BTreeMap<String, ExternModule>,
    pub exports: Option<Exports>,
    pub web: WebConfig,
    pub service: ServiceConfig,
    pub lispex: Option<LispexConfig>,
}

pub const LISPEX_BOUNDED_PROFILE_ID: &str = "lispex/r7rs-rule-embedded-core/1";
pub const LISPEX_APPLICATION_PROFILE_ID: &str = "topaz/lispex-decision-application/1";
pub const LISPEX_APPLICATION_LANGUAGE: LangVersion = LangVersion::V5_18;
pub const LISPEX_APPLICATION_STD_VERSION: &str = "5.18";
pub const LISPEX_COMPLETE_PROFILE_ID: &str = "lispex/r7rs-rule-current-profile-bounded/1";
pub const LISPEX_COMPLETE_APPLICATION_PROFILE_ID: &str = "topaz/lispex-decision-application/2";
pub const LISPEX_COMPLETE_APPLICATION_LANGUAGE: LangVersion = LangVersion::V5_20;
pub const LISPEX_COMPLETE_APPLICATION_STD_VERSION: &str = "5.20";
pub(crate) const LISPEX_APPLICATION_EXPORTS: &[&str] = &[
    "canonicalBytes",
    "defaultLimits",
    "evaluate",
    "inspectRule",
    "valueFromCanonical",
];

#[derive(Debug, Clone, PartialEq, Eq)]
/// Evaluator selection is package-owned; runtime discovery and fallback are not represented.
pub struct LispexConfig {
    pub profile: String,
    pub application: Option<String>,
    pub application_quotas: Option<String>,
    pub rules: Vec<LispexRule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Preparation consumes the source under these limits before locking binds both.
pub struct LispexRule {
    pub name: String,
    pub source: String,
    pub limits: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Carries enough product identity to verify prepared rules without running preparation again.
pub struct LispexLock {
    pub profile: String,
    pub application: Option<String>,
    pub application_quotas: Option<String>,
    pub application_quotas_sha256: Option<String>,
    pub feature_set_sha256: String,
    pub component_id: String,
    pub component_manifest_sha256: String,
    pub evaluator_sha256: String,
    pub abi_id: String,
    pub value_codec_id: String,
    pub meter_model_id: String,
    pub artifact_contract_id: String,
    pub transcript_id: String,
    pub receipt_core_id: String,
    pub adapter_id: String,
    pub admission_sha256: String,
    pub target: String,
    pub target_disposition: String,
    pub handle_catalog_path: String,
    pub handle_catalog_sha256: String,
    pub rules: Vec<LispexLockRule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Binds source and limits to the artifact later verified without re-preparation.
pub struct LispexLockRule {
    pub name: String,
    pub source: String,
    pub source_sha256: String,
    pub limits: String,
    pub limits_sha256: String,
    pub preparation_request_sha256: String,
    pub preparation_submission_sha256: String,
    pub prepared_artifact_path: String,
    pub prepared_artifact_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Settles the language mode before the entry source is loaded.
pub struct PackageSection {
    pub name: String,
    pub version: String,
    pub language: LangVersion,
    pub entry: String,
    pub license: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Selected build target and deterministic-build requirement.
pub struct BuildConfig {
    pub target: String,
    pub deterministic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Keeps bundle inputs separate from browser lifecycle selection.
pub struct WebConfig {
    pub title: String,
    pub styles: Vec<String>,
    pub assets: Vec<String>,
    pub lifecycle: WebLifecycle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Service target policy; defaults remain loopback-only and bounded.
pub struct ServiceConfig {
    pub bind: String,
    pub port: u16,
    pub workers: u16,
    pub max_connections: u16,
    pub queue_capacity: u16,
    pub max_target_bytes: u32,
    pub max_header_bytes: u32,
    pub max_headers: u16,
    pub max_body_bytes: u32,
    pub header_timeout_ms: u32,
    pub body_timeout_ms: u32,
    pub handler_timeout_ms: u32,
    pub shutdown_grace_ms: u32,
    pub log_format: ServiceLogFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Wire logging mode accepted by the service target.
pub enum ServiceLogFormat {
    Text,
    Json,
    Off,
}

impl ServiceLogFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
            Self::Off => "off",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
/// Installed Web runtime lifecycle contract selected by a package.
pub enum WebLifecycle {
    #[default]
    V1,
    V2,
}

impl WebLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::V1 => "v1",
            Self::V2 => "v2",
        }
    }
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            title: "Topaz application".to_string(),
            styles: Vec::new(),
            assets: Vec::new(),
            lifecycle: WebLifecycle::V1,
        }
    }
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1".to_string(),
            port: 8080,
            workers: 1,
            max_connections: 64,
            queue_capacity: 32,
            max_target_bytes: 8_192,
            max_header_bytes: 16_384,
            max_headers: 64,
            max_body_bytes: 1_048_576,
            header_timeout_ms: 5_000,
            body_timeout_ms: 5_000,
            handler_timeout_ms: 1_000,
            shutdown_grace_ms: 5_000,
            log_format: ServiceLogFormat::Text,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Registry and local dependencies share this shape; `hash` carries a content pin.
pub struct Dependency {
    pub version: Option<String>,
    pub path: Option<String>,
    pub hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
/// Host adapters grant only the filesystem and browser authority represented here.
pub struct Capabilities {
    pub fs: FsCapabilities,
    pub web: WebCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
/// Paths remain logical; physical resolution belongs to the host boundary.
pub struct FsCapabilities {
    pub read: Vec<String>,
    pub write: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
/// The generated browser host consumes these bits; compiler phases do not.
pub struct WebCapabilities {
    pub open_text: bool,
    pub download_text: bool,
    pub local_state: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Names the module whose callable values become the package export ABI.
pub struct Exports {
    pub module: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Extern module contract, artifact, sandbox, and replay declaration.
pub struct ExternModule {
    pub hash: String,
    pub abi_hash: String,
    pub functions: Vec<ExternFunction>,
    pub artifact: Option<ExternArtifact>,
    pub sandbox: ExternSandbox,
    pub replay: ExternReplay,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Callable extern ABI signature.
pub struct ExternFunction {
    pub name: String,
    pub params: Vec<AbiType>,
    pub result: AbiType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Replay sandboxes obtain results only from this package-relative fixture.
pub struct ExternReplay {
    pub fixture: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Wasm sandboxes load their implementation from this package-relative path.
pub struct ExternArtifact {
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Execution boundary used for an extern implementation.
pub enum ExternSandboxKind {
    Replay,
    Wasm,
}

impl ExternSandboxKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ExternSandboxKind::Replay => "replay",
            ExternSandboxKind::Wasm => "wasm",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Keeps extern execution policy in manifest data rather than runtime discovery.
pub struct ExternSandbox {
    pub kind: ExternSandboxKind,
    pub fuel: Option<u64>,
    pub memory_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Closed value grammar allowed across the extern ABI.
pub enum AbiType {
    Unit,
    Bool,
    Int,
    Float,
    String,
    Bytes,
    Array(Box<AbiType>),
    Option(Box<AbiType>),
    Result(Box<AbiType>, Box<AbiType>),
}

impl AbiType {
    pub fn canonical(&self) -> String {
        match self {
            AbiType::Unit => "()".to_string(),
            AbiType::Bool => "bool".to_string(),
            AbiType::Int => "int".to_string(),
            AbiType::Float => "float".to_string(),
            AbiType::String => "string".to_string(),
            AbiType::Bytes => "Bytes".to_string(),
            AbiType::Array(item) => format!("Array<{}>", item.canonical()),
            AbiType::Option(item) => format!("Option<{}>", item.canonical()),
            AbiType::Result(ok, err) => format!("Result<{},{}>", ok.canonical(), err.canonical()),
        }
    }
}

impl fmt::Display for AbiType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Loaded package root together with its exact manifest text and typed form.
pub struct Project {
    pub root: PathBuf,
    pub manifest_text: String,
    pub manifest: PackageManifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Stable package-admission error rendered by CLI consumers.
pub struct PackageError {
    message: String,
}

impl PackageError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PackageError {}
