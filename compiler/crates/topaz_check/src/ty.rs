//! The checker's type representation (CDR-004 §2).
//!
//! Aliases are resolved away at formation; unions are flattened,
//! deduplicated, and canonically ordered; records carry exact field
//! sets. `Var` exists only during local inference and never appears
//! in a checked signature.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Prim {
    Int,
    Float,
    String,
    Bool,
    Unit,
}

impl Prim {
    pub fn name(self) -> &'static str {
        match self {
            Prim::Int => "int",
            Prim::Float => "float",
            Prim::String => "string",
            Prim::Bool => "bool",
            Prim::Unit => "()",
        }
    }
}

/// A literal type's value (CDR-004 §2). Floats keep their source
/// text: literal-type identity is textual, and no arithmetic is ever
/// performed on a type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Lit {
    Str(String),
    Int(i64),
    Float(String),
    Bool(bool),
    Null,
}

impl Lit {
    /// The primitive a literal widens to. `null` has no primitive:
    /// it only occurs inside unions (`T | null`).
    pub fn prim(&self) -> Option<Prim> {
        match self {
            Lit::Str(_) => Some(Prim::String),
            Lit::Int(_) => Some(Prim::Int),
            Lit::Float(_) => Some(Prim::Float),
            Lit::Bool(_) => Some(Prim::Bool),
            Lit::Null => None,
        }
    }
}

/// Standard generic constructors (SPEC §3). `Array`/`Map`/`Set` are
/// invariant in their arguments (shared mutable references);
/// `Option`/`Result` are covariant (immutable values) — CDR-004 §3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ctor {
    Array,
    Map,
    Set,
    Option,
    Result,
    /// §10 integer ranges. Inference-only: `Range` is not a §3 type
    /// name, so type formation never produces it.
    Range,
}

impl Ctor {
    pub fn from_name(name: &str) -> Option<(Ctor, usize)> {
        Some(match name {
            "Array" => (Ctor::Array, 1),
            "Map" => (Ctor::Map, 2),
            "Set" => (Ctor::Set, 1),
            "Option" => (Ctor::Option, 1),
            "Result" => (Ctor::Result, 2),
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Ctor::Array => "Array",
            Ctor::Map => "Map",
            Ctor::Set => "Set",
            Ctor::Option => "Option",
            Ctor::Result => "Result",
            Ctor::Range => "range",
        }
    }

    /// Covariant constructors hold immutable values; the invariant
    /// ones are shared references with mutators (§22).
    pub fn covariant(self) -> bool {
        matches!(self, Ctor::Option | Ctor::Result)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Type {
    Prim(Prim),
    Literal(Lit),
    /// Two or more members after normalization; flattened, deduped,
    /// canonically ordered.
    Union(Vec<Type>),
    /// Exact field set (CDR-004 §3), sorted by field name.
    Record(Vec<(String, Type)>),
    Ctor(Ctor, Vec<Type>),
    Func {
        params: Vec<Type>,
        /// Element type of a final variadic parameter.
        variadic: Option<Box<Type>>,
        ret: Box<Type>,
    },
    /// A qualified exported type (`ns.Name`, SPEC §17) whose
    /// defining module's alias table is not visible to this pass.
    /// Identity-compared until module-aware checking resolves it.
    Foreign {
        name: String,
        args: Vec<Type>,
    },
    /// A rigid type parameter inside its declaring function's body:
    /// identity includes a per-declaration id so shadowed names in
    /// nested generic functions stay distinct.
    Skolem {
        name: String,
        id: u32,
        /// Stable semantic origin for observation/IR. The private numeric `id`
        /// remains an inference implementation detail and is never serialized.
        origin: String,
    },
    /// An opaque template value (SPEC §16): tag-specific typing stays
    /// internal; template values are non-comparable.
    Template,
    /// The opaque standard-library resource type (SPEC §22.3);
    /// non-comparable.
    File,
    /// §22 the opaque parsed-JSON type (`JSON.parse` result); non-comparable,
    /// inspected via accessor methods.
    JsonValue,
    /// §8 (v5.4) the `Bytes` type — an immutable byte array (the `Bytes`/encoding
    /// stdlib value). Unlike `File`/`JSONValue`, `Bytes` is a SCALAR-like leaf: it
    /// is BOTH eq- and ORDER-comparable (byte-wise eq, lexicographic order, like a
    /// `string`) and is a valid Map/Set KEY (immutable + hashable). It is NOT
    /// JSON-encodable (the checker rejects `JSON.stringify(bytes)`; `.toBase64()` is
    /// the explicit bridge). No generics. TERMINAL in the type walkers.
    Bytes,
    /// ADR-108 fixed-length mutable contiguous bytes. Shared identity like an
    /// Array, but the element representation is unboxed and length is invariant.
    /// Non-comparable, non-keyable, non-JSON, and not a Web ABI value.
    ByteBuffer,
    /// §10 (v5.4) the `Path` type — an immutable logical project-relative path.
    /// Path values are normalized to `/` separators, never absolute, and never
    /// escape above the project root. Scalar-like: eq/order/keyability follow the
    /// normalized string. NOT JSON-encodable; use `.toString()` explicitly.
    Path,
    /// §11 (v5.4) an opaque compiled regex. Regex values are not comparable,
    /// keyable, or JSON-encodable; the match/find/split/replace helpers expose the
    /// deterministic surface.
    Regex,
    /// §11 (v5.4) a regex match record value. Offsets are Unicode scalar indices
    /// (never byte offsets). Eq + JSON are defined by its exposed fields, but it is
    /// not a Map/Set key in this slice.
    Match,
    /// §12 (v5.4) an opaque parsed TOML tree. It is transformed through the `TOML`
    /// namespace (`toJson`, `stringify`) rather than inspected by direct members.
    TomlValue,
    /// §16 (v5.4) an immutable parsed URL value. Equality, ordering, and Map/Set
    /// key identity are all based on the canonical URL string.
    Url,
    /// §13 (v5.4) an immutable Gregorian calendar date. Eq/order/key use the
    /// canonical day count. JSON uses explicit `.toIso()`.
    Date,
    /// §14.1 (v5.4) explicit arbitrary-precision integer. Immutable scalar-like:
    /// eq/order/key are numeric; JSON uses explicit `.toString(radix)`.
    BigInt,
    /// §14.2 (v5.4) deterministic exact decimal. Immutable scalar-like:
    /// eq/order/key are numeric; JSON uses explicit `.toString()`.
    Decimal,
    /// §14.2 (v5.4) Decimal rounding mode enum. Builtin value namespace:
    /// `RoundingMode.HalfEven`, etc. Eq/Show only; no ordering/key/JSON.
    RoundingMode,
    /// §3 a user enum type (v5.3): a NOMINAL closed sum. Identity is the
    /// structural pair `(base, args)` — two same-shaped enums (`enum A{X}` vs
    /// `enum B{X}`) are DISTINCT types, and an enum is never an `int`.
    /// The flat instance key remains `form::nominal_instance_id(base, args)` for
    /// checker tables, diagnostics, and display. Generic args are traversed by
    /// type walkers; direct and mutually recursive enum declarations are formed
    /// through the nominal table while `Type::Enum` remains a terminal node for
    /// expansion walkers.
    /// Comparable (nominal-structural eq).
    Enum {
        base: String,
        args: Vec<Type>,
    },
    /// §3 a user NOMINAL RECORD type (v5.4): a named product. Identity is the
    /// structural pair `(base, args)` — two same-shaped records are DISTINCT, and
    /// a nominal record is NEVER a structural `Type::Record` (that stays separate
    /// so structural paths cannot accept nominal values). The flat instance key
    /// remains `form::nominal_instance_id(base, args)` for checker tables,
    /// diagnostics, and display. Generic args are traversed by type walkers.
    /// Comparable (nominal-structural eq).
    NominalRecord {
        base: String,
        args: Vec<Type>,
    },
    /// §3 a user NEWTYPE (v5.4): `newtype UserId = int`. A DISTINCT nominal wrapper
    /// over a base type. Identity is the structural pair `(base, args)` (so
    /// `UserId` is NOT a subtype of `int` and `int` is NOT a subtype of `UserId`
    /// — no implicit coercion either direction; the ONLY bridges are the
    /// constructor `UserId(x)` and `.value()`). The wrapped base type is NOT stored
    /// inline (it is looked up from the former's `newtype` table where needed —
    /// for `.value()`'s return type and comparability), so formation order never
    /// affects equality. The flat instance key remains
    /// `form::nominal_instance_id(base, args)` for checker tables, diagnostics,
    /// and display. Generic args are traversed by type walkers.
    Newtype {
        base: String,
        args: Vec<Type>,
    },
    /// A form this phase does not type yet. Unknown admits everything
    /// in both directions so a staged checker never reports a false
    /// positive; later phases replace Unknowns with real types.
    Unknown,
    /// Inference-local type variable (rank-1 instantiation, literal
    /// element holes). Never part of a checked signature.
    Var(u32),
}

impl Type {
    /// Visits this type and every recursively contained type in source-shape
    /// order, stopping at the first component accepted by `predicate`.
    ///
    /// This is the single inventory of structural type children for local
    /// inference predicates. Nominal declarations remain opaque here; their
    /// written generic arguments are still components of the nominal type.
    pub(crate) fn any_component(&self, predicate: &mut impl FnMut(&Type) -> bool) -> bool {
        if predicate(self) {
            return true;
        }
        match self {
            Type::Union(members) => members.iter().any(|member| member.any_component(predicate)),
            Type::Record(fields) => fields
                .iter()
                .any(|(_, field)| field.any_component(predicate)),
            Type::Ctor(_, args)
            | Type::Foreign { args, .. }
            | Type::Enum { args, .. }
            | Type::NominalRecord { args, .. }
            | Type::Newtype { args, .. } => args.iter().any(|arg| arg.any_component(predicate)),
            Type::Func {
                params,
                variadic,
                ret,
            } => {
                params.iter().any(|param| param.any_component(predicate))
                    || variadic
                        .as_deref()
                        .is_some_and(|value| value.any_component(predicate))
                    || ret.any_component(predicate)
            }
            _ => false,
        }
    }

    /// Visits every component using the same child inventory as
    /// `any_component`.
    pub(crate) fn for_each_component(&self, mut visit: impl FnMut(&Type)) {
        self.any_component(&mut |component| {
            visit(component);
            false
        });
    }

    /// Rebuilds this type through the same structural-child inventory as the
    /// read-only walkers. A replacement owns the whole matched component;
    /// otherwise every nested type is transformed recursively. Unions are
    /// normalized because a replacement can make previously distinct members
    /// equal.
    pub(crate) fn transform_components(
        &self,
        replacement: &mut impl FnMut(&Type) -> Option<Type>,
    ) -> Type {
        if let Some(transformed) = replacement(self) {
            return transformed;
        }
        match self {
            Type::Union(members) => Type::union(
                members
                    .iter()
                    .map(|member| member.transform_components(replacement))
                    .collect(),
            ),
            Type::Record(fields) => Type::Record(
                fields
                    .iter()
                    .map(|(name, field)| (name.clone(), field.transform_components(replacement)))
                    .collect(),
            ),
            Type::Ctor(constructor, arguments) => Type::Ctor(
                *constructor,
                arguments
                    .iter()
                    .map(|argument| argument.transform_components(replacement))
                    .collect(),
            ),
            Type::Foreign { name, args } => Type::Foreign {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|argument| argument.transform_components(replacement))
                    .collect(),
            },
            Type::Enum { base, args } => Type::Enum {
                base: base.clone(),
                args: args
                    .iter()
                    .map(|argument| argument.transform_components(replacement))
                    .collect(),
            },
            Type::NominalRecord { base, args } => Type::NominalRecord {
                base: base.clone(),
                args: args
                    .iter()
                    .map(|argument| argument.transform_components(replacement))
                    .collect(),
            },
            Type::Newtype { base, args } => Type::Newtype {
                base: base.clone(),
                args: args
                    .iter()
                    .map(|argument| argument.transform_components(replacement))
                    .collect(),
            },
            Type::Func {
                params,
                variadic,
                ret,
            } => Type::Func {
                params: params
                    .iter()
                    .map(|parameter| parameter.transform_components(replacement))
                    .collect(),
                variadic: variadic
                    .as_deref()
                    .map(|parameter| Box::new(parameter.transform_components(replacement))),
                ret: Box::new(ret.transform_components(replacement)),
            },
            other => other.clone(),
        }
    }

    /// Whether this type (or any component) is untyped in this phase;
    /// checks involving unknowns are suppressed.
    pub fn has_unknown(&self) -> bool {
        self.any_component(&mut |component| matches!(component, Type::Unknown | Type::Var(_)))
    }

    /// Widens literal types to their primitives (CDR-004 §4: applied
    /// at unannotated `let` and every `let mut`).
    pub fn widen(self) -> Type {
        match self {
            Type::Literal(lit) => match lit.prim() {
                Some(p) => Type::Prim(p),
                None => Type::Literal(lit), // null stays null
            },
            other => other,
        }
    }

    /// Builds a normalized union: flattens nested unions, removes
    /// duplicates, orders canonically, and collapses a single
    /// survivor to itself.
    pub fn union(members: Vec<Type>) -> Type {
        let mut flat = Vec::new();
        for m in members {
            match m {
                Type::Union(inner) => flat.extend(inner),
                other => flat.push(other),
            }
        }
        flat.sort(); // structural Ord — canonical identity, not display text
        flat.dedup();
        if flat.len() == 1 {
            flat.pop().expect("non-empty")
        } else {
            Type::Union(flat)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NonKeyableKey {
    pub(crate) subject: String,
    pub(crate) kind: &'static str,
}

/// CHECK-side mirror of `topaz_value::canonical_key` / `freeze` for Map/Set keys.
/// Unknown/opaque local inference shapes defer; every concrete type that the runtime
/// freeze leaf rejects is rejected here, recursively through structural key shapes.
/// Newtypes are keyable iff their base is keyable; the key snapshot still carries
/// the nominal id, so `UserId(1)` and `1` remain distinct keys.
/// Nominal records are keyable iff all declared field types are keyable; the key
/// snapshot carries the nominal id plus declaration-ordered field keys.
/// User enums are keyable iff all variant payload types are keyable; the key
/// snapshot carries the enum id, variant id/index, and positional payload keys.
pub(crate) fn non_keyable_map_set_key_with_nominals(
    ty: &Type,
    newtype_base: impl Fn(&str) -> Option<Type>,
    record_fields: impl Fn(&str) -> Option<Vec<Type>>,
    enum_payloads: impl Fn(&str) -> Option<Vec<Type>>,
) -> Option<NonKeyableKey> {
    let mut seen_newtypes = Vec::new();
    let mut seen_records = Vec::new();
    let mut seen_enums = Vec::new();
    non_keyable_map_set_key_inner(
        ty,
        &newtype_base,
        &record_fields,
        &enum_payloads,
        &mut seen_newtypes,
        &mut seen_records,
        &mut seen_enums,
    )
}

fn non_keyable_map_set_key_inner(
    ty: &Type,
    newtype_base: &impl Fn(&str) -> Option<Type>,
    record_fields: &impl Fn(&str) -> Option<Vec<Type>>,
    enum_payloads: &impl Fn(&str) -> Option<Vec<Type>>,
    seen_newtypes: &mut Vec<String>,
    seen_records: &mut Vec<String>,
    seen_enums: &mut Vec<String>,
) -> Option<NonKeyableKey> {
    match ty {
        // §8/§10 (v5.4) `Bytes`/`Path` are keyable immutable scalar-like leaves.
        Type::Prim(_)
        | Type::Literal(_)
        | Type::Bytes
        | Type::Path
        | Type::Url
        | Type::Date
        | Type::BigInt
        | Type::Decimal => None,
        Type::ByteBuffer => Some(NonKeyableKey {
            subject: "ByteBuffer".to_string(),
            kind: "mutable byte buffer",
        }),
        Type::Union(members) => members.iter().find_map(|member| {
            non_keyable_map_set_key_inner(
                member,
                newtype_base,
                record_fields,
                enum_payloads,
                seen_newtypes,
                seen_records,
                seen_enums,
            )
        }),
        Type::Record(fields) => fields.iter().find_map(|(_, field_ty)| {
            non_keyable_map_set_key_inner(
                field_ty,
                newtype_base,
                record_fields,
                enum_payloads,
                seen_newtypes,
                seen_records,
                seen_enums,
            )
        }),
        Type::Ctor(Ctor::Array | Ctor::Option | Ctor::Result, args) => {
            args.iter().find_map(|arg| {
                non_keyable_map_set_key_inner(
                    arg,
                    newtype_base,
                    record_fields,
                    enum_payloads,
                    seen_newtypes,
                    seen_records,
                    seen_enums,
                )
            })
        }
        Type::Ctor(Ctor::Map, _) => Some(NonKeyableKey {
            subject: "`Map` value".to_string(),
            kind: "Map",
        }),
        Type::Ctor(Ctor::Set, _) => Some(NonKeyableKey {
            subject: "`Set` value".to_string(),
            kind: "Set",
        }),
        Type::Ctor(Ctor::Range, _) => Some(NonKeyableKey {
            subject: "`range` value".to_string(),
            kind: "range",
        }),
        Type::Func { .. } => Some(NonKeyableKey {
            subject: "function value".to_string(),
            kind: "function",
        }),
        Type::Template => Some(NonKeyableKey {
            subject: "template value".to_string(),
            kind: "template",
        }),
        Type::File => Some(NonKeyableKey {
            subject: "`File` value".to_string(),
            kind: "File",
        }),
        Type::JsonValue => Some(NonKeyableKey {
            subject: "`JSONValue` value".to_string(),
            kind: "JSONValue",
        }),
        Type::Regex => Some(NonKeyableKey {
            subject: "`Regex` value".to_string(),
            kind: "Regex",
        }),
        Type::RoundingMode => Some(NonKeyableKey {
            subject: "`RoundingMode` value".to_string(),
            kind: "RoundingMode",
        }),
        Type::Match => Some(NonKeyableKey {
            subject: "`Match` value".to_string(),
            kind: "Match",
        }),
        Type::TomlValue => Some(NonKeyableKey {
            subject: "`TOMLValue` value".to_string(),
            kind: "TOMLValue",
        }),
        Type::Enum { base, args } => {
            let id = crate::form::nominal_instance_id(base, args);
            if seen_enums.iter().any(|seen| seen == &id) {
                return None;
            }
            let payloads = enum_payloads(&id)?;
            seen_enums.push(id);
            let result = payloads.iter().find_map(|payload_ty| {
                non_keyable_map_set_key_inner(
                    payload_ty,
                    newtype_base,
                    record_fields,
                    enum_payloads,
                    seen_newtypes,
                    seen_records,
                    seen_enums,
                )
            });
            seen_enums.pop();
            result
        }
        Type::NominalRecord { base, args } => {
            let id = crate::form::nominal_instance_id(base, args);
            if seen_records.iter().any(|seen| seen == &id) {
                return None;
            }
            let fields = record_fields(&id)?;
            seen_records.push(id);
            let result = fields.iter().find_map(|field_ty| {
                non_keyable_map_set_key_inner(
                    field_ty,
                    newtype_base,
                    record_fields,
                    enum_payloads,
                    seen_newtypes,
                    seen_records,
                    seen_enums,
                )
            });
            seen_records.pop();
            result
        }
        Type::Newtype { base, args } => {
            let id = crate::form::nominal_instance_id(base, args);
            if seen_newtypes.iter().any(|seen| seen == &id) {
                return None;
            }
            let base = newtype_base(&id)?;
            seen_newtypes.push(id);
            let result = non_keyable_map_set_key_inner(
                &base,
                newtype_base,
                record_fields,
                enum_payloads,
                seen_newtypes,
                seen_records,
                seen_enums,
            );
            seen_newtypes.pop();
            result
        }
        Type::Unknown | Type::Var(_) | Type::Foreign { .. } | Type::Skolem { .. } => None,
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Prim(p) => f.write_str(p.name()),
            Type::Literal(Lit::Str(s)) => write!(f, "\"{s}\""),
            Type::Literal(Lit::Int(n)) => write!(f, "{n}"),
            Type::Literal(Lit::Float(s)) => f.write_str(s),
            Type::Literal(Lit::Bool(b)) => write!(f, "{b}"),
            Type::Literal(Lit::Null) => f.write_str("null"),
            Type::Union(ms) => {
                for (i, m) in ms.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" | ")?;
                    }
                    write!(f, "{m}")?;
                }
                Ok(())
            }
            Type::Record(fields) => {
                f.write_str("{ ")?;
                for (i, (name, ty)) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{name}: {ty}")?;
                }
                f.write_str(" }")
            }
            Type::Ctor(Ctor::Range, _) => f.write_str("range"),
            Type::Ctor(c, args) => {
                f.write_str(c.name())?;
                f.write_str("<")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{a}")?;
                }
                f.write_str(">")
            }
            Type::Func {
                params,
                variadic,
                ret,
            } => {
                f.write_str("(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{p}")?;
                }
                if let Some(v) = variadic {
                    if !params.is_empty() {
                        f.write_str(", ")?;
                    }
                    write!(f, "...{v}")?;
                }
                write!(f, ") -> {ret}")
            }
            Type::Foreign { name, args } => {
                f.write_str(name)?;
                if !args.is_empty() {
                    f.write_str("<")?;
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            f.write_str(", ")?;
                        }
                        write!(f, "{a}")?;
                    }
                    f.write_str(">")?;
                }
                Ok(())
            }
            Type::Skolem { name, .. } => f.write_str(name),
            Type::Template => f.write_str("template"),
            Type::File => f.write_str("File"),
            Type::JsonValue => f.write_str("JSONValue"),
            Type::Bytes => f.write_str("Bytes"),
            Type::ByteBuffer => f.write_str("ByteBuffer"),
            Type::Path => f.write_str("Path"),
            Type::Regex => f.write_str("Regex"),
            Type::Match => f.write_str("Match"),
            Type::TomlValue => f.write_str("TOMLValue"),
            Type::Url => f.write_str("URL"),
            Type::Date => f.write_str("Date"),
            Type::BigInt => f.write_str("BigInt"),
            Type::Decimal => f.write_str("Decimal"),
            Type::RoundingMode => f.write_str("RoundingMode"),
            Type::Enum { base, args } => f.write_str(&crate::form::nominal_instance_id(base, args)),
            Type::NominalRecord { base, args } => {
                f.write_str(&crate::form::nominal_instance_id(base, args))
            }
            Type::Newtype { base, args } => {
                f.write_str(&crate::form::nominal_instance_id(base, args))
            }
            Type::Unknown => f.write_str("?"),
            Type::Var(n) => write!(f, "?{n}"),
        }
    }
}
