//! Versioned public spelling and compiler-owned standard boundary bindings.
//! Backing identities are unchanged; these bindings are admitted only in the
//! resolver's generated standard module, never by a user-chosen file name.

pub fn removed_free(name: &str) -> bool {
    matches!(
        name,
        "map" | "filter" | "reduce" | "print" | "input" | "open" | "assert"
    )
}

pub fn removed_static(namespace: &str, member: &str) -> bool {
    matches!(
        namespace,
        "Math" | "JSON" | "Encoding" | "Hash" | "Codec" | "Cli" | "FS" | "Test" | "CSV" | "TOML"
    ) || matches!(
        (namespace, member),
        ("Array" | "Set", "of")
            | ("Map", "new")
            | ("Bytes", "encodeUtf8" | "concat")
            | ("BigInt" | "Decimal", "fromInt")
    )
}

pub struct StandardIntrinsic {
    pub name: &'static str,
    pub namespace: &'static str,
    pub member: &'static str,
    pub runtime: &'static str,
}

pub const BOUNDARY_MODULES: &[&str] = &[
    "std.fs", "std.io", "std.json", "std.csv", "std.toml", "std.test",
];

macro_rules! intrinsic {
    ($name:literal, $namespace:literal, $member:literal, $runtime:literal) => {
        StandardIntrinsic {
            name: $name,
            namespace: $namespace,
            member: $member,
            runtime: $runtime,
        }
    };
}

pub fn boundary_intrinsics(module: &str) -> &'static [StandardIntrinsic] {
    match module {
        "std.fs" => &[
            intrinsic!("__stdReadText", "FS", "readText", "FsReadText"),
            intrinsic!("__stdWriteText", "FS", "writeText", "FsWriteText"),
            intrinsic!("__stdReadBytes", "FS", "readBytes", "FsReadBytes"),
            intrinsic!("__stdWriteBytes", "FS", "writeBytes", "FsWriteBytes"),
            intrinsic!("__stdList", "FS", "list", "FsList"),
        ],
        "std.io" => &[
            intrinsic!("__stdWriteLine", "", "print", "Print"),
            intrinsic!("__stdReadStdin", "", "input", "Input"),
            intrinsic!("__stdOpen", "", "open", "Open"),
        ],
        "std.json" => &[
            intrinsic!("__stdJsonParse", "JSON", "parse", "JsonParse"),
            intrinsic!("__stdJsonStringify", "JSON", "stringify", "JsonStringify"),
            intrinsic!("__stdJsonParseAs", "JSON", "parseAs", "JsonParseAs"),
        ],
        "std.csv" => &[
            intrinsic!("__stdCsvParse", "CSV", "parse", "CsvParse"),
            intrinsic!(
                "__stdCsvParseWithHeader",
                "CSV",
                "parseWithHeader",
                "CsvParseWithHeader"
            ),
            intrinsic!("__stdCsvStringify", "CSV", "stringify", "CsvStringify"),
            intrinsic!(
                "__stdCsvStringifyWithHeader",
                "CSV",
                "stringifyWithHeader",
                "CsvStringifyWithHeader"
            ),
        ],
        "std.toml" => &[
            intrinsic!("__stdTomlParse", "TOML", "parse", "TomlParse"),
            intrinsic!("__stdTomlStringify", "TOML", "stringify", "TomlStringify"),
            intrinsic!("__stdTomlFromJson", "TOML", "fromJson", "TomlFromJson"),
        ],
        "std.test" => &[
            intrinsic!("__stdAssert", "Test", "assert", "TestAssert"),
            intrinsic!("__stdAssertEq", "Test", "assertEq", "TestAssertEq"),
            intrinsic!("__stdAssertNe", "Test", "assertNe", "TestAssertNe"),
            intrinsic!(
                "__stdAssertContains",
                "Test",
                "assertContains",
                "TestAssertContains"
            ),
            intrinsic!("__stdAssertOk", "Test", "assertOk", "TestAssertOk"),
            intrinsic!("__stdAssertErr", "Test", "assertErr", "TestAssertErr"),
            intrinsic!("__stdAssertSome", "Test", "assertSome", "TestAssertSome"),
            intrinsic!("__stdAssertNone", "Test", "assertNone", "TestAssertNone"),
            intrinsic!(
                "__stdAssertGolden",
                "Test",
                "assertGolden",
                "TestAssertGolden"
            ),
        ],
        _ => &[],
    }
}

/// Migration advice for removed public spellings; callers apply the edition and binding checks.
pub fn removed_static_hint(namespace: &str, member: &str) -> Option<&'static str> {
    match (namespace, member) {
        ("Array", "of") => Some("use [a, b]"),
        ("Set", "of") => Some("use set { a, b }"),
        ("Map", "new") => Some("use map {}"),
        ("Bytes", "encodeUtf8") => Some("use s.encodeUtf8()"),
        ("Bytes", "concat") => Some("use a.concat(b)"),
        ("BigInt", "fromInt") => Some("use n.toBigInt()"),
        ("Decimal", "fromInt") => Some("use n.toDecimal()"),
        ("Math", "sqrt") => Some("use x.sqrt()"),
        ("Math", "abs") => Some("use x.abs()"),
        ("Math", "floor") => Some("use x.floor() for int, or x.floor().toFloat() for float"),
        ("Math", "ceil") => Some("use x.ceil() for int, or x.ceil().toFloat() for float"),
        ("Math", "round") => Some("use x.round() for int, or x.round().toFloat() for float"),
        ("Math", "sin") => Some("use x.sin()"),
        ("Math", "cos") => Some("use x.cos()"),
        ("Math", "tan") => Some("use x.tan()"),
        ("Math", "isNaN") => Some("use x.isNaN()"),
        ("Math", "isFinite") => Some("use x.isFinite()"),
        ("Math", "parseFloat") => Some("use s.parseFloat()"),
        ("Math", "min") => Some("use a.atMost(b)"),
        ("Math", "max") => Some("use a.atLeast(b)"),
        ("JSON", "parse") => Some("import std.json and use json.parse(s)"),
        ("JSON", "stringify") => Some("import std.json and use json.stringify(v)"),
        ("JSON", "parseAs") => Some("import std.json and use json.parseAs<T>(s)"),
        ("JSON", "decode") => Some("use v.decode<T>()"),
        ("TOML", "toJson") => Some("use t.toJson()"),
        ("Encoding", "utf8Encode") => Some("use s.encodeUtf8()"),
        ("Encoding", "utf8Decode") => Some("use b.decodeUtf8()"),
        ("Encoding", "hexEncode") => Some("use b.toHex()"),
        ("Encoding", "hexDecode") => Some("use Bytes.fromHex(s)"),
        ("Encoding", "base64Encode") => Some("use b.toBase64()"),
        ("Encoding", "base64Decode") => Some("use Bytes.fromBase64(s)"),
        ("Hash", "sha256") => Some("use b.sha256()"),
        ("Hash", "sha512") => Some("use b.sha512()"),
        ("Hash", "hmacSha256") => Some("use key.hmacSha256(data)"),
        ("Hash", "crc32") => Some("use b.crc32()"),
        ("Cli", "hasFlag") => Some("use args.hasFlag(name)"),
        ("Cli", "option") => Some("use args.option(name)"),
        ("Cli", "options") => Some("use args.options(name)"),
        ("Cli", "positionals") => Some("use args.positionals()"),
        ("Codec", "gzipCompress") => Some("use b.gzipCompress()"),
        ("Codec", "gzipDecompress") => Some("use b.gzipDecompress()"),
        ("Codec", "deflateCompress") => Some("use b.deflateCompress()"),
        ("Codec", "deflateFixedCompress") => Some("use b.deflateFixedCompress()"),
        ("Codec", "zlibFixedCompress") => Some("use b.zlibFixedCompress()"),
        ("Codec", "reedSolomon255223Protect") => Some("use b.reedSolomon255223Protect()"),
        ("Codec", "deflateDecompress") => Some("use b.deflateDecompress()"),
        ("Codec", "zstdDecompress") => Some("use b.zstdDecompress()"),
        ("Codec", "zstdCompress") => Some("use b.zstdCompress(level)"),
        ("FS", "readText") => Some("import std.fs and use fs.readText(p)"),
        ("FS", "writeText") => Some("import std.fs and use fs.writeText(p, text)"),
        ("FS", "readBytes") => Some("import std.fs and use fs.readBytes(p)"),
        ("FS", "writeBytes") => Some("import std.fs and use fs.writeBytes(p, bytes)"),
        ("FS", "list") => Some("import std.fs and use fs.list(p)"),
        ("CSV", "parse") => Some("import std.csv and use csv.parse(s)"),
        ("CSV", "parseWithHeader") => Some("import std.csv and use csv.parseWithHeader(s)"),
        ("CSV", "stringify") => Some("import std.csv and use csv.stringify(rows)"),
        ("CSV", "stringifyWithHeader") => {
            Some("import std.csv and use csv.stringifyWithHeader(rows, columns)")
        }
        ("TOML", "parse") => Some("import std.toml and use toml.parse(s)"),
        ("TOML", "stringify") => Some("import std.toml and use toml.stringify(v)"),
        ("TOML", "fromJson") => Some("import std.toml and use toml.fromJson(v)"),
        ("Test", "assert") => Some("import std.test and use test.assert(condition)"),
        ("Test", "assertEq") => Some("import std.test and use test.assertEq(a, b)"),
        ("Test", "assertNe") => Some("import std.test and use test.assertNe(a, b)"),
        ("Test", "assertContains") => {
            Some("import std.test and use test.assertContains(text, part)")
        }
        ("Test", "assertOk") => Some("import std.test and use test.assertOk(result)"),
        ("Test", "assertErr") => Some("import std.test and use test.assertErr(result)"),
        ("Test", "assertSome") => Some("import std.test and use test.assertSome(option)"),
        ("Test", "assertNone") => Some("import std.test and use test.assertNone(option)"),
        ("Test", "assertGolden") => Some("import std.test and use test.assertGolden(name, actual)"),
        _ => None,
    }
}

pub fn removed_module_hint(module: &str) -> &'static str {
    match module {
        "std.math" => {
            "; use float receiver methods such as x.sqrt(), a.atMost(b), a.atLeast(b), and s.parseFloat() in 5.21"
        }
        "std.encoding" => {
            "; use s.encodeUtf8(), b.decodeUtf8(), b.toHex(), Bytes.fromHex(s), b.toBase64(), and Bytes.fromBase64(s) in 5.21"
        }
        "std.hash" => "; use b.sha256(), b.sha512(), key.hmacSha256(data), and b.crc32() in 5.21",
        "std.codec" => {
            "; use Bytes receiver methods such as b.gzipCompress() and b.zstdCompress(level) in 5.21"
        }
        "std.cli" => {
            "; use args.hasFlag(name), args.option(name), args.options(name), and args.positionals() in 5.21"
        }
        "std.gen" => "; use [for x in lo..hi => x] and [false, true] in 5.21",
        _ => "",
    }
}
