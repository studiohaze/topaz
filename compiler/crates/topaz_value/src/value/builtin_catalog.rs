use super::*;

/// The §22 builtin surface (CDR-003 §1: every effect goes through the
/// host; everything else is pure value manipulation). A thin
/// first-class dispatch TAG — the dispatch itself lives in the engine
/// (`topaz_interp` / `topaz_rt`), never here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    /// Compiler-generated `std.lispex` intrinsics. They are not part of the
    /// ambient prelude and are bound only inside generated capability modules.
    LispexRule,
    LispexValueFromCanonical,
    LispexCanonicalBytes,
    LispexDefaultLimits,
    LispexInspectRule,
    LispexEvaluate,
    LispexEvaluateWithEvidence,
    LispexConsumerArtifactFromBytes,
    LispexConsumerArtifactBytes,
    LispexPortableCoreBytes,
    LispexInspectConsumerArtifact,
    LispexVerifyConsumerArtifact,
    LispexFreshReplay,
    Print,
    ToInt,
    ToIntRadix,
    FromCodePoint,
    ToFloat,
    MapFn,
    FilterFn,
    ReduceFn,
    Open,
    /// §22 `input()` — the host-provided per-run text payload (zero-arg, returns a
    /// `string`). A host EFFECT like `print`/`open`, but pull not push.
    Input,
    // §18 (v5.4) test assertion namespace. `assert` is also exposed as the
    // legacy/free test-profile builtin; the richer helpers are surfaced through
    // `Test.*` and the virtual `std.test` module.
    TestAssert,
    TestAssertEq,
    TestAssertNe,
    TestAssertContains,
    TestAssertOk,
    TestAssertErr,
    TestAssertSome,
    TestAssertNone,
    TestAssertGolden,
    ArrayOf,
    MapNew,
    MapOfEntries,
    SetOf,
    JsonStringify,
    JsonParse,
    // §22 JSONValue accessors — bound receiver methods routed through
    // the shared `call_method` leaf (like ArrGet/MapGet).
    JsonKind,
    JsonIsNull,
    JsonAsString,
    JsonAsBool,
    JsonAsInt,
    JsonNumberText,
    JsonGet,
    JsonAt,
    JsonLength,
    // §22 JSONValue iteration accessors — array/object → Topaz collections.
    JsonAsArray,
    JsonKeys,
    JsonValues,
    // §8 (v5.4) the `Math` builtin NAMESPACE (the FIRST pure-compute stdlib slice).
    // Each is a STATIC member `Math.x(...)` (like `JSON.stringify`), routed through a
    // single shared `builtin_math_*` leaf so every float op is byte-identical across
    // interp, boxed emit, and (on decline→boxed) native. `sqrt`/`parseFloat` have a
    // value-level failure mode and return `Result<…, string>`; the rest are total.
    MathSqrt,
    MathAbs,
    MathFloor,
    MathCeil,
    MathRound,
    MathSin,
    MathCos,
    MathTan,
    MathMin,
    MathMax,
    MathIsNaN,
    MathIsFinite,
    MathParseFloat,
    // §8 (v5.4) the `Bytes` builtin NAMESPACE — the byte-array + encoding stdlib.
    // The STATIC constructors `Bytes.x(...)` (like `JSON.parse`) route through one
    // shared `builtin_bytes_*` leaf so the UTF-8 / hex / base64 codecs are
    // byte-identical across interp, boxed emit, and (on decline→boxed) native. The
    // codecs are PURE (no crate): `encodeUtf8` is total; `decodeUtf8`/`fromHex`/
    // `fromBase64` are fallible and return `Result<…, string>`.
    BytesEmpty,
    BytesEncodeUtf8,
    BytesFromArray,
    BytesFromHex,
    BytesFromBase64,
    BytesConcat,
    // §8 (v5.4) the `Bytes` INSTANCE methods (bound receiver, routed through the
    // shared `call_method` leaf like the JSONValue accessors).
    BytesDecodeUtf8,
    BytesToHex,
    BytesToBase64,
    BytesLength,
    BytesIsEmpty,
    BytesGet,
    BytesSlice,
    BytesToArray,
    // ADR-108 fixed-length mutable byte buffer.
    ByteBufferAllocate,
    ByteBufferFromBytes,
    ByteBufferLength,
    ByteBufferGet,
    ByteBufferSet,
    ByteBufferFill,
    ByteBufferCopy,
    ByteBufferToBytes,
    // §15 (v5.4) `Encoding` is the public codec namespace over the same Bytes
    // UTF-8/hex/base64 leaves. It is deliberately an aliasing facade, not a second
    // implementation.
    EncodingUtf8Encode,
    EncodingUtf8Decode,
    EncodingHexEncode,
    EncodingHexDecode,
    EncodingBase64Encode,
    EncodingBase64Decode,
    // §15 (v5.4) deterministic compression/codecs. The public surface is backed
    // by dependency-free canonical stored/raw-block subsets so the emitted
    // vendored runtime closure stays offline and byte-reproducible.
    CodecGzipCompress,
    CodecGzipDecompress,
    CodecDeflateCompress,
    CodecDeflateFixedCompress,
    CodecZlibFixedCompress,
    CodecReedSolomon255223Protect,
    CodecDeflateDecompress,
    CodecZstdCompress,
    CodecZstdDecompress,
    // §15 (v5.4) the `Hash` builtin NAMESPACE — SHA-256/SHA-512 (FIPS 180-4) +
    // HMAC-SHA256 (RFC 2104). Each STATIC member `Hash.x(...)` (like `Bytes.concat`)
    // routes through one shared in-house pure-Rust hash leaf so the digest is
    // byte-identical across interp, boxed emit, and (on decline→boxed) native.
    // Bytes in, Bytes out (the caller does `.toHex()` for display).
    HashSha256,
    HashSha512,
    HashHmacSha256,
    HashCrc32,
    // §10 (v5.4) deterministic filesystem namespace. These are effectful, but
    // capability-rooted host leaves: user code passes an explicit path and receives
    // a value-level Result. Directory listing order is fixed by the host ABI.
    FsReadText,
    FsWriteText,
    FsReadBytes,
    FsWriteBytes,
    FsList,
    // §10/§17 (v5.4) deterministic pure stdlib helpers. `Cli` scans an explicit
    // `Array<string>` argument vector; `Path` stores normalized logical paths.
    CliHasFlag,
    CliOption,
    CliOptions,
    CliPositionals,
    PathFrom,
    PathCwdRelative,
    PathProject,
    PathJoin,
    PathParent,
    PathFileName,
    PathExtension,
    PathWithExtension,
    PathNormalize,
    PathToString,
    // §11 (v5.4) deterministic regex stdlib. `Regex.compile` is fallible
    // (`Result<Regex,string>`); instance helpers are pure and expose Match values
    // with scalar offsets, never byte offsets.
    RegexCompile,
    RegexIsMatch,
    RegexFind,
    RegexFindAll,
    RegexSplit,
    RegexReplaceAll,
    // §12/§16 (v5.4) data-format stdlib namespaces.
    CsvParse,
    CsvParseWithHeader,
    CsvStringify,
    CsvStringifyWithHeader,
    TomlParse,
    TomlStringify,
    TomlToJson,
    TomlFromJson,
    UrlParse,
    UrlScheme,
    UrlHost,
    UrlPath,
    UrlQuery,
    UrlFragment,
    UrlToString,
    // §13 (v5.4) deterministic date stdlib. No wall-clock access.
    DateFromYmd,
    DateParseIso,
    DateToIso,
    DateAddDays,
    DateYear,
    DateMonth,
    DateDay,
    // §14.1 (v5.4) explicit arbitrary-precision integer value. `int` remains
    // fixed-width checked; only values constructed through `BigInt` get
    // unbounded integer arithmetic.
    BigIntFromInt,
    BigIntParse,
    BigIntToString,
    BigIntToInt,
    BigIntDiv,
    BigIntMod,
    // §14.2 (v5.4) deterministic decimal value. This core slice covers exact
    // parse/format/int bridges and + - *; rounding/division land separately.
    DecimalFromInt,
    DecimalParse,
    DecimalToString,
    DecimalScale,
    DecimalToInt,
    DecimalRound,
    DecimalDiv,
    ArrPush,
    /// §6 (v5.4) `xs.pop()` — removes + returns the LAST element as `Option<T>`
    /// (`None` if empty). A `let mut`-gated mutator; a `call_method` leaf.
    ArrPop,
    /// §6 (v5.4) `xs.clear()` — empties the array IN PLACE (returns Unit). A
    /// `let mut`-gated mutator; a `call_method` leaf.
    ArrClear,
    /// §6 (v5.4) `xs.reverse()` — reverses the array IN PLACE (returns Unit). A
    /// `let mut`-gated mutator; a `call_method` leaf.
    ArrReverse,
    /// §6 (v5.4) `xs.insert(index, value)` — inserts `value` at `index`; an
    /// out-of-range index (`index < 0 || index > length`) FAULTS (`FAULT_INDEX`).
    /// A `let mut`-gated mutator; a `call_method` leaf.
    ArrInsert,
    /// §6 (v5.4) `xs.removeAt(index)` — removes + returns the element at `index`
    /// as `Option<T>` (`None` if the index is out of range). A `let mut`-gated
    /// mutator; a `call_method` leaf.
    ArrRemoveAt,
    /// §6 (v5.4) `xs.sort()` — sorts the array IN PLACE ascending (the SAME
    /// `values_compare`/`sort_values_stable` leaf `sorted` uses, written back into
    /// the cell). A `let mut`-gated mutator; a `call_method` leaf. Distinct from the
    /// RETURN-new `ArrSorted`.
    ArrSort,
    /// §6 (v5.4) `xs.sortBy(f)` — sorts the array IN PLACE ascending by the KEY
    /// projection `f(x)`, STABLE (the SAME `sorted_by_keys` leaf `sortedBy` uses,
    /// written back). INVOKES a user closure per element, so it is NOT a `call_method`
    /// leaf — both engines consume the shared callback-key state before write-back.
    /// A `let mut`-gated mutator.
    /// Distinct from the RETURN-new `ArrSortedBy`.
    ArrSortBy,
    /// §6 (v5.4) `xs.retain(f)` — keeps only the elements where `f(x)` is true; the
    /// predicate is called exactly once per element in index order, then the kept
    /// elements are written back IN PLACE (returns Unit). INVOKES a user closure per
    /// element, so it is NOT a `call_method` leaf — both engines consume the shared
    /// callback-retain state. A `let mut`-gated mutator.
    ArrRetain,
    ArrGet,
    ArrSlice,
    ArrJoin,
    ArrIndexOf,
    ArrSorted,
    /// §22 (v5.4) `xs.sortedBy(f)` — a NEW array sorted ascending by the KEY
    /// projection `f(x)`, STABLE, NON-mutating. Unlike `sorted` (a `call_method`
    /// leaf) it INVOKES a user closure per element, so each engine drives the calls
    /// itself (the interpreter via a `KSortBy` continuation that collects the keys
    /// then sorts; the emitter inline). The keys order through the SHARED
    /// `values_compare` leaf, so the projection sort is byte-identical run≡build.
    ArrSortedBy,
    MapInsert,
    MapGet,
    MapGetOr,
    MapRemove,
    MapContainsKey,
    /// §6 (v5.4) `m.isEmpty()` — whether the map has no entries (a `call_method` leaf).
    MapIsEmpty,
    /// §6 (v5.4) `m.clear()` — empties the map IN PLACE (a `let mut`-gated mutator,
    /// `call_method` leaf).
    MapClear,
    /// §6 (v5.4) `m.update(k, initial, f)` — IN-PLACE: replace `m[k]` with `f(existing)`
    /// keeping its slot, or append `initial` if absent. INVOKES a user closure, so it is
    /// NOT a `call_method` leaf — each engine drives the call itself (the interpreter via a
    /// `KMapUpdate` continuation, the emitter inline). A `let mut`-gated mutator.
    MapUpdate,
    /// §6 (v5.4) `m.mapValues(f)` — NON-mutating: a NEW `Map<K, W>` with each value mapped
    /// through `f`, keys + insertion order preserved. INVOKES a closure per value → driven
    /// by a `KMapValues` continuation (emitter inline).
    MapMapValues,
    /// §6 (v5.4) `m.filter(f)` — NON-mutating: a NEW `Map<K, V>` keeping the entries where
    /// `f(k, v)` is true, in insertion order. INVOKES a closure per entry → driven by a
    /// `KMapFilter` continuation (emitter inline).
    MapFilter,
    SetAdd,
    SetRemove,
    SetContains,
    /// §6 (v5.4) `s.isEmpty()` — whether the set has no elements (a `call_method` leaf).
    SetIsEmpty,
    /// §6 (v5.4) `s.toArray()` — the elements as `Array<T>` in insertion order (`call_method`).
    SetToArray,
    /// §6 (v5.4) `s.union(o)` / `s.intersection(o)` / `s.difference(o)` — NON-mutating set
    /// algebra; a NEW `Set<T>` in deterministic insertion order (`call_method` leaves).
    SetUnion,
    SetIntersection,
    SetDifference,
    /// §6 (v5.4) `s.clear()` — empties the set IN PLACE (a `let mut`-gated mutator, `call_method`).
    SetClear,
    Scalars,
    StrStartsWith,
    StrEndsWith,
    StrContains,
    StrIndexOf,
    StrLastIndexOf,
    StrCodePointAt,
    StrTrim,
    StrTrimStart,
    StrTrimEnd,
    StrSplit,
    StrByteLength,
    StrSlice,
    StrReplace,
    IntAtLeast,
    IntAtMost,
    /// §22.2 `opt.okOr(error)` — the EAGER Option→Result bridge: `Some(v)->Ok(v)`,
    /// `None->Err(error)`. A pure value method (the error is already a value), so
    /// it rides the shared `call_method` leaf like `get`/`scalars`.
    OkOr,
    /// §22.2 `opt.okOrElse(f)` — the LAZY Option→Result bridge: `Some(v)->Ok(v)`
    /// WITHOUT calling `f`, `None->Err(f())`. The shared callback transition owns
    /// the branch and result wrap while each engine drives the requested call.
    OkOrElse,
    /// §22 `opt.map(f)` — the LAZY Option callback: `Some(v)->Some(f(v))`,
    /// `None->None` (f untouched). Both evaluators consume the shared receiver-map
    /// transition that wraps the callback result in `Some`.
    OptionMap,
    /// §22 `opt.flatMap(f)` — like `map` but `f` returns `Option<U>` directly:
    /// `Some(v)->f(v)`, `None->None`. The shared receiver-map transition uses
    /// identity callback completion instead of wrapping the result again.
    OptionFlatMap,
    /// §22 `res.map(f)` — LAZY Ok-only: `Ok(v)->Ok(f(v))`, `Err(e)->Err(e)`.
    /// Both evaluators consume the shared receiver-map transition that wraps the
    /// callback result in `Ok`.
    ResultMap,
    /// §22 `res.flatMap(f)` — LAZY Ok-only: `Ok(v)->f(v)` (f returns
    /// `Result<U,E>` directly), `Err(e)->Err(e)`. The shared receiver-map
    /// transition uses identity callback completion.
    ResultFlatMap,
    FileRead,
    FileWrite,
    FileClose,
    /// §3 (v5.4) `id.value()` — the newtype UNWRAP method: zero-arg, returns the
    /// wrapped base value. Routed through the shared `call_method` leaf (which calls
    /// `newtype_value`) so the unwrap AND the `--unchecked` non-newtype fault are
    /// byte-identical run≡build.
    NewtypeValue,
}

impl Builtin {
    /// Return the compiler-owned `std.lispex` operation represented by this
    /// builtin, or `None` for every ambient and standard-library builtin.
    pub fn lispex_application_operation(self) -> Option<LispexApplicationOperation> {
        Some(match self {
            Builtin::LispexRule => LispexApplicationOperation::Rule,
            Builtin::LispexValueFromCanonical => LispexApplicationOperation::ValueFromCanonical,
            Builtin::LispexCanonicalBytes => LispexApplicationOperation::CanonicalBytes,
            Builtin::LispexDefaultLimits => LispexApplicationOperation::DefaultLimits,
            Builtin::LispexInspectRule => LispexApplicationOperation::InspectRule,
            Builtin::LispexEvaluate => LispexApplicationOperation::Evaluate,
            Builtin::LispexEvaluateWithEvidence => LispexApplicationOperation::EvaluateWithEvidence,
            Builtin::LispexConsumerArtifactFromBytes => {
                LispexApplicationOperation::ConsumerArtifactFromBytes
            }
            Builtin::LispexConsumerArtifactBytes => {
                LispexApplicationOperation::ConsumerArtifactBytes
            }
            Builtin::LispexPortableCoreBytes => LispexApplicationOperation::PortableCoreBytes,
            Builtin::LispexInspectConsumerArtifact => {
                LispexApplicationOperation::InspectConsumerArtifact
            }
            Builtin::LispexVerifyConsumerArtifact => {
                LispexApplicationOperation::VerifyConsumerArtifact
            }
            Builtin::LispexFreshReplay => LispexApplicationOperation::FreshReplay,
            _ => return None,
        })
    }

    /// Resolve an ambient prelude function name to its first-class runtime
    /// callable. Compiler-scoped intrinsics deliberately remain outside this
    /// catalog so they cannot become ambient through an engine fallback.
    pub fn free(name: &str) -> Option<Self> {
        Some(match name {
            "print" => Builtin::Print,
            "toInt" => Builtin::ToInt,
            "toIntRadix" => Builtin::ToIntRadix,
            "fromCodePoint" => Builtin::FromCodePoint,
            "toFloat" => Builtin::ToFloat,
            "input" => Builtin::Input,
            "map" => Builtin::MapFn,
            "filter" => Builtin::FilterFn,
            "reduce" => Builtin::ReduceFn,
            "open" => Builtin::Open,
            "assert" => Builtin::TestAssert,
            _ => return None,
        })
    }

    /// Resolve a prelude static namespace member to its first-class runtime
    /// callable. The interpreter and generated-Rust emitter share this catalog,
    /// so unchecked member-value evaluation cannot drift between `run` and
    /// `build`. Generic compiler-only members such as `JSON.parseAs` are not
    /// runtime callables and remain outside this catalog.
    pub fn static_namespace(namespace: &str, member: &str) -> Option<Self> {
        Some(match (namespace, member) {
            ("Array", "of") => Builtin::ArrayOf,
            ("Map", "new") => Builtin::MapNew,
            ("Map", "ofEntries") => Builtin::MapOfEntries,
            ("Set", "of") => Builtin::SetOf,
            ("JSON", "stringify") => Builtin::JsonStringify,
            ("JSON", "parse") => Builtin::JsonParse,
            ("Math", "sqrt") => Builtin::MathSqrt,
            ("Math", "abs") => Builtin::MathAbs,
            ("Math", "floor") => Builtin::MathFloor,
            ("Math", "ceil") => Builtin::MathCeil,
            ("Math", "round") => Builtin::MathRound,
            ("Math", "sin") => Builtin::MathSin,
            ("Math", "cos") => Builtin::MathCos,
            ("Math", "tan") => Builtin::MathTan,
            ("Math", "min") => Builtin::MathMin,
            ("Math", "max") => Builtin::MathMax,
            ("Math", "isNaN") => Builtin::MathIsNaN,
            ("Math", "isFinite") => Builtin::MathIsFinite,
            ("Math", "parseFloat") => Builtin::MathParseFloat,
            ("Bytes", "empty") => Builtin::BytesEmpty,
            ("Bytes", "encodeUtf8") => Builtin::BytesEncodeUtf8,
            ("Bytes", "fromArray") => Builtin::BytesFromArray,
            ("Bytes", "fromHex") => Builtin::BytesFromHex,
            ("Bytes", "fromBase64") => Builtin::BytesFromBase64,
            ("Bytes", "concat") => Builtin::BytesConcat,
            ("ByteBuffer", "allocate") => Builtin::ByteBufferAllocate,
            ("ByteBuffer", "fromBytes") => Builtin::ByteBufferFromBytes,
            ("Encoding", "utf8Encode") => Builtin::EncodingUtf8Encode,
            ("Encoding", "utf8Decode") => Builtin::EncodingUtf8Decode,
            ("Encoding", "hexEncode") => Builtin::EncodingHexEncode,
            ("Encoding", "hexDecode") => Builtin::EncodingHexDecode,
            ("Encoding", "base64Encode") => Builtin::EncodingBase64Encode,
            ("Encoding", "base64Decode") => Builtin::EncodingBase64Decode,
            ("Codec", "gzipCompress") => Builtin::CodecGzipCompress,
            ("Codec", "gzipDecompress") => Builtin::CodecGzipDecompress,
            ("Codec", "deflateCompress") => Builtin::CodecDeflateCompress,
            ("Codec", "deflateFixedCompress") => Builtin::CodecDeflateFixedCompress,
            ("Codec", "zlibFixedCompress") => Builtin::CodecZlibFixedCompress,
            ("Codec", "reedSolomon255223Protect") => Builtin::CodecReedSolomon255223Protect,
            ("Codec", "deflateDecompress") => Builtin::CodecDeflateDecompress,
            ("Codec", "zstdCompress") => Builtin::CodecZstdCompress,
            ("Codec", "zstdDecompress") => Builtin::CodecZstdDecompress,
            ("Hash", "sha256") => Builtin::HashSha256,
            ("Hash", "sha512") => Builtin::HashSha512,
            ("Hash", "hmacSha256") => Builtin::HashHmacSha256,
            ("Hash", "crc32") => Builtin::HashCrc32,
            ("FS", "readText") => Builtin::FsReadText,
            ("FS", "writeText") => Builtin::FsWriteText,
            ("FS", "readBytes") => Builtin::FsReadBytes,
            ("FS", "writeBytes") => Builtin::FsWriteBytes,
            ("FS", "list") => Builtin::FsList,
            ("Cli", "hasFlag") => Builtin::CliHasFlag,
            ("Cli", "option") => Builtin::CliOption,
            ("Cli", "options") => Builtin::CliOptions,
            ("Cli", "positionals") => Builtin::CliPositionals,
            ("Path", "from") => Builtin::PathFrom,
            ("Path", "cwdRelative") => Builtin::PathCwdRelative,
            ("Path", "project") => Builtin::PathProject,
            ("Regex", "compile") => Builtin::RegexCompile,
            ("CSV", "parse") => Builtin::CsvParse,
            ("CSV", "parseWithHeader") => Builtin::CsvParseWithHeader,
            ("CSV", "stringify") => Builtin::CsvStringify,
            ("CSV", "stringifyWithHeader") => Builtin::CsvStringifyWithHeader,
            ("TOML", "parse") => Builtin::TomlParse,
            ("TOML", "stringify") => Builtin::TomlStringify,
            ("TOML", "toJson") => Builtin::TomlToJson,
            ("TOML", "fromJson") => Builtin::TomlFromJson,
            ("URL", "parse") => Builtin::UrlParse,
            ("Date", "fromYmd") => Builtin::DateFromYmd,
            ("Date", "parseIso") => Builtin::DateParseIso,
            ("BigInt", "fromInt") => Builtin::BigIntFromInt,
            ("BigInt", "parse") => Builtin::BigIntParse,
            ("Decimal", "fromInt") => Builtin::DecimalFromInt,
            ("Decimal", "parse") => Builtin::DecimalParse,
            ("Test", "assert") => Builtin::TestAssert,
            ("Test", "assertEq") => Builtin::TestAssertEq,
            ("Test", "assertNe") => Builtin::TestAssertNe,
            ("Test", "assertContains") => Builtin::TestAssertContains,
            ("Test", "assertOk") => Builtin::TestAssertOk,
            ("Test", "assertErr") => Builtin::TestAssertErr,
            ("Test", "assertSome") => Builtin::TestAssertSome,
            ("Test", "assertNone") => Builtin::TestAssertNone,
            ("Test", "assertGolden") => Builtin::TestAssertGolden,
            _ => return None,
        })
    }

    /// The `(min, max)` argument-arity range of this builtin as a callable VALUE
    /// (`max = None` for variadic) — the SINGLE source of truth shared by the
    /// interpreter's `callable_arity` (§22 conformance) and the emitted runtime's
    /// `callable_shape_matches`, so a function-type shape test (`(int) -> int`)
    /// decides identically run≡build.
    pub fn arity_range(&self) -> (usize, Option<usize>) {
        match self {
            Builtin::LispexRule
            | Builtin::LispexValueFromCanonical
            | Builtin::LispexCanonicalBytes
            | Builtin::LispexDefaultLimits
            | Builtin::LispexInspectRule
            | Builtin::LispexConsumerArtifactFromBytes
            | Builtin::LispexConsumerArtifactBytes
            | Builtin::LispexPortableCoreBytes
            | Builtin::LispexInspectConsumerArtifact
            | Builtin::LispexVerifyConsumerArtifact => (1, Some(1)),
            Builtin::LispexEvaluate
            | Builtin::LispexEvaluateWithEvidence
            | Builtin::LispexFreshReplay => (3, Some(3)),
            Builtin::Print
            | Builtin::ToInt
            | Builtin::FromCodePoint
            | Builtin::ToFloat
            | Builtin::Open => (1, Some(1)),
            Builtin::ToIntRadix => (2, Some(2)),
            // §22 `input()` — zero-arg, free (a host pull).
            Builtin::Input => (0, Some(0)),
            // §18 `Test.assert(condition, message = "assertion failed")`.
            Builtin::TestAssert => (1, Some(2)),
            Builtin::TestAssertEq
            | Builtin::TestAssertNe
            | Builtin::TestAssertContains
            | Builtin::TestAssertGolden => (2, Some(2)),
            Builtin::TestAssertOk
            | Builtin::TestAssertErr
            | Builtin::TestAssertSome
            | Builtin::TestAssertNone => (1, Some(1)),
            // Receiver-bound, zero-argument.
            Builtin::Scalars
            | Builtin::StrTrim
            | Builtin::StrTrimStart
            | Builtin::StrTrimEnd
            | Builtin::StrByteLength
            | Builtin::ArrSorted
            // §6 (v5.4) array mutators — zero-arg in-place: `pop`/`clear`/`reverse`/`sort`.
            | Builtin::ArrPop
            | Builtin::ArrClear
            | Builtin::ArrReverse
            | Builtin::ArrSort => (0, Some(0)),
            // §22 string stdlib — one argument.
            Builtin::StrStartsWith
            | Builtin::StrEndsWith
            | Builtin::StrContains
            | Builtin::StrIndexOf
            | Builtin::StrLastIndexOf
            | Builtin::StrCodePointAt
            | Builtin::StrSplit => (1, Some(1)),
            Builtin::MapFn | Builtin::FilterFn => (2, Some(2)),
            Builtin::ReduceFn => (3, Some(3)),
            Builtin::ArrayOf | Builtin::SetOf => (0, None),
            Builtin::MapNew => (0, Some(0)),
            Builtin::MapOfEntries => (1, Some(1)),
            Builtin::JsonStringify | Builtin::JsonParse => (1, Some(1)),
            // §8 (v5.4) `Math` namespace — one float (or string) argument, except
            // `min`/`max` which take two.
            Builtin::MathSqrt
            | Builtin::MathAbs
            | Builtin::MathFloor
            | Builtin::MathCeil
            | Builtin::MathRound
            | Builtin::MathSin
            | Builtin::MathCos
            | Builtin::MathTan
            | Builtin::MathIsNaN
            | Builtin::MathIsFinite
            | Builtin::MathParseFloat => (1, Some(1)),
            Builtin::MathMin | Builtin::MathMax => (2, Some(2)),
            // §8 (v5.4) `Bytes` namespace. The 1-arg static codecs + the 0-arg
            // instance accessors; `concat` takes two `Bytes`, `slice` two `int`s.
            Builtin::BytesEncodeUtf8
            | Builtin::BytesFromArray
            | Builtin::BytesFromHex
            | Builtin::BytesFromBase64 => (1, Some(1)),
            Builtin::BytesConcat | Builtin::BytesSlice => (2, Some(2)),
            Builtin::BytesGet => (1, Some(1)),
            Builtin::BytesEmpty => (0, Some(0)),
            Builtin::BytesDecodeUtf8
            | Builtin::BytesToHex
            | Builtin::BytesToBase64
            | Builtin::BytesLength
            | Builtin::BytesIsEmpty
            | Builtin::BytesToArray => (0, Some(0)),
            Builtin::ByteBufferAllocate => (1, Some(2)),
            Builtin::ByteBufferFromBytes | Builtin::ByteBufferGet => (1, Some(1)),
            Builtin::ByteBufferSet => (2, Some(2)),
            Builtin::ByteBufferFill => (3, Some(3)),
            Builtin::ByteBufferCopy => (4, Some(4)),
            Builtin::ByteBufferLength | Builtin::ByteBufferToBytes => (0, Some(0)),
            Builtin::EncodingUtf8Encode
            | Builtin::EncodingUtf8Decode
            | Builtin::EncodingHexEncode
            | Builtin::EncodingHexDecode
            | Builtin::EncodingBase64Encode
            | Builtin::EncodingBase64Decode
            | Builtin::CodecGzipCompress
            | Builtin::CodecGzipDecompress
            | Builtin::CodecDeflateCompress
            | Builtin::CodecDeflateFixedCompress
            | Builtin::CodecZlibFixedCompress
            | Builtin::CodecReedSolomon255223Protect
            | Builtin::CodecDeflateDecompress
            | Builtin::CodecZstdDecompress => (1, Some(1)),
            Builtin::CodecZstdCompress => (1, Some(2)),
            // §15 (v5.4) `Hash` namespace — the digests take one `Bytes`; the MAC
            // takes a key + a message (two `Bytes`).
            Builtin::HashSha256 | Builtin::HashSha512 | Builtin::HashCrc32 => (1, Some(1)),
            Builtin::HashHmacSha256 => (2, Some(2)),
            Builtin::FsReadText | Builtin::FsReadBytes | Builtin::FsList => (1, Some(1)),
            Builtin::FsWriteText | Builtin::FsWriteBytes => (2, Some(2)),
            Builtin::CliHasFlag | Builtin::CliOption | Builtin::CliOptions => (2, Some(2)),
            Builtin::CliPositionals => (1, Some(1)),
            Builtin::PathFrom
            | Builtin::PathCwdRelative
            | Builtin::PathProject
            | Builtin::PathJoin
            | Builtin::PathWithExtension => (1, Some(1)),
            Builtin::PathParent
            | Builtin::PathFileName
            | Builtin::PathExtension
            | Builtin::PathNormalize
            | Builtin::PathToString => (0, Some(0)),
            Builtin::RegexCompile
            | Builtin::RegexIsMatch
            | Builtin::RegexFind
            | Builtin::RegexFindAll
            | Builtin::RegexSplit => (1, Some(1)),
            Builtin::RegexReplaceAll => (2, Some(2)),
            Builtin::CsvParse
            | Builtin::CsvParseWithHeader
            | Builtin::CsvStringify
            | Builtin::TomlParse
            | Builtin::TomlStringify
            | Builtin::TomlToJson
            | Builtin::TomlFromJson
            | Builtin::UrlParse => (1, Some(1)),
            Builtin::CsvStringifyWithHeader => (2, Some(2)),
            Builtin::UrlScheme
            | Builtin::UrlHost
            | Builtin::UrlPath
            | Builtin::UrlQuery
            | Builtin::UrlFragment
            | Builtin::UrlToString => (0, Some(0)),
            Builtin::DateFromYmd => (3, Some(3)),
            Builtin::DateParseIso => (1, Some(1)),
            Builtin::DateAddDays => (1, Some(1)),
            Builtin::DateToIso | Builtin::DateYear | Builtin::DateMonth | Builtin::DateDay => {
                (0, Some(0))
            }
            Builtin::BigIntFromInt => (1, Some(1)),
            Builtin::BigIntParse => (2, Some(2)),
            Builtin::BigIntToString | Builtin::BigIntDiv | Builtin::BigIntMod => (1, Some(1)),
            Builtin::BigIntToInt => (0, Some(0)),
            Builtin::DecimalFromInt | Builtin::DecimalParse => (1, Some(1)),
            Builtin::DecimalToString | Builtin::DecimalScale | Builtin::DecimalToInt => {
                (0, Some(0))
            }
            Builtin::DecimalRound => (1, Some(2)),
            Builtin::DecimalDiv => (2, Some(3)),
            // §22 zero-arg JSONValue accessors.
            Builtin::JsonKind
            | Builtin::JsonIsNull
            | Builtin::JsonAsString
            | Builtin::JsonAsBool
            | Builtin::JsonAsInt
            | Builtin::JsonNumberText
            | Builtin::JsonLength
            | Builtin::JsonAsArray
            | Builtin::JsonKeys
            | Builtin::JsonValues => (0, Some(0)),
            Builtin::ArrPush
            | Builtin::ArrGet
            | Builtin::ArrJoin
            | Builtin::ArrIndexOf
            | Builtin::ArrSortedBy
            // §6 (v5.4) one-arg array mutators: `removeAt(index)` + the callback
            // mutators `sortBy(f)` / `retain(f)`.
            | Builtin::ArrRemoveAt
            | Builtin::ArrSortBy
            | Builtin::ArrRetain
            | Builtin::MapGet
            | Builtin::MapRemove
            | Builtin::MapContainsKey
            | Builtin::SetAdd
            | Builtin::SetRemove
            | Builtin::SetContains
            | Builtin::JsonGet
            | Builtin::JsonAt => (1, Some(1)),
            // §22 `arr.slice(start, end)` / `str.slice(start, end)` — two fixed arguments.
            // §6 (v5.4) `arr.insert(index, value)` — two fixed arguments.
            Builtin::ArrSlice | Builtin::StrSlice | Builtin::StrReplace | Builtin::ArrInsert => {
                (2, Some(2))
            }
            // §22.2 the Option→Result bridge: one argument (the error / the callback).
            Builtin::OkOr
            | Builtin::OkOrElse
            | Builtin::OptionMap
            | Builtin::OptionFlatMap
            | Builtin::ResultMap
            | Builtin::ResultFlatMap => (1, Some(1)),
            Builtin::MapInsert | Builtin::MapGetOr => (2, Some(2)),
            Builtin::IntAtLeast | Builtin::IntAtMost => (1, Some(1)),
            Builtin::FileRead | Builtin::FileClose => (0, Some(0)),
            Builtin::FileWrite => (1, Some(1)),
            // §3 (v5.4) `id.value()` — zero-arg newtype unwrap.
            Builtin::NewtypeValue => (0, Some(0)),
            // §6 (v5.4) collections: zero-arg queries/mutators, one-arg set algebra +
            // value HOFs, three-arg `update`.
            Builtin::MapIsEmpty
            | Builtin::MapClear
            | Builtin::SetIsEmpty
            | Builtin::SetToArray
            | Builtin::SetClear => (0, Some(0)),
            Builtin::SetUnion
            | Builtin::SetIntersection
            | Builtin::SetDifference
            | Builtin::MapMapValues
            | Builtin::MapFilter => (1, Some(1)),
            Builtin::MapUpdate => (3, Some(3)),
        }
    }
}

/// §22 builtin parameter names (the checker's signature table is
/// the source of truth; mirrored here for runtime named calls).
pub fn builtin_param_names(kind: Builtin) -> &'static [&'static str] {
    match kind {
        Builtin::LispexRule => &["name"],
        Builtin::LispexValueFromCanonical => &["bytes"],
        Builtin::LispexCanonicalBytes => &["value"],
        Builtin::LispexDefaultLimits | Builtin::LispexInspectRule => &["rule"],
        Builtin::LispexEvaluate | Builtin::LispexEvaluateWithEvidence => {
            &["rule", "input", "limits"]
        }
        Builtin::LispexConsumerArtifactFromBytes => &["bytes"],
        Builtin::LispexConsumerArtifactBytes
        | Builtin::LispexPortableCoreBytes
        | Builtin::LispexInspectConsumerArtifact
        | Builtin::LispexVerifyConsumerArtifact => &["artifact"],
        Builtin::LispexFreshReplay => &["rule", "input", "artifact"],
        Builtin::Print => &["value"],
        Builtin::ToInt => &["text"],
        Builtin::ToIntRadix => &["text", "radix"],
        Builtin::FromCodePoint => &["n"],
        Builtin::ToFloat => &["n"],
        Builtin::MapFn | Builtin::FilterFn => &["xs", "f"],
        Builtin::ReduceFn => &["xs", "initial", "f"],
        Builtin::Open => &["path"],
        Builtin::ArrayOf | Builtin::SetOf | Builtin::MapNew | Builtin::Input => &[],
        Builtin::MapOfEntries => &["entries"],
        Builtin::TestAssert => &["condition", "message"],
        Builtin::TestAssertEq | Builtin::TestAssertNe => &["actual", "expected"],
        Builtin::TestAssertContains => &["text", "needle"],
        Builtin::TestAssertOk
        | Builtin::TestAssertErr
        | Builtin::TestAssertSome
        | Builtin::TestAssertNone => &["value"],
        Builtin::TestAssertGolden => &["path", "actual"],
        Builtin::JsonStringify => &["value"],
        Builtin::JsonParse => &["text"],
        // §8 (v5.4) `Math` namespace parameter names (lockstep with the schemes).
        Builtin::MathSqrt
        | Builtin::MathAbs
        | Builtin::MathFloor
        | Builtin::MathCeil
        | Builtin::MathRound
        | Builtin::MathSin
        | Builtin::MathCos
        | Builtin::MathTan
        | Builtin::MathIsNaN
        | Builtin::MathIsFinite => &["x"],
        Builtin::MathMin | Builtin::MathMax => &["a", "b"],
        Builtin::MathParseFloat => &["s"],
        // §8 (v5.4) `Bytes` namespace parameter names (lockstep with the schemes).
        Builtin::BytesEmpty => &[],
        Builtin::BytesEncodeUtf8 => &["s"],
        Builtin::BytesFromArray => &["values"],
        Builtin::BytesFromHex | Builtin::BytesFromBase64 => &["s"],
        Builtin::BytesConcat => &["a", "b"],
        Builtin::BytesGet => &["index"],
        Builtin::BytesSlice => &["start", "end"],
        // The zero-arg `Bytes` instance accessors.
        Builtin::BytesDecodeUtf8
        | Builtin::BytesToHex
        | Builtin::BytesToBase64
        | Builtin::BytesLength
        | Builtin::BytesIsEmpty
        | Builtin::BytesToArray => &[],
        Builtin::ByteBufferAllocate => &["length", "value"],
        Builtin::ByteBufferFromBytes => &["value"],
        Builtin::ByteBufferGet => &["index"],
        Builtin::ByteBufferSet => &["index", "value"],
        Builtin::ByteBufferFill => &["start", "length", "value"],
        Builtin::ByteBufferCopy => &["source", "sourceStart", "targetStart", "length"],
        Builtin::ByteBufferLength | Builtin::ByteBufferToBytes => &[],
        Builtin::EncodingUtf8Encode => &["text"],
        Builtin::EncodingUtf8Decode
        | Builtin::EncodingHexEncode
        | Builtin::EncodingBase64Encode => &["bytes"],
        Builtin::EncodingHexDecode | Builtin::EncodingBase64Decode => &["text"],
        Builtin::CodecGzipCompress
        | Builtin::CodecGzipDecompress
        | Builtin::CodecDeflateCompress
        | Builtin::CodecDeflateFixedCompress
        | Builtin::CodecZlibFixedCompress
        | Builtin::CodecReedSolomon255223Protect
        | Builtin::CodecDeflateDecompress
        | Builtin::CodecZstdDecompress => &["bytes"],
        Builtin::CodecZstdCompress => &["bytes", "level"],
        // §15 (v5.4) `Hash` namespace parameter names (lockstep with the schemes).
        Builtin::HashSha256 | Builtin::HashSha512 | Builtin::HashCrc32 => &["data"],
        Builtin::HashHmacSha256 => &["key", "message"],
        Builtin::FsReadText | Builtin::FsReadBytes | Builtin::FsList => &["path"],
        Builtin::FsWriteText => &["path", "text"],
        Builtin::FsWriteBytes => &["path", "bytes"],
        Builtin::CliHasFlag | Builtin::CliOption | Builtin::CliOptions => &["args", "name"],
        Builtin::CliPositionals => &["args"],
        Builtin::PathFrom | Builtin::PathCwdRelative | Builtin::PathProject => &["text"],
        Builtin::PathJoin => &["child"],
        Builtin::PathWithExtension => &["ext"],
        Builtin::PathParent
        | Builtin::PathFileName
        | Builtin::PathExtension
        | Builtin::PathNormalize
        | Builtin::PathToString => &[],
        Builtin::RegexCompile => &["pattern"],
        Builtin::RegexIsMatch | Builtin::RegexFind | Builtin::RegexFindAll | Builtin::RegexSplit => {
            &["text"]
        }
        Builtin::RegexReplaceAll => &["text", "replacement"],
        Builtin::CsvParse | Builtin::CsvParseWithHeader | Builtin::TomlParse | Builtin::UrlParse => {
            &["text"]
        }
        Builtin::CsvStringify => &["rows"],
        Builtin::CsvStringifyWithHeader => &["rows", "columns"],
        Builtin::TomlStringify | Builtin::TomlToJson | Builtin::TomlFromJson => &["value"],
        Builtin::UrlScheme
        | Builtin::UrlHost
        | Builtin::UrlPath
        | Builtin::UrlQuery
        | Builtin::UrlFragment
        | Builtin::UrlToString => &[],
        Builtin::DateFromYmd => &["year", "month", "day"],
        Builtin::DateParseIso => &["text"],
        Builtin::DateAddDays => &["days"],
        Builtin::DateToIso | Builtin::DateYear | Builtin::DateMonth | Builtin::DateDay => &[],
        Builtin::BigIntFromInt => &["n"],
        Builtin::BigIntParse => &["text", "radix"],
        Builtin::BigIntToString => &["radix"],
        Builtin::BigIntDiv | Builtin::BigIntMod => &["other"],
        Builtin::BigIntToInt => &[],
        Builtin::DecimalFromInt => &["n"],
        Builtin::DecimalParse => &["text"],
        Builtin::DecimalRound => &["scale", "mode"],
        Builtin::DecimalDiv => &["other", "scale", "mode"],
        Builtin::DecimalToString | Builtin::DecimalScale | Builtin::DecimalToInt => &[],
        Builtin::ArrPush => &["x"],
        // §6 (v5.4) array mutation API param names (lockstep with the schemes).
        Builtin::ArrInsert => &["index", "value"],
        Builtin::ArrRemoveAt => &["index"],
        Builtin::ArrSortBy | Builtin::ArrRetain => &["f"],
        Builtin::ArrGet => &["i"],
        Builtin::ArrSlice => &["start", "end"],
        Builtin::StrSlice => &["start", "end"],
        Builtin::StrReplace => &["old", "new"],
        Builtin::IntAtLeast => &["min"],
        Builtin::IntAtMost => &["max"],
        Builtin::ArrJoin => &["sep"],
        Builtin::ArrIndexOf => &["x"],
        Builtin::ArrSortedBy => &["f"],
        Builtin::MapInsert => &["k", "v"],
        Builtin::MapGetOr => &["k", "default"],
        Builtin::MapGet | Builtin::MapRemove | Builtin::MapContainsKey => &["k"],
        Builtin::SetAdd | Builtin::SetRemove | Builtin::SetContains => &["x"],
        // §6 (v5.4) collections
        Builtin::MapIsEmpty
        | Builtin::MapClear
        | Builtin::SetIsEmpty
        | Builtin::SetToArray
        | Builtin::SetClear => &[],
        Builtin::SetUnion | Builtin::SetIntersection | Builtin::SetDifference => &["other"],
        Builtin::MapMapValues | Builtin::MapFilter => &["f"],
        Builtin::MapUpdate => &["k", "initial", "f"],
        Builtin::OkOr => &["error"],
        Builtin::OkOrElse
        | Builtin::OptionMap
        | Builtin::OptionFlatMap
        | Builtin::ResultMap
        | Builtin::ResultFlatMap => &["f"],
        Builtin::StrStartsWith => &["prefix"],
        Builtin::StrEndsWith => &["suffix"],
        Builtin::StrContains | Builtin::StrIndexOf | Builtin::StrLastIndexOf => &["sub"],
        Builtin::StrCodePointAt => &["i"],
        Builtin::StrSplit => &["sep"],
        Builtin::Scalars
        | Builtin::StrTrim
        | Builtin::StrTrimStart
        | Builtin::StrTrimEnd
        | Builtin::StrByteLength
        | Builtin::ArrSorted
        // §6 (v5.4) zero-arg array mutators.
        | Builtin::ArrPop
        | Builtin::ArrClear
        | Builtin::ArrReverse
        | Builtin::ArrSort
        | Builtin::FileRead
        | Builtin::FileClose
        | Builtin::JsonKind
        | Builtin::JsonIsNull
        | Builtin::JsonAsString
        | Builtin::JsonAsBool
        | Builtin::JsonAsInt
        | Builtin::JsonNumberText
        | Builtin::JsonLength
        | Builtin::JsonAsArray
        | Builtin::JsonKeys
        | Builtin::JsonValues
        | Builtin::NewtypeValue => &[],
        Builtin::FileWrite => &["s"],
        Builtin::JsonGet => &["key"],
        Builtin::JsonAt => &["index"],
    }
}

pub fn rounding_mode_value(mode: RoundingMode) -> Value {
    Value::Enum {
        enum_id: Rc::from("RoundingMode"),
        declaration_identity: None,
        method_identity: None,
        variant: Rc::from(mode.name()),
        variant_index: mode.index(),
        payloads: Rc::from([] as [Value; 0]),
    }
}

pub fn rounding_mode_variant(name: &str) -> Option<RoundingMode> {
    RoundingMode::from_name(name)
}

pub fn builtin_default_arg(kind: Builtin, index: usize) -> Option<Value> {
    match (kind, index) {
        (Builtin::TestAssert, 1) => Some(Value::str("assertion failed")),
        (Builtin::ByteBufferAllocate, 1) => Some(Value::Int(0)),
        (Builtin::CodecZstdCompress, 1) => Some(Value::Int(3)),
        (Builtin::DecimalRound, 1) | (Builtin::DecimalDiv, 2) => {
            Some(rounding_mode_value(RoundingMode::HalfEven))
        }
        _ => None,
    }
}

/// Fill named arguments into an existing positional slot vector. Callers retain
/// ownership of arity, variadic tails, defaults, and missing-argument handling;
/// this shared boundary owns parameter-name lookup and duplicate/unknown faults.
pub fn bind_named_arg_slots<'a, N, F>(
    mut slots: Vec<Option<Value>>,
    parameter_count: usize,
    parameter_name: F,
    named: Vec<(N, Value)>,
    span: Span,
) -> Result<Vec<Option<Value>>, RtError>
where
    N: AsRef<str>,
    F: Fn(usize) -> Option<&'a str>,
{
    if slots.len() < parameter_count {
        slots.resize_with(parameter_count, || None);
    }
    for (name, value) in named {
        let name = name.as_ref();
        match (0..parameter_count).find(|&index| parameter_name(index) == Some(name)) {
            Some(index) => {
                if slots[index].is_some() {
                    return Err(fault(
                        codes::GUARD_ARITY,
                        format!("parameter `{name}` is given twice (§5)"),
                        span,
                    ));
                }
                slots[index] = Some(value);
            }
            None => {
                return Err(fault(
                    codes::GUARD_ARITY,
                    format!("no parameter named `{name}` (§5)"),
                    span,
                ));
            }
        }
    }
    Ok(slots)
}

/// Bind positional and named arguments to a builtin value's effective parameter
/// slots. Receiver-bound callback HOFs omit their leading free-function receiver
/// parameter; every other builtin uses its full runtime parameter catalog.
pub fn bind_builtin_named_args<N: AsRef<str>>(
    kind: Builtin,
    receiver_bound: bool,
    positional: Vec<Value>,
    named: Vec<(N, Value)>,
    span: Span,
) -> Result<Vec<Value>, RtError> {
    let all_names = builtin_param_names(kind);
    let parameter_offset = usize::from(
        receiver_bound && matches!(kind, Builtin::MapFn | Builtin::FilterFn | Builtin::ReduceFn),
    );
    let names = &all_names[parameter_offset..];
    let slots = bind_named_arg_slots(
        positional.into_iter().map(Some).collect(),
        names.len(),
        |index| names.get(index).copied(),
        named,
        span,
    )?;
    let mut args = Vec::with_capacity(slots.len());
    for (index, slot) in slots.into_iter().enumerate() {
        match slot {
            Some(value) => args.push(value),
            None => match builtin_default_arg(kind, parameter_offset + index) {
                Some(default) => args.push(default),
                None => {
                    return Err(fault(
                        codes::GUARD_ARITY,
                        format!(
                            "missing argument for parameter `{}` (§5)",
                            names.get(index).copied().unwrap_or("?")
                        ),
                        span,
                    ));
                }
            },
        }
    }
    Ok(args)
}

/// §22 `toInt(s)` — the SHARED builtin both engines call. String-only and
/// PURE (no host): the trimmed text parses to an `int`, yielding
/// `Some(int)` on success and `None` on a parse failure; a non-string
/// faults. The parse uses the SAME `i64::from_str` both engines rely on,
/// so the success/failure boundary cannot drift.
pub fn builtin_to_int(arg: Value, span: Span) -> Result<Value, RtError> {
    match arg {
        Value::Str(s) => match s.trim().parse::<i64>() {
            Ok(v) => Ok(Value::Some(Rc::new(Value::Int(v)))),
            Err(_) => Ok(Value::None),
        },
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`toInt` takes a `string`, found `{}`", other.kind()),
            span,
        )),
    }
}

/// §22 `toInt(text, radix)` — the 2-arg form. Parses `text` in base `radix` (2..=36) via
/// `i64::from_str_radix`; an out-of-range radix yields `None` (NOT a panic — from_str_radix
/// panics outside 2..=36, so guard first). Non-string text or non-int radix faults GUARD_TYPE.
/// SHARED leaf both engines call. The 1-arg `toInt(text)` keeps its own `builtin_to_int`
/// (`parse::<i64>`) for byte-identical behavior.
pub fn builtin_to_int_radix(text: Value, radix: Value, span: Span) -> Result<Value, RtError> {
    let r = match radix {
        Value::Int(r) => r,
        other => {
            return Err(fault(
                codes::GUARD_TYPE,
                format!(
                    "`toIntRadix` radix must be an `int`, found `{}`",
                    other.kind()
                ),
                span,
            ));
        }
    };
    let s = match text {
        Value::Str(s) => s,
        other => {
            return Err(fault(
                codes::GUARD_TYPE,
                format!("`toIntRadix` takes a `string`, found `{}`", other.kind()),
                span,
            ));
        }
    };
    if !(2..=36).contains(&r) {
        return Ok(Value::None);
    }
    match i64::from_str_radix(s.trim(), r as u32) {
        Ok(v) => Ok(Value::Some(Rc::new(Value::Int(v)))),
        Err(_) => Ok(Value::None),
    }
}

/// §22 `fromCodePoint(n) -> Option<string>` — the SHARED leaf both engines call: the single-scalar
/// string for Unicode scalar value `n`, or `None` when `n` is not a valid scalar (negative, above
/// U+10FFFF, or a surrogate U+D800..U+DFFF — `char::from_u32` rejects those). The inverse of
/// `str.codePointAt`; total (no fault path beyond a non-int argument), so it composes with `??`.
pub fn builtin_from_code_point(arg: Value, span: Span) -> Result<Value, RtError> {
    match arg {
        Value::Int(n) => match u32::try_from(n).ok().and_then(char::from_u32) {
            Some(c) => Ok(Value::Some(Rc::new(Value::str(c.to_string())))),
            None => Ok(Value::None),
        },
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`fromCodePoint` takes an `int`, found `{}`", other.kind()),
            span,
        )),
    }
}

/// §22 `toFloat(n)` — the SHARED leaf both engines call: explicit int->float.
/// The spec keeps numeric domains separate (no implicit widening), so this is
/// the one sanctioned int->float coercion.
pub fn builtin_to_float(arg: Value, span: Span) -> Result<Value, RtError> {
    match arg {
        Value::Int(n) => Ok(Value::Float(n as f64)),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`toFloat` takes an `int`, found `{}`", other.kind()),
            span,
        )),
    }
}

/// §22 `input()` — the SHARED leaf both engines call: the host's per-run text
/// payload as a `Value::Str`. PURE-ish (a host pull, no fault path): a host with
/// no input returns `""`, and the result is deterministic within a run, so the
/// interpreter and the emitted binary observe the identical string.
pub fn builtin_input(host: &dyn Host) -> Value {
    Value::str(host.input())
}
