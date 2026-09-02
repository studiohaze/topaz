//! Resolver diagnostic codes (TPZ3xxx range, CDR-002 §6).
//!
//! These codes are **stable once fixture-pinned** in
//! `corpus/v5.2/modules/`, each as the asserted primary diagnostic.
//! Renumbering or removal of a pinned code is a breaking change and
//! requires a design-record decision.

use topaz_diag::Code;

/// A module path that resolves to no file under the root (SPEC v5.2
/// §17: exact-scalar mapping; `.tpz` exactly). Pinned in
/// `corpus/v5.2/modules/`.
pub const UNRESOLVED_MODULE: Code = Code::new("TPZ3001");
/// An explicit `--root` that does not contain the entry file (SPEC
/// v5.2 §17). Pinned in `corpus/v5.2/modules/`.
pub const ROOT_CONTAINMENT: Code = Code::new("TPZ3002");
/// A module source or directory listing that cannot cross the loader boundary
/// because it is unreadable, is not valid UTF-8, has an unrepresentable entry,
/// or exceeds `topaz_diag::MAX_SOURCE_LEN`. Reported as a diagnostic, never a
/// panic.
pub const SOURCE_BOUND: Code = Code::new("TPZ3003");
/// Module-name collision under the three keys (exact scalar,
/// NFC/NFD canonical equivalence, default case fold) among the
/// candidates observed while resolving (SPEC v5.2 §17). Pinned in
/// `corpus/v5.2/modules/`.
pub const MODULE_COLLISION: Code = Code::new("TPZ3004");
/// A module path whose physical location escapes the root
/// (symlink/alias containment, SPEC v5.2 §17). Pinned in
/// `corpus/v5.2/modules/`.
pub const PHYSICAL_CONTAINMENT: Code = Code::new("TPZ3005");
/// An import cycle (SPEC v5.2 §17: every cycle is a static error,
/// including self-import; one diagnostic per cyclic SCC, attributed
/// to the lexicographically smallest member, reporting the
/// lexicographically smallest simple cycle through that anchor).
/// Pinned in `corpus/v5.2/modules/`.
pub const IMPORT_CYCLE: Code = Code::new("TPZ3006");
/// A runtime-bearing free statement at the top level of an imported
/// module (SPEC v5.2 §17: build-role-relative — the same file may be
/// a valid entry). Pinned in `corpus/v5.2/modules/`.
pub const IMPORTED_FREE_STATEMENT: Code = Code::new("TPZ3007");
/// A collision in the single module lexical namespace (SPEC v5.2
/// §17). Pinned in `corpus/v5.2/modules/`.
pub const NAME_COLLISION: Code = Code::new("TPZ3008");
/// Accessing or selecting a name a module does not export (SPEC
/// v5.2 §17). Pinned in `corpus/v5.2/modules/`.
pub const NOT_EXPORTED: Code = Code::new("TPZ3009");
/// Importing a module that exports nothing (SPEC v5.2 §17: no
/// side-effect-only imports). Pinned in `corpus/v5.2/modules/`.
pub const ZERO_EXPORT_IMPORT: Code = Code::new("TPZ3010");
/// `export let mut` (SPEC v5.2 §17: exported bindings are immutable
/// views). Pinned in `corpus/v5.2/modules/`.
pub const EXPORT_LET_MUT: Code = Code::new("TPZ3011");
/// A namespace binding used as a value (SPEC v5.2 §17). Pinned in
/// `corpus/v5.2/modules/`.
pub const NAMESPACE_NOT_VALUE: Code = Code::new("TPZ3012");
/// A namespace member of the wrong kind: keyword-named, or a value
/// export in type position (SPEC v5.2 §17). Pinned in
/// `corpus/v5.2/modules/`.
pub const NAMESPACE_MEMBER_KIND: Code = Code::new("TPZ3013");
/// A module-private type alias in an exported public surface (SPEC
/// v5.2 §17). Pinned in `corpus/v5.2/modules/`.
pub const PRIVATE_TYPE_IN_EXPORT: Code = Code::new("TPZ3014");
/// Assignment to an imported binding (SPEC v5.2 §17: imports grant
/// read access only). Pinned in `corpus/v5.2/modules/`.
pub const READONLY_IMPORT: Code = Code::new("TPZ3015");
/// An import addressing a reserved module path root (`std`,
/// `topaz` — SPEC v5.2 §17). Pinned in `corpus/v5.2/modules/`.
pub const RESERVED_ROOT: Code = Code::new("TPZ3016");
/// A logical module imported by more than one import item in the
/// same module (SPEC v5.2 §17: at most one item per module;
/// reported before name-collision diagnostics). Pinned in
/// `corpus/v5.2/modules/`.
pub const DUPLICATE_IMPORT: Code = Code::new("TPZ3017");
/// An imported-module binding initializer that DIRECTLY reaches a
/// later same-module runtime binding (`j >= k`) in an
/// immediately-evaluated position (SPEC v5.2 §17, the initializer
/// reference rule, narrowed to match §4: delayed positions —
/// short-circuit RHS, optional-call arguments, and
/// function/lambda/`defer`/branch/arm/loop bodies — are not scanned and
/// fault dynamically if reached during init). Pinned in
/// `corpus/v5.2/modules/`.
pub const INIT_FORWARD_REFERENCE: Code = Code::new("TPZ3018");
/// A manifest extern namespace is imported, but the exact extern module/function
/// is not declared by `topaz.toml`.
pub const EXTERN_DECL: Code = Code::new(topaz_diag::extern_codes::DECL);
