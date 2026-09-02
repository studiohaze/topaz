//! Stage 0 compiler-kernel facade.
//!
//! The ordinary host owns physical I/O. The kernel receives only explicit,
//! replayable source, directory, and containment facts and reruns from the
//! complete accumulated request until the import closure is complete.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

mod canonical;
mod observation;
mod build_provenance {
    include!(concat!(env!("OUT_DIR"), "/bootstrap_build_provenance.rs"));
}

pub use observation::{
    AST_SCHEMA, BUNDLE_SCHEMA, COMPARISON_SCHEMA, CanonicalPreviewAstAttribute,
    CanonicalPreviewAstNode, CanonicalPreviewAstValue, CanonicalPreviewCheckDiagnostic,
    CanonicalPreviewCheckLabel, CanonicalPreviewDiagnostic, CanonicalPreviewImportEdge,
    CanonicalPreviewModule, CanonicalPreviewResolvedDeclaration,
    CanonicalPreviewResolvedDiagnostic, CanonicalPreviewResolvedExport,
    CanonicalPreviewResolvedReference, CanonicalPreviewResolvedScope, CanonicalPreviewToken,
    ComparisonLayer, ComparisonRecord, CompilerPreviewCompletion, DIAGNOSTICS_SCHEMA,
    LOWERED_SCHEMA, ObservationBundle, ObservationFile, RESOLVED_SCHEMA, RUST_SOURCE_SCHEMA,
    ResolvedPreviewObservationInput, SOURCE_SET_SCHEMA, STAGE1_PRODUCT_SCHEMA,
    STAGE2_FIXED_POINT_SCHEMA, STAGE2_PRODUCT_SCHEMA, TOKENS_SCHEMA, TYPED_SCHEMA,
    TypedPreviewObservationInput, build_ast_preview_observation, build_observation,
    build_resolved_preview_observation, build_token_preview_observation,
    build_typed_preview_observation, canonical_token_kind, compare_native_binaries,
    compare_observations, complete_compiler_preview_observation, semantic_type_atoms_json,
};

use topaz_diag::has_errors;
use topaz_resolve::{FileProvider, ResolveOutput, normalize_path, resolve_with_version};
use topaz_syntax::LangVersion;

pub const REQUEST_SCHEMA: &str = "topaz.compiler.request/v1";
pub const RESPONSE_SCHEMA: &str = "topaz.compiler.response/v1";
pub const PROVENANCE_SCHEMA: &str = "topaz.compiler.bootstrap-provenance/v1";
pub const STAGE0_ENGINE: &str = "rust-stage0";

/// Build-time identity of the compiler sources embedded in this Stage 0 kernel.
pub fn compiler_source_set_id() -> &'static str {
    build_provenance::COMPILER_SOURCE_SET_ID
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Logical source root exposed to the fact-driven compiler kernel.
pub struct Mount {
    pub id: String,
    pub logical_root: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Whether compilation starts from a standalone entry or a package contract.
pub enum BuildRole {
    Standalone,
    Package,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Package decisions supplied to the kernel without granting physical I/O.
pub struct PackageFacts {
    pub identity: Option<String>,
    pub build_role: BuildRole,
    pub deterministic: bool,
    pub executable_profile: Option<String>,
    pub dependency_mount_ids: Vec<String>,
    pub extern_modules: BTreeSet<String>,
    pub extern_replay_errors: BTreeMap<String, String>,
    pub generated_std_modules: BTreeMap<String, topaz_resolve::GeneratedStdModule>,
    pub capabilities: BTreeSet<String>,
    pub locked: bool,
}

impl PackageFacts {
    pub fn standalone() -> Self {
        Self {
            identity: None,
            build_role: BuildRole::Standalone,
            deterministic: true,
            executable_profile: None,
            dependency_mount_ids: Vec::new(),
            extern_modules: BTreeSet::new(),
            extern_replay_errors: BTreeMap::new(),
            generated_std_modules: BTreeMap::new(),
            capabilities: BTreeSet::new(),
            locked: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
/// Last compiler projection requested by a kernel invocation.
pub enum TerminalPhase {
    Tokens,
    Ast,
    Resolved,
    Typed,
    Lowered,
    RustSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Limits are request data, allowing the same fact set to replay under the same ceilings.
pub struct ResourceBudgets {
    pub max_source_facts: u64,
    pub max_total_source_bytes: u64,
    pub max_raw_tokens: u64,
    pub max_layout_tokens: u64,
    pub max_ast_nodes: u64,
    pub max_hir_nodes: u64,
    pub max_lowered_nodes: u64,
    pub max_diagnostics: u64,
    pub max_projection_bytes: u64,
    pub max_generated_rust_bytes: u64,
}

impl Default for ResourceBudgets {
    fn default() -> Self {
        Self {
            max_source_facts: 16_384,
            max_total_source_bytes: 64 * 1024 * 1024,
            max_raw_tokens: 4_000_000,
            max_layout_tokens: 4_000_000,
            max_ast_nodes: 2_000_000,
            max_hir_nodes: 2_000_000,
            max_lowered_nodes: 4_000_000,
            max_diagnostics: 100_000,
            max_projection_bytes: 512 * 1024 * 1024,
            max_generated_rust_bytes: 512 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
/// Deterministic question the kernel asks its host about a logical path.
pub enum HostQuery {
    ReadSource {
        mount_id: String,
        logical_path: String,
    },
    ListDirectory {
        mount_id: String,
        logical_path: String,
    },
    PhysicalContainment {
        mount_id: String,
        logical_path: String,
    },
}

impl HostQuery {
    pub fn mount_id(&self) -> &str {
        match self {
            Self::ReadSource { mount_id, .. }
            | Self::ListDirectory { mount_id, .. }
            | Self::PhysicalContainment { mount_id, .. } => mount_id,
        }
    }

    pub fn logical_path(&self) -> &str {
        match self {
            Self::ReadSource { logical_path, .. }
            | Self::ListDirectory { logical_path, .. }
            | Self::PhysicalContainment { logical_path, .. } => logical_path,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Missing, unreadable, and invalid UTF-8 remain distinct resolver inputs.
pub enum SourceFact {
    Present(String),
    Missing,
    Unreadable { reason_code: String },
    InvalidUtf8,
}

impl From<topaz_resolve::SourceRead> for SourceFact {
    fn from(read: topaz_resolve::SourceRead) -> Self {
        match read {
            topaz_resolve::SourceRead::Present(source) => Self::Present(source),
            topaz_resolve::SourceRead::Missing => Self::Missing,
            topaz_resolve::SourceRead::Unreadable { reason_code } => {
                Self::Unreadable { reason_code }
            }
            topaz_resolve::SourceRead::InvalidUtf8 => Self::InvalidUtf8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
/// Portable kind of a directory entry supplied as a host fact.
pub enum DirectoryEntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
/// Name and kind projected from one logical directory listing.
pub struct DirectoryEntry {
    pub name: String,
    pub kind: DirectoryEntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Directory failure remains data so replay never consults ambient storage.
pub enum DirectoryFact {
    Present(Vec<DirectoryEntry>),
    Missing,
    Unreadable { reason_code: String },
}

impl From<topaz_resolve::DirectoryRead> for DirectoryFact {
    fn from(read: topaz_resolve::DirectoryRead) -> Self {
        match read {
            topaz_resolve::DirectoryRead::Present(entries) => Self::Present(
                entries
                    .into_iter()
                    .map(|(name, is_dir)| DirectoryEntry {
                        name,
                        kind: if is_dir {
                            DirectoryEntryKind::Directory
                        } else {
                            DirectoryEntryKind::File
                        },
                    })
                    .collect(),
            ),
            topaz_resolve::DirectoryRead::Missing => Self::Missing,
            topaz_resolve::DirectoryRead::Unreadable { reason_code } => {
                Self::Unreadable { reason_code }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Preserves containment without exposing an absolute host path to the kernel.
pub enum ContainmentFact {
    Inside { alias_class: String },
    Outside,
    Missing,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Typed answer corresponding to a [`HostQuery`] variant.
pub enum HostFact {
    Source(SourceFact),
    Directory(DirectoryFact),
    Containment(ContainmentFact),
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Owns all facts needed to replay compilation without physical I/O.
pub struct KernelRequest {
    schema: &'static str,
    language_version: LangVersion,
    entry: String,
    mounts: Vec<Mount>,
    package: PackageFacts,
    terminal_phase: TerminalPhase,
    budgets: ResourceBudgets,
    requested_schemas: Vec<String>,
    facts: BTreeMap<HostQuery, HostFact>,
}

impl KernelRequest {
    /// Creates a typed-phase request rooted at the normalized entry package.
    pub fn checked(
        entry: &str,
        root: Option<&str>,
        language_version: LangVersion,
        package: PackageFacts,
    ) -> Self {
        let entry = normalize_path(entry);
        let logical_root = root.map(normalize_path).unwrap_or_else(|| {
            entry
                .rsplit_once('/')
                .map_or_else(String::new, |(parent, _)| parent.to_string())
        });
        Self {
            schema: REQUEST_SCHEMA,
            language_version,
            entry,
            mounts: vec![Mount {
                id: "root".to_string(),
                logical_root,
            }],
            package,
            terminal_phase: TerminalPhase::Typed,
            budgets: ResourceBudgets::default(),
            requested_schemas: vec![RESPONSE_SCHEMA.to_string(), PROVENANCE_SCHEMA.to_string()],
            facts: BTreeMap::new(),
        }
    }

    pub fn schema(&self) -> &str {
        self.schema
    }

    pub fn language_version(&self) -> LangVersion {
        self.language_version
    }

    pub fn entry(&self) -> &str {
        &self.entry
    }

    pub fn mounts(&self) -> &[Mount] {
        &self.mounts
    }

    pub fn package(&self) -> &PackageFacts {
        &self.package
    }

    pub fn terminal_phase(&self) -> TerminalPhase {
        self.terminal_phase
    }

    pub fn budgets(&self) -> &ResourceBudgets {
        &self.budgets
    }

    pub fn budgets_mut(&mut self) -> &mut ResourceBudgets {
        &mut self.budgets
    }

    pub fn with_terminal_phase(mut self, terminal_phase: TerminalPhase) -> Self {
        self.terminal_phase = terminal_phase;
        self
    }

    pub fn requested_schemas(&self) -> &[String] {
        &self.requested_schemas
    }

    pub fn facts(&self) -> &BTreeMap<HostQuery, HostFact> {
        &self.facts
    }

    fn root_mount(&self) -> &Mount {
        &self.mounts[0]
    }

    /// Adds one type-matched answer exactly once to the replayable request.
    pub fn supply_fact(&mut self, query: HostQuery, fact: HostFact) -> Result<(), FactError> {
        if !fact_matches_query(&query, &fact) {
            return Err(FactError::KindMismatch);
        }
        if self.facts.contains_key(&query) {
            return Err(FactError::Duplicate);
        }
        self.facts.insert(query, fact);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Rejection raised while adding a host answer to a kernel request.
pub enum FactError {
    KindMismatch,
    Duplicate,
}

fn fact_matches_query(query: &HostQuery, fact: &HostFact) -> bool {
    matches!(
        (query, fact),
        (HostQuery::ReadSource { .. }, HostFact::Source(_))
            | (HostQuery::ListDirectory { .. }, HostFact::Directory(_))
            | (
                HostQuery::PhysicalContainment { .. },
                HostFact::Containment(_)
            )
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Compiler implementation recorded in bootstrap provenance.
pub enum CompilerRoute {
    CurrentKernel,
    RustCompatibility,
    RustUnchecked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Stage, engine, route, and fixed-point facts attached to compiler output.
pub struct BootstrapProvenance {
    pub schema: &'static str,
    pub product_version: &'static str,
    pub language_mode: String,
    pub engine: &'static str,
    pub producer_stage: u8,
    pub result_stage: u8,
    pub default_engine: &'static str,
    pub route: CompilerRoute,
    pub semantic_fixed_point: FixedPointStatus,
    pub generated_source_fixed_point: FixedPointStatus,
    pub native_binary_reproducibility: FixedPointStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Recorded disposition of one fixed-point or reproducibility comparison.
pub enum FixedPointStatus {
    NotRun,
    NotEstablished,
    Pass,
    Fail,
    NotApplicable,
}

impl BootstrapProvenance {
    /// Provenance for the current Rust Stage 0 kernel before later comparisons run.
    pub fn current_stage0() -> Self {
        Self {
            schema: PROVENANCE_SCHEMA,
            product_version: topaz_check::VERSION,
            language_mode: format!("topaz-{}", LangVersion::CURRENT.as_str()),
            engine: STAGE0_ENGINE,
            producer_stage: 0,
            result_stage: 0,
            default_engine: STAGE0_ENGINE,
            route: CompilerRoute::CurrentKernel,
            semantic_fixed_point: FixedPointStatus::NotRun,
            generated_source_fixed_point: FixedPointStatus::NotRun,
            native_binary_reproducibility: FixedPointStatus::NotRun,
        }
    }

    /// Provenance for the explicit Rust compatibility or unchecked route.
    pub fn compatibility(language_version: LangVersion, unchecked: bool) -> Self {
        Self {
            schema: PROVENANCE_SCHEMA,
            product_version: topaz_check::VERSION,
            language_mode: format!("topaz-{}", language_version.as_str()),
            engine: if unchecked {
                "rust-unchecked"
            } else {
                "rust-compat"
            },
            producer_stage: 0,
            result_stage: 0,
            default_engine: STAGE0_ENGINE,
            route: if unchecked {
                CompilerRoute::RustUnchecked
            } else {
                CompilerRoute::RustCompatibility
            },
            semantic_fixed_point: FixedPointStatus::NotApplicable,
            generated_source_fixed_point: FixedPointStatus::NotApplicable,
            native_binary_reproducibility: FixedPointStatus::NotRun,
        }
    }

    /// Provenance attached to the embedded Topaz front-end preview route.
    pub fn topaz_front_end_preview() -> Self {
        Self {
            schema: PROVENANCE_SCHEMA,
            product_version: topaz_check::VERSION,
            language_mode: format!("topaz-{}", LangVersion::CURRENT.as_str()),
            engine: "topaz-front-end-preview",
            producer_stage: 0,
            result_stage: 0,
            default_engine: STAGE0_ENGINE,
            route: CompilerRoute::CurrentKernel,
            semantic_fixed_point: FixedPointStatus::NotRun,
            generated_source_fixed_point: FixedPointStatus::NotRun,
            native_binary_reproducibility: FixedPointStatus::NotRun,
        }
    }

    /// Stage 1 producer and result identity before fixed-point comparison.
    pub fn topaz_stage1_preview() -> Self {
        Self {
            schema: PROVENANCE_SCHEMA,
            product_version: topaz_check::VERSION,
            language_mode: format!("topaz-{}", LangVersion::CURRENT.as_str()),
            engine: "topaz-stage1",
            producer_stage: 1,
            result_stage: 1,
            default_engine: STAGE0_ENGINE,
            route: CompilerRoute::CurrentKernel,
            semantic_fixed_point: FixedPointStatus::NotRun,
            generated_source_fixed_point: FixedPointStatus::NotRun,
            native_binary_reproducibility: FixedPointStatus::NotRun,
        }
    }

    /// Stage 2 producer and result identity for an ordinary preview.
    pub fn topaz_stage2_preview() -> Self {
        Self {
            schema: PROVENANCE_SCHEMA,
            product_version: topaz_check::VERSION,
            language_mode: format!("topaz-{}", LangVersion::CURRENT.as_str()),
            engine: "topaz-stage2",
            producer_stage: 2,
            result_stage: 2,
            default_engine: STAGE0_ENGINE,
            route: CompilerRoute::CurrentKernel,
            semantic_fixed_point: FixedPointStatus::NotRun,
            generated_source_fixed_point: FixedPointStatus::NotRun,
            native_binary_reproducibility: FixedPointStatus::NotRun,
        }
    }

    /// Stage 2 provenance with semantic and generated-source comparison pending.
    pub fn topaz_stage2_fixed_point() -> Self {
        Self {
            schema: PROVENANCE_SCHEMA,
            product_version: topaz_check::VERSION,
            language_mode: format!("topaz-{}", LangVersion::CURRENT.as_str()),
            engine: "topaz-stage2",
            producer_stage: 2,
            result_stage: 2,
            default_engine: STAGE0_ENGINE,
            route: CompilerRoute::CurrentKernel,
            semantic_fixed_point: FixedPointStatus::NotEstablished,
            generated_source_fixed_point: FixedPointStatus::NotEstablished,
            native_binary_reproducibility: FixedPointStatus::NotRun,
        }
    }
}

/// Compiler products accumulated through the requested terminal phase.
pub struct KernelUnit {
    pub resolved: ResolveOutput,
    pub checked: Option<topaz_check::CheckedUnit>,
    pub lowered: Option<topaz_hir::LoweredUnit>,
    pub rust_source: Option<String>,
    pub provenance: BootstrapProvenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Resource category reported when a kernel budget is exceeded.
pub enum ResourceDimension {
    SourceFacts,
    TotalSourceBytes,
    RawTokens,
    LayoutTokens,
    AstNodes,
    HirNodes,
    LoweredNodes,
    Diagnostics,
    ProjectionBytes,
    GeneratedRustBytes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Observed resource count and the request ceiling it crossed.
pub struct ResourceLimit {
    pub dimension: ResourceDimension,
    pub limit: u64,
    pub observed: u64,
}

/// Separates fact demand from terminal product, resource, and compiler failures.
pub enum KernelOutcome {
    NeedHostFacts(Vec<HostQuery>),
    Completed(Box<KernelUnit>),
    Rejected(Box<KernelUnit>),
    Declined { reason: &'static str },
    ResourceLimit(ResourceLimit),
    CompilerFault { message: String },
}

/// Final driven request, outcome, and number of fact-supply rounds.
pub struct KernelExecution {
    pub request: KernelRequest,
    pub outcome: KernelOutcome,
    pub rounds: u64,
}

fn enrich_typed_resolution_facts(resolved: &ResolveOutput, checked: &mut topaz_check::CheckedUnit) {
    let Some(typed) = checked.typed_hir.as_mut() else {
        return;
    };

    for call in &mut typed.calls {
        let mut targets = resolved
            .name_facts
            .references
            .iter()
            .filter(|reference| {
                reference.file == call.callee_span.file
                    && reference.span.lo >= call.callee_span.lo
                    && reference.span.hi <= call.callee_span.hi
                    && call
                        .plan
                        .admits_callee_reference(&reference.name, reference.span)
            })
            .take(2);
        let first_target = targets.next();
        let target = first_target.filter(|_| targets.next().is_none());
        if matches!(call.callee_type, topaz_hir::SemanticType::Unknown)
            && let Some(target) = target
            && let (Some(file), Some(span)) = (target.target_file, target.target_span)
            && let Some(node) = typed.nodes.iter().find(|node| {
                node.span.file == file
                    && node.span == span
                    && matches!(
                        node.kind,
                        topaz_hir::TypedNodeKind::Binding | topaz_hir::TypedNodeKind::Declaration
                    )
            })
        {
            call.callee_type = node.ty.clone();
        }
        if call.target_identity.is_none() {
            call.target_identity = target.map(|reference| {
                if let (Some(module), Some(name)) =
                    (&reference.target_module, &reference.target_name)
                {
                    format!("{module}::{name}")
                } else if let (Some(file), Some(span)) =
                    (reference.target_file, reference.target_span)
                {
                    format!("source:{}:{}:{}", file.0, span.lo, span.hi)
                } else {
                    format!("builtin::{}", reference.name)
                }
            });
        }
    }

    typed.captures = topaz_lower::derive_resolution_captures(resolved, typed);
}

/// Host boundary that answers explicit kernel queries without entering compilation.
pub trait HostFactSource {
    fn respond(&self, request: &KernelRequest, query: &HostQuery) -> HostFact;
}

#[cfg(test)]
struct ProviderHost<'a> {
    provider: &'a dyn FileProvider,
}

#[cfg(test)]
impl<'a> ProviderHost<'a> {
    fn new(provider: &'a dyn FileProvider) -> Self {
        Self { provider }
    }

    fn containment(&self, request: &KernelRequest, query: &HostQuery) -> ContainmentFact {
        let Some(mount) = request
            .mounts()
            .iter()
            .find(|mount| mount.id == query.mount_id())
        else {
            return ContainmentFact::Unresolved;
        };
        let logical_path = normalize_path(query.logical_path());
        let Some(physical) = self.provider.physical_id(&logical_path) else {
            return ContainmentFact::Missing;
        };
        let Some(root_physical) = self.provider.physical_id(&mount.logical_root) else {
            return ContainmentFact::Unresolved;
        };
        let physical = physical.replace('\\', "/");
        let root_physical = root_physical.replace('\\', "/");
        if physical.split('/').any(|segment| segment == "..") {
            return ContainmentFact::Outside;
        }
        let inside = root_physical.is_empty()
            || physical == root_physical
            || physical.starts_with(&format!("{root_physical}/"));
        if !inside {
            return ContainmentFact::Outside;
        }
        let relative_target = if root_physical.is_empty() {
            logical_path
                .strip_prefix(&mount.logical_root)
                .unwrap_or(&logical_path)
                .trim_start_matches('/')
                .to_string()
        } else {
            physical
                .strip_prefix(&root_physical)
                .unwrap_or(&physical)
                .trim_start_matches('/')
                .to_string()
        };
        let mount_group = request
            .mounts()
            .iter()
            .filter(|candidate| {
                self.provider
                    .physical_id(&candidate.logical_root)
                    .is_some_and(|value| value.replace('\\', "/") == root_physical)
            })
            .map(|candidate| candidate.id.as_str())
            .min()
            .unwrap_or(mount.id.as_str());
        ContainmentFact::Inside {
            alias_class: physical_alias_class(mount_group, &relative_target),
        }
    }
}

#[cfg(test)]
impl HostFactSource for ProviderHost<'_> {
    fn respond(&self, request: &KernelRequest, query: &HostQuery) -> HostFact {
        match query {
            HostQuery::ReadSource { logical_path, .. } => {
                HostFact::Source(self.provider.read(logical_path).into())
            }
            HostQuery::ListDirectory { logical_path, .. } => {
                HostFact::Directory(self.provider.read_directory(logical_path).into())
            }
            HostQuery::PhysicalContainment { .. } => {
                HostFact::Containment(self.containment(request, query))
            }
        }
    }
}

/// Derives a path-neutral identity for one physical target within a mount group.
pub fn physical_alias_class(mount_group: &str, relative_target: &str) -> String {
    let mut input = Vec::with_capacity(mount_group.len() + relative_target.len() + 1);
    input.extend_from_slice(mount_group.as_bytes());
    input.push(0);
    input.extend_from_slice(relative_target.as_bytes());
    let digest = topaz_value::value::sha256(&input);
    let mut output = String::from("sha256:");
    topaz_value::bytes_to_hex_into(&mut output, &digest);
    output
}

struct FactProvider<'a> {
    request: &'a KernelRequest,
    missing: RefCell<BTreeSet<HostQuery>>,
}

impl<'a> FactProvider<'a> {
    fn new(request: &'a KernelRequest) -> Self {
        Self {
            request,
            missing: RefCell::new(BTreeSet::new()),
        }
    }

    fn query(&self, query: HostQuery) -> Option<&HostFact> {
        if let Some(fact) = self.request.facts.get(&query) {
            return Some(fact);
        }
        self.missing.borrow_mut().insert(query);
        None
    }

    fn take_missing(&self) -> Vec<HostQuery> {
        std::mem::take(&mut *self.missing.borrow_mut())
            .into_iter()
            .collect()
    }

    fn source_query(&self, path: &str) -> HostQuery {
        HostQuery::ReadSource {
            mount_id: self.request.root_mount().id.clone(),
            logical_path: normalize_path(path),
        }
    }

    fn directory_query(&self, path: &str) -> HostQuery {
        HostQuery::ListDirectory {
            mount_id: self.request.root_mount().id.clone(),
            logical_path: normalize_path(path),
        }
    }

    fn containment_query(&self, path: &str) -> HostQuery {
        HostQuery::PhysicalContainment {
            mount_id: self.request.root_mount().id.clone(),
            logical_path: normalize_path(path),
        }
    }
}

impl FileProvider for FactProvider<'_> {
    fn read(&self, path: &str) -> topaz_resolve::SourceRead {
        match self.query(self.source_query(path)) {
            Some(HostFact::Source(SourceFact::Present(source))) => {
                topaz_resolve::SourceRead::Present(source.clone())
            }
            Some(HostFact::Source(SourceFact::Unreadable { reason_code })) => {
                topaz_resolve::SourceRead::Unreadable {
                    reason_code: reason_code.clone(),
                }
            }
            Some(HostFact::Source(SourceFact::InvalidUtf8)) => {
                topaz_resolve::SourceRead::InvalidUtf8
            }
            Some(HostFact::Source(SourceFact::Missing)) | None | Some(_) => {
                topaz_resolve::SourceRead::Missing
            }
        }
    }

    fn is_extern_file(&self, path: &str) -> bool {
        path.strip_suffix(".tpz")
            .map(|path| path.replace('/', "."))
            .is_some_and(|identity| self.request.package.extern_modules.contains(&identity))
    }

    fn is_extern_namespace(&self, identity: &str) -> bool {
        let root = identity.split('.').next().unwrap_or(identity);
        self.request
            .package
            .extern_modules
            .iter()
            .any(|module| module.split('.').next() == Some(root))
    }

    fn extern_replay_error(&self, identity: &str) -> Option<String> {
        self.request
            .package
            .extern_replay_errors
            .get(identity)
            .cloned()
    }

    fn generated_std_module(&self, identity: &str) -> Option<topaz_resolve::GeneratedStdModule> {
        self.request
            .package
            .generated_std_modules
            .get(identity)
            .cloned()
    }

    fn read_directory(&self, dir: &str) -> topaz_resolve::DirectoryRead {
        match self.query(self.directory_query(dir)) {
            Some(HostFact::Directory(DirectoryFact::Present(entries))) => {
                topaz_resolve::DirectoryRead::Present(
                    entries
                        .iter()
                        .map(|entry| {
                            (
                                entry.name.clone(),
                                entry.kind == DirectoryEntryKind::Directory,
                            )
                        })
                        .collect(),
                )
            }
            Some(HostFact::Directory(DirectoryFact::Unreadable { reason_code })) => {
                topaz_resolve::DirectoryRead::Unreadable {
                    reason_code: reason_code.clone(),
                }
            }
            Some(HostFact::Directory(DirectoryFact::Missing)) | None | Some(_) => {
                topaz_resolve::DirectoryRead::Missing
            }
        }
    }

    fn physical_id(&self, path: &str) -> Option<String> {
        let logical_path = normalize_path(path);
        if logical_path == self.request.root_mount().logical_root {
            return Some(format!("mount:{}", self.request.root_mount().id));
        }
        match self.query(self.containment_query(&logical_path))? {
            HostFact::Containment(ContainmentFact::Inside { alias_class }) => Some(format!(
                "mount:{}/{}",
                self.request.root_mount().id,
                alias_class
            )),
            HostFact::Containment(ContainmentFact::Outside) => {
                Some(format!("outside:{logical_path}"))
            }
            HostFact::Containment(ContainmentFact::Missing | ContainmentFact::Unresolved) => None,
            _ => None,
        }
    }
}

/// Advances a complete request once, returning missing facts or a terminal outcome.
pub fn execute(request: &KernelRequest) -> KernelOutcome {
    if !request.language_version.uses_self_hosted_product_default() {
        return KernelOutcome::Declined {
            reason: "current compiler kernel accepts only admitted self-hosted-default language modes",
        };
    }
    if request.terminal_phase < TerminalPhase::Resolved {
        return KernelOutcome::Declined {
            reason: "the current compiler kernel completes from resolved through generated Rust",
        };
    }
    if let Some(limit) = fact_budget_limit(request) {
        return KernelOutcome::ResourceLimit(limit);
    }

    let provider = FactProvider::new(request);
    let root = &request.root_mount().logical_root;
    let resolved = resolve_with_version(
        &provider,
        request.entry(),
        Some(root.as_str()),
        request.language_version,
    );
    let missing = provider.take_missing();
    if !missing.is_empty() {
        return KernelOutcome::NeedHostFacts(missing);
    }

    let provisional = KernelUnit {
        resolved,
        checked: None,
        lowered: None,
        rust_source: None,
        provenance: BootstrapProvenance::current_stage0(),
    };
    let (raw_tokens, layout_tokens, ast_nodes) = observation::front_end_counts(&provisional);
    for (dimension, observed, limit) in [
        (
            ResourceDimension::RawTokens,
            raw_tokens,
            request.budgets.max_raw_tokens,
        ),
        (
            ResourceDimension::LayoutTokens,
            layout_tokens,
            request.budgets.max_layout_tokens,
        ),
        (
            ResourceDimension::AstNodes,
            ast_nodes,
            request.budgets.max_ast_nodes,
        ),
    ] {
        if observed > limit {
            return KernelOutcome::ResourceLimit(ResourceLimit {
                dimension,
                limit,
                observed,
            });
        }
    }
    let resolved = provisional.resolved;
    let mut checked =
        if has_errors(&resolved.diagnostics) || request.terminal_phase < TerminalPhase::Typed {
            None
        } else {
            let modules = resolved
                .modules
                .iter()
                .map(|module| topaz_check::UnitModule {
                    identity: module.identity.clone(),
                    is_entry: module.is_entry,
                    is_extern: module.is_extern,
                    is_generated_std: module.is_generated_std,
                    extern_replay_error: module.extern_replay_error.clone(),
                    src: resolved.map.file(module.file).src(),
                    program: &module.program,
                })
                .collect::<Vec<_>>();
            Some(topaz_check::check_unit_typed_with_version(
                &modules,
                request.language_version,
            ))
        };
    if let Some(checked) = checked.as_mut() {
        enrich_typed_resolution_facts(&resolved, checked);
    }

    let diagnostic_count = resolved.diagnostics.len() as u64
        + checked
            .as_ref()
            .map_or(0, |output| output.diagnostics.len() as u64);
    if diagnostic_count > request.budgets.max_diagnostics {
        return KernelOutcome::ResourceLimit(ResourceLimit {
            dimension: ResourceDimension::Diagnostics,
            limit: request.budgets.max_diagnostics,
            observed: diagnostic_count,
        });
    }
    let rejected = has_errors(&resolved.diagnostics)
        || checked
            .as_ref()
            .is_some_and(|output| has_errors(&output.diagnostics));
    if let Some(typed) = checked
        .as_ref()
        .and_then(|checked| checked.typed_hir.as_ref())
        && typed.nodes.len() as u64 > request.budgets.max_hir_nodes
    {
        return KernelOutcome::ResourceLimit(ResourceLimit {
            dimension: ResourceDimension::HirNodes,
            limit: request.budgets.max_hir_nodes,
            observed: typed.nodes.len() as u64,
        });
    }
    let mut lowered = None;
    let mut rust_source = None;
    if !rejected
        && request.terminal_phase >= TerminalPhase::Lowered
        && let Some(checked_unit) = checked.as_ref()
    {
        let result = match topaz_lower::lower_checked(&resolved, checked_unit) {
            Ok(result) => result,
            Err(error) => {
                return KernelOutcome::CompilerFault {
                    message: format!("checked Lowered IR construction failed: {error}"),
                };
            }
        };
        if result.operations.len() as u64 > request.budgets.max_lowered_nodes {
            return KernelOutcome::ResourceLimit(ResourceLimit {
                dimension: ResourceDimension::LoweredNodes,
                limit: request.budgets.max_lowered_nodes,
                observed: result.operations.len() as u64,
            });
        }
        if request.terminal_phase == TerminalPhase::RustSource {
            let generated = match topaz_emit::emit_module(&result) {
                Ok(generated) => generated,
                Err(error) => {
                    return KernelOutcome::CompilerFault {
                        message: format!("checked Rust emission failed: {error}"),
                    };
                }
            };
            if generated.len() as u64 > request.budgets.max_generated_rust_bytes {
                return KernelOutcome::ResourceLimit(ResourceLimit {
                    dimension: ResourceDimension::GeneratedRustBytes,
                    limit: request.budgets.max_generated_rust_bytes,
                    observed: generated.len() as u64,
                });
            }
            rust_source = Some(generated);
        }
        lowered = Some(result);
    }
    let unit = Box::new(KernelUnit {
        resolved,
        checked,
        lowered,
        rust_source,
        provenance: BootstrapProvenance::current_stage0(),
    });
    if rejected {
        KernelOutcome::Rejected(unit)
    } else {
        KernelOutcome::Completed(unit)
    }
}

fn fact_budget_limit(request: &KernelRequest) -> Option<ResourceLimit> {
    let source_facts = request
        .facts
        .values()
        .filter(|fact| matches!(fact, HostFact::Source(_)))
        .count() as u64;
    if source_facts > request.budgets.max_source_facts {
        return Some(ResourceLimit {
            dimension: ResourceDimension::SourceFacts,
            limit: request.budgets.max_source_facts,
            observed: source_facts,
        });
    }
    let source_bytes = request
        .facts
        .values()
        .filter_map(|fact| match fact {
            HostFact::Source(SourceFact::Present(source)) => Some(source.len() as u64),
            _ => None,
        })
        .sum::<u64>();
    (source_bytes > request.budgets.max_total_source_bytes).then_some(ResourceLimit {
        dimension: ResourceDimension::TotalSourceBytes,
        limit: request.budgets.max_total_source_bytes,
        observed: source_bytes,
    })
}

/// Replays the kernel until its fact requests are satisfied or execution terminates.
pub fn drive_checked(source: &dyn HostFactSource, mut request: KernelRequest) -> KernelExecution {
    let mut rounds = 0_u64;
    loop {
        rounds += 1;
        match execute(&request) {
            KernelOutcome::NeedHostFacts(queries) if queries.is_empty() => {
                return KernelExecution {
                    request,
                    outcome: KernelOutcome::CompilerFault {
                        message: "kernel requested an empty host-fact set".to_string(),
                    },
                    rounds,
                };
            }
            KernelOutcome::NeedHostFacts(queries) => {
                for query in queries {
                    let fact = source.respond(&request, &query);
                    if let Err(error) = request.supply_fact(query, fact) {
                        return KernelExecution {
                            request,
                            outcome: KernelOutcome::CompilerFault {
                                message: format!("invalid host fact: {error:?}"),
                            },
                            rounds,
                        };
                    }
                }
            }
            outcome => {
                return KernelExecution {
                    request,
                    outcome,
                    rounds,
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use topaz_resolve::InMemoryProvider;

    fn clean_unit(outcome: &KernelOutcome) -> &KernelUnit {
        match outcome {
            KernelOutcome::Completed(unit) => unit,
            KernelOutcome::Rejected(unit) => panic!(
                "expected completed kernel unit, got rejection: resolve={:?}, check={:?}",
                unit.resolved
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.message.as_str())
                    .collect::<Vec<_>>(),
                unit.checked.as_ref().map(|checked| {
                    checked
                        .diagnostics
                        .iter()
                        .map(|diagnostic| diagnostic.message.as_str())
                        .collect::<Vec<_>>()
                })
            ),
            KernelOutcome::NeedHostFacts(queries) => {
                panic!("expected completed kernel unit, still needs {queries:?}")
            }
            KernelOutcome::Declined { reason } => {
                panic!("expected completed kernel unit, got decline: {reason}")
            }
            KernelOutcome::ResourceLimit(limit) => {
                panic!("expected completed kernel unit, got limit: {limit:?}")
            }
            KernelOutcome::CompilerFault { message } => {
                panic!("expected completed kernel unit, got fault: {message}")
            }
        }
    }

    fn provider_with_prefix(prefix: &str) -> InMemoryProvider {
        let mut provider = InMemoryProvider::new();
        provider.add_link("root", format!("{prefix}/project"));
        provider.add_file(
            format!("{prefix}/project/main.tpz"),
            "import util { value }\nprint(\"{value}\")\n",
        );
        provider.add_file(
            format!("{prefix}/project/util.tpz"),
            "export const value = 42\n",
        );
        provider.add_file(
            format!("{prefix}/project/unrelated.tpz"),
            "print(\"not in closure\")\n",
        );
        provider
    }

    #[test]
    fn integer_and_duration_observation_preserve_decimal_spelling() {
        let mut provider = InMemoryProvider::new();
        provider.add_file(
            "main.tpz",
            "let too_large = 9223372036854775808\n\
             concurrent(timeout: 18446744073709551615ms) { a: 1 } else { { a: 0 } }\n",
        );
        let execution = drive_checked(
            &ProviderHost::new(&provider),
            KernelRequest::checked(
                "main.tpz",
                None,
                LangVersion::CURRENT,
                PackageFacts::standalone(),
            )
            .with_terminal_phase(TerminalPhase::Resolved),
        );
        clean_unit(&execution.outcome);
        let observation = build_observation(&execution).expect("AST observation");
        let ast = observation
            .files
            .iter()
            .find(|file| file.path == "ast.jsonl")
            .and_then(|file| std::str::from_utf8(&file.bytes).ok())
            .expect("UTF-8 AST projection");
        let integer = ast
            .lines()
            .find(|row| row.contains("\"kind\":\"expression/integer\""))
            .expect("integer AST row");
        assert!(
            integer.contains("\"valueDecimal\":\"9223372036854775808\""),
            "{integer}"
        );
        assert!(
            ast.contains("\"kind\":\"expression/duration\",\"nodeId\":")
                && ast.contains("\"valueDecimal\":\"18446744073709551615\""),
            "{ast}"
        );
    }

    fn bootstrap_workload_provider() -> InMemoryProvider {
        let mut provider = InMemoryProvider::new();
        provider.add_file(
            "root/src/main.tpz",
            include_str!("../../../corpus/v5_10/bootstrap-workload/src/main.tpz"),
        );
        provider.add_file(
            "root/syntax.tpz",
            include_str!("../../../corpus/v5_10/bootstrap-workload/syntax.tpz"),
        );
        provider.add_file(
            "root/order.tpz",
            include_str!("../../../corpus/v5_10/bootstrap-workload/order.tpz"),
        );
        provider.add_file(
            "root/diagnostics.tpz",
            include_str!("../../../corpus/v5_10/bootstrap-workload/diagnostics.tpz"),
        );
        provider.add_file(
            "root/codegen.tpz",
            include_str!("../../../corpus/v5_10/bootstrap-workload/codegen.tpz"),
        );
        provider
    }

    fn bootstrap_workload_request() -> KernelRequest {
        KernelRequest::checked(
            "root/src/main.tpz",
            Some("root"),
            LangVersion::CURRENT,
            PackageFacts {
                identity: Some("topaz_bootstrap_workload@0.1.0".to_string()),
                build_role: BuildRole::Package,
                deterministic: true,
                executable_profile: Some("bootstrap".to_string()),
                dependency_mount_ids: Vec::new(),
                extern_modules: BTreeSet::new(),
                extern_replay_errors: BTreeMap::new(),
                generated_std_modules: BTreeMap::new(),
                capabilities: BTreeSet::new(),
                locked: true,
            },
        )
        .with_terminal_phase(TerminalPhase::RustSource)
    }

    #[test]
    fn compiler_grade_workload_is_complete_and_fact_order_independent() {
        let provider = bootstrap_workload_provider();
        let execution = drive_checked(&ProviderHost::new(&provider), bootstrap_workload_request());
        let unit = clean_unit(&execution.outcome);
        assert_eq!(
            unit.resolved
                .modules
                .iter()
                .map(|module| module.identity.as_str())
                .collect::<Vec<_>>(),
            ["order", "syntax", "codegen", "diagnostics", "src.main"]
        );
        let first = build_observation(&execution).expect("workload observation");

        let mut reversed = bootstrap_workload_request();
        for (query, fact) in execution.request.facts().iter().rev() {
            reversed
                .supply_fact(query.clone(), fact.clone())
                .expect("reversed fact");
        }
        let reversed_execution = KernelExecution {
            outcome: execute(&reversed),
            request: reversed,
            rounds: execution.rounds,
        };
        clean_unit(&reversed_execution.outcome);
        let second = build_observation(&reversed_execution).expect("reversed observation");
        assert_eq!(first, second);
        for layer in [
            ComparisonLayer::Semantic,
            ComparisonLayer::GeneratedSource,
            ComparisonLayer::Provenance,
        ] {
            assert!(
                compare_observations(&first, &second, layer)
                    .expect("comparison")
                    .equal
            );
        }
    }

    #[test]
    fn kernel_accepts_exact_self_hosted_default_profiles_only() {
        for version in [
            LangVersion::V5_16,
            LangVersion::V5_17,
            LangVersion::V5_18,
            LangVersion::V5_19,
        ] {
            let provider = provider_with_prefix("admitted-profile");
            let request = KernelRequest::checked(
                "root/main.tpz",
                Some("root"),
                version,
                PackageFacts::standalone(),
            );
            let execution = drive_checked(&ProviderHost::new(&provider), request);
            clean_unit(&execution.outcome);
        }

        let declined = execute(&KernelRequest::checked(
            "root/main.tpz",
            Some("root"),
            LangVersion::V5_15,
            PackageFacts::standalone(),
        ));
        assert!(matches!(declined, KernelOutcome::Declined { .. }));
    }

    #[test]
    fn compiler_grade_workload_enforces_every_resource_dimension() {
        for (dimension, configure) in [
            (
                ResourceDimension::SourceFacts,
                (|budgets: &mut ResourceBudgets| budgets.max_source_facts = 1)
                    as fn(&mut ResourceBudgets),
            ),
            (
                ResourceDimension::TotalSourceBytes,
                (|budgets: &mut ResourceBudgets| budgets.max_total_source_bytes = 1)
                    as fn(&mut ResourceBudgets),
            ),
            (
                ResourceDimension::RawTokens,
                (|budgets: &mut ResourceBudgets| budgets.max_raw_tokens = 1)
                    as fn(&mut ResourceBudgets),
            ),
            (
                ResourceDimension::LayoutTokens,
                (|budgets: &mut ResourceBudgets| budgets.max_layout_tokens = 1)
                    as fn(&mut ResourceBudgets),
            ),
            (
                ResourceDimension::AstNodes,
                (|budgets: &mut ResourceBudgets| budgets.max_ast_nodes = 1)
                    as fn(&mut ResourceBudgets),
            ),
            (
                ResourceDimension::HirNodes,
                (|budgets: &mut ResourceBudgets| budgets.max_hir_nodes = 1)
                    as fn(&mut ResourceBudgets),
            ),
            (
                ResourceDimension::LoweredNodes,
                (|budgets: &mut ResourceBudgets| budgets.max_lowered_nodes = 1)
                    as fn(&mut ResourceBudgets),
            ),
            (
                ResourceDimension::GeneratedRustBytes,
                (|budgets: &mut ResourceBudgets| budgets.max_generated_rust_bytes = 1)
                    as fn(&mut ResourceBudgets),
            ),
        ] {
            let provider = bootstrap_workload_provider();
            let mut request = bootstrap_workload_request();
            configure(request.budgets_mut());
            let execution = drive_checked(&ProviderHost::new(&provider), request);
            match execution.outcome {
                KernelOutcome::ResourceLimit(limit) => {
                    assert_eq!(limit.dimension, dimension);
                    assert_eq!(limit.limit, 1);
                    assert!(limit.observed > 1);
                }
                _ => panic!("expected {dimension:?} resource limit"),
            }
        }

        let provider = bootstrap_workload_provider();
        let mut projection_request = bootstrap_workload_request();
        projection_request.budgets_mut().max_projection_bytes = 1;
        let projection_execution = drive_checked(&ProviderHost::new(&provider), projection_request);
        assert!(matches!(
            projection_execution.outcome,
            KernelOutcome::Completed(_)
        ));
        let error = build_observation(&projection_execution)
            .expect_err("projection output must respect its byte budget");
        assert!(error.contains("projection-byte resource limit"), "{error}");

        let mut diagnostic_provider = bootstrap_workload_provider();
        diagnostic_provider.add_file("root/codegen.tpz", "function broken( {\n");
        let mut diagnostic_request = bootstrap_workload_request();
        diagnostic_request.budgets_mut().max_diagnostics = 0;
        let diagnostic_execution =
            drive_checked(&ProviderHost::new(&diagnostic_provider), diagnostic_request);
        match diagnostic_execution.outcome {
            KernelOutcome::ResourceLimit(limit) => {
                assert_eq!(limit.dimension, ResourceDimension::Diagnostics);
                assert_eq!(limit.limit, 0);
                assert!(limit.observed > 0);
            }
            _ => panic!("expected diagnostic resource limit"),
        }
    }

    #[derive(Clone)]
    struct FixedHost {
        source: SourceFact,
        outside_entry: bool,
    }

    impl HostFactSource for FixedHost {
        fn respond(&self, _request: &KernelRequest, query: &HostQuery) -> HostFact {
            match query {
                HostQuery::ReadSource { .. } => HostFact::Source(self.source.clone()),
                HostQuery::ListDirectory { .. } => {
                    HostFact::Directory(DirectoryFact::Present(Vec::new()))
                }
                HostQuery::PhysicalContainment { logical_path, .. }
                    if self.outside_entry && logical_path.ends_with("main.tpz") =>
                {
                    HostFact::Containment(ContainmentFact::Outside)
                }
                HostQuery::PhysicalContainment { .. } => {
                    HostFact::Containment(ContainmentFact::Inside {
                        alias_class: "sha256:test".to_string(),
                    })
                }
            }
        }
    }

    #[test]
    fn driver_reads_only_the_import_closure() {
        let provider = provider_with_prefix("machine-a");
        let request = KernelRequest::checked(
            "root/main.tpz",
            Some("root"),
            LangVersion::CURRENT,
            PackageFacts::standalone(),
        );
        let host = ProviderHost::new(&provider);
        let execution = drive_checked(&host, request);
        let unit = clean_unit(&execution.outcome);
        assert_eq!(unit.resolved.modules.len(), 2);
        assert!(
            provider
                .reads()
                .iter()
                .all(|path| !path.ends_with("unrelated.tpz"))
        );
        assert!(execution.request.facts().keys().all(|query| {
            !matches!(
                query,
                HostQuery::ReadSource { logical_path, .. }
                    if logical_path.ends_with("unrelated.tpz")
            )
        }));
        assert_eq!(unit.provenance.route, CompilerRoute::CurrentKernel);
    }

    #[test]
    fn relocation_preserves_the_normalized_request_model() {
        let first_provider = provider_with_prefix("machine-a");
        let second_provider = provider_with_prefix("machine-b");
        let request = || {
            KernelRequest::checked(
                "root/main.tpz",
                Some("root"),
                LangVersion::CURRENT,
                PackageFacts::standalone(),
            )
        };
        let first = drive_checked(&ProviderHost::new(&first_provider), request());
        let second = drive_checked(&ProviderHost::new(&second_provider), request());
        clean_unit(&first.outcome);
        clean_unit(&second.outcome);
        assert_eq!(first.request, second.request);
    }

    #[test]
    fn captured_facts_replay_after_the_source_provider_is_unavailable() {
        let request = KernelRequest::checked(
            "root/main.tpz",
            Some("root"),
            LangVersion::CURRENT,
            PackageFacts::standalone(),
        );
        let captured = {
            let provider = provider_with_prefix("machine-a");
            let execution = drive_checked(&ProviderHost::new(&provider), request);
            clean_unit(&execution.outcome);
            execution.request
        };

        let replayed = execute(&captured);
        let unit = clean_unit(&replayed);
        assert_eq!(unit.resolved.modules.len(), 2);
        assert_eq!(unit.provenance.route, CompilerRoute::CurrentKernel);
    }

    #[test]
    fn resolved_observation_is_canonical_relocatable_and_self_validating() {
        let observe = |prefix: &str| {
            let provider = provider_with_prefix(prefix);
            let request = KernelRequest::checked(
                "root/main.tpz",
                Some("root"),
                LangVersion::CURRENT,
                PackageFacts::standalone(),
            )
            .with_terminal_phase(TerminalPhase::Resolved);
            let execution = drive_checked(&ProviderHost::new(&provider), request);
            clean_unit(&execution.outcome);
            build_observation(&execution).expect("observation")
        };
        let first = observe("machine-a");
        let second = observe("machine-b");
        assert_eq!(first, second);
        first.validate().expect("valid canonical bundle");
        assert!(first.files.iter().any(|file| file.path == "tokens.jsonl"));
        assert!(first.files.iter().any(|file| file.path == "ast.jsonl"));
        assert!(first.files.iter().any(|file| file.path == "resolved.jsonl"));

        let provider = provider_with_prefix("machine-c");
        let captured = drive_checked(
            &ProviderHost::new(&provider),
            KernelRequest::checked(
                "root/main.tpz",
                Some("root"),
                LangVersion::CURRENT,
                PackageFacts::standalone(),
            )
            .with_terminal_phase(TerminalPhase::Resolved),
        );
        let mut reversed = KernelRequest::checked(
            "root/main.tpz",
            Some("root"),
            LangVersion::CURRENT,
            PackageFacts::standalone(),
        )
        .with_terminal_phase(TerminalPhase::Resolved);
        for (query, fact) in captured.request.facts().iter().rev() {
            reversed
                .supply_fact(query.clone(), fact.clone())
                .expect("unique fact");
        }
        let reversed_execution = KernelExecution {
            outcome: execute(&reversed),
            request: reversed,
            rounds: 1,
        };
        let reversed_bundle =
            build_observation(&reversed_execution).expect("reversed-fact observation");
        assert_eq!(first, reversed_bundle);

        for projection in [
            "request.json",
            "response.json",
            "provenance.json",
            "source-set.jsonl",
            "tokens.jsonl",
            "ast.jsonl",
            "resolved.jsonl",
            "diagnostics.jsonl",
            "topaz-observation.json",
        ] {
            let mut mutated = first.clone();
            let file = mutated
                .files
                .iter_mut()
                .find(|file| file.path == projection)
                .expect("projection");
            if let Some(first) = file.bytes.first_mut() {
                *first ^= 1;
            } else {
                file.bytes.push(b'x');
            }
            assert!(
                mutated.validate().is_err(),
                "mutation in {projection} must be rejected"
            );
        }
    }

    #[test]
    fn resolved_observation_covers_nominal_protocol_and_nested_scopes() {
        let mut provider = InMemoryProvider::new();
        provider.add_file(
            "main.tpz",
            r#"protocol Show {
    function show(value: Self) -> string
}

record Box<T> {
    value: T
}

impl Show<Box> {
    function show(value: Box<int>) -> string {
        let prefix = "box"
        let render = x => "{prefix}:{x}"
        render(value.value)
    }
}

let boxed = Box { value: 7 }
print(show(boxed))
"#,
        );
        let execution = drive_checked(
            &ProviderHost::new(&provider),
            KernelRequest::checked(
                "main.tpz",
                None,
                LangVersion::CURRENT,
                PackageFacts::standalone(),
            )
            .with_terminal_phase(TerminalPhase::Resolved),
        );
        clean_unit(&execution.outcome);
        let bundle = build_observation(&execution).expect("observation");
        bundle.validate().expect("valid observation");
        let resolved = bundle
            .files
            .iter()
            .find(|file| file.path == "resolved.jsonl")
            .and_then(|file| std::str::from_utf8(&file.bytes).ok())
            .expect("resolved projection");
        assert!(resolved.contains("\"declarationKind\":\"protocol\""));
        assert!(resolved.contains("\"kind\":\"function\""));
        assert!(resolved.contains("\"kind\":\"lambda\""));
        assert!(resolved.contains("\"rowKind\":\"reference\""));
    }

    #[test]
    fn typed_observation_covers_calls_and_closure_captures() {
        let mut provider = InMemoryProvider::new();
        provider.add_file(
            "main.tpz",
            "let offset = 2\n\
             let add = (value: int) => value + offset\n\
             let answer = add(40)\n",
        );
        let execution = drive_checked(
            &ProviderHost::new(&provider),
            KernelRequest::checked(
                "main.tpz",
                None,
                LangVersion::CURRENT,
                PackageFacts::standalone(),
            )
            .with_terminal_phase(TerminalPhase::Typed),
        );
        clean_unit(&execution.outcome);
        let bundle = build_observation(&execution).expect("typed observation");
        bundle.validate().expect("valid typed observation");
        let typed = bundle
            .files
            .iter()
            .find(|file| file.path == "typed.jsonl")
            .expect("typed projection");
        let text = std::str::from_utf8(&typed.bytes).expect("UTF-8");
        assert!(text.contains("\"rowKind\":\"node\""), "{text}");
        assert!(text.contains("\"rowKind\":\"call\""), "{text}");
        assert!(text.contains("\"rowKind\":\"capture\""), "{text}");
        assert!(text.contains("\"targetIdentity\":\"main::add\""), "{text}");
        assert!(text.contains("\"plan\":{\"arguments\":"), "{text}");
        assert!(!text.contains("\"plan\":\""), "{text}");
        assert!(
            !text.contains("\"calleeType\":{\"kind\":\"unknown\"}"),
            "{text}"
        );

        let mut mutated = bundle.clone();
        let typed = mutated
            .files
            .iter_mut()
            .find(|file| file.path == "typed.jsonl")
            .expect("typed projection");
        typed.bytes[0] ^= 1;
        assert!(mutated.validate().is_err());
    }

    #[test]
    fn lowered_and_rust_source_observation_are_canonical_and_budgeted() {
        let mut provider = InMemoryProvider::new();
        provider.add_file(
            "main.tpz",
            "let offset = 2\n\
             let add = (value: int) => value + offset\n\
             print(\"{add(40)}\")\n",
        );
        let execution = drive_checked(
            &ProviderHost::new(&provider),
            KernelRequest::checked(
                "main.tpz",
                None,
                LangVersion::CURRENT,
                PackageFacts::standalone(),
            )
            .with_terminal_phase(TerminalPhase::RustSource),
        );
        clean_unit(&execution.outcome);
        let bundle = build_observation(&execution).expect("Rust-source observation");
        bundle.validate().expect("valid complete observation");

        let mut different_provenance = bundle.clone();
        let provenance = different_provenance
            .files
            .iter_mut()
            .find(|file| file.path == "provenance.json")
            .expect("provenance");
        provenance.bytes = String::from_utf8(provenance.bytes.clone())
            .expect("UTF-8")
            .replace("\"engine\":\"rust-stage0\"", "\"engine\":\"rust-stage1\"")
            .into_bytes();
        crate::observation::refresh_test_manifest(&mut different_provenance);
        different_provenance
            .validate()
            .expect("valid alternate producer");
        assert!(
            compare_observations(&bundle, &different_provenance, ComparisonLayer::Semantic)
                .expect("semantic comparison")
                .equal
        );
        assert!(
            compare_observations(
                &bundle,
                &different_provenance,
                ComparisonLayer::GeneratedSource
            )
            .expect("source comparison")
            .equal
        );
        let provenance_comparison =
            compare_observations(&bundle, &different_provenance, ComparisonLayer::Provenance)
                .expect("provenance comparison");
        assert!(!provenance_comparison.equal);
        assert_eq!(
            provenance_comparison.first_failing_phase.as_deref(),
            Some("provenance")
        );

        let mut different_outcome = bundle.clone();
        let response = different_outcome
            .files
            .iter_mut()
            .find(|file| file.path == "response.json")
            .expect("response");
        response.bytes = String::from_utf8(response.bytes.clone())
            .expect("UTF-8")
            .replace("\"status\":\"completed\"", "\"status\":\"rejected\"")
            .into_bytes();
        crate::observation::refresh_test_manifest(&mut different_outcome);
        different_outcome
            .validate()
            .expect("valid alternate semantic outcome");
        let outcome_comparison =
            compare_observations(&bundle, &different_outcome, ComparisonLayer::Semantic)
                .expect("outcome comparison");
        assert!(!outcome_comparison.equal);
        assert_eq!(
            outcome_comparison.first_failing_phase.as_deref(),
            Some("outcome")
        );

        let lowered = bundle
            .files
            .iter()
            .find(|file| file.path == "lowered.jsonl")
            .expect("lowered projection");
        let lowered_text = std::str::from_utf8(&lowered.bytes).expect("UTF-8");
        assert!(lowered_text.contains("\"rowKind\":\"operation\""));
        assert!(lowered_text.contains("\"rowKind\":\"runtime-leaf\""));
        assert!(lowered_text.contains("\"expressionKind\":\"call\""));
        assert!(lowered_text.contains("\"rowKind\":\"runtime-template\""));

        let rust = bundle
            .files
            .iter()
            .find(|file| file.path == "rust-source.jsonl")
            .expect("Rust-source projection");
        let rust_text = std::str::from_utf8(&rust.bytes).expect("UTF-8");
        assert!(rust_text.contains("\"rowKind\":\"generated-source\""));
        assert!(rust_text.contains("\"source\":\"use std::rc::Rc;"));
        assert!(rust_text.contains("run_with_host"));

        for path in ["lowered.jsonl", "rust-source.jsonl"] {
            let mut mutated = bundle.clone();
            let file = mutated
                .files
                .iter_mut()
                .find(|file| file.path == path)
                .expect("projection member");
            file.bytes[0] ^= 1;
            assert!(mutated.validate().is_err(), "{path} mutation was accepted");
        }

        for (dimension, configure) in [
            (
                ResourceDimension::LoweredNodes,
                (|budgets: &mut ResourceBudgets| budgets.max_lowered_nodes = 1)
                    as fn(&mut ResourceBudgets),
            ),
            (
                ResourceDimension::GeneratedRustBytes,
                (|budgets: &mut ResourceBudgets| budgets.max_generated_rust_bytes = 1)
                    as fn(&mut ResourceBudgets),
            ),
        ] {
            let mut request = KernelRequest::checked(
                "main.tpz",
                None,
                LangVersion::CURRENT,
                PackageFacts::standalone(),
            )
            .with_terminal_phase(TerminalPhase::RustSource);
            configure(request.budgets_mut());
            let execution = drive_checked(&ProviderHost::new(&provider), request);
            match execution.outcome {
                KernelOutcome::ResourceLimit(limit) => {
                    assert_eq!(limit.dimension, dimension);
                    assert_eq!(limit.limit, 1);
                    assert!(limit.observed > 1);
                }
                _ => panic!("expected {dimension:?} resource limit"),
            }
        }
    }

    #[test]
    fn source_byte_budget_fails_with_an_exact_dimension() {
        let mut provider = InMemoryProvider::new();
        provider.add_file("main.tpz", "print(\"too large\")\n");
        let mut request = KernelRequest::checked(
            "main.tpz",
            None,
            LangVersion::CURRENT,
            PackageFacts::standalone(),
        );
        request.budgets_mut().max_total_source_bytes = 1;
        let execution = drive_checked(&ProviderHost::new(&provider), request);
        match execution.outcome {
            KernelOutcome::ResourceLimit(limit) => {
                assert_eq!(limit.dimension, ResourceDimension::TotalSourceBytes);
                assert_eq!(limit.limit, 1);
                assert!(limit.observed > 1);
            }
            _ => panic!("expected source-byte resource limit"),
        }
    }

    #[test]
    fn front_end_row_budgets_fail_at_the_exact_phase() {
        for (dimension, configure) in [
            (
                ResourceDimension::RawTokens,
                (|budgets: &mut ResourceBudgets| budgets.max_raw_tokens = 1)
                    as fn(&mut ResourceBudgets),
            ),
            (
                ResourceDimension::LayoutTokens,
                (|budgets: &mut ResourceBudgets| budgets.max_layout_tokens = 1)
                    as fn(&mut ResourceBudgets),
            ),
            (
                ResourceDimension::AstNodes,
                (|budgets: &mut ResourceBudgets| budgets.max_ast_nodes = 1)
                    as fn(&mut ResourceBudgets),
            ),
        ] {
            let mut provider = InMemoryProvider::new();
            provider.add_file("main.tpz", "let value = 1\nprint(\"{value}\")\n");
            let mut request = KernelRequest::checked(
                "main.tpz",
                None,
                LangVersion::CURRENT,
                PackageFacts::standalone(),
            );
            configure(request.budgets_mut());
            let execution = drive_checked(&ProviderHost::new(&provider), request);
            match execution.outcome {
                KernelOutcome::ResourceLimit(limit) => {
                    assert_eq!(limit.dimension, dimension);
                    assert_eq!(limit.limit, 1);
                    assert!(limit.observed > 1);
                }
                _ => panic!("expected {dimension:?} resource limit"),
            }
        }
    }

    #[test]
    fn missing_entry_is_a_rejection_not_a_kernel_fault() {
        let provider = InMemoryProvider::new();
        let request = KernelRequest::checked(
            "main.tpz",
            None,
            LangVersion::CURRENT,
            PackageFacts::standalone(),
        );
        let execution = drive_checked(&ProviderHost::new(&provider), request);
        match execution.outcome {
            KernelOutcome::Rejected(unit) => {
                assert_eq!(unit.resolved.diagnostics.len(), 1);
                assert!(unit.checked.is_none());
            }
            _ => panic!("expected deterministic source rejection"),
        }
    }

    #[test]
    fn unreadable_and_invalid_utf8_are_deterministic_rejections() {
        for (source, expected_message) in [
            (
                SourceFact::Unreadable {
                    reason_code: "permission-denied".to_string(),
                },
                "permission-denied",
            ),
            (SourceFact::InvalidUtf8, "source is not valid UTF-8"),
        ] {
            let request = KernelRequest::checked(
                "main.tpz",
                None,
                LangVersion::CURRENT,
                PackageFacts::standalone(),
            );
            let execution = drive_checked(
                &FixedHost {
                    source: source.clone(),
                    outside_entry: false,
                },
                request,
            );
            match &execution.outcome {
                KernelOutcome::Rejected(unit) => {
                    assert_eq!(unit.resolved.diagnostics.len(), 1);
                    assert_eq!(unit.resolved.diagnostics[0].code.as_str(), "TPZ3003");
                    assert!(
                        unit.resolved.diagnostics[0]
                            .message
                            .contains(expected_message)
                    );
                    assert!(unit.checked.is_none());
                }
                _ => panic!("expected deterministic source rejection"),
            }
            assert!(
                execution
                    .request
                    .facts()
                    .values()
                    .any(|fact| matches!(fact, HostFact::Source(observed) if observed == &source))
            );
        }
    }

    #[test]
    fn physical_escape_is_rejected_through_a_containment_fact() {
        let request = KernelRequest::checked(
            "root/main.tpz",
            Some("root"),
            LangVersion::CURRENT,
            PackageFacts::standalone(),
        );
        let execution = drive_checked(
            &FixedHost {
                source: SourceFact::Present("print(\"safe\")\n".to_string()),
                outside_entry: true,
            },
            request,
        );
        match execution.outcome {
            KernelOutcome::Rejected(unit) => {
                assert_eq!(unit.resolved.diagnostics.len(), 1);
                assert!(
                    unit.resolved.diagnostics[0]
                        .message
                        .contains("resolves outside the root")
                );
            }
            _ => panic!("expected physical containment rejection"),
        }
    }

    #[test]
    fn compatibility_provenance_is_explicit() {
        let checked = BootstrapProvenance::compatibility(LangVersion::V5_9, false);
        let unchecked = BootstrapProvenance::compatibility(LangVersion::CURRENT, true);
        assert_eq!(checked.route, CompilerRoute::RustCompatibility);
        assert_eq!(checked.engine, "rust-compat");
        assert_eq!(unchecked.route, CompilerRoute::RustUnchecked);
        assert_eq!(unchecked.engine, "rust-unchecked");
    }
}
