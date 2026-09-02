//! The evaluation machine (CDR-003 §3): an explicit frame stack —
//! Topaz recursion never consumes the Rust call stack, and the frame
//! granularity is the stepping surface the §15 cooperative scheduler
//! uses. Control flow travels as machine states, never Rust panics.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use topaz_diag::Span;
use topaz_syntax::ast::*;
use topaz_syntax::{LangVersion, parse_duration_milliseconds};

use crate::host::Host;
use crate::value::{
    Builtin, CALL_DEPTH_LIMIT, CallbackHofExecution, CallbackHofKind, CallbackHofPending,
    CallbackHofStep, CallbackKeyCollection, CallbackKeyPending, CallbackKeyStep,
    CallbackMapHofExecution, CallbackMapHofKind, CallbackMapHofPending, CallbackMapHofStep,
    CallbackMapUpdatePending, CallbackMapUpdateStep, CallbackOkOrElsePending, CallbackOkOrElseStep,
    CallbackReceiverMapPending, CallbackReceiverMapStep, CallbackRetainExecution,
    CallbackRetainPending, CallbackRetainStep, ClosureBody, ClosureData, ClosureParams,
    ExternFunction, ReceiverBuiltinRoute, Schema, SchemaAliasDecl, SchemaDecls, SchemaEnumDecl,
    SchemaNewtypeDecl, SchemaRecordDecl, Value, array_spread_extend, binary_value,
    bind_builtin_named_args, bind_named_arg_slots, builtin_json_decode, builtin_json_parse_as,
    builtin_map_of, builtin_protocol_dispatch, builtin_set_of, call_host_builtin, call_method,
    call_pure_builtin, call_resource_method, call_spread_extend, case_guard_bool, cmp_guard,
    condition_bool, decode_escapes, exact_args, for_items, index_slot, index_value, iterable_items,
    make_range, make_template, member_value, no_member_fault, nominal_record_field_required,
    nominal_spread_base_required, prepare_callback_hof, prepare_callback_key_collection,
    prepare_callback_map_hof, prepare_callback_map_update, prepare_callback_ok_or_else,
    prepare_callback_receiver_flat_map, prepare_callback_receiver_map, prepare_callback_retain,
    project_lispex_application_host_value, receiver_builtin, receiver_builtin_by_kind,
    record_update_base, record_update_merge, recursion_fault, render, rounding_mode_value,
    rounding_mode_variant, schema_of, short_circuit_lhs, sorted_by_keys, try_value, unary_value,
    update_fields_value, values_equal, walk_fields_value, wrap_optional,
};

mod assignment;
mod call;
mod environment;
mod execution;
mod expression;
mod frame;
mod module_scope;
mod pattern;
mod schema;

use assignment::apply_op;
use call::callable_arity;
use environment::{
    BindingCell, ClosureCallSlot, PreparedClosureCall, child_env, is_mutable, lookup, rebind,
    target_has_optional,
};
pub use environment::{EnvRef, Scope};
use frame::{Frame, FrameFamily};
use pattern::walk_fields;
use schema::{const_guarded, text_in};

/// One module's top-level alias table: name → (params, body,
/// exported) — §6 runtime conformance and §17 export boundaries.
/// Owned (the interpreter's value model is lifetime-free): alias
/// bodies are reference-counted clones of the AST, so they detach
/// from the borrowed program (CDR-006 E-1b).
type AliasTable = BTreeMap<String, (Rc<[Ident]>, Rc<Type>, bool)>;

/// §3 (v5.3/v5.4) declared user enums: enum name → (variant name → payload
/// ARITY). Lets the interpreter recognize `Color.Red` construction, bare/`Bin`
/// variant patterns, AND the payload arity of each variant. ARITY (not just a
/// payloadful bool) is the SHARED run≡build invariant: a bare reference to a
/// payloadful variant, a wrong-arity construction/pattern, etc. fault identically
/// across interp + boxed emit. Top-level, same-module only, so one program-global
/// table suffices. The per-variant value is `(arity, decl_index)` — the 0-based
/// DECLARATION-ORDER position is stamped into each constructed `Value::Enum` so
/// `<`/`sorted` order by declaration order (§4), the SAME index the emitter assigns.
type EnumVariants = BTreeMap<String, (usize, u32)>;

#[derive(Clone)]
struct EnumRuntimeDef {
    runtime_id: Rc<str>,
    method_identity: Option<Rc<str>>,
    variants: Rc<EnumVariants>,
    decl: Rc<EnumDecl>,
    decl_src: Rc<str>,
}

type EnumTable = BTreeMap<String, EnumRuntimeDef>;

/// §3 (v5.4) declared user NOMINAL RECORDS: record name → its fields in
/// DECLARATION order, each `(field name, optional default expr)`. The interpreter
/// uses this to recognize `User { … }` construction/update, validate
/// explicit/unknown/missing fields, and evaluate missing fields' defaults in the
/// deterministic order (spread → explicit L→R → missing defaults decl-order). The
/// default `Expr` handle is retained with its defining source so selected imports
/// do not evaluate a module-local default under the importing file's source view.
type RecordFields = Rc<[(String, Option<RecordDefault>)]>;

#[derive(Clone)]
struct RecordRuntimeDef {
    runtime_id: Rc<str>,
    method_identity: Option<Rc<str>>,
    fields: RecordFields,
    decl: Rc<RecordDecl>,
    decl_src: Rc<str>,
}

type RecordTable = BTreeMap<String, RecordRuntimeDef>;

#[derive(Clone)]
struct RecordDefault {
    src: Rc<str>,
    env: EnvRef,
    expr: Rc<Expr>,
}

/// §3 (v5.4) declared user NEWTYPES: lookup name → runtime identity and full
/// declaration. Local declarations map `UserId` to its own identity; selected
/// imports may bind an alias such as `UID` to the same definition.
#[derive(Clone)]
struct NewtypeRuntimeDef {
    runtime_id: Rc<str>,
    method_identity: Option<Rc<str>>,
    decl: Rc<NewtypeDecl>,
    decl_src: Rc<str>,
}

type NewtypeTable = BTreeMap<String, NewtypeRuntimeDef>;

struct NominalFieldPlan {
    name: Rc<str>,
    expr: Rc<Expr>,
    src: Rc<str>,
    env: Option<EnvRef>,
    is_default: bool,
}

/// Per-module declarations for typed JSON schema lowering. Nominal runtime
/// descriptors own their declarations; this catalog preserves the defining
/// module's aliases and type-name routes during schema construction.
#[derive(Default)]
struct SchemaDeclTables {
    aliases: BTreeMap<String, Rc<TypeAlias>>,
    records: BTreeMap<String, Rc<RecordDecl>>,
    enums: BTreeMap<String, Rc<EnumDecl>>,
    newtypes: BTreeMap<String, Rc<NewtypeDecl>>,
}

/// Type-name routing for one module's typed JSON schemas. Schema ASTs retain
/// spans into their defining source, so a selected or namespace-qualified name
/// must switch both the logical module and source before inspecting the
/// declaration. Keeping this per defining module also lets an exported alias
/// refer to that module's own imports without capturing the caller's imports.
#[derive(Default)]
struct SchemaImportScope {
    namespaces: BTreeMap<String, Rc<str>>,
    selected: BTreeMap<String, (Rc<str>, String)>,
}

#[derive(Clone, Default)]
struct ModuleNominalDefs {
    enum_defs: EnumTable,
    record_defs: RecordTable,
    newtype_defs: NewtypeTable,
}

struct ModuleTypeScope {
    declaration_identity: Rc<str>,
    src: Rc<str>,
    aliases: AliasTable,
    schema_decls: SchemaDeclTables,
    schema_imports: SchemaImportScope,
    nominals: ModuleNominalDefs,
}

struct ModuleRuntimeScope {
    env: EnvRef,
    exports: std::collections::BTreeSet<String>,
    private_default_values: std::collections::BTreeSet<String>,
}

impl ModuleRuntimeScope {
    fn new(env: EnvRef) -> Self {
        Self {
            env,
            exports: std::collections::BTreeSet::new(),
            private_default_values: std::collections::BTreeSet::new(),
        }
    }
}

struct ModuleScope {
    runtime: ModuleRuntimeScope,
    types: ModuleTypeScope,
}

struct SelectedImportProjection {
    value: Option<Value>,
    enum_definition: Option<EnumRuntimeDef>,
    record_definition: Option<RecordRuntimeDef>,
    newtype_definition: Option<NewtypeRuntimeDef>,
}

struct NominalIdentityProjection {
    declaration: Option<Rc<str>>,
    method: Option<Rc<str>>,
}

#[derive(Default)]
struct ModuleCallablePlan {
    item_indices: Vec<usize>,
    inherent_method_targets: std::collections::BTreeSet<String>,
}

struct ModuleAdmissionIdentity {
    declaration: Rc<str>,
    runtime_scope: Rc<str>,
}

struct ModuleAdmission {
    identity: ModuleAdmissionIdentity,
    types: ModuleTypeScope,
    private_default_values: std::collections::BTreeSet<String>,
    callables: ModuleCallablePlan,
}

#[derive(Default)]
struct CurrentModuleIdentity {
    declaration: Rc<str>,
    runtime_scope: Rc<str>,
}

impl CurrentModuleIdentity {
    fn declaration_or_entry(&self) -> &str {
        &self.declaration
    }
}

#[derive(Default)]
struct CurrentModuleContext {
    identity: CurrentModuleIdentity,
    is_extern: bool,
}

/// §4 (v5.4) declared user RECEIVER METHODS: `(nominal type id, method name)` → the
/// method's `Value::Closure` (built once at load over the module's top-level env, so
/// the body sees sibling functions + methods). A call `recv.m(args)` reads the
/// receiver's runtime nominal id, looks up `(id, m)`, and invokes the closure with
/// `recv` prepended as the first argument (STATIC dispatch → a free call). Top-level,
/// same-module only.
type MethodTable = BTreeMap<(String, String), Value>;

fn receiver_method_identity(module: &str, nominal: &str) -> String {
    let module = if module.is_empty() {
        "__entry__"
    } else {
        module
    };
    format!("{module}::{nominal}")
}

fn protocol_method_identity(module: &str, protocol: &str, nominal: &str) -> String {
    let module = if module.is_empty() {
        "__entry__"
    } else {
        module
    };
    format!("{module}::{protocol}<{nominal}>")
}

/// §3 (v5.3) a variant name that is a reserved prelude constructor: such a name
/// is REJECTED as a user variant by the checker (TPZ5022) and excluded from the
/// runtime enum table by BOTH engines, so it keeps its prelude meaning even on
/// the `--unchecked` path (run≡build). Mirrors the emitter's `collect_enum_defs`.
fn is_reserved_variant_name(name: &str) -> bool {
    matches!(name, "None" | "Some" | "Ok" | "Err")
}

#[derive(Debug, Clone)]
enum DeferredAction {
    Expr(Rc<Expr>),
    CloseResource { value: Value, span: Span },
}

pub use topaz_value::{RtError, codes, fault};

/// What a finished machine produced.
pub type RunResult = Result<Value, RtError>;

enum CallbackKeyDestination {
    ReturnArray,
    WriteArray(Rc<RefCell<Vec<Value>>>),
}

/// Fixed cooperative quantum (CDR-003 §7): deterministic with the
/// virtual clock; the exact value is documented runtime policy.
const STEP_QUANTUM: usize = 50;

/// The complete suspendable execution state of one sub-machine. Compiler tables,
/// module inventories, language policy, and the host remain on `Machine`; every
/// value here must travel together across a sub-run boundary.
struct ExecutionContext {
    frames: Vec<Frame>,
    values: Vec<Value>,
    env: EnvRef,
    /// Calls swap `src`; a suspended sub-run must not leak its view.
    src: Rc<str>,
    /// Paired with `src`: a suspended call must not leak callee parameters.
    type_params: Rc<[Ident]>,
    /// §4 nested Topaz call count, inherited at a contained sub-run boundary.
    call_depth: usize,
    /// Record-default private namespace authority, inherited by contained work.
    record_default_depth: usize,
    /// Comprehension accumulators belong only to the frames in this context.
    comp_accs: Vec<CompAccum>,
}

/// One concurrent arm as a suspended sub-machine (CDR-003 §7).
struct ArmRun {
    name: String,
    execution: ExecutionContext,
    done: Option<Value>,
}

struct ConcurrentState {
    arms: Vec<ArmRun>,
    deadline: Option<u64>,
    else_block: Option<Rc<Block>>,
}

/// What an unwind is carrying (§5/§13/§14).
#[derive(Debug, Clone)]
pub enum UnwindAction {
    Return {
        value: Value,
        span: Span,
    },
    /// `break (label)? (value)?` (§5). `label` is the resolved label
    /// NAME (text after `'`); `None` targets the nearest enclosing loop. `value`
    /// is the loop expression's result for a `loop` boundary (`Value::Unit` for a
    /// value-less `break`; ignored by a `while`/`for` boundary, which has no value).
    Break {
        span: Span,
        label: Option<String>,
        value: Value,
    },
    /// `continue (label)?` (§5).
    Continue {
        span: Span,
        label: Option<String>,
    },
}

pub struct Machine<'a> {
    language_version: LangVersion,
    src: Rc<str>,
    host: &'a dyn Host,
    frames: Vec<Frame>,
    values: Vec<Value>,
    env: EnvRef,
    /// Resolver module identity → runtime and type scope. Initialization, imports,
    /// qualified lookup, schemas, exports, and record defaults share this entry.
    module_scopes: BTreeMap<Rc<str>, ModuleScope>,
    /// Declaration identity and resolver runtime-scope key for the module currently
    /// initializing. Entry modules have no declaration qualifier while retaining
    /// their resolver identity for runtime lookup.
    current_module: CurrentModuleContext,
    /// Source allocation → resolver module identity. Closure calls swap `src`, so
    /// this index restores the owning module type scope after initialization moves on.
    source_module_index: BTreeMap<usize, Rc<str>>,
    /// §3 (v5.3) declared user enums (name → runtime identity + variant set),
    /// collected at program load. Consulted by enum construction (`Color.Red`)
    /// and enum-variant pattern matching.
    enum_defs: EnumTable,
    /// §3 (v5.4) declared user nominal records (name → runtime identity + ordered
    /// fields + defaults), collected at program load. Consulted by `User { … }`
    /// construction/update, nominal-record pattern matching, and field-access.
    record_defs: RecordTable,
    /// §3 (v5.4) declared user newtypes (runtime identity + declaration), collected
    /// at program load. Consulted by construction, conformance, unwrap, and patterns.
    newtype_defs: NewtypeTable,
    /// §4 (v5.4) declared user receiver methods (`(type id, method) → closure`),
    /// collected at program load. Consulted by a method call `recv.m(args)` — the
    /// receiver's runtime nominal id picks the closure (STATIC dispatch). A MANUAL
    /// protocol-impl method `impl Show<User> { … }` is ALSO registered here, under the
    /// protocol-qualified type id `"Show<User>"`, so a `Show.show(x)` dispatch finds it
    /// by `("{protocol}<{nominal_id}>", method)` before falling to the derived leaf.
    method_defs: MethodTable,
    /// §4 (v5.4) declared PROTOCOL names (the builtins `Show`/`Eq`/`Order` + any user
    /// `protocol Foo { … }`), collected at program load. A call `Protocol.method(x)`
    /// where the head is in this set dispatches via `KProtocolCall`.
    protocol_defs: std::collections::BTreeSet<String>,
    /// §3/§7 the CURRENT function's generic type-param names (spans into
    /// `self.src`). Swapped to the callee's on every closure call (mirroring
    /// `self.src`) and restored at the `CallBoundary`. A bare type pattern over
    /// one of these names — when it is NOT a builtin and NOT a visible alias —
    /// ERASES (always-matches), since generics carry no runtime type. A name
    /// that SHADOWS a same-named builtin/alias is NOT erased here (that shadow
    /// ordering is owner-gated unresolved semantics, A20).
    type_params: Rc<[Ident]>,
    /// §4 the live count of nested Topaz CALLS (closure applications) — incremented
    /// before each `CallBoundary` push, decremented on its pop. `apply_call` faults
    /// `GUARD_RECURSION` once it would exceed `CALL_DEPTH_LIMIT`, the SAME shared cap
    /// the emitted `call_value` enforces, so deep recursion stops identically in both
    /// engines (the interpreter would otherwise recurse far past the native stack the
    /// emitted binary overflows).
    call_depth: usize,
    /// True while a nominal record field default expression is being evaluated.
    /// Only this context may read a namespace-private immutable runtime value.
    record_default_depth: usize,
    /// §6.4 (v5.4) the in-progress COMPREHENSION accumulators, innermost last. A
    /// comprehension pushes one on entry, its surviving iterations append the body
    /// element/entry to the TOP, and it is popped + finalized through the SAME
    /// shared `builtin_set_of` / `builtin_map_of` / `Value::array` leaf the literals
    /// use (so the result/order/faults are byte-identical to emitted code). The
    /// emitter models this exact accumulator as a Rust `__cacc` local. A comprehension
    /// body that UNWINDS (a `?` propagating an `Err`, a `return`) drops its
    /// accumulator via `comp_accs.truncate` keyed on the saved depth.
    comp_accs: Vec<CompAccum>,
}

/// §6.4 an in-progress comprehension accumulator: a flat element list (array/set)
/// or a key/value pair list (map). Mirrors the emitter's `Vec<Value>` /
/// `Vec<(Value, Value)>` `__cacc`.
enum CompAccum {
    List(Vec<Value>),
    Pairs(Vec<(Value, Value)>),
}
