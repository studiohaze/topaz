use super::*;

/// Structural fuel (node budget) for comparison, canonicalization,
/// and rendering: `Rc` cycles are representable by unchecked
/// programs (CDR-003 §2), and these walks must terminate on them.
pub const STRUCT_FUEL: usize = 100_000;

/// Depth bound for the same walks — cycles grow depth without
/// consuming proportional fuel, and the Rust stack is the resource
/// actually at risk.
pub const STRUCT_DEPTH: usize = 128;

/// §4 the CALL-DEPTH (recursion) limit, shared by BOTH engines. The interpreter
/// recurses on an explicit HEAP frame stack (bounded only by memory), but emitted
/// native code recurses on the NATIVE stack and overflows (~5000 frames) — so without
/// a shared limit, deep recursion silently succeeds under `topaz run` yet aborts under
/// `topaz build`. Both engines count nested Topaz CALLS (closure applications, not
/// builtins) and fault `GUARD_RECURSION` at this depth, well below the native overflow
/// (with margin for fatter frames), so the boundary is deterministic and identical.
/// Interim: a single global cap (a fat-frame function can still overflow natively below
/// it — a documented residual); the robust fix is a heap-allocated emit call stack.
pub const CALL_DEPTH_LIMIT: usize = 1000;

/// The shared `GUARD_RECURSION` constructor — both engines build the call-depth fault
/// HERE so its code/message cannot drift (CDR-006 §2 shared-leaf discipline).
pub fn recursion_fault(span: Span) -> RtError {
    fault(
        codes::GUARD_RECURSION,
        format!("call depth exceeded the recursion limit of {CALL_DEPTH_LIMIT}"),
        span,
    )
}

/// Opaque run-scoped resource id (§22.3 `File` today, any future
/// handle type later). The backing/open table is owned by the run
/// (reached through `Host`/`RtCx`); the value carries only identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceId(pub u64);

/// §10 (v5.4) host-provided directory entry for `FS.list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostDirEntry {
    pub name: String,
    pub kind: String,
    pub size_bytes: Option<i64>,
}

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    Bool(bool),
    Unit,
    Null,
    /// §22.1 prelude constructors.
    Some(Rc<Value>),
    None,
    Ok(Rc<Value>),
    Err(Rc<Value>),
    /// Shared mutable aggregates (§22.2 mutator surface).
    Array(Rc<RefCell<Vec<Value>>>),
    Map(Rc<RefCell<OrderedMap>>),
    Set(Rc<RefCell<OrderedSet>>),
    /// Immutable record value (§8: update is shallow copy; field
    /// assignment is path copy-update — CDR-003 §6).
    Record(Rc<BTreeMap<String, Value>>),
    /// §22.3 opaque resource (`File` today): non-comparable, rejected
    /// as a Map/Set key, rendered opaquely, clone-shares-identity.
    Resource(ResourceId),
    /// Function value — an AST-backed closure (interpreter) or a
    /// compiled function (emitted), behind the callable ABI.
    Closure(Rc<dyn TpzCall>),
    /// A §16 tagged-template value: not comparable; no execution
    /// behavior in v0.3 (CDR-003 §8).
    Template(Rc<dyn TpzTemplate>),
    /// A Form-A namespace binding (§17): a compile-time resolution
    /// object — valid only as a member-access head; never comparable,
    /// storable uses are resolver-rejected and guarded here.
    Namespace(Rc<str>),
    /// A §22 builtin, optionally bound to a receiver value.
    Builtin {
        kind: Builtin,
        recv: Option<Rc<Value>>,
    },
    /// Runtime-only, unforgeable carrier for the capability-gated Lispex
    /// application API. The payload type has no public constructor and user
    /// syntax cannot construct, destructure, compare, encode, or render it.
    LispexApplicationOpaque(Rc<crate::lispex_application::LispexApplicationOpaqueValue>),
    /// `f >> g` composition (§11): callable, not comparable.
    Composed(Rc<(Value, Value)>),
    /// Integer range value (§10): lazily stepped, not comparable.
    Range {
        lo: i64,
        hi: i64,
        inclusive: bool,
        step: i64,
    },
    /// §22 a parsed JSON tree (`JSON.parse`): an OPAQUE immutable value, distinct
    /// from Topaz values so JSON's `null`/number/object semantics are preserved
    /// (not collapsed into `Option`/`int|float`/`Record`). Inspected via accessor
    /// methods (`kind`/`asString`/`get`/…); rejected as a Map/Set key.
    Json(Rc<JsonValue>),
    /// §3 a user enum value (v5.3/v5.4): a NOMINAL closed-sum variant. `enum_id`
    /// is the declaring enum's nominal identity (two same-shaped enums are
    /// distinct); `variant` is the variant name; `variant_index` is the variant's
    /// 0-based DECLARATION-ORDER position (so `<`/`sorted` order by declaration
    /// order, §4 — both engines stamp the SAME index from the enum decl at
    /// construction, the run≡build invariant); `payloads` is the variant's
    /// fixed-arity tuple payload (EMPTY for a payload-less variant, length N for
    /// an N-payload variant — v5.4). An `Rc<[Value]>` is immutable and cheap to
    /// clone; a payload may itself be an enum (recursive/mutual enums, v5.4).
    /// Rendered `EnumId.Variant`(`(p0, p1, …)`). eq compares by NAME (nominal),
    /// ordering by `variant_index`; eq/render ITERATE the payloads.
    Enum {
        enum_id: Rc<str>,
        declaration_identity: Option<Rc<str>>,
        method_identity: Option<Rc<str>>,
        variant: Rc<str>,
        variant_index: u32,
        payloads: Rc<[Value]>,
    },
    /// §3 a user NOMINAL RECORD value (v5.4): a named product type
    /// (`record User { name: string, age: int }`). `record_id` is the declaring
    /// record's NOMINAL identity (two same-shaped records are DISTINCT, and a
    /// nominal record is never a structural `Record`); `fields` are the
    /// declaration-ORDERED `(name, value)` pairs (so render/derive/Order keep
    /// declaration order). An `Rc<[…]>` is immutable and cheap to clone; a field
    /// may itself be a record or enum (record↔enum mutual recursion, v5.4).
    /// Rendered `RecordId { name: v, … }`. eq/render ITERATE the fields; structural
    /// `Value::Record` is UNCHANGED so old structural paths never accept this.
    NominalRecord {
        record_id: Rc<str>,
        declaration_identity: Option<Rc<str>>,
        method_identity: Option<Rc<str>>,
        fields: Rc<[(Rc<str>, Value)]>,
    },
    /// §3 a user NEWTYPE value (v5.4): a DISTINCT nominal wrapper over a base
    /// value (`newtype UserId = int`). `newtype_id` is the declaring newtype's
    /// NOMINAL identity (so `UserId(5)` is NEVER an `int`, and two newtypes over
    /// the same base are distinct); `inner` is the wrapped base value, reached
    /// ONLY via `.value()` or a pattern destructure — there is no implicit
    /// coercion. Rendered `UserId(inner)`. eq compares same id + inner; render
    /// recurses; comparability consults the BASE type's comparability.
    Newtype {
        newtype_id: Rc<str>,
        declaration_identity: Option<Rc<str>>,
        method_identity: Option<Rc<str>>,
        inner: Rc<Value>,
    },
    /// §8 (v5.4) an IMMUTABLE byte array (`Bytes`): the value type behind the
    /// `Bytes`/encoding stdlib (UTF-8 / hex / base64). `Rc<[u8]>` is immutable and
    /// cheap to clone (no `RefCell` — unlike `Array`, a `Bytes` never mutates in
    /// place; `slice`/`concat` return a NEW value). Bytes are a SCALAR-like leaf:
    /// `==` is byte-wise, `<`/`sorted` order LEXICOGRAPHICALLY (like a `string`),
    /// and a `Bytes` IS a valid Map/Set key (immutable + hashable → its canonical
    /// key is the bytes). Rendered `Bytes(<lowercase-hex>)` (deterministic +
    /// lossless). NOT JSON-encodable (the checker rejects `JSON.stringify(bytes)`;
    /// the explicit `.toBase64()` is the bridge) — check==runtime.
    Bytes(Rc<[u8]>),
    /// ADR-108 fixed-length mutable contiguous bytes. Clone shares identity.
    ByteBuffer(Rc<RefCell<Vec<u8>>>),
    /// §10 (v5.4) an immutable logical path. The payload is normalized with `/`
    /// separators, is never absolute, and never escapes above the project root.
    /// Scalar-like: equality/order/keyability use this canonical string.
    Path(Rc<str>),
    /// §11 (v5.4) an opaque compiled regex. The engine is a small deterministic
    /// shared subset implemented in this crate so the vendored closure stays
    /// external-dependency-free. The original pattern is retained for render.
    Regex(Rc<MiniRegex>),
    /// §11 (v5.4) a regex match value. `start`/`end` are Unicode scalar indices;
    /// capture groups store only strings, never byte offsets.
    RegexMatch(Rc<RegexMatchData>),
    /// §12 (v5.4) an immutable parsed TOML tree.
    Toml(Rc<TomlValue>),
    /// §16 (v5.4) an immutable parsed URL value. No networking.
    Url(Rc<UrlData>),
    /// §13 (v5.4) an immutable Gregorian calendar date, stored as days since
    /// 1970-01-01. No wall-clock or timezone database.
    Date(DateData),
    /// §14.1 (v5.4) an explicit arbitrary-precision integer. This is intentionally
    /// separate from `int`: existing `int` arithmetic stays fixed-width checked.
    BigInt(Rc<BigIntData>),
    /// §14.2 (v5.4) a deterministic decimal: exact integer coefficient plus
    /// canonical decimal scale. No binary float conversion is involved.
    Decimal(Rc<DecimalData>),
}

/// A parsed JSON tree node (the payload of [`Value::Json`]). Immutable; objects
/// are key-sorted with duplicates rejected at parse time, so `.get` is total.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    String(Rc<str>),
    Number(JsonNumber),
    Array(Rc<[JsonValue]>),
    Object(Rc<std::collections::BTreeMap<Rc<str>, JsonValue>>),
}

/// A JSON number, kept as its exact accepted spelling plus the exact `i64` value
/// when it is integral and representable (so `asInt` is lossless and `numberText`
/// round-trips). JSON has one number type; Topaz does NOT coerce it to int/float
/// at parse — that is the typed-decode layer's job.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonNumber {
    pub lexeme: Rc<str>,
    pub int: Option<i64>,
}

/// §22 (v5.4) typed JSON decode descriptor. `JSON.parseAs<T>` and
/// `JSON.decode<T>` lower `T` into this eager schema, then both engines feed the
/// same `(JSONValue, Schema)` pair to `decode_json`.
pub type SchemaField = (Rc<str>, Schema, Option<Value>);
pub type SchemaVariant = (Rc<str>, u32, Rc<[Schema]>);

#[derive(Debug, Clone)]
pub enum Schema {
    Int,
    Str,
    Bool,
    Unit,
    Null,
    Array(Rc<Schema>),
    Option(Rc<Schema>),
    Map(Rc<Schema>),
    StructRecord {
        fields: Rc<[SchemaField]>,
    },
    Record {
        record_id: Rc<str>,
        declaration_identity: Option<Rc<str>>,
        fields: Rc<[SchemaField]>,
    },
    Enum {
        enum_id: Rc<str>,
        declaration_identity: Option<Rc<str>>,
        variants: Rc<[SchemaVariant]>,
    },
    Newtype {
        newtype_id: Rc<str>,
        declaration_identity: Option<Rc<str>>,
        base: Rc<Schema>,
    },
    Json,
}

/// A parsed TOML tree node. This v5.4 core parser supports deterministic config
/// TOML: strings, bools, integers/floats, arrays, and tables.
#[derive(Debug, Clone, PartialEq)]
pub enum TomlValue {
    Bool(bool),
    String(Rc<str>),
    Integer(i64),
    Float(Rc<str>),
    Array(Rc<[TomlValue]>),
    Table(Rc<BTreeMap<Rc<str>, TomlValue>>),
}

/// Parsed URL components plus the canonical string used for equality/render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlData {
    pub canonical: Rc<str>,
    pub scheme: Rc<str>,
    pub authority: Option<Rc<str>>,
    pub host: Option<Rc<str>>,
    pub path: Rc<str>,
    pub query: Rc<[(Rc<str>, Rc<str>)]>,
    pub fragment: Option<Rc<str>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DateData {
    pub days: i64,
}

/// Sign-magnitude arbitrary-precision integer, little-endian base 1e9 limbs.
/// `sign == 0` iff `limbs` is empty; otherwise sign is `-1` or `1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigIntData {
    pub(super) sign: i8,
    pub(super) limbs: Rc<[u32]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecimalData {
    pub(super) coeff: BigIntData,
    pub(super) scale: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundingMode {
    Down,
    Up,
    TowardZero,
    AwayFromZero,
    HalfUp,
    HalfEven,
}

impl RoundingMode {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Down => "Down",
            Self::Up => "Up",
            Self::TowardZero => "TowardZero",
            Self::AwayFromZero => "AwayFromZero",
            Self::HalfUp => "HalfUp",
            Self::HalfEven => "HalfEven",
        }
    }

    pub(super) fn index(self) -> u32 {
        match self {
            Self::Down => 0,
            Self::Up => 1,
            Self::TowardZero => 2,
            Self::AwayFromZero => 3,
            Self::HalfUp => 4,
            Self::HalfEven => 5,
        }
    }

    pub(super) fn from_name(name: &str) -> Option<Self> {
        match name {
            "Down" => Some(Self::Down),
            "Up" => Some(Self::Up),
            "TowardZero" => Some(Self::TowardZero),
            "AwayFromZero" => Some(Self::AwayFromZero),
            "HalfUp" => Some(Self::HalfUp),
            "HalfEven" => Some(Self::HalfEven),
            _ => None,
        }
    }
}

/// A `JSON.parse` failure, surfaced to Topaz as `Err({message, line, column})`
/// (1-based) — invalid JSON is a value-level error, never a runtime fault.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonParseError {
    pub message: String,
    pub line: u32,
    pub column: u32,
}

/// Resolve the declaration identity carried by Topaz 5.20 nominal values,
/// falling back to the published source name for older language profiles.
pub fn nominal_declaration_identity<'a>(
    source_name: &'a str,
    declaration_identity: Option<&'a str>,
) -> &'a str {
    declaration_identity.unwrap_or(source_name)
}

impl Value {
    pub fn str(s: impl AsRef<str>) -> Value {
        Value::Str(Rc::from(s.as_ref()))
    }

    pub fn array(items: Vec<Value>) -> Value {
        Value::Array(Rc::new(RefCell::new(items)))
    }

    pub fn record(fields: impl IntoIterator<Item = (String, Value)>) -> Value {
        Value::Record(Rc::new(fields.into_iter().collect()))
    }

    /// §3 (v5.4) build a NOMINAL record value from its id + declaration-ordered
    /// `(field, value)` pairs. The interpreter and the emitted boxed code both go
    /// through this leaf so the constructed value is byte-identical (run≡build).
    pub fn nominal_record(
        record_id: impl AsRef<str>,
        fields: impl IntoIterator<Item = (Rc<str>, Value)>,
    ) -> Value {
        Self::nominal_record_with_method_identity(record_id, None::<&str>, fields)
    }

    pub fn nominal_record_with_method_identity(
        record_id: impl AsRef<str>,
        method_identity: Option<impl AsRef<str>>,
        fields: impl IntoIterator<Item = (Rc<str>, Value)>,
    ) -> Value {
        Value::NominalRecord {
            record_id: Rc::from(record_id.as_ref()),
            declaration_identity: None,
            method_identity: method_identity.map(|identity| Rc::from(identity.as_ref())),
            fields: fields.into_iter().collect(),
        }
    }

    pub fn nominal_record_with_identities(
        record_id: impl AsRef<str>,
        declaration_identity: impl AsRef<str>,
        method_identity: Option<impl AsRef<str>>,
        fields: impl IntoIterator<Item = (Rc<str>, Value)>,
    ) -> Value {
        Value::NominalRecord {
            record_id: Rc::from(record_id.as_ref()),
            declaration_identity: Some(Rc::from(declaration_identity.as_ref())),
            method_identity: method_identity.map(|identity| Rc::from(identity.as_ref())),
            fields: fields.into_iter().collect(),
        }
    }

    /// §3 (v5.4) the value of a NOMINAL record's field by name, or `None` when the
    /// value is not that record / lacks the field. Shared leaf so the boxed
    /// emitter and the interpreter read a field identically (run≡build) — a linear
    /// scan over the decl-ordered fields.
    pub fn nominal_field(&self, name: &str) -> Option<&Value> {
        match self {
            Value::NominalRecord { fields, .. } => fields
                .iter()
                .find(|(n, _)| n.as_ref() == name)
                .map(|(_, v)| v),
            _ => None,
        }
    }

    /// §3 (v5.4) whether this value is a NOMINAL record with the given id.
    pub fn is_nominal_record(&self, id: &str) -> bool {
        matches!(self, Value::NominalRecord { record_id, .. } if record_id.as_ref() == id)
    }

    pub fn is_nominal_record_declaration(&self, identity: &str) -> bool {
        matches!(
            self,
            Value::NominalRecord {
                record_id,
                declaration_identity,
                ..
            } if nominal_declaration_identity(record_id, declaration_identity.as_deref()) == identity
        )
    }

    /// §4 (v5.4) the runtime NOMINAL identity of a value — the declaring type's name
    /// for a record/enum/newtype, else `None`. Both engines read it identically to
    /// pick a user receiver method `(id, method)` at a method call site (STATIC
    /// dispatch on the carrier's nominal id), so run≡build.
    pub fn nominal_id(&self) -> Option<&str> {
        match self {
            Value::NominalRecord { record_id, .. } => Some(record_id),
            Value::Enum { enum_id, .. } => Some(enum_id),
            Value::Newtype { newtype_id, .. } => Some(newtype_id),
            _ => None,
        }
    }

    /// Stable declaration identity used by Topaz 5.20 nominal semantics. Older
    /// profiles carry no such field and therefore retain their published short
    /// source-name identity.
    pub fn nominal_declaration_id(&self) -> Option<&str> {
        match self {
            Value::NominalRecord {
                record_id,
                declaration_identity,
                ..
            } => Some(nominal_declaration_identity(
                record_id,
                declaration_identity.as_deref(),
            )),
            Value::Enum {
                enum_id,
                declaration_identity,
                ..
            } => Some(nominal_declaration_identity(
                enum_id,
                declaration_identity.as_deref(),
            )),
            Value::Newtype {
                newtype_id,
                declaration_identity,
                ..
            } => Some(nominal_declaration_identity(
                newtype_id,
                declaration_identity.as_deref(),
            )),
            _ => None,
        }
    }

    /// §4 receiver-method dispatch identity. Values without receiver methods use
    /// their public nominal id; method-bearing values carry the defining-module
    /// qualified identity without changing render/equality/ABI nominal spelling.
    pub fn method_dispatch_id(&self) -> Option<&str> {
        match self {
            Value::NominalRecord {
                record_id,
                method_identity,
                ..
            } => Some(method_identity.as_deref().unwrap_or(record_id)),
            Value::Enum {
                enum_id,
                method_identity,
                ..
            } => Some(method_identity.as_deref().unwrap_or(enum_id)),
            Value::Newtype {
                newtype_id,
                method_identity,
                ..
            } => Some(method_identity.as_deref().unwrap_or(newtype_id)),
            _ => None,
        }
    }

    /// §3 (v5.4) build a NEWTYPE value `UserId(inner)` from its id + the wrapped
    /// base value. The interpreter and the emitted boxed code both go through this
    /// leaf so the constructed value is byte-identical (run≡build).
    pub fn newtype(newtype_id: impl AsRef<str>, inner: Value) -> Value {
        Self::newtype_with_method_identity(newtype_id, None::<&str>, inner)
    }

    pub fn newtype_with_method_identity(
        newtype_id: impl AsRef<str>,
        method_identity: Option<impl AsRef<str>>,
        inner: Value,
    ) -> Value {
        Value::Newtype {
            newtype_id: Rc::from(newtype_id.as_ref()),
            declaration_identity: None,
            method_identity: method_identity.map(|identity| Rc::from(identity.as_ref())),
            inner: Rc::new(inner),
        }
    }

    pub fn newtype_with_identities(
        newtype_id: impl AsRef<str>,
        declaration_identity: impl AsRef<str>,
        method_identity: Option<impl AsRef<str>>,
        inner: Value,
    ) -> Value {
        Value::Newtype {
            newtype_id: Rc::from(newtype_id.as_ref()),
            declaration_identity: Some(Rc::from(declaration_identity.as_ref())),
            method_identity: method_identity.map(|identity| Rc::from(identity.as_ref())),
            inner: Rc::new(inner),
        }
    }

    /// §3 (v5.4) whether this value is a NEWTYPE with the given id.
    pub fn is_newtype(&self, id: &str) -> bool {
        matches!(self, Value::Newtype { newtype_id, .. } if newtype_id.as_ref() == id)
    }

    pub fn is_newtype_declaration(&self, identity: &str) -> bool {
        matches!(
            self,
            Value::Newtype {
                newtype_id,
                declaration_identity,
                ..
            } if nominal_declaration_identity(newtype_id, declaration_identity.as_deref()) == identity
        )
    }

    /// Kind name used by dynamic-guard diagnostics (TPZ5xxx).
    pub fn kind(&self) -> &'static str {
        match self {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "string",
            Value::Bool(_) => "bool",
            Value::Unit => "()",
            Value::Null => "null",
            Value::Some(_) | Value::None => "Option",
            Value::Ok(_) | Value::Err(_) => "Result",
            Value::Array(_) => "Array",
            Value::Map(_) => "Map",
            Value::Set(_) => "Set",
            Value::Record(_) => "record",
            Value::Resource(_) => "File",
            Value::Closure(_) => "function",
            Value::Builtin { .. } => "function",
            Value::LispexApplicationOpaque(value) => value.kind_name(),
            Value::Namespace(_) => "namespace",
            Value::Template(_) => "template",
            Value::Composed(_) => "function",
            Value::Range { .. } => "range",
            Value::Json(_) => "JSONValue",
            Value::Enum { .. } => "enum",
            Value::NominalRecord { .. } => "record",
            Value::Newtype { .. } => "newtype",
            Value::Bytes(_) => "Bytes",
            Value::ByteBuffer(_) => "ByteBuffer",
            Value::Path(_) => "Path",
            Value::Regex(_) => "Regex",
            Value::RegexMatch(_) => "Match",
            Value::Toml(_) => "TOMLValue",
            Value::Url(_) => "URL",
            Value::Date(_) => "Date",
            Value::BigInt(_) => "BigInt",
            Value::Decimal(_) => "Decimal",
        }
    }
}
