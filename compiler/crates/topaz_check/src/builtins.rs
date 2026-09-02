//! The §22 standard-library surface as data (CDR-004 §5): one table
//! feeds call typing. Free functions and static members are schemes
//! (rank-1, fresh variables per call site); receiver members resolve
//! their element types directly from the receiver.

use crate::ty::{Ctor, Prim, Type};

/// A rank-1 signature: `vars` type parameters appearing as `Var(i)`
/// in `params`/`variadic`/`ret`; `required` counts parameters without
/// defaults; `names` enable named-argument matching.
#[derive(Debug, Clone)]
pub struct Scheme {
    pub vars: u32,
    pub params: Vec<Type>,
    pub names: Vec<String>,
    /// Whether the parameter-name table is authoritative: true for
    /// builtins and user declarations (even with zero fixed
    /// parameters); false for bare function TYPES, whose named
    /// arguments stay unjudged.
    pub names_known: bool,
    pub required: usize,
    /// Per fixed parameter: declared with a default. Empty means the
    /// prefix rule (the first `required` parameters are required).
    pub defaulted: Vec<bool>,
    pub variadic: Option<Type>,
    pub ret: Type,
}

impl Scheme {
    /// Whether fixed slot `i` must be explicitly bound (§5/§7).
    pub fn slot_required(&self, i: usize) -> bool {
        if self.defaulted.is_empty() {
            i < self.required
        } else {
            !self.defaulted.get(i).copied().unwrap_or(false)
        }
    }
}

fn unit() -> Type {
    Type::Prim(Prim::Unit)
}

fn string() -> Type {
    Type::Prim(Prim::String)
}

fn int() -> Type {
    Type::Prim(Prim::Int)
}

fn float() -> Type {
    Type::Prim(Prim::Float)
}

fn boolean() -> Type {
    Type::Prim(Prim::Bool)
}

/// §8 (v5.4) the `Bytes` type (the byte-array/encoding stdlib value).
fn bytes() -> Type {
    Type::Bytes
}

fn byte_buffer() -> Type {
    Type::ByteBuffer
}

/// §10 (v5.4) the `Path` type (normalized logical project-relative path).
fn path() -> Type {
    Type::Path
}

/// §11 (v5.4) the `Regex` type (opaque compiled pattern).
fn regex() -> Type {
    Type::Regex
}

/// §11 (v5.4) the `Match` type (scalar-offset regex match record).
fn regex_match() -> Type {
    Type::Match
}

/// §12 (v5.4) parsed TOML tree value.
fn toml_value() -> Type {
    Type::TomlValue
}

/// §16 (v5.4) parsed URL value.
fn url() -> Type {
    Type::Url
}

/// §13 (v5.4) deterministic Gregorian date value.
fn date() -> Type {
    Type::Date
}

/// §14.1 (v5.4) explicit arbitrary-precision integer value.
fn bigint() -> Type {
    Type::BigInt
}

/// §14.2 (v5.4) deterministic exact decimal value.
fn decimal() -> Type {
    Type::Decimal
}

fn rounding_mode() -> Type {
    Type::RoundingMode
}

fn var(i: u32) -> Type {
    Type::Var(i)
}

fn option(t: Type) -> Type {
    Type::Ctor(Ctor::Option, vec![t])
}

fn result(t: Type, e: Type) -> Type {
    Type::Ctor(Ctor::Result, vec![t, e])
}

fn array(t: Type) -> Type {
    Type::Ctor(Ctor::Array, vec![t])
}

fn map(k: Type, v: Type) -> Type {
    Type::Ctor(Ctor::Map, vec![k, v])
}

fn func(params: Vec<Type>, ret: Type) -> Type {
    Type::Func {
        params,
        variadic: None,
        ret: Box::new(ret),
    }
}

/// Whether `member` is an in-place mutator on collection
/// `receiver` (§9: such a call requires a mutable root binding).
pub fn is_mutator(receiver: &Type, member: &str) -> bool {
    matches!(
        (receiver, member),
        // §6 (v5.4) array mutation API: `pop`/`removeAt` return `Option<T>`; `insert`
        // faults on an out-of-range index; `sort`/`sortBy` reorder in place (the
        // RETURN-new `sorted`/`sortedBy` are NOT mutators); `retain` keeps by a
        // predicate; `clear`/`reverse` are the simple in-place mutators.
        (
            Type::Ctor(Ctor::Array, _),
            "push"
                | "pop"
                | "clear"
                | "reverse"
                | "insert"
                | "removeAt"
                | "sort"
                | "sortBy"
                | "retain"
        )
            // §6 (v5.4) `clear` empties in place; `update(k, initial, f)` mutates the
            // entry at `k` (replace via `f`, keeping its slot) or appends `initial`.
            | (Type::Ctor(Ctor::Map, _), "insert" | "remove" | "clear" | "update")
            | (Type::Ctor(Ctor::Set, _), "add" | "remove" | "clear")
            | (Type::ByteBuffer, "set" | "fill" | "copy")
    )
}

/// `Iterable<T>` (§22.1): the §10 iteration types — `Array<T>`,
/// `Set<T>`, and integer ranges. (`map.keys` arrives as `Array<K>`.)
pub fn iterable_elem(ty: &Type) -> Option<Type> {
    match ty {
        Type::Ctor(Ctor::Array | Ctor::Set | Ctor::Range, args) => Some(args[0].clone()),
        _ => None,
    }
}

/// Every name `free_function` resolves — the callable candidates for an
/// Unbound-name "did you mean …?" suggestion. Kept in lockstep with
/// `free_function` by the `free_function_names_match_resolver` test below.
pub const FREE_FUNCTION_NAMES: &[&str] = &[
    "print",
    "toInt",
    "toIntRadix",
    "fromCodePoint",
    "toFloat",
    "input",
    "Some",
    "Ok",
    "Err",
    "map",
    "filter",
    "reduce",
    "open",
    "assert",
];

/// Every name `constant` resolves — the nullary builtin value candidates (e.g.
/// `None`) for an unbound-name suggestion in VALUE position. Kept in lockstep
/// with `constant` by `constant_names_match_resolver`.
pub const CONSTANT_NAMES: &[&str] = &["None"];

/// Every builtin receiver member name, independent of receiver type. Inherent
/// user methods may not reuse one of these spellings because runtime receiver
/// dispatch must never choose between a user method and a builtin member. This
/// is the single checker-side authority consumed by method admission; the
/// two-way catalog test below keeps it equal to [`receiver_member_names`].
pub const RESERVED_RECEIVER_MEMBER_NAMES: &[&str] = &[
    "atLeast",
    "atMost",
    "scalars",
    "startsWith",
    "endsWith",
    "contains",
    "indexOf",
    "lastIndexOf",
    "trim",
    "trimStart",
    "trimEnd",
    "byteLength",
    "codePointAt",
    "split",
    "slice",
    "replace",
    "okOr",
    "okOrElse",
    "map",
    "flatMap",
    "filter",
    "reduce",
    "push",
    "get",
    "length",
    "join",
    "sorted",
    "sortedBy",
    "pop",
    "clear",
    "reverse",
    "insert",
    "removeAt",
    "sort",
    "sortBy",
    "retain",
    "getOr",
    "remove",
    "keys",
    "values",
    "entries",
    "isEmpty",
    "containsKey",
    "update",
    "mapValues",
    "add",
    "toArray",
    "union",
    "intersection",
    "difference",
    "read",
    "write",
    "close",
    "tag",
    "parts",
    "kind",
    "isNull",
    "asString",
    "asBool",
    "asInt",
    "numberText",
    "at",
    "asArray",
    "decodeUtf8",
    "toHex",
    "toBase64",
    "set",
    "fill",
    "copy",
    "toBytes",
    "parent",
    "fileName",
    "extension",
    "withExtension",
    "normalize",
    "toString",
    "isMatch",
    "find",
    "findAll",
    "replaceAll",
    "start",
    "end",
    "text",
    "groups",
    "named",
    "scheme",
    "host",
    "path",
    "query",
    "fragment",
    "toIso",
    "addDays",
    "year",
    "month",
    "day",
    "toInt",
    "div",
    "mod",
    "scale",
    "round",
    // The nominal newtype unwrap is resolved outside `receiver_member`.
    "value",
];

pub fn is_reserved_receiver_member_name(name: &str) -> bool {
    RESERVED_RECEIVER_MEMBER_NAMES.contains(&name)
}

/// Every namespace resolved by [`static_member`]. The namespace-local member
/// arrays below are the single name authority used by checking, completion,
/// and Stage 0/self-host agreement fixtures.
pub const STATIC_NAMESPACE_NAMES: &[&str] = &[
    "Array",
    "Map",
    "Set",
    "JSON",
    "Test",
    "Math",
    "Bytes",
    "ByteBuffer",
    "Encoding",
    "Codec",
    "Hash",
    "FS",
    "Cli",
    "Path",
    "Regex",
    "CSV",
    "TOML",
    "URL",
    "Date",
    "BigInt",
    "Decimal",
];

/// Builtin protocol static surfaces, formed by the protocol checker rather than
/// [`static_member`]. Tooling consumes the same namespace/member spellings.
pub const SHOW_PROTOCOL_SURFACE: (&str, &str) = ("Show", "show");
pub const EQ_PROTOCOL_SURFACE: (&str, &str) = ("Eq", "equals");
pub const ORDER_PROTOCOL_SURFACE: (&str, &str) = ("Order", "compare");
pub const BUILTIN_PROTOCOL_SURFACES: &[(&str, &str)] = &[
    SHOW_PROTOCOL_SURFACE,
    EQ_PROTOCOL_SURFACE,
    ORDER_PROTOCOL_SURFACE,
];

/// The closed value surface of the builtin `RoundingMode` enum-like namespace.
/// The expression checker and tooling both consume this table.
pub const ROUNDING_MODE_VALUE_NAMES: &[&str] = &[
    "Down",
    "Up",
    "TowardZero",
    "AwayFromZero",
    "HalfUp",
    "HalfEven",
];

pub fn static_value_member_names(namespace: &str) -> &'static [&'static str] {
    match namespace {
        "RoundingMode" => ROUNDING_MODE_VALUE_NAMES,
        _ => &[],
    }
}

pub fn static_member_names(namespace: &str) -> &'static [&'static str] {
    match namespace {
        "Array" => &["of"],
        "Map" => &["new", "ofEntries"],
        "Set" => &["of"],
        "JSON" => &["stringify", "parseAs", "decode", "parse"],
        "Test" => &[
            "assert",
            "assertEq",
            "assertNe",
            "assertContains",
            "assertOk",
            "assertErr",
            "assertSome",
            "assertNone",
            "assertGolden",
        ],
        "Math" => &[
            "sqrt",
            "abs",
            "floor",
            "ceil",
            "round",
            "sin",
            "cos",
            "tan",
            "isNaN",
            "isFinite",
            "parseFloat",
            "min",
            "max",
        ],
        "Bytes" => &[
            "empty",
            "encodeUtf8",
            "fromArray",
            "fromHex",
            "fromBase64",
            "concat",
        ],
        "ByteBuffer" => &["allocate", "fromBytes"],
        "Encoding" => &[
            "utf8Encode",
            "utf8Decode",
            "hexEncode",
            "hexDecode",
            "base64Encode",
            "base64Decode",
        ],
        "Codec" => &[
            "gzipCompress",
            "gzipDecompress",
            "deflateCompress",
            "deflateFixedCompress",
            "zlibFixedCompress",
            "reedSolomon255223Protect",
            "deflateDecompress",
            "zstdCompress",
            "zstdDecompress",
        ],
        "Hash" => &["sha256", "sha512", "hmacSha256", "crc32"],
        "FS" => &["readText", "writeText", "readBytes", "writeBytes", "list"],
        "Cli" => &["hasFlag", "option", "options", "positionals"],
        "Path" => &["from", "cwdRelative", "project"],
        "Regex" => &["compile"],
        "CSV" => &[
            "parse",
            "parseWithHeader",
            "stringify",
            "stringifyWithHeader",
        ],
        "TOML" => &["parse", "stringify", "toJson", "fromJson"],
        "URL" => &["parse"],
        "Date" => &["fromYmd", "parseIso"],
        "BigInt" => &["fromInt", "parse"],
        "Decimal" => &["fromInt", "parse"],
        _ => &[],
    }
}

/// Free prelude/core functions (§22.1/§22.2/§22.3/§22.4).
pub fn free_function(name: &str) -> Option<Scheme> {
    Some(match name {
        "print" => Scheme {
            vars: 0,
            params: vec![string()],
            names: vec!["value".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 1,
            variadic: None,
            ret: unit(),
        },
        "toInt" => Scheme {
            vars: 0,
            params: vec![string()],
            names: vec!["text".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 1,
            variadic: None,
            ret: option(int()),
        },
        // `toIntRadix(text, radix) -> Option<int>` — parse in base 2..=36 (out-of-range → None).
        "toIntRadix" => Scheme {
            vars: 0,
            params: vec![string(), int()],
            names: vec!["text".to_string(), "radix".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 2,
            variadic: None,
            ret: option(int()),
        },
        // §22 `fromCodePoint(n) -> Option<string>` — the inverse of `str.codePointAt`; None for
        // a non-scalar n (negative, > U+10FFFF, or a surrogate).
        "fromCodePoint" => Scheme {
            vars: 0,
            params: vec![int()],
            names: vec!["n".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 1,
            variadic: None,
            ret: option(string()),
        },
        // §22 `toFloat(n) -> float` — explicit int->float (no implicit widening).
        "toFloat" => Scheme {
            vars: 0,
            params: vec![int()],
            names: vec!["n".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 1,
            variadic: None,
            ret: float(),
        },
        // §22 `input() -> string`: zero-arg host pull (the per-run text payload).
        "input" => Scheme {
            vars: 0,
            params: vec![],
            names: vec![],
            names_known: true,
            defaulted: Vec::new(),
            required: 0,
            variadic: None,
            ret: string(),
        },
        "Some" => Scheme {
            vars: 1,
            params: vec![var(0)],
            names: vec!["value".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 1,
            variadic: None,
            ret: option(var(0)),
        },
        "Ok" => Scheme {
            vars: 2,
            params: vec![var(0)],
            names: vec!["value".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 1,
            variadic: None,
            ret: result(var(0), var(1)),
        },
        "Err" => Scheme {
            vars: 2,
            params: vec![var(1)],
            names: vec!["error".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 1,
            variadic: None,
            ret: result(var(0), var(1)),
        },
        "map" => Scheme {
            vars: 2,
            params: vec![Type::Unknown, func(vec![var(0)], var(1))],
            names: vec!["xs".to_string(), "f".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 2,
            variadic: None,
            ret: array(var(1)),
        },
        "filter" => Scheme {
            vars: 1,
            params: vec![Type::Unknown, func(vec![var(0)], boolean())],
            names: vec!["xs".to_string(), "f".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 2,
            variadic: None,
            ret: array(var(0)),
        },
        "reduce" => Scheme {
            vars: 2,
            params: vec![Type::Unknown, var(1), func(vec![var(1), var(0)], var(1))],
            names: vec!["xs".to_string(), "initial".to_string(), "f".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 3,
            variadic: None,
            ret: var(1),
        },
        "open" => Scheme {
            vars: 0,
            params: vec![string()],
            names: vec!["path".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 1,
            variadic: None,
            ret: result(Type::File, string()),
        },
        // Test profile only (§22.4); profile policing is not the
        // checker's job.
        "assert" => Scheme {
            vars: 0,
            params: vec![boolean(), string()],
            names: vec!["condition".to_string(), "message".to_string()],
            names_known: true,
            defaulted: vec![false, true],
            required: 1,
            variadic: None,
            ret: unit(),
        },
        _ => return None,
    })
}

/// The `xs` parameter of map/filter/reduce is `Iterable<T>`; the table
/// stores Unknown there and the call engine refines T from the
/// argument via this hook.
pub fn iterable_param_fixup(name: &str) -> bool {
    matches!(name, "map" | "filter" | "reduce")
}

/// Polymorphic constructor values (§22.1).
pub fn constant(name: &str) -> Option<Scheme> {
    match name {
        // None: Option<T> — a value, modeled as a zero-param scheme.
        "None" => Some(Scheme {
            vars: 1,
            params: vec![],
            names: vec![],
            names_known: true,
            defaulted: Vec::new(),
            required: 0,
            variadic: None,
            ret: option(var(0)),
        }),
        _ => None,
    }
}

/// Static members: `Array.of`, `Map.new`, `Set.of` (§22.2).
pub fn static_member(ty_name: &str, member: &str) -> Option<Scheme> {
    if !static_member_names(ty_name).contains(&member) {
        return None;
    }
    Some(match (ty_name, member) {
        ("Array", "of") => Scheme {
            vars: 1,
            params: vec![],
            names: vec![],
            names_known: true,
            defaulted: Vec::new(),
            required: 0,
            variadic: Some(var(0)),
            ret: array(var(0)),
        },
        ("Map", "new") => Scheme {
            vars: 2,
            params: vec![],
            names: vec![],
            names_known: true,
            defaulted: Vec::new(),
            required: 0,
            variadic: None,
            ret: Type::Ctor(Ctor::Map, vec![var(0), var(1)]),
        },
        ("Map", "ofEntries") => Scheme {
            vars: 2,
            params: vec![array(Type::Record(vec![
                ("key".to_string(), var(0)),
                ("value".to_string(), var(1)),
            ]))],
            names: vec!["entries".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 1,
            variadic: None,
            ret: map(var(0), var(1)),
        },
        ("Set", "of") => Scheme {
            vars: 1,
            params: vec![],
            names: vec![],
            names_known: true,
            defaulted: Vec::new(),
            required: 0,
            variadic: Some(var(0)),
            ret: Type::Ctor(Ctor::Set, vec![var(0)]),
        },
        // §22 `JSON.stringify<T>(value: T) -> Result<string, string>` — the scheme is
        // polymorphic in `T`, but the CALL SITE arms a JSON-encodability gate
        // (`json_encodable_in`, expr.rs): a statically non-encodable argument is rejected
        // at CHECK (TPZ5533), so check==runtime. The `Result` `Err` arm remains for the
        // runtime leaf's backstop (`--unchecked`) and any dynamically-shaped value.
        ("JSON", "stringify") => Scheme {
            vars: 1,
            params: vec![var(0)],
            names: vec!["value".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 1,
            variadic: None,
            ret: result(string(), string()),
        },
        // §22 `JSON.parseAs<T>(text: string) -> Result<T, string>` — parse a string
        // then decode to `T`. The call checker gates `T` for JSON-decodability.
        ("JSON", "parseAs") => Scheme {
            vars: 1,
            params: vec![string()],
            names: vec!["text".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 1,
            variadic: None,
            ret: result(var(0), string()),
        },
        // §22 `JSON.decode<T>(value: JSONValue) -> Result<T, string>` — decode an
        // already parsed JSON tree using the same schema/leaf as `parseAs`.
        ("JSON", "decode") => Scheme {
            vars: 1,
            params: vec![Type::JsonValue],
            names: vec!["value".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 1,
            variadic: None,
            ret: result(var(0), string()),
        },
        // §22 `JSON.parse(text: string) -> Result<JSONValue, {message, line, column}>`
        // — invalid JSON is a value-level Err (1-based location), never a fault.
        ("JSON", "parse") => Scheme {
            vars: 0,
            params: vec![string()],
            names: vec!["text".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 1,
            variadic: None,
            ret: result(
                Type::JsonValue,
                Type::Record(vec![
                    ("column".to_string(), int()),
                    ("line".to_string(), int()),
                    ("message".to_string(), string()),
                ]),
            ),
        },
        // §18 (v5.4) test assertion namespace. `assertEq`/`assertNe` model the
        // planned `T: Eq` surface as a rank-1 builtin today; the call checker gates
        // concrete direct calls for comparability, and the runtime leaf remains the
        // backstop for generic wrappers until protocol bounds land.
        ("Test", "assert") => Scheme {
            vars: 0,
            params: vec![boolean(), string()],
            names: vec!["condition".to_string(), "message".to_string()],
            names_known: true,
            defaulted: vec![false, true],
            required: 1,
            variadic: None,
            ret: unit(),
        },
        ("Test", "assertEq") | ("Test", "assertNe") => Scheme {
            vars: 1,
            params: vec![var(0), var(0)],
            names: vec!["actual".to_string(), "expected".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 2,
            variadic: None,
            ret: unit(),
        },
        ("Test", "assertContains") => Scheme {
            vars: 0,
            params: vec![string(), string()],
            names: vec!["text".to_string(), "needle".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 2,
            variadic: None,
            ret: unit(),
        },
        ("Test", "assertOk") => Scheme {
            vars: 2,
            params: vec![result(var(0), var(1))],
            names: vec!["value".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 1,
            variadic: None,
            ret: var(0),
        },
        ("Test", "assertErr") => Scheme {
            vars: 2,
            params: vec![result(var(0), var(1))],
            names: vec!["value".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 1,
            variadic: None,
            ret: var(1),
        },
        ("Test", "assertSome") => Scheme {
            vars: 1,
            params: vec![option(var(0))],
            names: vec!["value".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 1,
            variadic: None,
            ret: var(0),
        },
        ("Test", "assertNone") => Scheme {
            vars: 1,
            params: vec![option(var(0))],
            names: vec!["value".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 1,
            variadic: None,
            ret: unit(),
        },
        ("Test", "assertGolden") => Scheme {
            vars: 0,
            params: vec![string(), string()],
            names: vec!["path".to_string(), "actual".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 2,
            variadic: None,
            ret: unit(),
        },
        // §8 (v5.4) the `Math` builtin NAMESPACE (the FIRST pure-compute stdlib
        // slice). Each is a monomorphic static member `Math.x(...)` over `float`
        // (the `**`/Pow float precedent). `sqrt`/`parseFloat` carry a value-level
        // failure mode and return `Result<…, string>` (a negative `sqrt` / an
        // unparseable string is an `Err`, never NaN and never a fault); the rest
        // are total. `min`/`max` take two floats; the predicates return `bool`.
        ("Math", "sqrt") => math_unary(result(float(), string()), "x"),
        ("Math", "abs") => math_unary(float(), "x"),
        ("Math", "floor") => math_unary(float(), "x"),
        ("Math", "ceil") => math_unary(float(), "x"),
        ("Math", "round") => math_unary(float(), "x"),
        ("Math", "sin") => math_unary(float(), "x"),
        ("Math", "cos") => math_unary(float(), "x"),
        ("Math", "tan") => math_unary(float(), "x"),
        ("Math", "isNaN") => math_unary(boolean(), "x"),
        ("Math", "isFinite") => math_unary(boolean(), "x"),
        ("Math", "parseFloat") => Scheme {
            vars: 0,
            params: vec![string()],
            names: vec!["s".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 1,
            variadic: None,
            ret: result(float(), string()),
        },
        ("Math", "min") | ("Math", "max") => Scheme {
            vars: 0,
            params: vec![float(), float()],
            names: vec!["a".to_string(), "b".to_string()],
            names_known: true,
            defaulted: Vec::new(),
            required: 2,
            variadic: None,
            ret: float(),
        },
        // §8 (v5.4) the `Bytes` builtin NAMESPACE static constructors. `encodeUtf8` is
        // total (`string -> Bytes`); `fromHex`/`fromBase64` are fallible (`string ->
        // Result<Bytes, string>`); `concat` is total over two `Bytes`. Each routes
        // through one shared `builtin_bytes_*` leaf (byte-identical run≡build).
        ("Bytes", "empty") => bytes_static(vec![], vec![], bytes()),
        ("Bytes", "encodeUtf8") => bytes_static(vec![string()], vec!["s"], bytes()),
        ("Bytes", "fromArray") => bytes_static(
            vec![array(int())],
            vec!["values"],
            result(bytes(), string()),
        ),
        ("Bytes", "fromHex") | ("Bytes", "fromBase64") => {
            bytes_static(vec![string()], vec!["s"], result(bytes(), string()))
        }
        ("Bytes", "concat") => bytes_static(vec![bytes(), bytes()], vec!["a", "b"], bytes()),
        ("ByteBuffer", "allocate") => Scheme {
            vars: 0,
            params: vec![int(), int()],
            names: vec!["length".to_string(), "value".to_string()],
            names_known: true,
            required: 1,
            defaulted: vec![false, true],
            variadic: None,
            ret: byte_buffer(),
        },
        ("ByteBuffer", "fromBytes") => bytes_static(vec![bytes()], vec!["value"], byte_buffer()),
        // §15 (v5.4) `Encoding` is the public codec namespace over the same Bytes
        // UTF-8/hex/base64 leaves. The static surface mirrors the plan wording while
        // preserving the existing `Bytes` value representation and error policy.
        ("Encoding", "utf8Encode") => bytes_static(vec![string()], vec!["text"], bytes()),
        ("Encoding", "utf8Decode") => {
            bytes_static(vec![bytes()], vec!["bytes"], result(string(), string()))
        }
        ("Encoding", "hexEncode") | ("Encoding", "base64Encode") => {
            bytes_static(vec![bytes()], vec!["bytes"], string())
        }
        ("Encoding", "hexDecode") | ("Encoding", "base64Decode") => {
            bytes_static(vec![string()], vec!["text"], result(bytes(), string()))
        }
        ("Codec", "gzipCompress")
        | ("Codec", "gzipDecompress")
        | ("Codec", "deflateCompress")
        | ("Codec", "deflateFixedCompress")
        | ("Codec", "zlibFixedCompress")
        | ("Codec", "reedSolomon255223Protect")
        | ("Codec", "deflateDecompress")
        | ("Codec", "zstdDecompress") => {
            bytes_static(vec![bytes()], vec!["bytes"], result(bytes(), string()))
        }
        ("Codec", "zstdCompress") => Scheme {
            vars: 0,
            params: vec![bytes(), int()],
            names: vec!["bytes".to_string(), "level".to_string()],
            names_known: true,
            defaulted: vec![false, true],
            required: 1,
            variadic: None,
            ret: result(bytes(), string()),
        },
        // §15 the `Hash` builtin namespace. Cryptographic digests/MACs return
        // `Bytes`; CRC-32 returns its unsigned value as `int`. Every method is
        // total and has no `Result` wrapper. Each routes through one shared
        // pure-Rust leaf (byte-identical run≡build).
        ("Hash", "sha256") | ("Hash", "sha512") => {
            bytes_static(vec![bytes()], vec!["data"], bytes())
        }
        ("Hash", "hmacSha256") => {
            bytes_static(vec![bytes(), bytes()], vec!["key", "message"], bytes())
        }
        ("Hash", "crc32") => bytes_static(vec![bytes()], vec!["data"], int()),
        // §10 (v5.4) capability-rooted filesystem helpers. Effects cross the Host
        // boundary, but errors remain value-level Results.
        ("FS", "readText") => Scheme {
            vars: 0,
            params: vec![Type::union(vec![string(), path()])],
            names: vec!["path".into()],
            names_known: true,
            required: 1,
            defaulted: Vec::new(),
            variadic: None,
            ret: result(string(), string()),
        },
        ("FS", "writeText") => Scheme {
            vars: 0,
            params: vec![Type::union(vec![string(), path()]), string()],
            names: vec!["path".into(), "text".into()],
            names_known: true,
            required: 2,
            defaulted: Vec::new(),
            variadic: None,
            ret: result(unit(), string()),
        },
        ("FS", "readBytes") => Scheme {
            vars: 0,
            params: vec![Type::union(vec![string(), path()])],
            names: vec!["path".into()],
            names_known: true,
            required: 1,
            defaulted: Vec::new(),
            variadic: None,
            ret: result(bytes(), string()),
        },
        ("FS", "writeBytes") => Scheme {
            vars: 0,
            params: vec![Type::union(vec![string(), path()]), bytes()],
            names: vec!["path".into(), "bytes".into()],
            names_known: true,
            required: 2,
            defaulted: Vec::new(),
            variadic: None,
            ret: result(unit(), string()),
        },
        ("FS", "list") => Scheme {
            vars: 0,
            params: vec![Type::union(vec![string(), path()])],
            names: vec!["path".into()],
            names_known: true,
            required: 1,
            defaulted: Vec::new(),
            variadic: None,
            ret: result(
                array(Type::Record(vec![
                    ("kind".to_string(), string()),
                    ("name".to_string(), string()),
                    ("sizeBytes".to_string(), option(int())),
                ])),
                string(),
            ),
        },
        // §10/§17 (v5.4) deterministic pure stdlib helpers. `Cli` never reads the
        // host; callers pass `args` explicitly. `Path` constructors normalize a
        // string into an immutable logical path or return an error string.
        ("Cli", "hasFlag") => Scheme {
            vars: 0,
            params: vec![array(string()), string()],
            names: vec!["args".into(), "name".into()],
            names_known: true,
            required: 2,
            defaulted: Vec::new(),
            variadic: None,
            ret: boolean(),
        },
        ("Cli", "option") => Scheme {
            vars: 0,
            params: vec![array(string()), string()],
            names: vec!["args".into(), "name".into()],
            names_known: true,
            required: 2,
            defaulted: Vec::new(),
            variadic: None,
            ret: option(string()),
        },
        ("Cli", "options") | ("Cli", "positionals") => Scheme {
            vars: 0,
            params: if member == "positionals" {
                vec![array(string())]
            } else {
                vec![array(string()), string()]
            },
            names: if member == "positionals" {
                vec!["args".into()]
            } else {
                vec!["args".into(), "name".into()]
            },
            names_known: true,
            required: if member == "positionals" { 1 } else { 2 },
            defaulted: Vec::new(),
            variadic: None,
            ret: array(string()),
        },
        ("Path", "from") | ("Path", "cwdRelative") | ("Path", "project") => Scheme {
            vars: 0,
            params: vec![string()],
            names: vec!["text".into()],
            names_known: true,
            required: 1,
            defaulted: Vec::new(),
            variadic: None,
            ret: result(path(), string()),
        },
        // §11 (v5.4) deterministic regex engine. Dynamic patterns return Result;
        // malformed patterns are data errors, not runtime faults.
        ("Regex", "compile") => Scheme {
            vars: 0,
            params: vec![string()],
            names: vec!["pattern".into()],
            names_known: true,
            required: 1,
            defaulted: Vec::new(),
            variadic: None,
            ret: result(regex(), string()),
        },
        // §12 (v5.4) CSV/TOML data-format helpers. Errors are value-level strings.
        ("CSV", "parse") => Scheme {
            vars: 0,
            params: vec![string()],
            names: vec!["text".into()],
            names_known: true,
            required: 1,
            defaulted: Vec::new(),
            variadic: None,
            ret: result(array(array(string())), string()),
        },
        ("CSV", "parseWithHeader") => Scheme {
            vars: 0,
            params: vec![string()],
            names: vec!["text".into()],
            names_known: true,
            required: 1,
            defaulted: Vec::new(),
            variadic: None,
            ret: result(array(map(string(), string())), string()),
        },
        ("CSV", "stringify") => Scheme {
            vars: 0,
            params: vec![array(array(string()))],
            names: vec!["rows".into()],
            names_known: true,
            required: 1,
            defaulted: Vec::new(),
            variadic: None,
            ret: string(),
        },
        ("CSV", "stringifyWithHeader") => Scheme {
            vars: 0,
            params: vec![array(map(string(), string())), array(string())],
            names: vec!["rows".into(), "columns".into()],
            names_known: true,
            required: 2,
            defaulted: Vec::new(),
            variadic: None,
            ret: string(),
        },
        ("TOML", "parse") => Scheme {
            vars: 0,
            params: vec![string()],
            names: vec!["text".into()],
            names_known: true,
            required: 1,
            defaulted: Vec::new(),
            variadic: None,
            ret: result(toml_value(), string()),
        },
        ("TOML", "stringify") => Scheme {
            vars: 0,
            params: vec![toml_value()],
            names: vec!["value".into()],
            names_known: true,
            required: 1,
            defaulted: Vec::new(),
            variadic: None,
            ret: result(string(), string()),
        },
        ("TOML", "toJson") => Scheme {
            vars: 0,
            params: vec![toml_value()],
            names: vec!["value".into()],
            names_known: true,
            required: 1,
            defaulted: Vec::new(),
            variadic: None,
            ret: Type::JsonValue,
        },
        ("TOML", "fromJson") => Scheme {
            vars: 0,
            params: vec![Type::JsonValue],
            names: vec!["value".into()],
            names_known: true,
            required: 1,
            defaulted: Vec::new(),
            variadic: None,
            ret: result(toml_value(), string()),
        },
        // §16 (v5.4) URL value helpers (no networking).
        ("URL", "parse") => Scheme {
            vars: 0,
            params: vec![string()],
            names: vec!["text".into()],
            names_known: true,
            required: 1,
            defaulted: Vec::new(),
            variadic: None,
            ret: result(url(), string()),
        },
        // §13 (v5.4) deterministic date helpers. No wall-clock access.
        ("Date", "fromYmd") => Scheme {
            vars: 0,
            params: vec![int(), int(), int()],
            names: vec!["year".into(), "month".into(), "day".into()],
            names_known: true,
            required: 3,
            defaulted: Vec::new(),
            variadic: None,
            ret: result(date(), string()),
        },
        ("Date", "parseIso") => Scheme {
            vars: 0,
            params: vec![string()],
            names: vec!["text".into()],
            names_known: true,
            required: 1,
            defaulted: Vec::new(),
            variadic: None,
            ret: result(date(), string()),
        },
        ("BigInt", "fromInt") => Scheme {
            vars: 0,
            params: vec![int()],
            names: vec!["n".into()],
            names_known: true,
            required: 1,
            defaulted: Vec::new(),
            variadic: None,
            ret: bigint(),
        },
        ("BigInt", "parse") => Scheme {
            vars: 0,
            params: vec![string(), int()],
            names: vec!["text".into(), "radix".into()],
            names_known: true,
            required: 2,
            defaulted: Vec::new(),
            variadic: None,
            ret: option(bigint()),
        },
        ("Decimal", "fromInt") => Scheme {
            vars: 0,
            params: vec![int()],
            names: vec!["n".into()],
            names_known: true,
            required: 1,
            defaulted: Vec::new(),
            variadic: None,
            ret: decimal(),
        },
        ("Decimal", "parse") => Scheme {
            vars: 0,
            params: vec![string()],
            names: vec!["text".into()],
            names_known: true,
            required: 1,
            defaulted: Vec::new(),
            variadic: None,
            ret: option(decimal()),
        },
        _ => return None,
    })
}

/// §8 (v5.4) a fixed-arity `Bytes.x(...)` static-member scheme — the shared shape
/// for the `Bytes` namespace constructors (`params`/`names`/`ret` spelled per call).
fn bytes_static(params: Vec<Type>, names: Vec<&str>, ret: Type) -> Scheme {
    let required = params.len();
    Scheme {
        vars: 0,
        params,
        names: names.into_iter().map(str::to_string).collect(),
        names_known: true,
        defaulted: Vec::new(),
        required,
        variadic: None,
        ret,
    }
}

/// §8 (v5.4) a one-`float`-argument `Math.x` static-member scheme with the given
/// return type and parameter name — the shared shape for `sqrt`/`abs`/`floor`/
/// `ceil`/`round`/`isNaN`/`isFinite` (`min`/`max`/`parseFloat` differ in arity or
/// arg type and are spelled out at the call site).
fn math_unary(ret: Type, param: &str) -> Scheme {
    Scheme {
        vars: 0,
        params: vec![float()],
        names: vec![param.to_string()],
        names_known: true,
        defaulted: Vec::new(),
        required: 1,
        variadic: None,
        ret,
    }
}

pub enum Member {
    Property(Type),
    Method(Scheme),
}

/// Receiver members (§22.2/§22.3). Element types come straight from
/// the receiver, so these schemes are already concrete (vars: 0).
pub fn receiver_member(receiver: &Type, member: &str) -> Option<Member> {
    // `receiver_member_names` is the AUTHORITATIVE set of valid member names. A
    // Member resolves only for a name it lists, so the two tables cannot disagree
    // about which names are valid: a member added to the arms below without listing
    // its name there simply will not resolve (caught by that member's own test),
    // and a name listed there without an arm fails `member_names_match_receiver_member`.
    if !receiver_member_names(receiver).contains(&member) {
        return None;
    }
    let mono = |params: Vec<Type>, names: Vec<&'static str>, ret: Type| {
        Member::Method(Scheme {
            vars: 0,
            names_known: true,
            defaulted: Vec::new(),
            required: params.len(),
            names: names.into_iter().map(str::to_string).collect(),
            params,
            variadic: None,
            ret,
        })
    };
    match receiver {
        // §22 int floor/ceiling helpers (clamp building blocks): `n.atLeast(m)` = max(n,m),
        // `n.atMost(m)` = min(n,m), including a positive decrement clamped at zero.
        Type::Prim(Prim::Int) | Type::Literal(crate::ty::Lit::Int(_)) => match member {
            "atLeast" => Some(mono(vec![int()], vec!["min"], int())),
            "atMost" => Some(mono(vec![int()], vec!["max"], int())),
            _ => None,
        },
        Type::Prim(Prim::String) | Type::Literal(crate::ty::Lit::Str(_)) => match member {
            "scalars" => Some(mono(vec![], vec![], array(string()))),
            // §22 string stdlib — scalar-based, read-only.
            "startsWith" => Some(mono(vec![string()], vec!["prefix"], boolean())),
            "endsWith" => Some(mono(vec![string()], vec!["suffix"], boolean())),
            "contains" => Some(mono(vec![string()], vec!["sub"], boolean())),
            "indexOf" => Some(mono(vec![string()], vec!["sub"], option(int()))),
            "lastIndexOf" => Some(mono(vec![string()], vec!["sub"], option(int()))),
            "codePointAt" => Some(mono(vec![int()], vec!["i"], option(int()))),
            "trim" | "trimStart" | "trimEnd" => Some(mono(vec![], vec![], string())),
            "byteLength" => Some(mono(vec![], vec![], int())),
            "split" => Some(mono(vec![string()], vec!["sep"], array(string()))),
            "slice" => Some(mono(vec![int(), int()], vec!["start", "end"], string())),
            "replace" => Some(mono(vec![string(), string()], vec!["old", "new"], string())),
            _ => None,
        },
        // §22.2 the Option→Result bridge. Unlike the other receiver members these
        // are GENERIC in the error type `E` (the only place a receiver method
        // introduces a fresh variable): `okOr(error: E) -> Result<T, E>` and
        // `okOrElse(f: () -> E) -> Result<T, E>`. `T` comes straight from the
        // receiver Ctor (already concrete), so `E` is the lone scheme var
        // (`Var(0)`), solved from the `error` argument / the callback's return
        // type — or left to a contextual type if unbound (§22.1), exactly as the
        // `Ok`/`Err`/`None` constructors. `okOrElse`'s `f` is a zero-arg function
        // type, which the call engine type-checks like any `() -> E` argument;
        // the LAZINESS (only calling `f` on `None`) is the runtime's job.
        Type::Ctor(Ctor::Option, args) => {
            let t = args[0].clone();
            let scheme = |params: Vec<Type>, names: Vec<&'static str>| {
                Member::Method(Scheme {
                    vars: 1,
                    names_known: true,
                    defaulted: Vec::new(),
                    required: params.len(),
                    names: names.into_iter().map(str::to_string).collect(),
                    params,
                    variadic: None,
                    ret: result(t.clone(), var(0)),
                })
            };
            match member {
                "okOr" => Some(scheme(vec![var(0)], vec!["error"])),
                "okOrElse" => Some(scheme(vec![func(vec![], var(0))], vec!["f"])),
                // §22 `opt.map(f)` — `(T) -> U`, result `Option<U>` (lazy Some-only).
                // The okOr* helpers return Result, so `map` builds its own scheme.
                "map" => Some(Member::Method(Scheme {
                    vars: 1,
                    params: vec![func(vec![t.clone()], var(0))],
                    names: vec!["f".to_string()],
                    names_known: true,
                    defaulted: Vec::new(),
                    required: 1,
                    variadic: None,
                    ret: option(var(0)),
                })),
                // §22 `opt.flatMap(f)` — `(T) -> Option<U>`, result `Option<U>` (the
                // callback already returns an Option, so no extra Some-wrap).
                "flatMap" => Some(Member::Method(Scheme {
                    vars: 1,
                    params: vec![func(vec![t.clone()], option(var(0)))],
                    names: vec!["f".to_string()],
                    names_known: true,
                    defaulted: Vec::new(),
                    required: 1,
                    variadic: None,
                    ret: option(var(0)),
                })),
                _ => None,
            }
        }
        Type::Ctor(Ctor::Result, args) => {
            let (t, e) = (args[0].clone(), args[1].clone());
            match member {
                // §22 `res.map(f)` — `(T) -> U`, result `Result<U, E>` (lazy Ok-only,
                // Err passes through unchanged).
                "map" => Some(Member::Method(Scheme {
                    vars: 1,
                    params: vec![func(vec![t.clone()], var(0))],
                    names: vec!["f".to_string()],
                    names_known: true,
                    defaulted: Vec::new(),
                    required: 1,
                    variadic: None,
                    ret: result(var(0), e.clone()),
                })),
                // §22 `res.flatMap(f)` — `(T) -> Result<U,E>`, result `Result<U,E>`
                // (the callback already returns a Result, so no extra Ok-wrap).
                "flatMap" => Some(Member::Method(Scheme {
                    vars: 1,
                    params: vec![func(vec![t], result(var(0), e.clone()))],
                    names: vec!["f".to_string()],
                    names_known: true,
                    defaulted: Vec::new(),
                    required: 1,
                    variadic: None,
                    ret: result(var(0), e),
                })),
                _ => None,
            }
        }
        Type::Ctor(Ctor::Array, args) => {
            let t = args[0].clone();
            match member {
                "push" => Some(mono(vec![t], vec!["x"], unit())),
                "get" => Some(mono(vec![int()], vec!["i"], option(t))),
                "length" => Some(Member::Property(int())),
                // §22 array stdlib: `slice(start, end) -> Array<T>` (half-open,
                // clamped); `join(sep) -> string` (each element rendered, joined);
                // `indexOf(x) -> int?` (first equal index, or None).
                "slice" => Some(mono(vec![int(), int()], vec!["start", "end"], array(t))),
                "join" => Some(mono(vec![string()], vec!["sep"], string())),
                "indexOf" => Some(mono(vec![t], vec!["x"], option(int()))),
                // `sorted() -> Array<T>` — ascending natural order; non-mutating. The
                // element ORDER-comparability gate (int/float/string and order-comparable
                // nominals) is applied at the CALL SITE in `expr.rs` (it needs the
                // enum/record/newtype tables); the runtime `values_compare` leaf agrees,
                // so check==runtime (no check-pass-then-fault).
                "sorted" => Some(mono(vec![], vec![], array(t))),
                // §22 (v5.4) `sortedBy(f) -> Array<T>` — ascending by the KEY projection
                // `f(x)`, STABLE, non-mutating. The callback `(T) -> K` introduces a FRESH
                // var (the key type K is not derivable from the receiver), so the scheme is
                // generic (vars: 1); the RESULT element type is the receiver's `T` (NOT K).
                // K must be ORDER-comparable — gated at the CALL SITE in `expr.rs` against
                // the resolved key type (the runtime sorts the keys via `values_compare`).
                "sortedBy" => Some(Member::Method(Scheme {
                    vars: 1,
                    params: vec![func(vec![t.clone()], var(0))],
                    names: vec!["f".to_string()],
                    names_known: true,
                    defaulted: Vec::new(),
                    required: 1,
                    variadic: None,
                    ret: array(t),
                })),
                // §22 receiver HOF: `xs.filter(f)` keeps elements where `f(x)` is
                // true — `(T) -> bool`, result `Array<T>` (no fresh type var).
                "filter" => Some(mono(
                    vec![func(vec![t.clone()], boolean())],
                    vec!["f"],
                    array(t),
                )),
                // `xs.map(f)` — `(T) -> U`, result `Array<U>`. The result element
                // type U is a FRESH var (not derivable from the receiver), so unlike
                // the other receiver members this scheme is generic (vars: 1).
                "map" => Some(Member::Method(Scheme {
                    vars: 1,
                    params: vec![func(vec![t.clone()], var(0))],
                    names: vec!["f".to_string()],
                    names_known: true,
                    defaulted: Vec::new(),
                    required: 1,
                    variadic: None,
                    ret: array(var(0)),
                })),
                // `xs.reduce(initial, f)` — `(initial: U, f: (U, T) -> U) -> U`.
                "reduce" => Some(Member::Method(Scheme {
                    vars: 1,
                    params: vec![var(0), func(vec![var(0), t], var(0))],
                    names: vec!["initial".to_string(), "f".to_string()],
                    names_known: true,
                    defaulted: Vec::new(),
                    required: 2,
                    variadic: None,
                    ret: var(0),
                })),
                // §6 (v5.4) array mutation API — IN-PLACE mutators (`let mut`-gated by
                // `is_mutator`) except where noted. The receiver cell mutates through the
                // shared `Rc<RefCell<Vec>>`, so an aliased binding sees the change.
                // `pop()` removes + returns the LAST element (`None` if empty).
                "pop" => Some(mono(vec![], vec![], option(t.clone()))),
                // `clear()` empties in place; `reverse()` reverses in place — both Unit.
                "clear" => Some(mono(vec![], vec![], unit())),
                "reverse" => Some(mono(vec![], vec![], unit())),
                // `insert(index, value)` — inserts at `index`; an out-of-range
                // (`index < 0 || index > length`) index FAULTS at runtime (§6.5).
                "insert" => Some(mono(vec![int(), t.clone()], vec!["index", "value"], unit())),
                // `removeAt(index)` — removes + returns the element at `index`
                // (`None` if the index is out of range, §6.5).
                "removeAt" => Some(mono(vec![int()], vec!["index"], option(t.clone()))),
                // `sort()` — IN-PLACE ascending natural order (reuses the `sorted`
                // comparator). The element ORDER-comparability gate is applied at the
                // CALL SITE in `expr.rs` (exactly like `sorted`). Returns Unit.
                "sort" => Some(mono(vec![], vec![], unit())),
                // `sortBy(f)` — IN-PLACE ascending by the KEY projection `f(x)`, STABLE.
                // Like `sortedBy` the callback `(T) -> K` introduces a FRESH var (the key
                // K is not derivable from the receiver), so the scheme is generic; K must
                // be ORDER-comparable (gated at the CALL SITE). Returns Unit (the
                // distinct, RETURN-new `sortedBy` keeps its array result).
                "sortBy" => Some(Member::Method(Scheme {
                    vars: 1,
                    params: vec![func(vec![t.clone()], var(0))],
                    names: vec!["f".to_string()],
                    names_known: true,
                    defaulted: Vec::new(),
                    required: 1,
                    variadic: None,
                    ret: unit(),
                })),
                // `retain(f)` — keep only elements where `f(x)` is true; the predicate
                // `(T) -> bool` is called exactly once per element in index order.
                // Returns Unit.
                "retain" => Some(mono(
                    vec![func(vec![t.clone()], boolean())],
                    vec!["f"],
                    unit(),
                )),
                _ => None,
            }
        }
        Type::Ctor(Ctor::Map, args) => {
            let (k, v) = (args[0].clone(), args[1].clone());
            match member {
                "insert" => Some(mono(vec![k, v], vec!["k", "v"], unit())),
                "get" => Some(mono(vec![k], vec!["k"], option(v))),
                "getOr" => Some(mono(vec![k, v.clone()], vec!["k", "default"], v)),
                "remove" => Some(mono(vec![k], vec!["k"], option(v))),
                "keys" => Some(Member::Property(array(k))),
                "values" => Some(Member::Property(array(v))),
                "entries" => Some(Member::Property(array(Type::Record(vec![
                    ("key".to_string(), k.clone()),
                    ("value".to_string(), v.clone()),
                ])))),
                "length" => Some(Member::Property(int())),
                "isEmpty" => Some(mono(vec![], vec![], boolean())),
                "containsKey" => Some(mono(vec![k.clone()], vec!["k"], boolean())),
                // §6 (v5.4) `m.clear()` — empties in place (a mutator; `let mut`-gated by
                // `is_mutator`). Returns Unit, like `insert`.
                "clear" => Some(mono(vec![], vec![], unit())),
                // §6 (v5.4) `m.update(k, initial, f)` — IN-PLACE: if `k` is present, replace
                // its value with `f(existing)` (keeping the existing insertion slot); if
                // absent, insert `initial` (appended). `f: (V) -> V`. A mutator (gated by
                // `is_mutator`); returns Unit. The callback type is concrete (V from the
                // receiver), so the scheme stays `vars: 0`.
                "update" => Some(mono(
                    vec![k, v.clone(), func(vec![v.clone()], v)],
                    vec!["k", "initial", "f"],
                    unit(),
                )),
                // §6 (v5.4) `m.mapValues(f)` — NON-mutating: a NEW `Map<K, W>` with every
                // value mapped through `f: (V) -> W`, keys + insertion order preserved. The
                // result value type W is a FRESH var (not derivable from the receiver), so
                // the scheme is generic (vars: 1).
                "mapValues" => Some(Member::Method(Scheme {
                    vars: 1,
                    params: vec![func(vec![v.clone()], var(0))],
                    names: vec!["f".to_string()],
                    names_known: true,
                    defaulted: Vec::new(),
                    required: 1,
                    variadic: None,
                    ret: Type::Ctor(Ctor::Map, vec![k.clone(), var(0)]),
                })),
                // §6 (v5.4) `m.filter(f)` — NON-mutating: a NEW `Map<K, V>` keeping the
                // entries where `f(k, v)` is true, in insertion order. `f: (K, V) -> bool`.
                "filter" => Some(mono(
                    vec![func(vec![k.clone(), v.clone()], boolean())],
                    vec!["f"],
                    Type::Ctor(Ctor::Map, vec![k, v]),
                )),
                _ => None,
            }
        }
        Type::Ctor(Ctor::Set, args) => {
            let t = args[0].clone();
            match member {
                "add" => Some(mono(vec![t.clone()], vec!["x"], unit())),
                "remove" => Some(mono(vec![t.clone()], vec!["x"], boolean())),
                "contains" => Some(mono(vec![t.clone()], vec!["x"], boolean())),
                "length" => Some(Member::Property(int())),
                "isEmpty" => Some(mono(vec![], vec![], boolean())),
                // §6 (v5.4) `s.toArray()` — the elements as an `Array<T>` in insertion order.
                "toArray" => Some(mono(vec![], vec![], array(t.clone()))),
                // §6 (v5.4) set algebra — NON-mutating, each returns a NEW `Set<T>` with a
                // DETERMINISTIC insertion-order result (see the runtime leaves): `union` =
                // self's elements then other's new ones; `intersection` = self's elements
                // also in other; `difference` = self's elements not in other.
                "union" => Some(mono(
                    vec![Type::Ctor(Ctor::Set, vec![t.clone()])],
                    vec!["other"],
                    Type::Ctor(Ctor::Set, vec![t.clone()]),
                )),
                "intersection" => Some(mono(
                    vec![Type::Ctor(Ctor::Set, vec![t.clone()])],
                    vec!["other"],
                    Type::Ctor(Ctor::Set, vec![t.clone()]),
                )),
                "difference" => Some(mono(
                    vec![Type::Ctor(Ctor::Set, vec![t.clone()])],
                    vec!["other"],
                    Type::Ctor(Ctor::Set, vec![t]),
                )),
                // §6 (v5.4) `s.clear()` — empties in place (a mutator; `let mut`-gated).
                "clear" => Some(mono(vec![], vec![], unit())),
                _ => None,
            }
        }
        Type::File => match member {
            "read" => Some(mono(vec![], vec![], result(string(), string()))),
            "write" => Some(mono(vec![string()], vec!["s"], result(unit(), string()))),
            "close" => Some(mono(vec![], vec![], unit())),
            _ => None,
        },
        // §16 template accessors (the only members a `template` exposes; the
        // interpolated values are deliberately NOT reachable — injection safety).
        Type::Template => match member {
            "tag" => Some(Member::Property(string())),
            "parts" => Some(Member::Property(array(string()))),
            _ => None,
        },
        // §22 JSONValue accessors inspect a parsed JSON tree. `kind`
        // is total; `as*`/`get`/`at`/`length` return `Option` (None on a shape/type
        // mismatch). `get`/`at` descend into objects/arrays as `Option<JSONValue>`.
        Type::JsonValue => match member {
            "kind" => Some(mono(vec![], vec![], string())),
            "isNull" => Some(mono(vec![], vec![], boolean())),
            "asString" => Some(mono(vec![], vec![], option(string()))),
            "asBool" => Some(mono(vec![], vec![], option(boolean()))),
            "asInt" => Some(mono(vec![], vec![], option(int()))),
            "numberText" => Some(mono(vec![], vec![], option(string()))),
            "get" => Some(mono(vec![string()], vec!["key"], option(Type::JsonValue))),
            "at" => Some(mono(vec![int()], vec!["index"], option(Type::JsonValue))),
            "length" => Some(mono(vec![], vec![], option(int()))),
            // §22 iteration: array elements / object keys / object values.
            "asArray" => Some(mono(vec![], vec![], option(array(Type::JsonValue)))),
            "keys" => Some(mono(vec![], vec![], option(array(string())))),
            "values" => Some(mono(vec![], vec![], option(array(Type::JsonValue)))),
            _ => None,
        },
        // §8 (v5.4) the `Bytes` INSTANCE methods. `decodeUtf8` is fallible
        // (`Result<string, string>`); `toHex`/`toBase64`/`length` are total; `slice`
        // CLAMPS (never faults, like `arr.slice`).
        Type::Bytes => match member {
            "decodeUtf8" => Some(mono(vec![], vec![], result(string(), string()))),
            "toHex" => Some(mono(vec![], vec![], string())),
            "toBase64" => Some(mono(vec![], vec![], string())),
            "length" => Some(mono(vec![], vec![], int())),
            "isEmpty" => Some(mono(vec![], vec![], boolean())),
            "get" => Some(mono(vec![int()], vec!["index"], option(int()))),
            "slice" => Some(mono(vec![int(), int()], vec!["start", "end"], bytes())),
            "toArray" => Some(mono(vec![], vec![], array(int()))),
            _ => None,
        },
        Type::ByteBuffer => match member {
            "length" => Some(mono(vec![], vec![], int())),
            "get" => Some(mono(vec![int()], vec!["index"], int())),
            "set" => Some(mono(vec![int(), int()], vec!["index", "value"], unit())),
            "fill" => Some(mono(
                vec![int(), int(), int()],
                vec!["start", "length", "value"],
                unit(),
            )),
            "copy" => Some(mono(
                vec![byte_buffer(), int(), int(), int()],
                vec!["source", "sourceStart", "targetStart", "length"],
                unit(),
            )),
            "toBytes" => Some(mono(vec![], vec![], bytes())),
            _ => None,
        },
        // §10 (v5.4) Path instance helpers over a normalized logical path.
        Type::Path => match member {
            "join" => Some(mono(
                vec![string()],
                vec!["child"],
                result(path(), string()),
            )),
            "parent" => Some(mono(vec![], vec![], option(path()))),
            "fileName" | "extension" => Some(mono(vec![], vec![], option(string()))),
            "withExtension" => Some(mono(vec![string()], vec!["ext"], result(path(), string()))),
            "normalize" => Some(mono(vec![], vec![], path())),
            "toString" => Some(mono(vec![], vec![], string())),
            _ => None,
        },
        // §11 (v5.4) Regex instance helpers. All offsets in Match values are scalar
        // indices, converted in the shared runtime leaf.
        Type::Regex => match member {
            "isMatch" => Some(mono(vec![string()], vec!["text"], boolean())),
            "find" => Some(mono(vec![string()], vec!["text"], option(regex_match()))),
            "findAll" => Some(mono(vec![string()], vec!["text"], array(regex_match()))),
            "split" => Some(mono(vec![string()], vec!["text"], array(string()))),
            "replaceAll" => Some(mono(
                vec![string(), string()],
                vec!["text", "replacement"],
                string(),
            )),
            _ => None,
        },
        Type::Match => match member {
            "start" | "end" => Some(Member::Property(int())),
            "text" => Some(Member::Property(string())),
            "groups" => Some(Member::Property(array(option(string())))),
            "named" => Some(Member::Property(map(string(), string()))),
            _ => None,
        },
        Type::Url => match member {
            "scheme" | "path" | "toString" => Some(mono(vec![], vec![], string())),
            "host" | "fragment" => Some(mono(vec![], vec![], option(string()))),
            "query" => Some(mono(vec![], vec![], map(string(), array(string())))),
            _ => None,
        },
        Type::Date => match member {
            "toIso" => Some(mono(vec![], vec![], string())),
            "addDays" => Some(mono(vec![int()], vec!["days"], date())),
            "year" | "month" | "day" => Some(mono(vec![], vec![], int())),
            _ => None,
        },
        Type::BigInt => match member {
            "toString" => Some(mono(vec![int()], vec!["radix"], string())),
            "toInt" => Some(mono(vec![], vec![], option(int()))),
            "div" | "mod" => Some(mono(
                vec![bigint()],
                vec!["other"],
                result(bigint(), string()),
            )),
            _ => None,
        },
        Type::Decimal => match member {
            "toString" => Some(mono(vec![], vec![], string())),
            "scale" => Some(mono(vec![], vec![], int())),
            "toInt" => Some(mono(vec![], vec![], option(int()))),
            "round" => Some(Member::Method(Scheme {
                vars: 0,
                names_known: true,
                defaulted: vec![false, true],
                required: 1,
                names: vec!["scale".into(), "mode".into()],
                params: vec![int(), rounding_mode()],
                variadic: None,
                ret: decimal(),
            })),
            "div" => Some(Member::Method(Scheme {
                vars: 0,
                names_known: true,
                defaulted: vec![false, false, true],
                required: 2,
                names: vec!["other".into(), "scale".into(), "mode".into()],
                params: vec![decimal(), int(), rounding_mode()],
                variadic: None,
                ret: result(decimal(), string()),
            })),
            _ => None,
        },
        _ => None,
    }
}

/// The member/method names a concrete builtin receiver exposes — the SAME set
/// `receiver_member` resolves. Used to offer a "did you mean" hint on an unknown
/// member (TPZ5006). Kept in lockstep with `receiver_member` by the test
/// `member_names_match_receiver_member`; an empty slice means "no suggestions".
pub fn receiver_member_names(receiver: &Type) -> &'static [&'static str] {
    match receiver {
        Type::Prim(Prim::Int) | Type::Literal(crate::ty::Lit::Int(_)) => &["atLeast", "atMost"],
        Type::Prim(Prim::String) | Type::Literal(crate::ty::Lit::Str(_)) => &[
            "scalars",
            "startsWith",
            "endsWith",
            "contains",
            "indexOf",
            "lastIndexOf",
            "trim",
            "trimStart",
            "trimEnd",
            "byteLength",
            "codePointAt",
            "split",
            "slice",
            "replace",
        ],
        Type::Ctor(Ctor::Option, _) => &["okOr", "okOrElse", "map", "flatMap"],
        Type::Ctor(Ctor::Result, _) => &["map", "flatMap"],
        Type::Ctor(Ctor::Array, _) => &[
            "push", "get", "length", "slice", "join", "indexOf", "sorted", "sortedBy", "filter",
            "map", "reduce", "pop", "clear", "reverse", "insert", "removeAt", "sort", "sortBy",
            "retain",
        ],
        Type::Ctor(Ctor::Map, _) => &[
            "insert",
            "get",
            "getOr",
            "remove",
            "keys",
            "values",
            "entries",
            "length",
            "isEmpty",
            "containsKey",
            "clear",
            "update",
            "mapValues",
            "filter",
        ],
        Type::Ctor(Ctor::Set, _) => &[
            "add",
            "remove",
            "contains",
            "length",
            "isEmpty",
            "toArray",
            "union",
            "intersection",
            "difference",
            "clear",
        ],
        Type::File => &["read", "write", "close"],
        Type::Template => &["tag", "parts"],
        Type::JsonValue => &[
            "kind",
            "isNull",
            "asString",
            "asBool",
            "asInt",
            "numberText",
            "get",
            "at",
            "length",
            "asArray",
            "keys",
            "values",
        ],
        Type::Bytes => &[
            "decodeUtf8",
            "toHex",
            "toBase64",
            "length",
            "isEmpty",
            "get",
            "slice",
            "toArray",
        ],
        Type::ByteBuffer => &["length", "get", "set", "fill", "copy", "toBytes"],
        Type::Path => &[
            "join",
            "parent",
            "fileName",
            "extension",
            "withExtension",
            "normalize",
            "toString",
        ],
        Type::Regex => &["isMatch", "find", "findAll", "split", "replaceAll"],
        Type::Match => &["start", "end", "text", "groups", "named"],
        Type::Url => &["scheme", "host", "path", "query", "fragment", "toString"],
        Type::Date => &["toIso", "addDays", "year", "month", "day"],
        Type::BigInt => &["toString", "toInt", "div", "mod"],
        Type::Decimal => &["toString", "scale", "toInt", "round", "div"],
        _ => &[],
    }
}

/// The CALLABLE members of a receiver — the subset of [`receiver_member_names`]
/// that resolve to a `Member::Method`, not an access-only `Member::Property`.
/// Derived from the existing Property/Method tagging, so it cannot drift from
/// `receiver_member` and grows automatically as the method table does. A
/// member-CALL "did you mean" suggestion uses this so it never offers a
/// non-callable member (e.g. `xs.lenght()` must NOT suggest the `length`
/// property, only methods like `push`/`get`).
pub fn callable_member_names(receiver: &Type) -> Vec<&'static str> {
    receiver_member_names(receiver)
        .iter()
        .copied()
        .filter(|m| matches!(receiver_member(receiver, m), Some(Member::Method(_))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty::Ctor;

    fn array_int() -> Type {
        Type::Ctor(Ctor::Array, vec![Type::Prim(Prim::Int)])
    }

    fn member_drift_types() -> Vec<Type> {
        vec![
            Type::Prim(Prim::Int),
            Type::Prim(Prim::String),
            Type::Ctor(Ctor::Option, vec![Type::Prim(Prim::Int)]),
            Type::Ctor(
                Ctor::Result,
                vec![Type::Prim(Prim::Int), Type::Prim(Prim::String)],
            ),
            array_int(),
            Type::Ctor(
                Ctor::Map,
                vec![Type::Prim(Prim::String), Type::Prim(Prim::Int)],
            ),
            Type::Ctor(Ctor::Set, vec![Type::Prim(Prim::Int)]),
            Type::File,
            Type::Template,
            Type::JsonValue,
            Type::Bytes,
            Type::ByteBuffer,
            Type::Path,
            Type::Regex,
            Type::Match,
            Type::Url,
            Type::Date,
            Type::BigInt,
            Type::Decimal,
        ]
    }

    #[test]
    fn member_names_match_receiver_member() {
        // Every advertised name must actually resolve, so the hint never points at
        // a member that does not exist (the two tables cannot drift apart).
        for ty in member_drift_types() {
            for name in receiver_member_names(&ty) {
                assert!(
                    receiver_member(&ty, name).is_some(),
                    "{ty}: advertised member `{name}` does not resolve"
                );
                assert!(
                    is_reserved_receiver_member_name(name),
                    "{ty}: builtin member `{name}` is not reserved for user methods"
                );
            }
        }
        let types = member_drift_types();
        for name in RESERVED_RECEIVER_MEMBER_NAMES {
            assert!(
                *name == "value"
                    || types
                        .iter()
                        .any(|ty| receiver_member_names(ty).contains(name)),
                "reserved receiver member `{name}` is not exposed by a builtin receiver"
            );
        }
    }

    #[test]
    fn callable_names_are_methods_and_exclude_properties() {
        // `callable_member_names` must yield exactly the Method-tagged members:
        // every name it returns resolves to a Method, and every access-only
        // Property (Array `length`, Map `keys`, Template `tag`/`parts`) is
        // EXCLUDED — so a member-CALL hint never offers a non-callable member.
        for ty in member_drift_types() {
            let callable = callable_member_names(&ty);
            for name in &callable {
                assert!(
                    matches!(receiver_member(&ty, name), Some(Member::Method(_))),
                    "{ty}: callable member `{name}` is not a Method"
                );
            }
            for name in receiver_member_names(&ty) {
                if matches!(receiver_member(&ty, name), Some(Member::Property(_))) {
                    assert!(
                        !callable.contains(name),
                        "{ty}: property `{name}` must be excluded from the callable set"
                    );
                }
            }
        }
        // Concretely: Array exposes `push`/`get` as callable but NOT `length`.
        let arr = array_int();
        let callable = callable_member_names(&arr);
        assert!(callable.contains(&"push") && callable.contains(&"get"));
        assert!(!callable.contains(&"length"));
    }

    #[test]
    fn static_member_catalog_is_unique_and_resolves() {
        let mut namespaces = std::collections::BTreeSet::new();
        for namespace in STATIC_NAMESPACE_NAMES {
            assert!(
                namespaces.insert(*namespace),
                "duplicate namespace `{namespace}`"
            );
            let mut members = std::collections::BTreeSet::new();
            assert!(
                !static_member_names(namespace).is_empty(),
                "static namespace `{namespace}` has no members"
            );
            for member in static_member_names(namespace) {
                assert!(
                    members.insert(*member),
                    "duplicate static member `{namespace}.{member}`"
                );
                assert!(
                    static_member(namespace, member).is_some(),
                    "`{namespace}.{member}` should resolve as a static member"
                );
            }
        }
    }

    #[test]
    fn builtin_member_typos_suggest_via_diag() {
        // The pure suggestion logic lives (and is tested) in `topaz_diag::suggest`;
        // here we only confirm the builtin member set wires into it — a real
        // `xs.lenght` resolves to `length`, and a short member (`get`) is never
        // offered for an unrelated short name (`set` would mislead a writer).
        let arr = array_int();
        let names = || receiver_member_names(&arr).iter().copied();
        assert_eq!(
            topaz_diag::suggest::did_you_mean("lenght", names()),
            "; did you mean `length`?"
        );
        assert_eq!(topaz_diag::suggest::closest("set", names()), None);
    }

    #[test]
    fn free_function_names_match_resolver() {
        // `FREE_FUNCTION_NAMES` is the unbound-name suggestion set; every entry
        // must actually resolve via `free_function`, or a "did you mean" would
        // offer a name that does not exist.
        for name in FREE_FUNCTION_NAMES {
            assert!(
                free_function(name).is_some(),
                "`{name}` is in FREE_FUNCTION_NAMES but free_function does not resolve it"
            );
        }
    }

    #[test]
    fn constant_names_match_resolver() {
        for name in CONSTANT_NAMES {
            assert!(
                constant(name).is_some(),
                "`{name}` is in CONSTANT_NAMES but constant does not resolve it"
            );
        }
    }
}
