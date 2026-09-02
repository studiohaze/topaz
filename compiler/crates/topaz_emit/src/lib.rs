//! `topaz_emit` — the Rust emitter (CDR-006 §2/§4). It consumes a
//! source-free [`topaz_hir::LoweredUnit`] and produces deterministic Rust source for a
//! self-contained crate whose `run_with_host(host) -> RunOutcome`
//! computes the program over the shared `topaz_value::Value` carrier
//! (control flow lowers to real Rust; DATA stays uniform).
//!
//! Emitted code links against `topaz_rt` and calls the shared §2 leaf
//! operators so arithmetic, comparison, and iteration agree with the
//! interpreter. The emitter covers checked multi-module programs, callable
//! exports, control flow, faults, deferred actions, builtins, and native
//! lowering where the checked program is eligible.

pub mod codes;

mod closure;
mod expr;
mod module;
pub mod native;
mod pattern;
mod statement;
mod types;

use closure::*;
use expr::*;
use module::*;
use pattern::*;
use statement::*;
use types::*;

pub use native::{
    NativeAttemptDecision, NativeFunctionDecision, NativeInput, NativeLoweringOutcome,
    describe_native_attempt, emit_native_checked, emit_native_items, emit_native_or_hybrid,
};

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use codes::UNSUPPORTED_CONSTRUCT;
use topaz_diag::{Diagnostic, Label, Span};
use topaz_hir::emission::{
    ArrayElement, AssignOp, BinaryOp, Block, CallArg, CaseArmBody, CaseClause, CompBody,
    CompClause, CompKind, ConcurrentArm, EnumDecl, Expr, ExprKind, FieldType, FunctionDecl,
    FunctionTypeParam, Ident, ImplMethod, ImportItem, ImportKind, ListPatternElem, NewtypeDecl,
    Pattern, PatternKind, PipeRhs, Program, RecordDecl, RecordPatternField, Stmt, StmtKind,
    StringPart, Type, TypeAlias, TypeKind, UnaryOp, boundary_guardable,
    call_args_contain_placeholder, contains_placeholder,
};
use topaz_hir::{LoweredModule, LoweredUnit, emission::LoweredText};
use topaz_syntax::parse_duration_milliseconds;
// `codes` (the emitter's own TPZ6xxx registry) lives in the local module above;
// the RUNTIME fault codes are reached as `topaz_value::codes::…` at their two real
// call sites, so the two registries never collide on the bare name `codes`.
use topaz_value::{
    Builtin, ReceiverBuiltinRoute, RtError, STRUCT_DEPTH, Schema, Value, binary_value,
    decode_escapes, fault, nominal_declaration_identity, receiver_builtin_name_shape, unary_value,
};

type NamedValue = (String, Value);

type ConstValues = HashMap<String, Value>;

type RuntimeDefaultRef = (String, String, String);

type RuntimeTargetRef = (String, String);

type RuntimeRefsByRecord = HashMap<String, Vec<RuntimeDefaultRef>>;

type RuntimeRefsByTarget = std::collections::BTreeMap<String, Vec<RuntimeTargetRef>>;

fn value_unary_op(op: UnaryOp) -> topaz_value::UnaryOp {
    match op {
        UnaryOp::Plus => topaz_value::UnaryOp::Plus,
        UnaryOp::Minus => topaz_value::UnaryOp::Minus,
        UnaryOp::Not => topaz_value::UnaryOp::Not,
    }
}

fn value_binary_op(op: BinaryOp) -> topaz_value::BinaryOp {
    match op {
        BinaryOp::Pow => topaz_value::BinaryOp::Pow,
        BinaryOp::Mul => topaz_value::BinaryOp::Mul,
        BinaryOp::Div => topaz_value::BinaryOp::Div,
        BinaryOp::Rem => topaz_value::BinaryOp::Rem,
        BinaryOp::Add => topaz_value::BinaryOp::Add,
        BinaryOp::Sub => topaz_value::BinaryOp::Sub,
        BinaryOp::Lt => topaz_value::BinaryOp::Lt,
        BinaryOp::Le => topaz_value::BinaryOp::Le,
        BinaryOp::Gt => topaz_value::BinaryOp::Gt,
        BinaryOp::Ge => topaz_value::BinaryOp::Ge,
        BinaryOp::Eq => topaz_value::BinaryOp::Eq,
        BinaryOp::Ne => topaz_value::BinaryOp::Ne,
        BinaryOp::In => topaz_value::BinaryOp::In,
        BinaryOp::And => topaz_value::BinaryOp::And,
        BinaryOp::Or => topaz_value::BinaryOp::Or,
        BinaryOp::Coalesce => topaz_value::BinaryOp::Coalesce,
    }
}

/// Why the native emitter could not lower a program. The `kind` is the stable
/// IDENTITY of the failure (and the only thing tests compare); the `span`
/// LOCATES the offending construct for a rendered diagnostic and is attached at
/// the nearest enclosing boundary as the error unwinds (§ `emit_items` and the
/// per-statement loop in `emit_entry_body_seeded`). Coverage is HONEST — the
/// emitter never emits code it cannot guarantee matches the interpreter, so the
/// differential gate only ever sees programs the emitter fully understands.
#[derive(Debug, Clone)]
pub struct EmitError {
    pub kind: EmitErrorKind,
    pub span: Option<Span>,
}

/// The KIND of an [`EmitError`] — its stable identity, independent of where it
/// was raised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitErrorKind {
    /// No entry module in the unit — an internal defect (a checked program
    /// always has one), not a user-facing coverage gap.
    NoEntry,
    /// A statement or expression kind the native emitter cannot lower yet
    /// (rendered as TPZ6001); the payload names the construct.
    Unsupported(&'static str),
    /// A literal that does not parse as the interpreter would read it (e.g. an
    /// out-of-range numeric literal).
    MalformedLiteral(&'static str),
    /// The v5.4 NATIVE backend declined to lower this program (a shape outside
    /// the monomorphized scalar island, or one it cannot guarantee
    /// byte-identical). Rendered as `TPZ6002`; the payload names the construct.
    /// A native decline ALWAYS falls back to the boxed backend — it never
    /// reaches the user as a hard error (the boxed lowering either succeeds or
    /// raises its own `TPZ6001`), so it is honest coverage, never a divergence.
    NativeDeclined(&'static str),
}

/// Two errors are equal when their KIND matches; the span is rendering metadata,
/// not part of the failure's identity. This keeps the emitter's `assert_eq!`
/// coverage tests span-agnostic while still carrying a real span to the CLI.
impl PartialEq for EmitError {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

impl Eq for EmitError {}

impl EmitError {
    /// A native-coverage gap (TPZ6001) for `what`, not yet located.
    pub fn unsupported(what: &'static str) -> Self {
        Self {
            kind: EmitErrorKind::Unsupported(what),
            span: None,
        }
    }
    /// The no-entry internal defect.
    pub fn no_entry() -> Self {
        Self {
            kind: EmitErrorKind::NoEntry,
            span: None,
        }
    }
    /// A literal the emitter cannot reproduce, for `what`.
    pub fn malformed_literal(what: &'static str) -> Self {
        Self {
            kind: EmitErrorKind::MalformedLiteral(what),
            span: None,
        }
    }
    /// A native-backend decline (TPZ6002) for `what`, not yet located. The
    /// caller falls back to the boxed backend on this kind.
    pub fn native_declined(what: &'static str) -> Self {
        Self {
            kind: EmitErrorKind::NativeDeclined(what),
            span: None,
        }
    }
    /// Whether this error is a NATIVE-backend decline — the signal the caller
    /// uses to fall back to the boxed backend rather than fail.
    pub fn is_native_decline(&self) -> bool {
        matches!(self.kind, EmitErrorKind::NativeDeclined(_))
    }
    /// Locate this error at `span` if it is not already located. The FIRST
    /// (innermost) boundary to attach a span wins, so a tighter inner span is
    /// preserved over a coarser enclosing one as the error unwinds.
    #[must_use]
    pub fn at(mut self, span: Span) -> Self {
        if self.span.is_none() {
            self.span = Some(span);
        }
        self
    }
    /// The user-facing diagnostic for this error, or `None` when it is an
    /// internal defect (`NoEntry`) or a located span is unavailable — in which
    /// case the CLI falls back to a plain message. `Unsupported` renders as the
    /// TPZ6001 umbrella with the construct in the message and a "still runs
    /// under `topaz run`" remedy note.
    pub fn diagnostic(&self) -> Option<Diagnostic> {
        match self.kind {
            EmitErrorKind::Unsupported(what) => {
                let span = self.span?;
                let mut diag = Diagnostic::error(
                    UNSUPPORTED_CONSTRUCT,
                    format!("native compilation does not support {what} yet"),
                    Label::new(span, "not yet supported by `topaz build`"),
                );
                diag.notes.push(
                    "this program still runs with `topaz run`; \
                     please file a feature request if you need a native binary"
                        .to_string(),
                );
                Some(diag)
            }
            // A native decline is an INTERNAL fallback signal, never a
            // user-facing diagnostic: the caller retries on the boxed backend,
            // which owns the real diagnostic (success or its own TPZ6001).
            EmitErrorKind::NoEntry
            | EmitErrorKind::MalformedLiteral(_)
            | EmitErrorKind::NativeDeclined(_) => None,
        }
    }
}

impl std::fmt::Display for EmitError {
    /// A human-readable reason (the CLI fallback when there is no located
    /// diagnostic). The `Unsupported` / `MalformedLiteral` payload is the
    /// internal construct name.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            EmitErrorKind::NoEntry => write!(f, "the unit has no entry module"),
            EmitErrorKind::Unsupported(what) => write!(f, "unsupported: {what}"),
            EmitErrorKind::MalformedLiteral(what) => write!(f, "malformed literal: {what}"),
            EmitErrorKind::NativeDeclined(what) => write!(f, "native backend declined: {what}"),
        }
    }
}

/// Emit the self-contained CRATE source for a single-module unit (the
/// shape `topaz emit` writes — CDR-006 §4). It is the items plus the
/// crate-level `#![forbid(unsafe_code)]`.
pub fn emit_unit(unit: &LoweredUnit) -> Result<String, EmitError> {
    Ok(format!(
        "#![forbid(unsafe_code)]\n{}",
        emit_items(unit, None)?
    ))
}

type HybridClosureSpan = (u32, u32, u32);

type HybridClosuresBySpan = std::collections::BTreeMap<HybridClosureSpan, String>;

type HybridClosuresByFunction = std::collections::BTreeMap<String, HybridClosuresBySpan>;

type HybridClosuresByModule = std::collections::BTreeMap<String, HybridClosuresByFunction>;

#[derive(Default)]
pub(crate) struct HybridPlan {
    pub(crate) helpers: String,
    closures: HybridClosuresByModule,
}

impl HybridPlan {
    fn closure(&self, module: &str, name: &str, name_span: Span) -> Option<&String> {
        self.closures
            .get(module)?
            .get(name)?
            .get(&(name_span.file.0, name_span.lo, name_span.hi))
    }

    pub(crate) fn has_closures(&self) -> bool {
        !self.closures.is_empty()
    }

    pub(crate) fn insert_closure(
        &mut self,
        module: String,
        name: String,
        name_span: Span,
        closure: String,
    ) {
        self.closures
            .entry(module)
            .or_default()
            .entry(name)
            .or_default()
            .insert((name_span.file.0, name_span.lo, name_span.hi), closure);
    }
}

pub(crate) fn emit_module_with_hybrid(
    unit: &LoweredUnit,
    hybrid: HybridPlan,
) -> Result<String, EmitError> {
    emit_items(unit, Some(hybrid))
}

/// Emit just the ITEMS (`run_with_host` + `entry` + their prelude
/// `use`s, no crate-level inner attribute), so many fixtures can be
/// `include!`d each inside its own `mod` in the differential harness
/// (CDR-006 §7). The lowering is identical to [`emit_unit`] — only the
/// envelope differs — so the harness proves exactly what ships.
pub fn emit_module(unit: &LoweredUnit) -> Result<String, EmitError> {
    emit_items(unit, None)
}

/// §4/§5 the binding kind of an in-scope local, carried alongside its name in
/// `locals`. Three states (the `bool is_mutable` grew a third case for the
/// rebinding cell).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bind {
    /// An immutable `let` — read as `x.clone()`, never an assignment target,
    /// captured by VALUE snapshot.
    Imm,
    /// A mutable `let mut` that NO closure captures — a plain Rust `let mut`,
    /// assignable in place. A closure capture of a `Mut` is REFUSED (the safety
    /// gate: an escape-analysis miss declines rather than diverges).
    Mut,
    /// A mutable `let mut` that a closure CAPTURES — a shared
    /// `Rc<RefCell<Value>>` cell (`cell_new`/`cell_get`/`cell_set`), the
    /// one-binding analog of the interpreter's whole-env `Rc` capture, so a
    /// mutation is visible everywhere the cell is shared. Captured by cloning
    /// the Rc (NOT a snapshot).
    Cell,
    /// An IMMUTABLE recursion cell: a `function` that is self- or
    /// forward-referenced within its consecutive declaration cluster, lowered to
    /// a `Rc<RefCell<Value>>` cell that is `cell_new(Value::Unit)`-seeded BEFORE
    /// the cluster's bodies and `cell_set` to the closure afterwards (the
    /// emitter analog of the interpreter's closure↔env `Rc` cycle, which is what
    /// lets the body reference the function by name). READS and CAPTURES behave
    /// exactly like [`Bind::Cell`] (`cell_get` / Rc clone); ASSIGNMENT is
    /// REFUSED like [`Bind::Imm`] (a `function` name is immutable — `f = …`
    /// faults `TPZ5003` in the interpreter), which is the ONE thing that
    /// distinguishes it from `Cell`.
    ImmCell,
    /// §7 a TOP-LEVEL `function` bound through an `Rc<RefCell<Option<Value>>>`
    /// forward-reference cell (`top_cell`/`top_cell_set`/`top_cell_get`), seeded
    /// `None` at the module top and filled `top_cell_set` when the declaration
    /// executes. Unlike [`Bind::ImmCell`] (a `Value::Unit`-seeded consecutive
    /// recursion cluster), a TOP cell distinguishes "declared but not yet run"
    /// from a real `Unit`, so a forward call reached before the declaration faults
    /// `GUARD_UNBOUND` (the interpreter's positional use-before-binding), not a
    /// spurious `Unit` call. READ via `top_cell_get` (fallible, at the identifier
    /// span — handled in the `Ident` arm, NOT `read_local`); CAPTURED by Rc clone
    /// like a cell; ASSIGNMENT refused like a function name.
    TopFnCell,
    /// A module-top immutable runtime value pre-seeded as an unfilled top cell so
    /// delayed method/function bodies may capture it before its declaration runs.
    TopValueCell,
    /// Mutable counterpart of [`Bind::TopValueCell`]. Reads/writes go through the
    /// same option cell; only this variant is an assignment target.
    TopMutValueCell,
    /// §17 a NAMESPACE import binding (`import m` / `import m as a`), bound to the
    /// imported module's record. Behaves EXACTLY like [`Bind::Imm`] for reads,
    /// captures, and assignment — the distinction exists only so a typed-annotation
    /// `type_test` can tell a name is a NAMESPACE (the head a qualified type `m.Id`
    /// resolves through) from an ordinary value, and refuse a qualified type whose
    /// head is shadowed by a non-namespace local. (A SELECTED import stays `Imm`.)
    Namespace,
}

enum OkOrElseCallMode<'a> {
    Direct,
    Optional { leading: Option<&'a str> },
}

struct RenderedOkOrElseArgs<'a> {
    values: Vec<String>,
    positional: Vec<usize>,
    named: Vec<(&'a str, usize)>,
}

#[derive(Clone, Copy)]
enum FixedNamespaceRuntime {
    Shared,
    Host,
}

#[derive(Clone, Copy)]
struct FixedNamespaceSpec {
    leaf: &'static str,
    params: &'static [&'static str],
    defaults: &'static [Option<&'static str>],
    locate_spread_at_argument: bool,
    runtime: FixedNamespaceRuntime,
}

enum RenderedSpreadTailArg {
    Positional(String),
    Spread { value: String, span: String },
}

struct RenderedNamedArg {
    name: String,
    value: String,
}

enum RenderedCallArgs {
    OrderFault(String),
    Static {
        positional: Vec<String>,
        named: Vec<RenderedNamedArg>,
        call_span: String,
    },
    Spread {
        prefix: Vec<String>,
        tail: Vec<RenderedSpreadTailArg>,
        named: Vec<RenderedNamedArg>,
        first_spread_span: String,
        call_span: String,
    },
}

struct RenderedPipeSpreadArgs {
    prefix: Vec<String>,
    tail: Vec<RenderedSpreadTailArg>,
    first_spread_span: String,
}

/// The body of `entry`: the top-level statement sequence wrapped as the
/// `Ok(value)` the async entry returns. The program's value is its tail
/// expression statement (`Unit` otherwise — CDR-003 §5/§1a).
enum EntryFinal<'a> {
    Initialized {
        explicit_main: Option<Span>,
        exports: &'a [String],
    },
}

struct ModuleExportSurface {
    all: HashSet<String>,
    runtime: HashSet<String>,
    runtime_order: Vec<String>,
}

enum BuiltRuntimeExportSurface<'a> {
    Module(&'a HashSet<String>),
    Extern(HashSet<String>),
}

/// §17 how the entry binds one imported module: the whole namespace under a single
/// alias (`import m` / `import m as a`), or a SELECTION of its exports each under its
/// own (optionally aliased) local name (`import { foo, bar as b } from m`).
enum ImportPlan {
    /// Bind the module's record under this local name.
    Namespace(String),
    /// Bind each `(export name, local name)`; `span` is the import statement's span,
    /// for the "not exported" fault the interpreter raises during its `exec_import`.
    Selected {
        binds: Vec<(String, String)>,
        span: Span,
    },
}

struct FunctionStatementEmission<'ctx, 'a, 'c> {
    stmt: &'ctx Stmt,
    stmt_idx: usize,
    src: &'ctx LoweredText,
    aliases: &'ctx Aliases<'a, 'c>,
    locals: &'ctx mut Vec<(String, Bind)>,
    base: usize,
    at_module_top: bool,
    rec_celled_idx: &'ctx [usize],
    captured: &'ctx [&'ctx str],
    lines: &'ctx mut String,
}

struct LetStatementEmission<'ctx, 'a, 'c> {
    stmt: &'ctx Stmt,
    mutable: bool,
    pattern: &'ctx Pattern,
    value: &'ctx Expr,
    cells: &'ctx [&'ctx str],
    captured: &'ctx [&'ctx str],
    self_runtime_default_cells: &'ctx HashMap<String, String>,
    src: &'ctx LoweredText,
    aliases: &'ctx Aliases<'a, 'c>,
    locals: &'ctx mut Vec<(String, Bind)>,
    base: usize,
    in_loop: bool,
    lines: &'ctx mut String,
}

struct UsingStatementEmission<'ctx, 'a, 'c> {
    stmt: &'ctx Stmt,
    name: &'ctx Ident,
    value: &'ctx Expr,
    body: &'ctx Block,
    src: &'ctx LoweredText,
    aliases: &'ctx Aliases<'a, 'c>,
    locals: &'ctx Vec<(String, Bind)>,
    in_loop: bool,
    lines: &'ctx mut String,
}

struct DeferStatementEmission<'ctx, 'a, 'c> {
    stmt: &'ctx Stmt,
    action: &'ctx Expr,
    src: &'ctx LoweredText,
    aliases: &'ctx Aliases<'a, 'c>,
    locals: &'ctx [(String, Bind)],
    lines: &'ctx mut String,
}

#[derive(Clone, Copy)]
enum LoopControlKind<'ctx> {
    Break(Option<&'ctx Expr>),
    Continue,
}

struct LoopControlEmission<'ctx, 'a, 'c> {
    stmt: &'ctx Stmt,
    kind: LoopControlKind<'ctx>,
    label: Option<&'ctx Ident>,
    src: &'ctx LoweredText,
    aliases: &'ctx Aliases<'a, 'c>,
    locals: &'ctx [(String, Bind)],
    in_loop: bool,
    lines: &'ctx mut String,
}

struct ReturnStatementEmission<'ctx, 'a, 'c> {
    value: Option<&'ctx Expr>,
    src: &'ctx LoweredText,
    aliases: &'ctx Aliases<'a, 'c>,
    locals: &'ctx [(String, Bind)],
    in_loop: bool,
    lines: &'ctx mut String,
}

struct NestedFunctionCellSeeding<'ctx> {
    stmts: &'ctx [Stmt],
    src: &'ctx LoweredText,
    locals: &'ctx mut Vec<(String, Bind)>,
    base: usize,
    lines: &'ctx mut String,
}

struct RecursionClusterSeeding<'ctx> {
    stmt: &'ctx Stmt,
    names: Option<&'ctx [String]>,
    locals: &'ctx mut Vec<(String, Bind)>,
    base: usize,
    captured: &'ctx [&'ctx str],
    lines: &'ctx mut String,
}

type SavedStatementFlow = (Vec<String>, Vec<usize>, usize);

struct StatementDeferFlow {
    defer_scope: bool,
    block_has_defer: bool,
    saved: Option<SavedStatementFlow>,
    block_stack: Option<String>,
}

struct StatementSequenceEmission<'ctx, 'a, 'c> {
    stmts: &'ctx [Stmt],
    tail: Option<&'ctx Expr>,
    src: &'ctx LoweredText,
    aliases: &'ctx Aliases<'a, 'c>,
    locals: &'ctx mut Vec<(String, Bind)>,
    base: usize,
    in_loop: bool,
    // §14 true ONLY for a FUNCTION body — the scope whose `defer`s the emitted closure
    // wrapper drains (`__defers`). It is the BASE of the block-defer flow stack; a nested
    // block / loop body / match arm passes false and, when it contains a direct `defer`,
    // gets its OWN `__defersN` drained at its exit (see the `FlowCtx` setup at the top of
    // this fn). So `defer` is supported at every scope now, not only function bodies.
    defer_scope: bool,
    // §17 true ONLY for the FLAT module-top statement sequence (entry + non-entry
    // module bodies) — NOT a function body, block, loop, or match arm. A function
    // declared in a sequence where this is `false` becomes `in_nested` (its closure
    // env chains to enclosing locals the capture-pruned emit locals miss), so a
    // qualified type in its body refuses; a function declared at the flat module top
    // resolves. Only read at the function-declaration site.
    at_module_top: bool,
}

struct ConstStatementEmission<'ctx, 'a, 'c> {
    stmt: &'ctx Stmt,
    name: &'ctx Ident,
    value: &'ctx Expr,
    src: &'ctx LoweredText,
    aliases: &'ctx Aliases<'a, 'c>,
    locals: &'ctx mut Vec<(String, Bind)>,
    base: usize,
    captured: &'ctx [&'ctx str],
    in_loop: bool,
    lines: &'ctx mut String,
}

#[derive(Clone, Copy)]
enum FunctionBinding {
    TopCell,
    RecursionCell,
    Local,
}

struct FunctionParameters<'a> {
    variadic: Option<&'a str>,
    fixed_count: usize,
    names: Vec<&'a str>,
    defaults: Vec<Option<String>>,
    locals: Vec<(String, Bind)>,
}

struct FunctionBoundaryGuards {
    parameters: String,
    result: Option<String>,
    variadic_element: Option<String>,
}

/// Lower a `for pattern in iter { body }` (§5). BOTH forms materialize
/// the SAME shared `for_items` list and bind the loop pattern per
/// iteration in the body's own (per-iteration) scope; they differ only
/// in what becomes of the body value:
/// - `collect = false` (statement position): iterate for effects, body
///   value discarded; `break`/`continue` are allowed.
/// - `collect = true` (expression position): collect each body value
///   into `Value::array(acc)` — the `for`'s value; bare `break`/`continue`
///   are a §5 static error, so the body is lowered as not-in-loop. Labeled
///   control may still pass through to an enclosing labeled `loop`.
///
/// The iterable is lowered outside the loop context because it is
/// evaluated before this loop's frame exists, so a `break` there would
/// target an OUTER loop, which unlabeled Rust cannot express).
struct ForEmission<'ctx, 'a, 'c> {
    pattern: &'ctx Pattern,
    iter: &'ctx Expr,
    body: &'ctx Block,
    span: Span,
    src: &'ctx LoweredText,
    aliases: &'ctx Aliases<'a, 'c>,
    locals: &'ctx [(String, Bind)],
    collect: bool,
}

struct ForPatternBinding {
    loop_variable: String,
    prelude: String,
}

struct ForPatternEmission<'ctx, 'a, 'c> {
    pattern: &'ctx Pattern,
    src: &'ctx LoweredText,
    aliases: &'ctx Aliases<'a, 'c>,
    span: &'ctx str,
    in_loop: bool,
    typed_unsupported: &'static str,
}

/// §6.4 (v5.4) recursively lower a comprehension's CLAUSE LIST into nested Rust
/// `for`/`if`, accumulating into the enclosing `__cacc` Rust binding. The clauses
/// nest left-to-right exactly like the interpreter runs them; the for-clause
/// patterns bind through the SAME `for_items` + pattern machinery a real `for`
/// uses (simple identifier, `_`, or a structural destructure that FAULTS GUARD_TYPE
/// on a non-matching element, identical to the interpreter). The base case (no
/// clauses left) appends the body element/entry. `scope` carries the bindings in
/// scope so far (loop variables of the enclosing clauses).
#[derive(Clone, Copy)]
struct ComprehensionEmission<'ctx, 'a, 'c> {
    body: &'ctx CompBody,
    kind: CompKind,
    span: Span,
    src: &'ctx LoweredText,
    aliases: &'ctx Aliases<'a, 'c>,
}

struct OrPatternPreparation {
    bound_names: Vec<String>,
    binding_tuple: String,
    first_match_chain: String,
}

/// A §6 constructor SUBPATTERN, recursively, into let-chain CONDITIONS plus
/// `let` BIND statements over `access` — the Rust `Value` expression for this
/// subpattern's value (`(**__inner)` at each level). A `_` adds nothing; a
/// `Binding` binds `access.clone()`; a `Literal` adds a `values_equal` condition;
/// a nested CONSTRUCTOR adds a `let Value::V(__innerK) = &access` condition (a
/// fresh `__innerK` per level via `counter`) and recurses on its inner access
/// `(**__innerK)`. So `Some(Some(x))` becomes `let Value::Some(__inner) = &__scrut
/// && let Value::Some(__inner1) = &(**__inner)` with `let x = (**__inner1).clone()`.
/// A nested RECORD `{ a: p }` (a `let Value::Record(__recK) = &access` + one
/// refutable `get` binding and recursion per field) and a nested LIST `[p, …]` /
/// `[p, ..r, q]` (a `let Value::Array(__arrK) = &access` + one refutable slice
/// pattern binding every element and optional middle, followed by recursion over
/// those references) are handled too, so patterns nest arbitrarily
/// (`{ a: { b } }`, `[Some(x), [y]]`, `[a, [b, ..c]]`). A nested TYPED subpattern
/// `n: T` adds a `type_test` condition + binds the name. A range subpattern adds an
/// int-in-range predicate, and an or-subpattern lowers each alternative to a
/// first-match extraction block. A `type_test`-undecidable typed subpattern (a
/// `Map` with an undecidable inner type, or an undecidable alias) is refused.
/// §3 (v5.3/v5.4) shared N-payload enum-variant pattern lowering: emit the
/// (conditions, bind-lines) for `Ctor(p0, p1, …)` against the `Value` PLACE
/// `access` (`__scrut` at the top level, a nested place inside a subpattern). The
/// first condition destructures the value once; the second checks enum_id ∈ owners,
/// variant == ctor, AND payload arity == N. Subsequent refutable `get` bindings feed
/// each payload position to recursive lowering. A payload-less variant binds no
/// payload. Mirrors the interpreter's position-wise `pat`.
struct SubpatternEmitter<'ctx, 'a, 'c> {
    src: &'ctx LoweredText,
    aliases: &'ctx Aliases<'a, 'c>,
    scope: &'ctx mut Vec<(String, Bind)>,
    span: &'ctx str,
    counter: &'ctx mut usize,
    in_loop: bool,
    locals: &'ctx [(String, Bind)],
}

struct MatchCaseEmission<'ctx, 'a, 'c> {
    case: &'ctx CaseClause,
    span: &'ctx str,
    src: &'ctx LoweredText,
    aliases: &'ctx Aliases<'a, 'c>,
    locals: &'ctx [(String, Bind)],
    in_loop: bool,
    arms: &'ctx mut String,
}

/// Emit `recv.reduce(...)` when the args include a NAMED form (`reduce(initial:,
/// f:)`, possibly reordered or mixed). The FIRST consumer of `topaz_hir`: the
/// call-plan supplies the source-ordered argument SHAPE; this lowers the args to
/// temps in SOURCE order (so their side effects run in source order, matching the
/// interpreter), then slot-binds them to the receiver-form params `["initial",
/// "f"]`. `member_value`-first preserves the record-field shadow (with labels via
/// `call_value_named`). The positional `reduce(init, f)` form is handled by the
/// caller's byte-identical shared path, never here.
#[derive(Clone, Copy)]
struct RenderedCallContext<'ctx, 'a, 'c> {
    expression: ExprEmitContext<'ctx, 'a, 'c>,
    member_span: &'ctx str,
    call_span: &'ctx str,
}

#[derive(Clone, Copy)]
struct NamedReceiverArgErrors {
    missing_plan: &'static str,
    unexpected_shape: &'static str,
    too_many: &'static str,
    unknown: &'static str,
    spread: &'static str,
    duplicate: &'static str,
    missing: &'static str,
}

struct RenderedNamedReceiverArgs {
    slots: Vec<usize>,
    temps: String,
    positional: String,
    named: String,
}

struct PipeStaticArgs {
    values: Vec<String>,
    slots: Vec<usize>,
    temps: String,
    positional: Vec<usize>,
    named: Vec<(String, usize)>,
}

/// The shared tail of a NESTED destructuring `let` (record-field or list-element
/// subpatterns): given the user-bound `names` (the scope-diff), the let-chain
/// `conds`, the `binds`, and the matched `variant` (`Value::Record(__dm)` /
/// `Value::Array(__db)`), it runs the duplicate / same-scope / captured-shadow
/// checks, lowers the value, and emits `let (<names>) = { let __dv = <value>; if
/// let <variant> = &__dv && <conds> { <binds> (<names>) } else { <fault> } };` — so
/// the matched temporaries stay block-scoped while the names escape. Registers the
/// names in `locals`.
struct NestedDestructureEmission<'ctx, 'a, 'c> {
    bound: Vec<String>,
    conds: Vec<String>,
    binds: Vec<String>,
    variant: Option<&'ctx str>,
    value: &'ctx Expr,
    fault: &'ctx str,
    src: &'ctx LoweredText,
    aliases: &'ctx Aliases<'a, 'c>,
    locals: &'ctx mut Vec<(String, Bind)>,
    base: usize,
    captured: &'ctx [&'ctx str],
    in_loop: bool,
}

/// §4 a DESTRUCTURING `let [a, b] = v` / `let [head, ..tail] = v` / `let { x, y }
/// = r` / `let { x: Some(v) } = r`. The interpreter's `KLetPattern` matches the
/// value and faults `GUARD_TYPE` "`let` pattern did not match the value (§4)" (at
/// the statement span) on a wrong type / wrong length / missing field / a failed
/// field subpattern. A NO-REST list is exact-length; a list with a rest needs only
/// `>= prefix + suffix` (the prefix binds from the front, the suffix from the back,
/// and a named `..mid` binds the middle as an array); a record checks only the
/// NAMED fields are present (a subset, like the `case` record pattern). A record
/// FIELD, or a NO-REST list ELEMENT, may carry a CONSTRUCTOR / literal subpattern
/// (`{ x: Some(v) }`, `let [Some(x), 5] = v`) — routed through `emit_subpattern` in
/// the refutable let-chain form after one slice pattern has bound every element.
/// The bound names are destructured out of a tuple the guard block
/// RETURNS, so the `__d*` temporaries stay block-scoped while the names escape into
/// this scope.
///
/// A `mut` destructure or MORE than one rest is refused. Nested record/list
/// subpatterns — including a REST list with a nested fixed element
/// (`[Some(x), ..tail]`) — route through `emit_subpattern` like other nested forms.
struct DestructureLetEmission<'ctx, 'a, 'c> {
    pattern: &'ctx Pattern,
    value: &'ctx Expr,
    span: Span,
    mutable: bool,
    src: &'ctx LoweredText,
    aliases: &'ctx Aliases<'a, 'c>,
    locals: &'ctx mut Vec<(String, Bind)>,
    base: usize,
    captured: &'ctx [&'ctx str],
    in_loop: bool,
}

/// §5 a rebinding `x (op)= e` of a mutable local (the non-index assign path).
/// A `Cell` target writes through `cell_set` and reads through `cell_get` (the
/// borrow drops before the next access, and the RHS evaluates BEFORE the
/// `borrow_mut` inside `cell_set`); a plain `Mut` assigns/reads in place. Both
/// preserve the interpreter's read-then-write order and short-circuit. The
/// target is a mutable local, so its unbound/immutable faults cannot arise.
struct AssignmentEmission<'ctx, 'a, 'c> {
    op: AssignOp,
    value: &'ctx Expr,
    span: Span,
    src: &'ctx LoweredText,
    aliases: &'ctx Aliases<'a, 'c>,
    locals: &'ctx [(String, Bind)],
    in_loop: bool,
}

/// How an assignment target's root binding classifies the write — shared by
/// the index-assign and record-path branches so the two stay consistent.
enum AssignRoot<'a> {
    /// A mutable LOCAL root, OR a non-`Ident` base (root `None`): the write is
    /// emitted (a `None` base writes the returned shared cell — the interpreter's
    /// `require_mut_root(None)` passes).
    Writable,
    /// An immutable `let` / recursion-cell LOCAL root — faults GUARD_IMMUTABLE.
    Immutable(&'a str),
    /// A const/import/unbound root that is not a writable local — refused (a safe
    /// over-refusal; the interpreter faults GUARD_IMMUTABLE there).
    Refuse,
}

/// Lower an expression to a Rust expression of type `Value`. This
/// slice covers literals; the interpreter's literal reading is matched
/// exactly (the value is computed at emit time and emitted as a
/// constant, so it is byte-identical to the interpreter's).
#[derive(Clone, Copy)]
struct ExprEmitContext<'ctx, 'a, 'c> {
    src: &'ctx LoweredText,
    aliases: &'ctx Aliases<'a, 'c>,
    locals: &'ctx [(String, Bind)],
    in_loop: bool,
}

/// Build the `Value::Closure` over an `EmittedClosure` from already-lowered
/// pieces — the param names (bound from `args` in order, and carried as
/// string literals for the §5 arity faults), the captures (cloned at creation,
/// owned by a `move` closure, re-cloned per call — a `Value` SNAPSHOT for an
/// immutable, or the `Rc` of a rebinding CELL for a captured-mutable), and the
/// body expression. Shared by lambdas (an expression body) and `function`
/// declarations (a block body lowered via [`emit_block`]).
struct ClosureEmission<'ctx> {
    param_names: &'ctx [&'ctx str],
    captures: &'ctx [&'ctx str],
    defaults: &'ctx [Option<String>],
    variadic: Option<&'ctx str>,
    variadic_guard: Option<&'ctx str>,
    param_guards: &'ctx str,
    body: &'ctx str,
    return_guard: Option<&'ctx str>,
    // §14 true when this function body contains accepted top-level `defer`s: wrap
    // the body so its `__defers` stack drains LIFO on the NON-fault exits (the inner
    // block's Rust `Ok` = a Topaz `return`/`?`/normal completion) and NOT on an
    // ordinary fault (Rust `Err`), matching the interpreter. No-defer closures pass
    // false and keep the byte-for-byte-unchanged wrapper.
    has_defers: bool,
}

/// §14 block-level `defer` flow state, threaded (via `Aliases.flow`, a shared
/// `Rc<RefCell>`) through the emit of one body. `stacks` is the lexical stack of ACTIVE
/// defer-stack VARIABLE NAMES, innermost LAST: a function/closure body's `__defers` is
/// the base entry, and each nested block CONTAINING a direct `defer` pushes its own
/// `__defersN`. A `defer` statement targets the innermost (`stacks.last()`); an early
/// exit (`return`/`?`/`break`/`continue`) drains a SUFFIX of `stacks` inner→outer before
/// it transfers control. The whole stack is saved/restored around a nested function body
/// (that body gets its OWN `__defers` base), so a function never drains an enclosing
/// block's defers.
#[derive(Default)]
struct FlowCtx {
    stacks: Vec<String>,
    /// `stacks.len()` at each enclosing loop-body entry — `break`/`continue` drain only
    /// `stacks[*loop_markers.last()..]` (the stacks opened INSIDE the current loop).
    loop_markers: Vec<usize>,
    /// One entry per enclosing loop body, in lockstep with `loop_markers`,
    /// describing how a `break`/`continue` lowers to Rust for that loop. A `while`/
    /// `for` is `LoopFrameKind::Plain` (unlabeled Rust loop, value-less break); a
    /// `loop` EXPRESSION is `LoopFrameKind::Value` carrying its source label (if any)
    /// and the unique Rust label it emitted (`break 'lN value`).
    loop_frames: Vec<LoopFrameKind>,
    /// A monotonic id for unique Rust loop labels (`'lN`).
    next_loop_label: usize,
    /// The index where the CURRENT function's BLOCK stacks begin — i.e. just past its
    /// `__defers` (so `1` when the function body has direct defers, else `0`). A
    /// `return`/`?` drains `stacks[fn_base..]` (the crossed block stacks); the function's
    /// own `__defers` is drained by the closure WRAPPER, so it is NOT re-drained here.
    fn_base: usize,
    /// Monotonic id for unique `__defersN` block-stack variable names.
    next_id: usize,
}

/// How a `break`/`continue` lowers to Rust for one enclosing loop. Kept
/// in lockstep with `FlowCtx::loop_markers` (one entry per loop body).
#[derive(Clone)]
enum LoopFrameKind {
    /// A `while`/`for` loop — UNLABELED in the emitted Rust, value-less break.
    Plain,
    /// A `loop` EXPRESSION — labeled in Rust as `rust_label`; `break <value>`
    /// targets it. `src_label` is the Topaz `'name` (if any) for labeled control.
    Value {
        src_label: Option<String>,
        rust_label: String,
    },
}

/// §3 (v5.4) the emit-side nominal-record table: lookup name → runtime nominal id
/// plus fields in DECLARATION order. The lookup name may be a selected import
/// alias; `id` is the value's real nominal id.
#[derive(Clone)]
struct RecordDef<'a> {
    id: &'a str,
    origin_identity: &'a str,
    declaration_identity: Option<String>,
    method_identity: Option<String>,
    fields: Vec<(&'a str, Option<(&'a LoweredText, &'a Expr)>)>,
}

type RecordDefs<'a> = HashMap<&'a str, RecordDef<'a>>;

/// §3 (v5.3/v5.4) the emit-side enum table: lookup name → runtime enum id plus
/// variant name → `(payload arity, decl index)`. The decl index stamps each
/// constructed `Value::Enum`'s `variant_index` so ordering is by declaration
/// order (§4), run≡build with the interpreter's `enum_defs`.
#[derive(Clone)]
struct EnumDef<'a> {
    id: &'a str,
    declaration_identity: Option<String>,
    method_identity: Option<String>,
    variants: HashMap<&'a str, (usize, u32)>,
}

type EnumDefs<'a> = HashMap<&'a str, EnumDef<'a>>;

#[derive(Clone)]
struct NewtypeDef<'a> {
    id: &'a str,
    declaration_identity: Option<String>,
    method_identity: Option<String>,
}

type NewtypeDefs<'a> = HashMap<&'a str, NewtypeDef<'a>>;

/// §4 (v5.4) the emit-side method table: registry KEY → its declared methods (each
/// an `ImplMethod`, carrying the method's `FunctionDecl` to lower as a closure). The
/// key is the bare type id `"User"` for an inherent `impl User`, OR the
/// protocol-qualified `"Show<User>"` for a MANUAL protocol impl `impl Show<User>` —
/// the SAME keys the interpreter registers in `method_defs`. An OWNED `String`
/// because the qualified key is synthesized (not a slice of `src`). Same-module only.
type MethodDefs<'a> = HashMap<String, Vec<&'a ImplMethod>>;

type GenericAliasTable<'a> = HashMap<&'a str, (&'a [Ident], &'a Type)>;

type SchemaAliasDecls<'a> = HashMap<&'a str, &'a TypeAlias>;

type SchemaRecordDecls<'a> = HashMap<&'a str, &'a RecordDecl>;

type SchemaEnumDecls<'a> = HashMap<&'a str, &'a EnumDecl>;

type SchemaNewtypeDecls<'a> = HashMap<&'a str, &'a NewtypeDecl>;

type ImportedSchemaRecordModules<'a> = HashMap<&'a str, String>;

type ImportedSchemaEnumModules<'a> = HashMap<&'a str, String>;

type ImportedSchemaNewtypeModules<'a> = HashMap<&'a str, String>;

struct LocalDeclarationInventory<'a> {
    has_method_declarations: bool,
    top_binding_cardinality: HashMap<&'a str, usize>,
    method_targets: HashSet<&'a str>,
    exported_method_names: HashMap<&'a str, HashSet<&'a str>>,
    method_definitions: MethodDefs<'a>,
    method_names: HashSet<&'a str>,
    protocols: HashSet<&'a str>,
    table: HashMap<&'a str, &'a Type>,
    generic_table: GenericAliasTable<'a>,
    poison: HashSet<&'a str>,
    schema_aliases: SchemaAliasDecls<'a>,
    schema_records: SchemaRecordDecls<'a>,
    schema_enums: SchemaEnumDecls<'a>,
    schema_newtypes: SchemaNewtypeDecls<'a>,
    enum_defs: EnumDefs<'a>,
    record_defs: RecordDefs<'a>,
    newtype_defs: NewtypeDefs<'a>,
}

struct Aliases<'a, 'c> {
    table: Rc<HashMap<&'a str, &'a Type>>,
    generic_table: Rc<GenericAliasTable<'a>>,
    poison: Rc<HashSet<&'a str>>,
    /// §14 shared block-defer flow state (see [`FlowCtx`]).
    flow: Rc<RefCell<FlowCtx>>,
    /// §3/§7 the generic type-param names in scope for the CURRENT function body
    /// (empty at module top level and inside a lambda body — mirroring the
    /// interpreter's `ClosureData.type_params`, which is the decl's params for a
    /// named function and EMPTY for a lambda). A bare type pattern over one of
    /// these — when NOT a builtin and NOT a resolvable/poisoned alias — erases.
    type_params: &'a [Ident],
    /// §17 cross-module type context (every module's alias table + namespace map),
    /// shared by every view, for resolving a QUALIFIED type `m.Id` in `type_test`.
    type_ctx: &'c TypeCtx<'a>,
    /// The CURRENT (consuming) module's identity — the key into `type_ctx.modules`
    /// whose `namespaces` map names the importable modules for a qualified head.
    identity: &'a str,
    /// §17 true when a QUALIFIED type `m.Id` here sits in a body whose use-site emit
    /// locals are CAPTURE-PRUNED in a way that could MISS a shadow of `m` the
    /// interpreter sees. Set by [`Aliases::with_body`] from the declaration site: a
    /// function/lambda declared anywhere OTHER than the flat module top (inside a
    /// block / `for` / `match` / another body) gets `true`, because its closure env
    /// chains to those enclosing scopes' locals yet a type-annotation head is never
    /// CAPTURED into the emit locals. Module top, module-top blocks, and a function
    /// body declared DIRECTLY at module top stay `false` (use-site locals + the
    /// module-top namespace filtering decide the head soundly). The qualified arm
    /// REFUSES (→ TPZ6001) whenever this is `true`.
    in_nested: bool,
    /// §3 (v5.3/v5.4) declared top-level user enums: enum name → (variant name →
    /// payload ARITY). Lets the emitter recognize `Color.Red`/`Bin(a,b,c)`
    /// construction, bare/`Constructor` variant patterns, AND the payload arity of
    /// each variant. ARITY (not just a payloadful bool) is the SHARED run≡build
    /// invariant: a bare reference to a payloadful variant, a wrong-arity
    /// construction/pattern, etc. behave identically to the interpreter. Mirrors
    /// the interpreter's `enum_defs`. Shared (`Rc`) across the cloned child views.
    enums: Rc<EnumDefs<'a>>,
    /// §3 (v5.4) declared top-level nominal records: record name → its fields in
    /// DECLARATION order, each `(field name, optional (defining source, default
    /// expr))`. Lets the emitter recognize `User { … }` construction (vs a
    /// structural update) + nominal-record patterns, and emit defaults in the
    /// SAME deterministic order as the interpreter (run≡build). Shared (`Rc`)
    /// across cloned child views.
    records: Rc<RecordDefs<'a>>,
    /// §3 (v5.4) declared top-level newtype NAMES. Lets the emitter recognize
    /// `UserId(5)` construction and `case UserId(x)` patterns. Mirrors the
    /// interpreter's `newtype_defs`. Shared (`Rc`) across cloned child views.
    newtypes: Rc<NewtypeDefs<'a>>,
    /// §4 (v5.4) declared top-level receiver methods (type id → methods). Lets the
    /// emitter recognize a method call `recv.m(args)` and emit its dispatch. Mirrors
    /// the interpreter's `method_defs`. Shared (`Rc`) across cloned child views.
    methods: Rc<MethodDefs<'a>>,
    /// §4 (v5.4) the set of all declared method NAMES across every type — the fast
    /// gate the call-site uses to decide whether a `recv.m(args)` is a candidate
    /// user-method call (then the runtime nominal id picks the type). Shared.
    method_names: Rc<HashSet<&'a str>>,
    /// §4 (v5.4) declared PROTOCOL names (the builtins `Show`/`Eq`/`Order` + any user
    /// `protocol Foo { … }`). Lets the emitter recognize a `Protocol.method(x)` static
    /// dispatch. Mirrors the interpreter's `protocol_defs`. Shared across child views.
    protocols: Rc<HashSet<&'a str>>,
    /// Full alias and nominal declarations for typed JSON schema lowering.
    schema_aliases: Rc<SchemaAliasDecls<'a>>,
    schema_records: Rc<SchemaRecordDecls<'a>>,
    schema_enums: Rc<SchemaEnumDecls<'a>>,
    schema_newtypes: Rc<SchemaNewtypeDecls<'a>>,
    imported_schema_record_modules: Rc<ImportedSchemaRecordModules<'a>>,
    imported_schema_enum_modules: Rc<ImportedSchemaEnumModules<'a>>,
    imported_schema_newtype_modules: Rc<ImportedSchemaNewtypeModules<'a>>,
}

struct EmitSchemaDecls<'a, 'c> {
    type_ctx: &'c TypeCtx<'a>,
}

#[derive(Clone)]
struct EmitSchemaSubstitution<'a> {
    ty: &'a Type,
    env: Rc<EmitSchemaEnv<'a>>,
    scope: EmitSchemaScope<'a>,
}

#[derive(Clone)]
struct EmitSchemaScope<'a> {
    module: String,
    src: &'a LoweredText,
}

type EmitSchemaEnv<'a> = HashMap<String, EmitSchemaSubstitution<'a>>;

type EmitSchemaResolution = (String, u32, u32, usize);

struct NamedSchemaEmission<'a, 'ctx, 'input> {
    ty: &'a Type,
    head: &'input str,
    namespace: Option<&'input str>,
    display: &'input str,
    args: &'a [Type],
    decls: &'input EmitSchemaDecls<'a, 'ctx>,
    scope: &'input EmitSchemaScope<'a>,
    env: &'input EmitSchemaEnv<'a>,
}

/// §17 one module's type-resolution facts for QUALIFIED (`m.Id`) lookups.
struct ModuleAliasProjection<'a> {
    enum_defs: Rc<EnumDefs<'a>>,
    record_defs: Rc<RecordDefs<'a>>,
    newtype_defs: Rc<NewtypeDefs<'a>>,
    schema_records: Rc<SchemaRecordDecls<'a>>,
    schema_enums: Rc<SchemaEnumDecls<'a>>,
    schema_newtypes: Rc<SchemaNewtypeDecls<'a>>,
    method_names: Rc<HashSet<&'a str>>,
    schema_record_modules: Rc<ImportedSchemaRecordModules<'a>>,
    schema_enum_modules: Rc<ImportedSchemaEnumModules<'a>>,
    schema_newtype_modules: Rc<ImportedSchemaNewtypeModules<'a>>,
}

struct ModuleLocalTypeDeclarations<'a> {
    enum_defs: Rc<EnumDefs<'a>>,
    record_defs: Rc<RecordDefs<'a>>,
    newtype_defs: Rc<NewtypeDefs<'a>>,
    schema_aliases: Rc<SchemaAliasDecls<'a>>,
    schema_records: Rc<SchemaRecordDecls<'a>>,
    schema_enums: Rc<SchemaEnumDecls<'a>>,
    schema_newtypes: Rc<SchemaNewtypeDecls<'a>>,
}

struct ModuleExportedTypeSurface<'a> {
    names: HashSet<&'a str>,
    enum_defs: EnumDefs<'a>,
    record_defs: RecordDefs<'a>,
    newtype_defs: NewtypeDefs<'a>,
    receiver_methods: HashMap<&'a str, HashSet<&'a str>>,
}

struct ModuleLocalAliasResolution<'a> {
    table: Rc<HashMap<&'a str, &'a Type>>,
    generic_table: Rc<GenericAliasTable<'a>>,
    poison: Rc<HashSet<&'a str>>,
}

struct ModuleTypeImportBindings {
    namespaces: std::collections::BTreeMap<String, String>,
    selected_types: std::collections::BTreeMap<String, (String, String)>,
}

struct BuiltModuleTypeProjection<'a> {
    alias_projection: ModuleAliasProjection<'a>,
    exported_type_surface: ModuleExportedTypeSurface<'a>,
    type_imports: ModuleTypeImportBindings,
}

struct BuiltModuleDefaultFacts<'a> {
    runtime_values: ModuleRuntimeValueSurface<'a>,
    record_defaults: ModuleRecordDefaultFacts,
    external_hidden_runtime_refs: RuntimeRefsByTarget,
}

struct ModuleDefaultInputFacts<'a> {
    const_values: ConstValues,
    exported_const_values: Vec<NamedValue>,
    export_names: HashSet<&'a str>,
    immutable_let_names: HashSet<&'a str>,
    own_exported_runtime_refs: Vec<RuntimeDefaultRef>,
    self_runtime_refs: HashMap<String, Vec<RuntimeTargetRef>>,
    hidden_runtime_refs: RuntimeRefsByRecord,
    external_hidden_runtime_refs: RuntimeRefsByTarget,
    thunks: HashMap<String, Vec<RecordDefaultThunk>>,
}

struct ModuleDefaultImportFacts {
    selected_const_values: Vec<NamedValue>,
    selected_runtime_refs: Vec<RuntimeDefaultRef>,
}

struct ModuleNamespaceDefaultImportFacts {
    const_values: Vec<NamedValue>,
    runtime_refs: Vec<RuntimeDefaultRef>,
}

struct BuiltModuleLocalDeclarations<'a> {
    has_method_declarations: bool,
    top_binding_cardinality: HashMap<&'a str, usize>,
    method_names: HashSet<&'a str>,
    exported_method_names: HashMap<&'a str, HashSet<&'a str>>,
    local_aliases: ModuleLocalAliasResolution<'a>,
    local_types: ModuleLocalTypeDeclarations<'a>,
    local_methods: ModuleLocalMethodDeclarations<'a>,
}

struct BuiltModuleTypeContext<'a> {
    context: ModuleTypeCtx<'a>,
    has_method_declarations: bool,
    external_hidden_runtime_refs: RuntimeRefsByTarget,
}

struct ModuleLocalMethodDeclarations<'a> {
    definitions: Rc<MethodDefs<'a>>,
    protocols: Rc<HashSet<&'a str>>,
}

struct ModuleRecordDefaultFacts {
    const_values: ConstValues,
    runtime_refs: Vec<(String, String, String)>,
    self_runtime_refs: HashMap<String, Vec<(String, String)>>,
    hidden_runtime_refs: HashMap<String, Vec<(String, String, String)>>,
    thunks: HashMap<String, Vec<RecordDefaultThunk>>,
    external_hidden_runtime_refs: Vec<(String, String)>,
}

struct ModuleRuntimeValueSurface<'a> {
    exported_const_values: Vec<(String, Value)>,
    export_names: HashSet<&'a str>,
    immutable_let_names: HashSet<&'a str>,
}

struct ModuleEmissionIdentity<'a> {
    src: &'a LoweredText,
    runtime_identity: String,
    is_generated_std: bool,
}

struct ModuleTypeCtx<'a> {
    emission: ModuleEmissionIdentity<'a>,
    local_aliases: ModuleLocalAliasResolution<'a>,
    runtime_values: ModuleRuntimeValueSurface<'a>,
    record_defaults: ModuleRecordDefaultFacts,
    local_types: ModuleLocalTypeDeclarations<'a>,
    local_methods: ModuleLocalMethodDeclarations<'a>,
    alias_projection: ModuleAliasProjection<'a>,
    /// Exported type aliases, nominal declarations, and their receiver methods.
    /// Qualified type guards and import projection share this surface.
    exported_type_surface: ModuleExportedTypeSurface<'a>,
    /// This module's namespace imports that survive module-top as an UNAMBIGUOUS
    /// namespace binding: local namespace name → imported identity. A namespace
    /// whose local name collides with ANY other top-level binding (another import,
    /// a selected import, a `let`/`const`/`function`, or a `type` alias) is
    /// EXCLUDED — the head may then resolve to a non-namespace value, so a
    /// qualified type through it must refuse, not resolve through the dead import.
    type_imports: ModuleTypeImportBindings,
}

#[derive(Clone)]
struct RecordDefaultThunk {
    field: String,
    cell: String,
    hidden_field: String,
    label: String,
    span: Span,
}

/// §17 the unit's cross-module type context, keyed by module identity.
struct TypeCtx<'a> {
    has_method_declarations: bool,
    modules: std::collections::BTreeMap<String, ModuleTypeCtx<'a>>,
    hybrid: Option<HybridPlan>,
    closure_factories: RefCell<String>,
}

#[derive(Clone, Copy)]
struct TypeTestShared<'a, 'c, 'b> {
    src: &'a LoweredText,
    aliases: &'b Aliases<'a, 'c>,
    arg_src: &'a LoweredText,
    arg_aliases: &'b Aliases<'a, 'c>,
    use_locals: &'b [(String, Bind)],
}

#[derive(Clone, Copy)]
struct TypeTestEnv<'a, 'c, 'b> {
    src: &'a LoweredText,
    aliases: &'b Aliases<'a, 'c>,
    use_locals: &'b [(String, Bind)],
}

#[derive(Clone, Copy)]
enum CollectionTypeTest {
    Array,
    Set,
}

#[cfg(test)]
mod tests;
