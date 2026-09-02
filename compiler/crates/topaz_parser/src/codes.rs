//! Parser diagnostic codes (TPZ2xxx range, CDR-001 §5).
//!
//! These codes are **stable once fixture-pinned**: v5.1-era codes
//! are pinned in `corpus/v5.1/invalid/`, v5.2-era codes in
//! `corpus/v5.2/` (CDR-002 §3), each as the asserted primary
//! diagnostic. Renumbering or removal of a pinned code is a breaking
//! change to downstream consumers and requires a design-record
//! decision.

use topaz_diag::Code;

/// A token that does not fit the grammar at this point.
pub const UNEXPECTED_TOKEN: Code = Code::new("TPZ2001");
/// A template tag outside the registry {`p`, `r`, `sh`, `sql`}
/// (SPEC §16).
pub const UNKNOWN_TEMPLATE_TAG: Code = Code::new("TPZ2002");
/// An assignment target that is not an identifier, member access, or
/// index access (SPEC §5).
pub const INVALID_ASSIGNMENT_TARGET: Code = Code::new("TPZ2003");
/// A `defer` body that is neither a block nor a call (SPEC §14).
pub const INVALID_DEFER_BODY: Code = Code::new("TPZ2004");
/// A `concurrent` form mismatch: `else` without a timeout, or a
/// timeout without `else` (SPEC §15).
pub const CONCURRENT_FORM: Code = Code::new("TPZ2005");
/// An or-pattern alternative that binds names (SPEC v5.2 §6,
/// ADR-073: alternatives must bind no names; `_` is not a binding).
/// Pinned in `corpus/v5.2/syntax/`.
pub const OR_PATTERN_BINDING: Code = Code::new("TPZ2006");
/// An exported `let` whose pattern is not exactly one identifier
/// (SPEC v5.2 §17). Pinned in `corpus/v5.2/compat/module-eligible/`.
pub const EXPORT_BINDING_FORM: Code = Code::new("TPZ2007");
/// A reserved-unused module form: `use` items, string/template
/// module paths (SPEC v5.2 §17 — diagnostics, no semantics).
/// Pinned in `corpus/v5.2/compat/module-eligible/`.
pub const RESERVED_MODULE_FORM: Code = Code::new("TPZ2008");
/// A rejected module-adjacent form: export lists, `export import`,
/// alias+selection composition (SPEC v5.2 §17). Pinned in
/// `corpus/v5.2/compat/module-eligible/`.
pub const REJECTED_MODULE_FORM: Code = Code::new("TPZ2009");
/// An import item after a non-import top-level item (SPEC v5.2 §17:
/// imports form a prologue). Pinned in
/// `corpus/v5.2/compat/module-eligible/`.
pub const IMPORT_PROLOGUE: Code = Code::new("TPZ2010");
/// A malformed selection list (SPEC v5.2 §17): empty list, duplicate
/// selected source name, duplicate bound local name, or a keyword
/// entry (`ImportSpec` names are `Identifier` only). Pinned in
/// `corpus/v5.2/modules/`.
pub const IMPORT_LIST_FORM: Code = Code::new("TPZ2011");
/// `None` used as a binding name (SPEC v5.2 §22.1: `None` is a
/// polymorphic constructor value, not an ordinary variable; §6 makes
/// bare `None` a constructor pattern, so no pattern position can
/// bind it).
pub const RESERVED_BINDING_NAME: Code = Code::new("TPZ2012");

/// `~` is reserved (TPZ2013): Topaz is arithmetic-only and defines no bitwise
/// operations. The lexer keeps the `~` token for recovery; the parser rejects
/// it here rather than producing a unary operator.
pub const RESERVED_OPERATOR: Code = Code::new("TPZ2013");
