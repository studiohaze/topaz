//! Type formation (CDR-004 §2/§3, phase C-1): AST type expressions
//! become checker types, with alias resolution, constructor arity
//! checking, and the §3 well-formedness diagnostics.

use std::collections::HashMap;
use std::rc::Rc;

use topaz_diag::{Code, Diagnostic, Label, Span};
use topaz_syntax::LangVersion;
use topaz_syntax::ast;

use crate::builtins;
use crate::codes;
use crate::ty::{Ctor, Lit, Prim, Type, non_keyable_map_set_key_with_nominals};
use crate::unit::{
    ExportedAlias, ExportedEnum, ExportedNewtype, ExportedNominals, ExportedReceiverMethod,
    ExportedRecord, module_nominal_identity,
};
use std::collections::HashSet;

/// Every source-spellable builtin type name. Language tooling consumes this
/// formation-owned inventory instead of carrying a partial handwritten copy.
pub const SOURCE_BUILTIN_TYPE_NAMES: &[&str] = &[
    "int",
    "float",
    "string",
    "bool",
    "Array",
    "Map",
    "Set",
    "Option",
    "Result",
    "template",
    "JSONValue",
    "File",
    "Bytes",
    "ByteBuffer",
    "Path",
    "Regex",
    "Match",
    "TOMLValue",
    "URL",
    "Date",
    "BigInt",
    "Decimal",
    "RoundingMode",
];

struct AliasDef<'a> {
    params: Vec<&'a str>,
    body: &'a ast::Type,
    name_span: Span,
    /// The body formed in the DEFINITION-site environment (alias
    /// parameters as Var(i) placeholders), cached at validation so
    /// use-site frames can never re-bind names the body mentions.
    resolved: Option<Type>,
}

/// One variant of a declared user enum: its name and its formed tuple payload
/// (v5.4). `payloads` is the variant's positional payload types — EMPTY for a
/// payload-less variant (`Dot`), length 1 for a single-payload variant
/// (`Circle(int)`, v5.3), length N for a multi-payload tuple variant
/// (`Bin(Op, Expr, Expr)`, v5.4). Arity is `payloads.len()`. A payload type may
/// be the declaring enum's own `Type::Enum` (recursive/mutual enums, v5.4),
/// formed by the two-phase collection.
#[derive(Debug, Clone)]
pub struct EnumVariantInfo {
    pub name: String,
    /// The formed positional payload types (empty = payload-less).
    pub payloads: Vec<Type>,
}

/// A declared user enum: its variant set, in declaration order.
#[derive(Debug, Clone)]
pub struct EnumInfo {
    pub id: String,
    pub type_params: Vec<String>,
    pub variants: Vec<EnumVariantInfo>,
}

/// One field of a declared NOMINAL RECORD (v5.4): its name, its formed type, and
/// whether it has a DEFAULT (the default AST is held separately by the checker so
/// the `Former` stays AST-lifetime-free; here we only need to know defaultedness
/// for required-field checking). Fields are kept in DECLARATION order.
#[derive(Debug, Clone)]
pub struct RecordFieldInfo {
    pub name: String,
    pub ty: Type,
    pub has_default: bool,
}

/// A declared user nominal record: its field set, in DECLARATION order.
#[derive(Debug, Clone)]
pub struct RecordInfo {
    pub id: String,
    pub type_params: Vec<String>,
    pub fields: Vec<RecordFieldInfo>,
}

/// A declared user NEWTYPE (v5.4): the FORMED base type it wraps. `newtype UserId
/// = int` ⇒ `base = Type::Prim(Int)`. The base is consulted by `.value()` (its
/// return type) and by comparability (a newtype is comparable iff its base is).
#[derive(Debug, Clone)]
pub struct NewtypeInfo {
    pub id: String,
    pub type_params: Vec<String>,
    pub base: Type,
}

/// §4 (v5.4) one declared user RECEIVER METHOD's signature — the formed types of
/// its NON-self parameters (so a call `recv.m(args)` type-checks `args` against
/// `params`, with `self` already supplied by the receiver), the variadic tail, the
/// return type, and the call-site metadata (required count, fixed-param names,
/// per-param defaultedness) so named/defaulted method args check exactly like a
/// free function. The `self` parameter (`params[0]` at the source) is NOT stored
/// here — it is the receiver, threaded separately.
#[derive(Debug, Clone)]
pub struct MethodInfo {
    /// Formed types of the non-`self` parameters (declaration order).
    pub params: Vec<Type>,
    /// Element type of a trailing variadic non-`self` parameter, if any.
    pub variadic: Option<Type>,
    /// The method's return type (formed; `Unknown` when omitted).
    pub ret: Type,
    /// Non-defaulted fixed (non-`self`, non-variadic) parameter count.
    pub required: usize,
    /// Fixed (non-`self`) parameter names, for named-argument checking.
    pub names: Vec<String>,
    /// Per fixed (non-`self`) parameter: declared with a default.
    pub defaulted: Vec<bool>,
}

/// §4 (v5.4) one declared PROTOCOL's signature surface — its method signatures,
/// keyed by method name. Each signature is the formed parameter types of a FREE
/// FUNCTION (a protocol method takes the conforming value as an ordinary parameter,
/// NOT `self`), plus the call-site metadata. The conforming-type stand-in (`Self` or
/// the `<T>` parameter) forms as a fresh `Type::Var(0)`, so a call `P.m(x)` types its
/// arguments by SUBSTITUTING the receiver's concrete type for `T`. MVP: no default
/// methods, no associated types.
#[derive(Debug, Clone)]
pub struct ProtocolInfo {
    /// Method name → its signature (over the protocol's `T` = `Type::Var(0)`).
    pub methods: std::collections::BTreeMap<String, ProtocolMethodSig>,
}

/// §4 (v5.4) one protocol method SIGNATURE, formed over the protocol's conforming
/// type variable `T` (= `Type::Var(0)`): every `Self`/`T` mention in a parameter or
/// the return type is a `Type::Var(0)`. A call `P.m(x)` substitutes `x`'s concrete
/// type for `Var(0)` to type the remaining args + the result.
#[derive(Debug, Clone)]
pub struct ProtocolMethodSig {
    /// Formed parameter types (declaration order, over `T = Var(0)`).
    pub params: Vec<Type>,
    /// Element type of a trailing variadic parameter, if any.
    pub variadic: Option<Type>,
    /// The method's return type (formed over `T = Var(0)`; `Unknown` when omitted).
    pub ret: Type,
    /// Non-defaulted fixed (non-variadic) parameter count.
    pub required: usize,
    /// Fixed parameter names (named-argument checking).
    pub names: Vec<String>,
    /// Per fixed parameter: declared with a default.
    pub defaulted: Vec<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeriveCapability {
    Eq,
    Order,
    Json,
}

/// One accepted variant carried between the two collection phases: its name +
/// the AST payload types to form in phase 2 (once every enum name is known).
type PendingVariant<'a> = (String, &'a [ast::Type]);

/// One accepted enum carried between the two collection phases: its name, generic
/// parameters, and accepted variants.
type PendingEnum<'a> = (String, Vec<&'a str>, Vec<PendingVariant<'a>>);

/// One accepted record field carried between phases: its name, AST type, and
/// whether it declares a default.
type PendingField<'a> = (String, &'a ast::Type, bool);

/// One accepted record carried between phases: its name, generic parameters, and
/// accepted fields.
type PendingRecord<'a> = (String, Vec<&'a str>, Vec<PendingField<'a>>);

/// One accepted newtype carried between phases: its name + its AST base type to
/// form in phase 2 (once every nominal name is known).
type PendingNewtype<'a> = (String, Vec<&'a str>, &'a ast::Type);

type PendingNominals<'a> = (
    Vec<PendingEnum<'a>>,
    Vec<PendingRecord<'a>>,
    Vec<PendingNewtype<'a>>,
);

#[derive(Debug, Clone)]
struct InherentMethodInfo {
    signature: MethodInfo,
    dispatch_id: Option<String>,
}

type InherentMethodCatalog = HashMap<String, HashMap<String, InherentMethodInfo>>;

type MethodCatalog = HashMap<String, HashMap<String, MethodInfo>>;

type ProtocolMethodCatalog = HashMap<String, MethodCatalog>;

type ConformanceCatalog = HashMap<String, HashSet<String>>;

pub struct Former<'a> {
    src: &'a str,
    /// Type syntax is diagnosed by its declaration/signature formation pass.
    /// Function and method bodies re-form those already-admitted annotations
    /// only to replace scheme variables with rigid body variables.
    report_type_diagnostics: bool,
    /// Root spans of annotations that a declaration/signature pass actually
    /// formed. A structurally rejected protocol method can reach body checking
    /// without that pass, so its first formation must still report diagnostics.
    formed_signature_types: HashSet<Span>,
    /// Lexically scoped alias frames (SPEC §5: nested `type`
    /// declarations are block-scoped); lookups search innermost-out.
    aliases: Vec<HashMap<&'a str, AliasDef<'a>>>,
    /// Alias-expansion stack for cycle detection.
    expanding: Vec<&'a str>,
    /// The base environment of the frame currently being validated:
    /// intra-frame fallback expansion (an alias chain hitting a
    /// not-yet-cached sibling) must still see the enclosing
    /// function's rigid type parameters.
    validation_base: HashMap<&'a str, Type>,
    /// Module-aware mode (CDR-004 C-6): qualified types resolve
    /// through imported namespaces and unknown ones are TPZ5025.
    module_mode: bool,
    /// Namespace-bound exported alias tables (`ns.Name`).
    namespace_aliases: HashMap<String, HashMap<String, ExportedAlias>>,
    /// Namespace-bound exported nominal-record tables (`ns.Record`).
    namespace_records: HashMap<String, HashMap<String, ExportedRecord>>,
    /// Namespace-bound exported enum tables (`ns.Enum`).
    namespace_enums: HashMap<String, HashMap<String, ExportedEnum>>,
    /// Namespace-bound exported newtype tables (`ns.Newtype`).
    namespace_newtypes: HashMap<String, HashMap<String, ExportedNewtype>>,
    /// Namespaces whose exports are unknown (cycles): stay silent.
    ambient_namespaces: HashSet<String>,
    /// Selected type-alias imports (`import m { User }`).
    imported_aliases: HashMap<String, ExportedAlias>,
    /// Declaration-site enum metadata for selected nominal imports. Checker
    /// identities use the local binding (`M`) for collision safety, while ABI
    /// receipts must retain the runtime declaration id (`Msg`).
    imported_enum_sources: HashMap<String, ExportedEnum>,
    /// Declaration-site newtype metadata for selected nominal imports; see
    /// [`Former::imported_enum_sources`].
    imported_newtype_sources: HashMap<String, ExportedNewtype>,
    /// Selected imported nominal type names. Profiles through 5.19 reject these
    /// as materialized typed-JSON schemas; 5.20 replaces the module-less runtime
    /// identity with ADR-131 declaration identity.
    imported_schema_nominals: HashSet<String>,
    /// §3 declared user enums (v5.3), name → variant set. Populated once at
    /// program collection (top-level only for the MVP); consulted by type
    /// formation (`EnumName` → `Type::Enum`) and by the expression checker
    /// (construction/pattern recognition). NOT lexically scoped: enums are a
    /// program-global nominal namespace in the MVP.
    enums: HashMap<String, EnumInfo>,
    /// Names declared by this source module. Imported nominal metadata shares
    /// the lookup tables, so ownership-sensitive `impl` checks cannot infer
    /// ownership from table membership after module context installation.
    own_nominals: HashSet<String>,
    /// §3 declared user NOMINAL RECORDS (v5.4), name → field set. Populated by the
    /// SAME unified nominal collection pass as `enums` (phase 1 registers all
    /// nominal names, phase 2 forms enum payloads AND record fields) so a record
    /// field may refer to any enum/record in the module (record↔enum mutual
    /// recursion). Consulted by type formation (`RecordName` → `Type::NominalRecord`)
    /// and by the expression checker (construction/pattern/field access).
    records: HashMap<String, RecordInfo>,
    /// §3 declared user NEWTYPES (v5.4), name → its formed base type. Populated by
    /// the SAME unified nominal collection pass (phase 1 registers the name, phase 2
    /// forms the base) so a newtype base may refer to any enum/record/newtype in the
    /// module. Consulted by type formation (`NewtypeName` → `Type::Newtype`) and by
    /// the expression checker (construction `UserId(x)`, `.value()`, patterns).
    newtypes: HashMap<String, NewtypeInfo>,
    /// §4 (v5.4) declared user RECEIVER METHODS, keyed first by type id and then
    /// by method name. Call-site lookup borrows both source names directly.
    /// Populated by `collect_methods` (after the nominal tables form, so a method's
    /// parameter/return types may name any nominal type), consulted by the
    /// expression checker's member-call path to type a method call `recv.m(args)`.
    methods: InherentMethodCatalog,
    /// Exact source declarations admitted into `methods`. A rejected duplicate
    /// shares its receiver/name key with the earlier declaration, so body
    /// admission cannot be reconstructed from that lookup alone.
    accepted_receiver_methods: HashSet<Span>,
    /// §4 (v5.4) declared PROTOCOLS, name → its method-signature surface. The three
    /// builtin protocols `Show`/`Eq`/`Order` are PREDECLARED (so `Show.show(x)` works
    /// on a `derives Show` type with no `protocol Show { … }` in source); a user
    /// `protocol Foo { … }` adds to this table. Populated by `collect_protocols`,
    /// run BEFORE `collect_methods`/`collect_derives` (so a manual `impl Foo<T>` and a
    /// `Foo.m(x)` call can resolve the protocol surface).
    protocols: HashMap<String, ProtocolInfo>,
    /// §4 (v5.4) MANUAL protocol-impl method bodies' signatures, keyed by
    /// `(protocol, type_id, method)`. A manual `impl Show<User> { function show(value:
    /// User) -> string { … } }` registers its method here (the CONCRETE signature,
    /// `User` substituted for the protocol's `T`) so a `Show.show(u)` call type-checks
    /// against the user's actual parameter types and reports arity. Distinct from the
    /// derived path (which dispatches to the value.rs leaves, not a user body).
    protocol_methods: ProtocolMethodCatalog,
    /// §4 (v5.4) the CONFORMANCE table: the set of `(protocol, type_id)` pairs a
    /// `derives` clause (`collect_derives`) OR a manual `impl Protocol<Type>`
    /// (`collect_methods`) authorized — `("Eq", "User")`, `("Show", "Status")`.
    /// Consulted by `Protocol.method(x)` dispatch (the receiver's type must conform)
    /// and by witness tests. A derived conformance dispatches to the value.rs leaves;
    /// a manual one to the registered `protocol_methods` body.
    conformances: ConformanceCatalog,
    /// §4 (v5.4) DERIVED conformances only (subset of `conformances`): the
    /// `(protocol, type_id)` pairs authorized by a `derives` clause (NOT a manual
    /// `impl`). Distinguishes the two dispatch routes — derived → value leaf, manual →
    /// user body — at a `Protocol.method(x)` call site so the right runtime path is
    /// chosen (and so a derived-vs-manual DOUBLE conformance is a conflict).
    derived_conformances: ConformanceCatalog,
    /// The language version of this check session — threaded so version-gated
    /// features (v5.4 multi-payload enums, nominal records) are accepted/rejected
    /// by EDITION, not hardcoded. Defaults to the current language at the
    /// convenience entry.
    version: LangVersion,
    pub diagnostics: Vec<Diagnostic>,
}

pub(crate) struct ModuleContext {
    pub(crate) namespace_aliases: HashMap<String, HashMap<String, ExportedAlias>>,
    pub(crate) namespace_records: HashMap<String, HashMap<String, ExportedRecord>>,
    pub(crate) namespace_enums: HashMap<String, HashMap<String, ExportedEnum>>,
    pub(crate) namespace_newtypes: HashMap<String, HashMap<String, ExportedNewtype>>,
    pub(crate) imported_aliases: HashMap<String, ExportedAlias>,
    pub(crate) imported_records: HashMap<String, ExportedRecord>,
    pub(crate) imported_enums: HashMap<String, ExportedEnum>,
    pub(crate) imported_newtypes: HashMap<String, ExportedNewtype>,
    pub(crate) namespace_receiver_methods:
        HashMap<String, HashMap<String, HashMap<String, ExportedReceiverMethod>>>,
    pub(crate) imported_receiver_methods: HashMap<String, HashMap<String, ExportedReceiverMethod>>,
    pub(crate) ambient_namespaces: HashSet<String>,
    pub(crate) imported_conformances: HashSet<(String, String)>,
}

mod exports;
mod formation;
mod nominals;
mod protocols;

use exports::{
    enum_info_equivalent, enum_info_from_export, newtype_info_equivalent, newtype_info_from_export,
    record_info_equivalent, record_info_from_export,
};
pub(crate) use formation::substitute;
use formation::{
    reserved_type_name_kind, reserved_variant_name_kind, substitute_params, type_syntax_any,
};
pub(crate) use nominals::nominal_instance_id;
use nominals::synthetic_type_params;
use protocols::{catalog_contains_conformance, catalog_insert_conformance};

#[cfg(test)]
mod tests;
