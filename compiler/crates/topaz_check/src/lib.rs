//! topaz_check — the static type checker (CDR-004).
//!
//! Staged phases: C-1 forms and validates every type position
//! (alias bodies, signatures, annotations, typed patterns); C-2
//! types the expression core, suppressing every check that touches
//! an Unknown so unimplemented forms can never false-positive.

use std::collections::BTreeMap;

/// The Topaz toolchain/package version (the workspace version, e.g. `5.4.0-dev`). Exposed from a
/// NON-vendored crate (the emit closure vendors topaz_diag/syntax/value/rt, whose sources may
/// not use `env!`), so tools like the Living Docs reproducibility stamp can report the exact
/// toolchain. Distinct from the language edition `LangVersion`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod unit;
pub use unit::{
    ExportedAlias, ExportedEnum, ExportedEnumVariant, ExportedNewtype, ExportedValue,
    ModuleExports, UnitModule, check_unit, check_unit_with_version,
};

pub mod typed;
pub use typed::{
    CheckedUnit, HoverType, check_program_typed, check_program_typed_with_version,
    check_unit_typed, check_unit_typed_with_version, mono_of,
};

pub mod builtins;
pub mod expr;
pub mod form;
mod forward;
pub mod subtype;
pub mod ty;

use topaz_diag::Diagnostic;
use topaz_syntax::ast;

pub use expr::ExprChecker;
pub use form::Former;
pub use subtype::is_subtype;
pub use ty::{Ctor, Lit, Prim, Type};

/// Static-semantics diagnostic codes. TPZ5001–5008 graduate from the
/// interpreter's dynamic guards (CDR-004 §6); checker-only codes
/// start at TPZ5020.
pub mod codes {
    use topaz_diag::{Code, guard_codes};

    pub const TYPE_MISMATCH: Code = Code::new(guard_codes::TYPE);
    pub const UNBOUND: Code = Code::new(guard_codes::UNBOUND);
    pub const IMMUTABLE: Code = Code::new(guard_codes::IMMUTABLE);
    pub const ARITY: Code = Code::new(guard_codes::ARITY);
    pub const NOT_CALLABLE: Code = Code::new(guard_codes::NOT_CALLABLE);
    pub const NO_FIELD: Code = Code::new(guard_codes::NO_FIELD);
    pub const INCOMPARABLE: Code = Code::new(guard_codes::COMPARE);
    pub const REDECLARE: Code = Code::new(guard_codes::REDECLARE);

    pub const UNSOLVED: Code = Code::new("TPZ5020");
    pub const NON_EXHAUSTIVE: Code = Code::new("TPZ5021");
    pub const MALFORMED_TYPE: Code = Code::new("TPZ5022");
    pub const ALIAS_CYCLE: Code = Code::new("TPZ5023");
    pub const VARIADIC_POSITION: Code = Code::new("TPZ5024");
    pub const INVALID_QUALIFIED: Code = Code::new("TPZ5025");

    /// §4 (v5.4) a destructuring `let` whose pattern is REFUTABLE — it does NOT
    /// cover every value of the scrutinee type (an enum variant when the enum has
    /// more than one variant, a literal, a range, a length-refutable list pattern at
    /// v5.4, or a record/nominal pattern with a refutable field). Such a `let` would
    /// pass `check` then FAULT at runtime when the value does not match (`let`
    /// pattern did not match the value, §4); rejecting it statically keeps
    /// check==runtime. Use `if let` for the non-matching case.
    pub const REFUTABLE_LET: Code = Code::new("TPZ5026");

    /// §6 (v5.4) a `map { … }` LITERAL declares the SAME constant key twice
    /// (e.g. `map { "a": 1, "a": 2 }`). A statically-decidable duplicate (string /
    /// int / bool literal keys) is a CHECK error here; a duplicate among runtime
    /// VALUES is a runtime fault (TPZ4601) instead — the two are the same policy
    /// graduated to compile time where the keys are constants.
    pub const DUPLICATE_MAP_KEY: Code = Code::new("TPZ5602");

    /// §6.4 (v5.4) a comprehension's BODY type does not match the expected
    /// element/key/value type (e.g. an annotated `Array<int>` comprehension whose
    /// body yields a string). Reported when the body widens to a type that conflicts
    /// with the contextual element type.
    pub const COMP_BODY_MISMATCH: Code = Code::new("TPZ5610");
    /// §6.4 (v5.4) a `map { for … => body }` comprehension whose body is NOT a
    /// `key: value` entry. (The parser already requires the `:` shape, so this is a
    /// defensive guard for any future body form.)
    pub const COMP_MAP_BODY: Code = Code::new("TPZ5611");
    /// §6.4 (v5.4) an EMPTY comprehension — one whose element/key/value type cannot
    /// be inferred (no surviving iterations could constrain it, or the body type is
    /// otherwise unconstrained) AND there is no expected (contextual) type to fix it.
    /// Annotate the binding (e.g. `let xs: Array<int> = [ for … ]`).
    pub const COMP_EMPTY: Code = Code::new("TPZ5612");

    /// §3 (v5.4) the count of explicit call-site type arguments
    /// `f<T, U>(args)` does not match the callee's type-parameter count.
    pub const TYPE_ARG_ARITY: Code = Code::new("TPZ5510");
    /// §3 (v5.4) explicit type arguments were supplied to a callee that
    /// is not generic.
    pub const NO_TYPE_ARGS: Code = Code::new("TPZ5512");

    /// §4 (v5.4) a `derives` clause names a protocol that cannot be derived for
    /// the type — an UNKNOWN protocol name (not `Eq`/`Order`/`Show`/`JSON`),
    /// `Eq`/`Order` on a type with a non-comparable field/payload, or `JSON` on a
    /// type with a non-JSON round-trippable field/payload.
    pub const NOT_DERIVABLE: Code = Code::new("TPZ5530");

    /// §4.2 (v5.4) the COHERENCE / orphan rule: a manual `impl Protocol<Type>` is
    /// allowed only when the PROTOCOL or the TYPE is own-module. Both being foreign
    /// (here: both builtin, since cross-module impls aren't in this slice) is an
    /// ORPHAN impl. `impl Show<int>` etc.
    pub const ORPHAN_IMPL: Code = Code::new("TPZ5520");
    /// §4.2 (v5.4) a DUPLICATE conformance: a type already conforms to a protocol
    /// (via `derives` or a previous `impl`) and a second `impl Protocol<Type>` would
    /// register it twice. The conformance must be UNIQUE (no overlapping impls).
    pub const DUPLICATE_IMPL: Code = Code::new("TPZ5521");
    /// §4.1 (v5.4) a `Protocol.method(x)` static dispatch where the receiver's type
    /// does NOT conform to the protocol (no `derives` clause, no `impl`), OR the
    /// protocol has no such method. The clear "type X does not conform to P" /
    /// "protocol P has no method m" diagnostic.
    pub const NO_CONFORMANCE: Code = Code::new("TPZ5522");
    /// §4 (v5.4) one function type parameter repeats the same protocol in its
    /// conjunctive bound list. Bound conjunctions are unique sets, not an
    /// idempotent syntax that silently discards a repeated requirement.
    pub const DUPLICATE_BOUND: Code = Code::new("TPZ5523");
    /// §4/§17 (v5.4) an exported function exposes a module-local user protocol
    /// bound. User protocol definitions and manual witnesses do not cross module
    /// interfaces; only the four predeclared singleton protocols may appear.
    pub const NON_EXPORTABLE_BOUND: Code = Code::new("TPZ5524");

    /// §6 (v5.4) BINDING or-pattern AGREEMENT: the alternatives of an or-pattern
    /// `case A(x) | B(x) =>` must bind the SAME set of names, so that whichever
    /// alternative matches, the shared arm body sees every binding. A name bound by
    /// one alternative but missing from another (`case A(x) | B(y) =>`) is a
    /// TPZ5710 — the arm body could reference `x` after the `B(y)` alternative
    /// matched, leaving it unbound.
    pub const OR_PATTERN_NAMES: Code = Code::new("TPZ5710");
    /// §6 (v5.4) BINDING or-pattern TYPE agreement: a name bound by more than one
    /// alternative must have a UNIFYING type across them (`case A(x) | C(x) =>`
    /// where the `A`/`C` payloads differ), else the arm body would see `x` at an
    /// inconsistent type depending on which alternative matched. TPZ5711.
    pub const OR_PATTERN_TYPES: Code = Code::new("TPZ5711");

    /// §22 (v5.4) the argument to `JSON.stringify` has a type that is NOT
    /// JSON-encodable (a `float`, a `Result`/`Set`/`range`, a `Map` with a
    /// non-string key, a function/`File`/template, or a nominal record/enum/newtype
    /// whose fields/payloads/base include any of those). Gated at the CALL SITE so
    /// check==runtime — the shared `encode_json` leaf would otherwise return a runtime
    /// `Err`.
    pub const NOT_JSON_ENCODABLE: Code = Code::new("TPZ5533");
    /// §22 (v5.4) the target type argument of `JSON.parseAs<T>` or
    /// `JSON.decode<T>` is not statically JSON-decodable, or the call omitted the
    /// explicit target type needed to build a runtime schema.
    pub const NOT_JSON_DECODABLE: Code = Code::new("TPZ5534");

    /// §5 S6 (v5.4) a labeled `break 'l`/`continue 'l` names a loop label `'l`
    /// that is NOT in scope — there is no enclosing `loop 'l` (or it lies across
    /// a function/lambda boundary, which loop control may not cross). Statically
    /// rejected so it never reaches the interpreter's runtime "no loop labeled"
    /// fault.
    pub const NO_LOOP_LABEL: Code = Code::new("TPZ5720");
    /// §5 S6 (v5.4) the `break <value>` values targeting one `loop` expression do
    /// NOT all have the same type — `loop { break 1\n break "x" }` would make the
    /// loop's value type ambiguous. Every value-break (and a value-less `break`,
    /// which contributes Unit) targeting a loop must agree. TPZ5721.
    pub const BREAK_VALUE_MISMATCH: Code = Code::new("TPZ5721");
}

pub struct CheckOutput {
    pub diagnostics: Vec<Diagnostic>,
    /// The checked public export surface, keyed by logical module
    /// identity. CLI/web tooling renders this as a deterministic artifact only
    /// when the unit has no errors.
    pub exports: BTreeMap<String, ModuleExports>,
    /// Checked top-level type aliases for every module, including private aliases.
    /// This is not a public export surface. Backends use it only after the checker
    /// has resolved aliases and rejected cycles.
    pub local_aliases: BTreeMap<String, BTreeMap<String, ExportedAlias>>,
    /// §4 (v5.4) the DERIVE CONFORMANCE table: every `(protocol, type_id)` pair a
    /// `derives Eq, Order, Show` clause authorized for a record/enum. Derivation is
    /// checker-only bookkeeping — this surfaces the authorized capability for
    /// protocol-call dispatch, JSON derive metadata, and witness tests.
    /// Sorted for determinism. The multi-module entry aggregates the local
    /// conformance tables collected while checking each module.
    pub conformances: Vec<(String, String)>,
}

/// Entry: validates every type position (C-1) and types the expression core
/// (C-2) of a single program, at the CURRENT language version (the convenience
/// entry; the doc/test harnesses use this).
pub fn check_program(src: &str, program: &ast::Program) -> CheckOutput {
    check_program_with_version(src, program, topaz_syntax::LangVersion::CURRENT)
}

/// [`check_program`] pinned to a language `version` — so v5.4-only enum features
/// gate by edition.
pub fn check_program_with_version(
    src: &str,
    program: &ast::Program,
    version: topaz_syntax::LangVersion,
) -> CheckOutput {
    let mut former = Former::with_version(src, program, version);
    former.validate_aliases();
    let mut checker = ExprChecker::new(former);
    checker.check_items(&program.items);
    let mut conformances: Vec<(String, String)> = checker
        .former
        .conformances()
        .map(|(protocol, type_id)| (protocol.to_string(), type_id.to_string()))
        .collect();
    conformances.sort();
    CheckOutput {
        diagnostics: checker.former.diagnostics,
        exports: BTreeMap::new(),
        local_aliases: BTreeMap::new(),
        conformances,
    }
}
