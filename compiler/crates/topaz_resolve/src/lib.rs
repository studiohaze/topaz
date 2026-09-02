//! topaz_resolve — Topaz v5.2 module resolution (CDR-002 §5; opens
//! the v0.2 series).
//!
//! Implements the provider abstraction, dotted-path → file mapping
//! (exact Unicode-scalar correspondence, `.tpz` exactly), root/entry
//! and physical-containment semantics, transitive import closure,
//! collision keys, cycle policy, normative module order, name
//! resolution, and the initializer reference rule.

pub mod codes;
mod init_rule;
mod names;
mod norm;
mod provider;
mod stdlib;
#[rustfmt::skip]
mod unicode_norm;

pub use norm::{casefold, nfd};
pub use provider::{
    DirectoryRead, FileProvider, GeneratedStdModule, InMemoryProvider, PhysicalProvider,
    SourceRead, normalize_path, physical_path_identity, read_source_path,
};

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use topaz_diag::{Diagnostic, Label, SourceMap, Span};
use topaz_parser::{ParseOptions, parse_staged};
use topaz_syntax::ast::{Program, StmtKind};
use topaz_syntax::{LangVersion, Token};

/// One module of the compilation unit. Clean acyclic output uses the
/// normative ADR-078 processing order; rejected output keeps the
/// deterministic breadth-first discovery order when that order is
/// unavailable.
#[derive(Debug)]
pub struct ResolvedModule {
    /// Dotted logical identity relative to the root (`src.main`,
    /// `utils.strings`).
    pub identity: String,
    /// Root-relative file path.
    pub path: String,
    pub file: topaz_diag::FileId,
    pub raw_tokens: Vec<Token>,
    pub layout_tokens: Vec<Token>,
    pub program: Program,
    pub is_entry: bool,
    pub is_extern: bool,
    /// True only for a compiler-owned package-capability module supplied by
    /// [`FileProvider::generated_std_module`].
    pub is_generated_std: bool,
    pub extern_replay_error: Option<String>,
}

struct PendingModule {
    identity: String,
    path: String,
    file: topaz_diag::FileId,
    is_entry: bool,
    is_extern: bool,
    is_generated_std: bool,
    extern_replay_error: Option<String>,
}

#[derive(Debug)]
pub struct ResolveOutput {
    /// Language profile used to parse and resolve every module in this unit.
    /// Downstream execution and emission must preserve profile-specific runtime
    /// semantics instead of inferring them from the current compiler binary.
    pub language_version: LangVersion,
    pub modules: Vec<ResolvedModule>,
    pub map: SourceMap,
    pub diagnostics: Vec<Diagnostic>,
    /// Importer → imported identities (every import item; §17), for
    /// downstream import-chain diagnostics.
    pub import_edges: Vec<(String, String)>,
    /// Resolver-owned lexical scopes, declarations, exports, and references.
    ///
    /// These are semantic observations rather than a serialization contract;
    /// `topaz_kernel` assigns canonical source/node/symbol identities.
    pub name_facts: NameResolutionFacts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedNamespace {
    Value,
    Type,
    Module,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedScopeKind {
    Module,
    Function,
    Block,
    Pattern,
    Lambda,
    Comprehension,
    Using,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedDeclarationKind {
    NamespaceImport,
    SelectedImport,
    Function,
    TypeAlias,
    NominalType,
    Protocol,
    Let,
    Const,
    Parameter,
    Pattern,
    Using,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedReferenceRole {
    Read,
    Write,
    NamespaceMember,
    Type,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedScopeFact {
    pub file: topaz_diag::FileId,
    pub ordinal: u32,
    pub parent_ordinal: Option<u32>,
    pub kind: ResolvedScopeKind,
    pub owner: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDeclarationFact {
    pub file: topaz_diag::FileId,
    pub scope_ordinal: u32,
    pub name: String,
    pub namespace: ResolvedNamespace,
    pub kind: ResolvedDeclarationKind,
    pub span: Span,
    pub exported: bool,
    pub target_module: Option<String>,
    pub target_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedReferenceFact {
    pub file: topaz_diag::FileId,
    pub scope_ordinal: u32,
    pub name: String,
    pub namespace: ResolvedNamespace,
    pub role: ResolvedReferenceRole,
    pub span: Span,
    pub target_file: Option<topaz_diag::FileId>,
    pub target_span: Option<Span>,
    pub target_namespace: Option<ResolvedNamespace>,
    pub target_module: Option<String>,
    pub target_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedExportFact {
    pub file: topaz_diag::FileId,
    pub name: String,
    pub namespace: ResolvedNamespace,
    pub declaration_span: Span,
}

#[derive(Debug, Default)]
pub struct NameResolutionFacts {
    pub scopes: Vec<ResolvedScopeFact>,
    pub declarations: Vec<ResolvedDeclarationFact>,
    pub references: Vec<ResolvedReferenceFact>,
    pub exports: Vec<ResolvedExportFact>,
}

impl ResolveOutput {
    pub fn is_clean(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// §17 the import-chain suffix a module-initialization fault carries: the
/// shortest `entry -> … -> target` path through `import_edges` (BFS from the
/// entry), rendered `import chain: a -> b -> target`, or `imported from \`entry\``
/// when no path is found. BOTH engines call THIS — the interpreter wraps an
/// init fault through this adapter at runtime (`run_unit`), while generated
/// backends consume the same `topaz_diag` renderer from resolver or HIR data.
/// The wrapped fault message is therefore byte-identical (run == build).
pub fn import_chain(unit: &ResolveOutput, target: &str) -> String {
    let entry = unit
        .modules
        .iter()
        .find(|m| m.is_entry)
        .map(|m| m.identity.as_str())
        .unwrap_or_default();
    topaz_diag::render_import_chain(entry, &unit.import_edges, target)
}

/// Resolves the compilation unit rooted at `entry` (SPEC v5.2 §17):
/// the root defaults to the entry file's directory; an explicit
/// `root` must contain the entry. Module paths map to files by exact
/// scalar correspondence under the root. Files outside the closure
/// are never read.
///
/// Parses every module at [`LangVersion::CURRENT`], the product's current
/// language line. Use [`resolve_with_version`] to pin a compatibility version;
/// the CLI threads the explicit `--language-version` selection.
pub fn resolve(provider: &dyn FileProvider, entry: &str, root: Option<&str>) -> ResolveOutput {
    resolve_with_version(provider, entry, root, LangVersion::CURRENT)
}

/// Returns the virtual current `std.*` module source for tooling consumers that
/// need the same import surface as the resolver without re-resolving a unit.
pub fn std_module_source(segments: &[&str]) -> Option<(&'static str, &'static str)> {
    stdlib::module_source(segments)
}

/// Dotted identities of the virtual standard modules, in compiler catalog
/// order. Tooling consumes this inventory instead of maintaining a projection.
pub fn std_module_identities() -> impl Iterator<Item = &'static str> {
    stdlib::module_identities()
}

/// v5.4 explicit CLI entrypoint marker: an `export function main(...)` in the
/// entry module. The signature gate is owned by the checker/CLI slice; this
/// helper is the shared semantic switch so interp and emit cannot drift.
pub fn explicit_main_span(unit: &ResolveOutput) -> Option<Span> {
    let entry = unit.modules.iter().find(|module| module.is_entry)?;
    let src = unit.map.file(entry.file).src();
    program_explicit_main_span(&entry.program, src)
}

pub fn has_explicit_main(unit: &ResolveOutput) -> bool {
    explicit_main_span(unit).is_some()
}

fn program_explicit_main_span(program: &Program, src: &str) -> Option<Span> {
    program.items.iter().find_map(|stmt| match &stmt.kind {
        StmtKind::Export(inner) => match &inner.kind {
            StmtKind::Function(decl) if text(src, decl.name.span) == "main" => Some(decl.name.span),
            _ => None,
        },
        _ => None,
    })
}

fn text(src: &str, span: Span) -> &str {
    &src[span.lo as usize..span.hi as usize]
}

/// [`resolve`] with an explicit language version — modules parse at `version`.
/// The module system itself requires `>= V5_2`; v5.3 adds user enums; v5.4 adds
/// multi-payload/recursive enums.
pub fn resolve_with_version(
    provider: &dyn FileProvider,
    entry: &str,
    root: Option<&str>,
    version: LangVersion,
) -> ResolveOutput {
    let entry = normalize_path(entry);
    let mut map = SourceMap::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut modules: Vec<ResolvedModule> = Vec::new();

    // Parent segments would defeat prefix containment (`--root src`
    // with entry `src/../outside/main.tpz`): rejected outright in
    // the provider-independent lexical boundary. Physical containment
    // below then covers symlink and alias resolution (CDR-002 §5).
    if entry.split('/').any(|s| s == "..")
        || root.is_some_and(|r| normalize_path(r).split('/').any(|s| s == ".."))
    {
        let file = map
            .add_file(entry.clone(), String::from(" "))
            .expect("synthetic entry");
        diagnostics.push(Diagnostic::error(
            codes::ROOT_CONTAINMENT,
            "entry and root paths must not contain parent (`..`) segments",
            Label::new(Span::new(file, 0, 0), ""),
        ));
        return ResolveOutput {
            language_version: version,
            modules,
            map,
            diagnostics,
            import_edges: Vec::new(),
            name_facts: NameResolutionFacts::default(),
        };
    }

    let entry_dir = match entry.rsplit_once('/') {
        Some((dir, _)) => dir.to_string(),
        None => String::new(),
    };
    let root = match root {
        Some(r) => normalize_path(r),
        None => entry_dir,
    };

    // Explicit-root containment (SPEC v5.2 §17). The entry source is
    // still loaded so the diagnostic has a span anchor.
    let root_is_entry = !root.is_empty() && entry == root;
    let contained = root.is_empty() || entry.starts_with(&format!("{root}/"));
    let entry_src = match provider.read(&entry) {
        SourceRead::Present(source) => Ok(source),
        SourceRead::Missing => Err((
            codes::UNRESOLVED_MODULE,
            format!("entry file `{entry}` does not exist"),
        )),
        SourceRead::Unreadable { reason_code } => Err((
            codes::SOURCE_BOUND,
            format!("cannot load entry `{entry}`: {reason_code}"),
        )),
        SourceRead::InvalidUtf8 => Err((
            codes::SOURCE_BOUND,
            format!("cannot load entry `{entry}`: source is not valid UTF-8"),
        )),
    };
    let entry_src = match entry_src {
        Ok(source) => source,
        Err((code, message)) => {
            // An unavailable entry leaves no real span to point at, so a
            // synthetic one-byte file carries the diagnostic.
            let file = map
                .add_file(entry.clone(), String::from(" "))
                .expect("synthetic entry");
            diagnostics.push(Diagnostic::error(
                code,
                message,
                Label::new(Span::new(file, 0, 0), ""),
            ));
            return ResolveOutput {
                language_version: version,
                modules,
                map,
                diagnostics,
                import_edges: Vec::new(),
                name_facts: NameResolutionFacts::default(),
            };
        }
    };
    let entry_file = match map.add_file(entry.clone(), entry_src) {
        Ok(file) => file,
        Err(e) => {
            let file = map
                .add_file(format!("{entry} (loader)"), String::from(" "))
                .expect("synthetic entry");
            diagnostics.push(Diagnostic::error(
                codes::SOURCE_BOUND,
                format!("cannot load entry `{entry}`: {e}"),
                Label::new(Span::new(file, 0, 0), ""),
            ));
            return ResolveOutput {
                language_version: version,
                modules,
                map,
                diagnostics,
                import_edges: Vec::new(),
                name_facts: NameResolutionFacts::default(),
            };
        }
    };
    if !contained {
        let message = if root_is_entry {
            format!(
                "the source root `{root}` must be a directory containing the entry, not the entry file itself"
            )
        } else {
            format!("the root `{root}` does not contain the entry `{entry}`")
        };
        diagnostics.push(Diagnostic::error(
            codes::ROOT_CONTAINMENT,
            message,
            Label::new(Span::new(entry_file, 0, 0), ""),
        ));
        return ResolveOutput {
            language_version: version,
            modules,
            map,
            diagnostics,
            import_edges: Vec::new(),
            name_facts: NameResolutionFacts::default(),
        };
    }

    // Physical containment applies to the entry too (SPEC v5.2
    // §17): an entry reached through a link that leaves the root is
    // rejected like any other escape.
    if escapes_root(provider, &root, &entry) {
        diagnostics.push(Diagnostic::error(
            codes::PHYSICAL_CONTAINMENT,
            format!("the entry `{entry}` resolves outside the root (symlink/alias containment)"),
            Label::new(Span::new(entry_file, 0, 0), ""),
        ));
        return ResolveOutput {
            language_version: version,
            modules,
            map,
            diagnostics,
            import_edges: Vec::new(),
            name_facts: NameResolutionFacts::default(),
        };
    }

    let entry_identity = identity_of(&root, &entry);
    let mut physical_modules: BTreeMap<String, String> = BTreeMap::new();
    if let Some(physical) = provider.physical_id(&entry) {
        physical_modules.insert(physical.replace('\\', "/"), entry_identity.clone());
    }

    // Breadth-first closure. `queued` keys by logical identity, which
    // also terminates on cycles; cycle *diagnostics* are the cycle
    // slice's concern, not closure construction's.
    let mut queued: BTreeSet<String> = BTreeSet::new();
    queued.insert(entry_identity.clone());
    let mut work = VecDeque::from([PendingModule {
        identity: entry_identity,
        path: entry,
        file: entry_file,
        is_entry: true,
        is_extern: false,
        is_generated_std: false,
        extern_replay_error: None,
    }]);
    // Import-graph edges (SPEC v5.2 §17: every import item creates an
    // edge, regardless of form or later use): (from, to, span).
    let mut edges: Vec<(String, String, Span)> = Vec::new();

    while let Some(PendingModule {
        identity,
        path,
        file,
        is_entry,
        is_extern,
        is_generated_std,
        extern_replay_error,
    }) = work.pop_front()
    {
        let out = parse_staged(
            file,
            map.file(file).src(),
            ParseOptions {
                language_version: version,
            },
        );
        diagnostics.extend(out.raw.diagnostics);
        diagnostics.extend(out.layout.diagnostics);
        diagnostics.extend(out.parsed.diagnostics);
        let program = out.parsed.program;

        // Per-importing-module duplicate tracking (SPEC v5.2 §17: a
        // logical module may appear in at most one import item;
        // reported before any name-collision diagnostics).
        let mut imported_here: BTreeSet<String> = BTreeSet::new();

        // Imported-module surface (SPEC v5.2 §17, build-role-relative):
        // only the import prologue, declarations, bindings, and export
        // wrappers may appear at an imported module's top level.
        if !is_entry {
            for item in &program.items {
                let allowed = matches!(
                    item.kind,
                    StmtKind::Import(_)
                        | StmtKind::Export(_)
                        | StmtKind::Function(_)
                        | StmtKind::Impl(_)
                        | StmtKind::Protocol(_)
                        | StmtKind::TypeAlias(_)
                        | StmtKind::Enum(_)
                        | StmtKind::Record(_)
                        | StmtKind::Newtype(_)
                        | StmtKind::Let { .. }
                        | StmtKind::Const { .. }
                );
                if !allowed {
                    diagnostics.push(Diagnostic::error(
                        codes::IMPORTED_FREE_STATEMENT,
                        format!(
                            "imported module `{identity}` contains a top-level free statement; \
                             free statements are entry-only (the same file is valid as an entry)"
                        ),
                        Label::new(item.span, ""),
                    ));
                }
            }
        }

        for item in &program.items {
            let StmtKind::Import(import) = &item.kind else {
                continue;
            };
            let src = map.file(file).src();
            let segments: Vec<String> = import
                .path
                .segments
                .iter()
                .map(|seg| text(src, seg.span).to_string())
                .collect();
            let segment_refs: Vec<&str> = segments.iter().map(String::as_str).collect();
            let target_identity = segments.join(".");
            // Reserved roots (SPEC v5.2 §17): `std` opens as the
            // virtual standard-library root in v5.4; `topaz` remains
            // compiler-internal and reserved.
            if segments[0] == "topaz" || (segments[0] == "std" && version < LangVersion::V5_4) {
                let root_note = if segments[0] == "std" {
                    " before v5.4"
                } else {
                    ""
                };
                diagnostics.push(Diagnostic::error(
                    codes::RESERVED_ROOT,
                    format!(
                        "the module path root `{}` is reserved{root_note}; user modules cannot live under it",
                        segments[0],
                    ),
                    Label::new(import.path.span, ""),
                ));
                continue;
            }
            // Every import item creates a graph edge (SPEC v5.2
            // §17) — including duplicates, which are additionally a
            // static error of their own.
            edges.push((identity.clone(), target_identity.clone(), import.path.span));
            if !imported_here.insert(target_identity.clone()) {
                diagnostics.push(Diagnostic::error(
                    codes::DUPLICATE_IMPORT,
                    format!(
                        "`{target_identity}` is already imported by this module; a logical module may appear in at most one import item"
                    ),
                    Label::new(import.path.span, ""),
                ));
                continue;
            }
            if !queued.insert(target_identity.clone()) {
                continue; // already queued (also: cycles terminate)
            }
            if segments[0] == "std" {
                let generated = generated_std_module_allowed(&target_identity, version)
                    .then(|| provider.generated_std_module(&target_identity))
                    .flatten();
                let source = generated
                    .map(|module| (module.path, module.source, true))
                    .or_else(|| {
                        stdlib::module_source(&segment_refs)
                            .map(|(path, source)| (path.to_string(), source.to_string(), false))
                    });
                match source {
                    Some((target_path, target_src, target_generated)) => {
                        match map.add_file(target_path.clone(), target_src) {
                            Ok(target_file) => {
                                work.push_back(PendingModule {
                                    identity: target_identity,
                                    path: target_path,
                                    file: target_file,
                                    is_entry: false,
                                    is_extern: false,
                                    is_generated_std: target_generated,
                                    extern_replay_error: None,
                                });
                            }
                            Err(e) => {
                                diagnostics.push(Diagnostic::error(
                                    codes::SOURCE_BOUND,
                                    format!("cannot load std module `{target_identity}`: {e}"),
                                    Label::new(import.path.span, ""),
                                ));
                            }
                        }
                    }
                    None => {
                        let rel = segments.join("/") + ".tpz";
                        diagnostics.push(Diagnostic::error(
                            codes::UNRESOLVED_MODULE,
                            format!(
                                "no std module for `{target_identity}` (expected virtual `{rel}`)"
                            ),
                            Label::new(import.path.span, ""),
                        ));
                    }
                }
                continue;
            }
            let rel = segments.join("/") + ".tpz";
            let target_path = if root.is_empty() {
                rel.clone()
            } else {
                format!("{root}/{rel}")
            };
            // Physical containment (SPEC v5.2 §17): the resolved
            // location must stay inside the root.
            if escapes_root(provider, &root, &target_path) {
                diagnostics.push(Diagnostic::error(
                    codes::PHYSICAL_CONTAINMENT,
                    format!(
                        "`{target_identity}` resolves outside the root (symlink/alias containment)"
                    ),
                    Label::new(import.path.span, ""),
                ));
                continue;
            }
            // Per-segment walk (SPEC v5.2 §17): mapping is by exact
            // Unicode scalars — a case-insensitive real filesystem
            // must not silently open a fold-equivalent file — and
            // key-equal candidates observed along the way are a
            // collision, never a silent choice.
            match segment_walk(provider, &root, &segment_refs) {
                SegmentOutcome::Collision(detail) => {
                    diagnostics.push(Diagnostic::error(
                        codes::MODULE_COLLISION,
                        format!(
                            "the path `{target_identity}` collides with another candidate under the module name keys{detail}"
                        ),
                        Label::new(import.path.span, ""),
                    ));
                    continue;
                }
                SegmentOutcome::MissingExact => {
                    let code = if provider.is_extern_namespace(&target_identity) {
                        codes::EXTERN_DECL
                    } else {
                        codes::UNRESOLVED_MODULE
                    };
                    let message = if code == codes::EXTERN_DECL {
                        format!("extern module `{target_identity}` is not declared in topaz.toml")
                    } else {
                        format!(
                            "no module file for `{target_identity}` (expected `{target_path}` by exact scalars)"
                        )
                    };
                    diagnostics.push(Diagnostic::error(
                        code,
                        message,
                        Label::new(import.path.span, ""),
                    ));
                    continue;
                }
                SegmentOutcome::Unreadable(reason_code) => {
                    diagnostics.push(Diagnostic::error(
                        codes::SOURCE_BOUND,
                        format!(
                            "cannot inspect module path for `{target_identity}`: {reason_code}"
                        ),
                        Label::new(import.path.span, ""),
                    ));
                    continue;
                }
                SegmentOutcome::Ok => {}
            }
            match provider.read(&target_path) {
                SourceRead::Present(target_src) => {
                    let physical = provider
                        .physical_id(&target_path)
                        .map(|value| value.replace('\\', "/"));
                    if let Some(existing) = physical
                        .as_ref()
                        .and_then(|value| physical_modules.get(value))
                    {
                        diagnostics.push(Diagnostic::error(
                            codes::MODULE_COLLISION,
                            format!(
                                "the modules `{existing}` and `{target_identity}` resolve to the same physical file; one physical file cannot have two module identities"
                            ),
                            Label::new(import.path.span, ""),
                        ));
                        continue;
                    }
                    match map.add_file(target_path.clone(), target_src) {
                        Ok(target_file) => {
                            if let Some(physical) = physical {
                                physical_modules.insert(physical, target_identity.clone());
                            }
                            let target_is_extern = provider.is_extern_file(&target_path);
                            let target_extern_replay_error = if target_is_extern {
                                provider.extern_replay_error(&target_identity)
                            } else {
                                None
                            };
                            work.push_back(PendingModule {
                                identity: target_identity,
                                path: target_path,
                                file: target_file,
                                is_entry: false,
                                is_extern: target_is_extern,
                                is_generated_std: false,
                                extern_replay_error: target_extern_replay_error,
                            });
                        }
                        Err(e) => {
                            diagnostics.push(Diagnostic::error(
                                codes::SOURCE_BOUND,
                                format!("cannot load module `{target_identity}`: {e}"),
                                Label::new(import.path.span, ""),
                            ));
                        }
                    }
                }
                SourceRead::Missing => {
                    let code = if provider.is_extern_namespace(&target_identity) {
                        codes::EXTERN_DECL
                    } else {
                        codes::UNRESOLVED_MODULE
                    };
                    let message = if code == codes::EXTERN_DECL {
                        format!("extern module `{target_identity}` is not declared in topaz.toml")
                    } else {
                        format!("no module file for `{target_identity}` (expected `{target_path}`)")
                    };
                    diagnostics.push(Diagnostic::error(
                        code,
                        message,
                        Label::new(import.path.span, ""),
                    ));
                }
                SourceRead::Unreadable { reason_code } => {
                    diagnostics.push(Diagnostic::error(
                        codes::SOURCE_BOUND,
                        format!("cannot load module `{target_identity}`: {reason_code}"),
                        Label::new(import.path.span, ""),
                    ));
                }
                SourceRead::InvalidUtf8 => {
                    diagnostics.push(Diagnostic::error(
                        codes::SOURCE_BOUND,
                        format!(
                            "cannot load module `{target_identity}`: source is not valid UTF-8"
                        ),
                        Label::new(import.path.span, ""),
                    ));
                }
            }
        }

        modules.push(ResolvedModule {
            identity,
            path,
            file,
            raw_tokens: out.raw.tokens,
            layout_tokens: out.layout.tokens,
            program,
            is_entry,
            is_extern,
            is_generated_std,
            extern_replay_error,
        });
    }

    // Cycle policy (SPEC v5.2 §17): one diagnostic per cyclic SCC,
    // anchored at the lexicographically smallest member, reporting
    // the lexicographically smallest simple cycle through the
    // anchor. Self-import is the one-node case.
    let resolved: BTreeSet<&str> = modules.iter().map(|m| m.identity.as_str()).collect();
    let graph_edges: Vec<(String, String, Span)> = edges
        .into_iter()
        .filter(|(f, t, _)| resolved.contains(f.as_str()) && resolved.contains(t.as_str()))
        .collect();
    for scc in cyclic_sccs(&resolved, &graph_edges) {
        let anchor = *scc.first().expect("nonempty SCC");
        if scc.len() == 1 {
            let span = graph_edges
                .iter()
                .find(|(from, to, _)| *from == anchor && *to == anchor)
                .map(|(_, _, span)| *span)
                .expect("single-member cyclic SCC has a self edge");
            diagnostics.push(Diagnostic::error(
                codes::IMPORT_CYCLE,
                format!("module imports itself: `{anchor}`"),
                Label::new(span, ""),
            ));
        } else {
            let path = canonical_cycle(anchor, &scc, &graph_edges);
            let next = path
                .get(1)
                .expect("nontrivial canonical cycle has a next member");
            let span = graph_edges
                .iter()
                .find(|(from, to, _)| *from == anchor && to == next)
                .map(|(_, _, span)| *span)
                .expect("canonical cycle first edge exists");
            diagnostics.push(Diagnostic::error(
                codes::IMPORT_CYCLE,
                format!("import cycle: {}", path.join(" -> ")),
                Label::new(span, ""),
            ));
        }
    }

    // Normative processing order (SPEC v5.2 §17 / ADR-078):
    // dependency post-order with lexicographic tie-breaks. Only
    // meaningful for acyclic units; cyclic units are already
    // rejected above, and their module list keeps discovery order.
    let order = diagnostics
        .is_empty()
        .then(|| normative_order(&resolved, &graph_edges));
    drop(resolved);
    if let Some(order) = order {
        modules.sort_by_key(|m| order[m.identity.as_str()]);
    }

    let mut output = ResolveOutput {
        language_version: version,
        modules,
        map,
        diagnostics,
        import_edges: graph_edges
            .into_iter()
            .map(|(from, to, _)| (from, to))
            .collect(),
        name_facts: NameResolutionFacts::default(),
    };
    // Name resolution (SPEC v5.2 §17) runs once the unit's shape is
    // settled; a cyclic or unresolved unit reports those errors
    // first.
    if output.diagnostics.is_empty() {
        names::check(&mut output);
    }
    if output.diagnostics.is_empty() {
        init_rule::check(&mut output);
    }
    output
}

fn generated_std_module_allowed(identity: &str, version: LangVersion) -> bool {
    !matches!(identity, "std.lispex" | "std.lispex.rules") || version >= LangVersion::V5_18
}

/// Cyclic strongly connected components (size > 1, or a self-loop),
/// each sorted lexicographically, the list sorted by anchor.
fn cyclic_sccs<'a>(
    nodes: &BTreeSet<&'a str>,
    edges: &[(String, String, Span)],
) -> Vec<Vec<&'a str>> {
    // Tarjan, iterative over an index mapping.
    let index_of: BTreeMap<&str, usize> = nodes
        .iter()
        .copied()
        .enumerate()
        .map(|(i, name)| (name, i))
        .collect();
    let names: Vec<&str> = nodes.iter().copied().collect();
    let n = names.len();
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut self_loop = vec![false; n];
    for (f, t, _) in edges {
        let (fi, ti) = (index_of[f.as_str()], index_of[t.as_str()]);
        if fi == ti {
            self_loop[fi] = true;
        }
        succ[fi].push(ti);
    }
    let mut index = vec![usize::MAX; n];
    let mut low = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut counter = 0usize;
    let mut result: Vec<Vec<&str>> = Vec::new();

    enum Frame {
        Enter(usize),
        Resume(usize, usize),
    }
    for start in 0..n {
        if index[start] != usize::MAX {
            continue;
        }
        let mut call = vec![Frame::Enter(start)];
        while let Some(frame) = call.pop() {
            match frame {
                Frame::Enter(v) => {
                    index[v] = counter;
                    low[v] = counter;
                    counter += 1;
                    stack.push(v);
                    on_stack[v] = true;
                    call.push(Frame::Resume(v, 0));
                }
                Frame::Resume(v, mut i) => {
                    let mut descended = false;
                    while i < succ[v].len() {
                        let w = succ[v][i];
                        i += 1;
                        if index[w] == usize::MAX {
                            call.push(Frame::Resume(v, i));
                            call.push(Frame::Enter(w));
                            descended = true;
                            break;
                        } else if on_stack[w] {
                            low[v] = low[v].min(index[w]);
                        }
                    }
                    if descended {
                        continue;
                    }
                    if low[v] == index[v] {
                        let mut component = Vec::new();
                        while let Some(w) = stack.pop() {
                            on_stack[w] = false;
                            component.push(names[w]);
                            if w == v {
                                break;
                            }
                        }
                        component.sort();
                        if component.len() > 1 || self_loop[index_of[component[0]]] {
                            result.push(component);
                        }
                    } else if let Some(Frame::Resume(parent, _)) = call.last() {
                        let parent = *parent;
                        low[parent] = low[parent].min(low[v]);
                    }
                }
            }
        }
    }
    result.sort();
    result
}

/// Lexicographically smallest simple cycle through `anchor` inside
/// `scc` (closure-scoped units are small; bounded DFS).
fn canonical_cycle<'a>(
    anchor: &'a str,
    scc: &[&'a str],
    edges: &'a [(String, String, Span)],
) -> Vec<&'a str> {
    let mut successors: BTreeMap<&str, BTreeSet<&str>> = scc
        .iter()
        .copied()
        .map(|member| (member, BTreeSet::new()))
        .collect();
    for (from, to, _) in edges {
        if successors.contains_key(to.as_str())
            && let Some(from_successors) = successors.get_mut(from.as_str())
        {
            from_successors.insert(to.as_str());
        }
    }

    CanonicalCycleSearch::new(anchor)
        .find(&successors)
        .expect("cyclic SCC has a cycle through its anchor")
}

struct CanonicalCycleSearch<'a> {
    anchor: &'a str,
    path: Vec<&'a str>,
    visited: BTreeSet<&'a str>,
}

impl<'a> CanonicalCycleSearch<'a> {
    fn new(anchor: &'a str) -> Self {
        Self {
            anchor,
            path: vec![anchor],
            visited: BTreeSet::from([anchor]),
        }
    }

    fn find(mut self, successors: &BTreeMap<&'a str, BTreeSet<&'a str>>) -> Option<Vec<&'a str>> {
        if !self.visit(self.anchor, successors) {
            return None;
        }
        self.path.push(self.anchor);
        Some(self.path)
    }

    fn visit(
        &mut self,
        current: &'a str,
        successors: &BTreeMap<&'a str, BTreeSet<&'a str>>,
    ) -> bool {
        for &next in &successors[current] {
            if next == self.anchor && self.path.len() > 1 {
                return true;
            }
            if self.visited.insert(next) {
                self.path.push(next);
                if self.visit(next, successors) {
                    return true;
                }
                self.path.pop();
                self.visited.remove(next);
            }
        }
        false
    }
}

/// ADR-078 normative order: repeatedly take the lexicographically
/// smallest module whose dependencies are all processed (dependency
/// post-order with lexicographic tie-breaks). Defined for acyclic
/// units.
fn normative_order(
    nodes: &BTreeSet<&str>,
    edges: &[(String, String, Span)],
) -> BTreeMap<String, usize> {
    let mut remaining = nodes.clone();
    let mut done: BTreeSet<&str> = BTreeSet::new();
    let mut order = BTreeMap::new();
    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .find(|m| {
                edges
                    .iter()
                    .filter(|(f, _, _)| f == *m)
                    .all(|(_, t, _)| done.contains(t.as_str()) || !nodes.contains(t.as_str()))
            })
            .copied();
        match ready {
            Some(m) => {
                remaining.remove(m);
                done.insert(m);
                order.insert(m.to_string(), order.len());
            }
            None => break, // cyclic remainder; callers reject cycles first
        }
    }
    order
}

enum SegmentOutcome {
    Ok,
    /// Two or more key-equal candidates were observed.
    Collision(String),
    /// No exact-scalar candidate exists for a segment (even if a
    /// case-insensitive filesystem would open something).
    MissingExact,
    /// The directory entries needed to prove exact-scalar admission could not
    /// cross the loader boundary.
    Unreadable(String),
}

/// Walks one dotted path's directories under `root`. At each step
/// the target segment must exist by exact Unicode scalars among the
/// directory's entries, and at most one candidate may match the
/// segment's NFD / case-fold keys (SPEC v5.2 §17 — no silent
/// choice).
fn segment_walk(provider: &dyn FileProvider, root: &str, segments: &[&str]) -> SegmentOutcome {
    let mut dir = root.to_string();
    for (i, segment) in segments.iter().enumerate() {
        let last = i + 1 == segments.len();
        let target_name = if last {
            format!("{segment}.tpz")
        } else {
            (*segment).to_string()
        };
        let target_nfd = norm::nfd(&target_name);
        let target_fold = norm::casefold(&target_nfd);
        let mut exact = false;
        let mut matches: Vec<String> = Vec::new();
        let entries = match provider.read_directory(&dir) {
            DirectoryRead::Present(entries) => entries,
            DirectoryRead::Missing => return SegmentOutcome::MissingExact,
            DirectoryRead::Unreadable { reason_code } => {
                return SegmentOutcome::Unreadable(reason_code);
            }
        };
        for (name, is_dir) in entries {
            if last == is_dir {
                continue; // final segment wants a file; earlier want dirs
            }
            if name == target_name {
                exact = true;
            }
            let name_nfd = norm::nfd(&name);
            if name_nfd == target_nfd || norm::casefold(&name_nfd) == target_fold {
                matches.push(name);
            }
        }
        if matches.len() > 1 {
            return SegmentOutcome::Collision(format!(": {}", matches.join(" / ")));
        }
        if !exact {
            return SegmentOutcome::MissingExact;
        }
        dir = if dir.is_empty() {
            (*segment).to_string()
        } else {
            format!("{dir}/{segment}")
        };
    }
    SegmentOutcome::Ok
}

/// Whether `path`'s physical location escapes the root's physical
/// location (virtual links in fixtures; canonicalized paths on the
/// real filesystem).
fn escapes_root(provider: &dyn FileProvider, root: &str, path: &str) -> bool {
    let Some(physical) = provider.physical_id(path) else {
        return false; // nonexistent: the unresolved path reports it
    };
    let physical = physical.replace('\\', "/");
    if physical.split('/').any(|s| s == "..") {
        return true;
    }
    let Some(root_physical) = provider.physical_id(root) else {
        return false;
    };
    let root_physical = root_physical.replace('\\', "/");
    if root_physical.is_empty() {
        return false;
    }
    physical != root_physical && !physical.starts_with(&format!("{root_physical}/"))
}

/// Dotted logical identity of a root-relative file path.
fn identity_of(root: &str, path: &str) -> String {
    let rel = match path.strip_prefix(root) {
        Some(stripped) => stripped.trim_start_matches('/'),
        None => path,
    };
    rel.strip_suffix(".tpz").unwrap_or(rel).replace('/', ".")
}
