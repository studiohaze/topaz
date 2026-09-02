//! Expression typing (CDR-004 §4/§5, phases C-2/C-3).
//!
//! A staged bidirectional pass: forms this phase does not type yet
//! produce `Type::Unknown`, and every check involving an unknown is
//! suppressed — the checker never reports a false positive. Concrete
//! violations graduate the guards: TPZ5001 (type mismatch), TPZ5004
//! (arity), TPZ5005 (not callable), TPZ5006 (no such member),
//! TPZ5007 (incomparable), plus the §22.1 contextual-typing rule
//! (TPZ5020 unsolved type variables).

use std::collections::{HashMap, HashSet};

use topaz_diag::Span;
use topaz_syntax::LangVersion;
use topaz_syntax::ast;

use crate::builtins::{self, Member, Scheme};
use crate::codes;
use crate::form::{
    EnumInfo, EnumVariantInfo, Former, NewtypeInfo, RecordInfo, nominal_instance_id,
};
use crate::subtype::is_subtype;
use crate::ty::{Ctor, Lit, NonKeyableKey, Prim, Type, non_keyable_map_set_key_with_nominals};
use crate::unit::{
    ExportedAlias, ExportedEnum, ExportedEnumVariant, ExportedNewtype, ExportedNominals,
    ExportedReceiverMethod, ExportedRecord, ExportedRecordField, ExportedValue, ModuleExports,
};

/// Partial (join-space) inference vars are minted above this offset
/// so they can never collide with scheme-local var indices — the
/// occurs check and per-call substitutions stay meaningful across
/// the two spaces (scheme substs simply never index this high).
const PARTIAL_VAR_OFFSET: u32 = 1 << 20;

/// §6 (v5.4) the compile-time-constant VALUE of a `map { … }` literal key, used
/// to detect statically-obvious duplicate keys (TPZ5602). Only the literal forms
/// whose value is decidable at check time are represented; everything else is
/// compared dynamically (TPZ4601) and never reaches this enum.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ConstKey {
    Int(i64),
    Bool(bool),
    Str(String),
}

/// Callable metadata for a user binding (declared functions,
/// imported functions, and aliases that carry it). Lambdas and
/// other fn-typed values have none.
#[derive(Debug, Clone)]
struct FnMeta {
    /// Rank-1 type-parameter count.
    vars: u32,
    /// Protocol bounds aligned with scheme variables.
    bounds: Vec<Vec<String>>,
    /// Non-defaulted fixed-parameter count.
    required: usize,
    /// Fixed-parameter names; None when unavailable, keeping named
    /// arguments unjudged (§5).
    names: Option<Vec<String>>,
    /// Per fixed parameter: declared with a default.
    defaulted: Vec<bool>,
}

#[derive(Debug, Clone)]
struct ApplyOutcome {
    ty: Type,
    subst: Vec<Option<Type>>,
}

enum CallSortGate {
    Element(Type, String),
    Key(String),
}

#[derive(Default)]
struct CallGates {
    check_inferred_key: bool,
    sort: Option<CallSortGate>,
    json_encode: bool,
    json_decode: Option<String>,
    equality_assertion: Option<String>,
    protocol_bounds: Vec<Vec<String>>,
}

struct ResolvedCall {
    scheme: Scheme,
    iterable_param_fixup: bool,
    target_identity: Option<String>,
}

struct EnumConstruction {
    result: Type,
    callee_type: Option<Type>,
}

struct CallCompletion<'ast, 'context> {
    callee: &'ast ast::Expr,
    site: CallSite<'ast, 'context>,
    lispex_rule_target: Option<String>,
    resolved: Option<ResolvedCall>,
    gates: CallGates,
}

struct CallRequest<'ast, 'context> {
    callee: &'ast ast::Expr,
    args: &'ast [ast::CallArg],
    type_args: &'ast [ast::Type],
    context: Option<&'context Type>,
    span: Span,
    bare: bool,
    leading: Option<&'context Type>,
}

#[derive(Clone, Copy)]
struct CallSite<'ast, 'context> {
    args: &'ast [ast::CallArg],
    type_args: &'ast [ast::Type],
    context: Option<&'context Type>,
    span: Span,
    bare: bool,
    leading: Option<&'context Type>,
    is_print: bool,
}

#[derive(Clone, Copy)]
struct OptionalCallInput<'ast, 'context> {
    args: &'ast [ast::CallArg],
    leading: Option<&'context Type>,
    callee_span: Span,
}

enum CallResolution {
    Apply(ResolvedCall),
    Complete(Type),
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GateCheck {
    Accept,
    Reject,
    Defer,
}

impl GateCheck {
    fn and(self, other: GateCheck) -> GateCheck {
        match (self, other) {
            (GateCheck::Reject, _) | (_, GateCheck::Reject) => GateCheck::Reject,
            (GateCheck::Defer, _) | (_, GateCheck::Defer) => GateCheck::Defer,
            (GateCheck::Accept, GateCheck::Accept) => GateCheck::Accept,
        }
    }
}

#[derive(Clone, Copy)]
enum ExpressionFamily {
    Atomic,
    Control,
    Operation,
    Aggregate,
}

fn expression_family(kind: &ast::ExprKind) -> ExpressionFamily {
    match kind {
        ast::ExprKind::Int
        | ast::ExprKind::Float
        | ast::ExprKind::Bool(_)
        | ast::ExprKind::Null
        | ast::ExprKind::Unit
        | ast::ExprKind::Duration(_)
        | ast::ExprKind::String(_)
        | ast::ExprKind::Ident
        | ast::ExprKind::Placeholder
        | ast::ExprKind::Paren(_)
        | ast::ExprKind::Block(_) => ExpressionFamily::Atomic,
        ast::ExprKind::If { .. }
        | ast::ExprKind::Match { .. }
        | ast::ExprKind::For { .. }
        | ast::ExprKind::Loop { .. }
        | ast::ExprKind::Concurrent { .. } => ExpressionFamily::Control,
        ast::ExprKind::Call { .. }
        | ast::ExprKind::Member { .. }
        | ast::ExprKind::OptionalAccess { .. }
        | ast::ExprKind::Index { .. }
        | ast::ExprKind::Try(_)
        | ast::ExprKind::Unary { .. }
        | ast::ExprKind::Binary { .. }
        | ast::ExprKind::Range { .. }
        | ast::ExprKind::Compose { .. }
        | ast::ExprKind::Pipe { .. }
        | ast::ExprKind::Lambda { .. } => ExpressionFamily::Operation,
        ast::ExprKind::RecordLiteral { .. }
        | ast::ExprKind::RecordUpdate { .. }
        | ast::ExprKind::Array(_)
        | ast::ExprKind::SetLiteral(_)
        | ast::ExprKind::MapLiteral(_)
        | ast::ExprKind::Comprehension { .. } => ExpressionFamily::Aggregate,
    }
}

#[derive(Clone, Copy)]
enum PatternFamily {
    Scalar,
    Constructor,
    Record,
    Sequence,
}

fn pattern_family(kind: &ast::PatternKind) -> PatternFamily {
    match kind {
        ast::PatternKind::Wildcard
        | ast::PatternKind::Binding(_)
        | ast::PatternKind::Literal(_)
        | ast::PatternKind::Range { .. }
        | ast::PatternKind::Typed { .. } => PatternFamily::Scalar,
        ast::PatternKind::Constructor { .. } => PatternFamily::Constructor,
        ast::PatternKind::Record(_) | ast::PatternKind::NominalRecord { .. } => {
            PatternFamily::Record
        }
        ast::PatternKind::List(_) | ast::PatternKind::Or(_) => PatternFamily::Sequence,
    }
}

/// One enclosing loop-control context. Pushed on entering a loop body
/// and popped on exit, mirroring the interpreter's loop frames so the checker's
/// loop-control resolution matches the runtime exactly. A `while` / statement
/// `for` is VALUELESS (`value_loop = false`): it catches a bare `break`/
/// `continue` but has no value, so a value-break's type is discarded (the
/// interpreter discards it too). A `loop` EXPRESSION is `value_loop = true`: each
/// `break <value>` targeting it pushes the value's type into `breaks`, and the
/// loop's type is the join of `breaks` (Unit when empty).
///
/// Collecting expression-`for` and comprehensions are NOT user loop targets. They
/// push a barrier frame: bare `break`/`continue` stops there with a check error,
/// while labeled control can still pass through to an enclosing labeled `loop`.
struct LoopFrame {
    /// The loop's optional label NAME (the text after `'`); `None` for an
    /// unlabeled loop (every `while`/`for` is unlabeled in this slice). A
    /// `break 'l`/`continue 'l` matches the nearest frame with `label == Some("l")`.
    label: Option<String>,
    /// True for a `loop` EXPRESSION (value-bearing); false for `while`/`for`.
    value_loop: bool,
    /// Whether a bare `break`/`continue` may target this frame. False only for
    /// non-loop collection boundaries such as expression-`for` and comprehensions.
    bare_target: bool,
    /// Diagnostic noun phrase for a non-target barrier.
    bare_error: Option<&'static str>,
    /// The contextual type the loop's value is expected to have (from a
    /// surrounding annotation/expectation), propagated to each `break <value>`
    /// so e.g. `let x: Option<T> = loop { break None }` checks `None` against
    /// `Option<T>`. `None` when the loop is in a bare position or valueless.
    expected: Option<Type>,
    /// The type of every value-bearing `break` targeting a VALUE loop, in source
    /// order — Unit for a value-less `break`. The loop value is
    /// `join_branches(breaks)`. Always empty for a valueless loop.
    breaks: Vec<Type>,
}

/// The observations owned by one function or method body check. Entering and
/// leaving a callable must move the return, loop, and partial-inference contexts
/// as one boundary; its caller alone decides how an inferred result updates the
/// declaration catalog.
struct CallableBodyCheck {
    returns: Vec<Type>,
    tail: Option<Type>,
    hit_pending_return: bool,
}

#[derive(Default)]
struct BindingScope {
    bindings: HashMap<String, Type>,
    /// Names declared `let mut` (§4: assignment requires a mutable binding —
    /// TPZ5003).
    mutable: HashSet<String>,
    /// Functions whose omitted return type is still being inferred. The
    /// innermost binding decides, so completed shadowing declarations do not
    /// leak recursion taint (CDR-004 §7).
    pending_returns: HashSet<String>,
    /// Function-value aliases point at the source binding's scope so pending
    /// state is consulted live and clears when the source completes.
    pending_links: HashMap<String, (usize, String)>,
    /// Callable metadata shares the binding's shadowing boundary; an inner
    /// binding without metadata hides an outer declaration's metadata.
    fn_meta: HashMap<String, FnMeta>,
}

pub struct ExprChecker<'a> {
    pub former: Former<'a>,
    /// Lexical binding scopes. Each frame owns all shadowing-sensitive facts for
    /// its bindings: types, mutability, pending-return links, and callable
    /// metadata.
    scopes: Vec<BindingScope>,
    /// The saved left-hand value of the pipeline stage currently
    /// being typed; `_` placeholders take this type (§11).
    pipe_value: Option<Type>,
    /// True while typing branch/arm bodies of a contextless
    /// if/match: unsolved results return PARTIALLY solved (with
    /// renamed inference vars) so the branch join can solve them
    /// against each other (§22.1 "match-arm expected type") —
    /// `Ok(v)` and `Err(e)` arms mutually complete a Result.
    collect_partial: bool,
    /// Var-renaming base for collected partials; monotonic.
    partial_base: u32,
    /// Whether the body being checked called a pending-return fn.
    hit_pending_ret: bool,
    /// Module-aware mode (CDR-004 C-6): the name space is closed,
    /// so unbound names are TPZ5002 instead of ambient silence.
    module_mode: bool,
    /// Namespace imports: bound name → the target's export surface.
    namespaces: HashMap<String, ModuleExports>,
    /// Type-parameter environments stack with function nesting.
    tyenv: Vec<HashMap<&'a str, Type>>,
    /// Protocol bounds for rigid body type parameters, by skolem id.
    skolem_bounds: Vec<HashMap<u32, HashSet<String>>>,
    /// The enclosing function's declared return type (None when
    /// omitted — then `ret_join` infers it from the body).
    ret_ctx: Vec<Option<Type>>,
    /// Collected `return` types of the enclosing omitted-return
    /// function (C-6: omitted returns infer as the body join).
    ret_join: Vec<Vec<Type>>,
    /// The stack of enclosing `loop` expressions, innermost last —
    /// mirroring `ret_ctx`/`ret_join` for loop labels. Each frame collects the
    /// types of every `break <value>` (and Unit for a value-less `break`) that
    /// targets it; the loop expression's type is their join. A `break 'l`/
    /// `continue 'l` resolves to the nearest frame whose `label` matches `'l`.
    /// SAVED/RESTORED at every function/lambda boundary so a loop does NOT leak
    /// across a nested closure (a `break` inside a lambda is a static error, not
    /// a jump to the outer loop).
    loop_ctx: Vec<LoopFrame>,
    /// Per-declaration skolem freshness for body type parameters.
    skolem_counter: u32,
    /// Ids of the synthetic projection skolems minted by `project`
    /// (`FieldOf<T, x>` &c). They self-register here so a leaked
    /// projection can be told apart from a real, nameable type
    /// parameter (including an enclosing function's `T`) when an
    /// omitted return type is finalized.
    projection_ids: Vec<u32>,
    /// True while inferring the initializer of an UNANNOTATED
    /// binding — the one site known to never grow a contextual type,
    /// so §22.1 unsolved errors may fire (CDR-004 §4). Everywhere
    /// else unsolved stays silent until that context site exists.
    at_bare_binding: bool,
    /// The top-level namespace of the program/module being checked,
    /// for the §4 init-order pass. A reference to one of these names
    /// that is unbound at its use site is a FORWARD reference, owned by
    /// `forward::TopLevel` — the bare type check must not also report
    /// it as a plain "not bound".
    top_level: Option<crate::forward::TopLevel>,
    /// True while checking a nominal record field default. This is the only
    /// expression context where a namespace-private immutable runtime value from
    /// another module may be used as an internal default dependency.
    record_default_depth: usize,
    /// OPT-IN complete Typed-IR collection. Bindings retain their rich checker
    /// type here until the clean-unit boundary projects both semantic and
    /// representation facts into `topaz_hir`.
    typed_locals: Option<Vec<(String, Span, Type)>>,
    /// Complete semantic node observations for the full Typed IR. Entries are
    /// upserted by kind/span when the same expression is visited through a
    /// contextual and an inferred path.
    typed_nodes: Option<HashMap<(topaz_hir::TypedNodeKind, Span), Type>>,
    /// Exact direct generated-rule calls. This is populated by call typing, not
    /// by identifier spelling or a later source scan.
    typed_call_targets: Option<Vec<(Span, String)>>,
    /// Exact call-site-instantiated callee types. A pipeline plan deliberately
    /// exposes the whole stage as its public callee span, so this evidence must
    /// come from call typing rather than a later expression-span lookup.
    typed_call_callees: Option<HashMap<Span, Type>>,
    /// Concrete solutions for globally fresh branch-local inference variables.
    /// Facts are sharpened once when they leave the checker instead of rescanning
    /// the growing fact maps after every solved join.
    typed_inference_solutions: HashMap<u32, Type>,
    lispex_rule_factories: HashMap<String, String>,
    lispex_rule_namespaces: HashMap<String, HashMap<String, String>>,
}

/// What a set of unguarded match arms covers, for the decidable
/// exhaustiveness domains (CDR-004 §5): bool, literal/nominal unions,
/// Option, Result, and nominal records. Constructor coverage nests the payload coverage,
/// so `Some(true)/Some(false)/None` exhausts `Option<bool>`.
#[derive(Default)]
struct Coverage {
    irrefutable: bool,
    literals: Vec<Lit>,
    ctor_cov: HashMap<String, Coverage>,
    /// Nominal record declaration bases covered by an all-irrefutable field
    /// pattern. Generic instances share the runtime declaration identity.
    nominal_records: HashSet<String>,
}

/// The resolution of a member CALL against ONE concrete union arm.
enum ArmCall {
    /// The member is callable; check the args against this real signature.
    Callable(Scheme),
    /// The member exists but is not callable (a property / non-`Func` field) —
    /// the diagnostic has already been emitted.
    NotCallable,
    /// The member is absent on this arm (guarded upstream; nothing to check).
    Absent,
}

/// The result of resolving the element of an array SPREAD `[...e]`.
enum SpreadElem {
    /// The spreadee is (or projects from) an array; this is its element type.
    Elem(Type),
    /// The spreadee MAY be an array at runtime (a union of arrays, or a gradual
    /// receiver) — stage the element as `Unknown`, no diagnostic.
    Stage,
    /// The spreadee is decidably NOT an array — the runtime faults, so reject.
    NotArray,
}

mod binding;
mod call;
mod capability;
mod control;
mod inference;
mod literal;
mod member;
mod pattern;
mod util;

use call::specialized_call_callee_type;
pub(crate) use capability::{comparable_in, order_comparable_in};
use capability::{
    contains_byte_buffer_in, json_decodable_status, json_encodable_status, order_comparable_gate,
    type_has_schema_variable, type_has_var,
};
use inference::resolve_inference;
use member::unwrap_optional;
use pattern::{list_let_pattern_refutable, usable};
use util::{
    alias_source, arm_diverges, assignment_root, block_diverges, collect_vars_into,
    contains_projection, contains_rigid, contains_true_unknown, decidable_type,
    disconnected_overlap_pair, is_plain_literal, is_rigid_or_union_rigid, last_index_segment,
    nominal_ctx_matches, nominal_type_id, receiver_has_member, remap_vars, skolems_to_vars,
    strip_projections, substitute, target_has_optional, top_inner, type_overlap, unify, unify_with,
    union_arm_is_mutator, union_payload_contains_enum, unknown_for_vars,
};

#[cfg(test)]
mod tests;
