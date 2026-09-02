//! Generates the 3-COLUMN native differential harness (CDR-006 section 7, v5.4 native
//! emit). For each NATIVE-eligible fixture it resolves + type-checks the source,
//! converts the checker's `CheckedUnit` to the native backend's typed-HIR input,
//! and emits BOTH the boxed program (`emit_module`) and the native-checked
//! program (`emit_native_items`), each in its own `mod`. The crate `include!`s
//! the generated file, so all three columns (the interpreter runs the source;
//! the two emitted programs run in-process) compile against the SAME runtime
//! types: a green build is the compile-shape proof, and the test then runs each
//! program and asserts the interpreter, the boxed emit, and the native emit are
//! BYTE-IDENTICAL.
//!
//! A separate REFUSAL set pins that the native backend DECLINES the shapes it
//! cannot lower with a structured `TPZ6002` (never a leaked `rustc` error): the
//! build script asserts `emit_native_items` returns a `NativeDeclined` error for
//! each, so a future native lowering that silently mis-handles one fails here.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::Path;

use topaz_check::{UnitModule, check_unit_typed};
use topaz_emit::NativeInput;
use topaz_resolve::{FileProvider, InMemoryProvider, ResolveOutput, resolve};

/// The NATIVE-ELIGIBLE set: single-module scalar programs the native backend
/// MUST lower (and which run == build byte-identically across all three columns).
/// Each MUST emit on BOTH backends: a panic here means the native slice
/// regressed against this curated set (a real gate failure, not a silent skip).
const FIXTURES: &[(&str, &str)] = &[
    // --- bare scalar literals / arithmetic ---
    ("lit_int", "42"),
    ("lit_float", "3.5"),
    ("lit_bool", "true"),
    ("int_add", "let a = 2\nlet b = 3\na + b"),
    ("int_chain", "1 + 2 * 3 - 4"),
    ("int_pow", "2 ** 10"),
    ("float_arith", "1.5 + 2.0 * 3.0"),
    ("math_floor_native", "Math.floor(3.7)"),
    (
        "math_total_unary_native",
        "Math.abs(-0.0) + Math.ceil(3.1) + Math.round(-2.5) + Math.floor(3.7)",
    ),
    (
        "math_trig_native",
        "Math.sin(0.0) + Math.cos(0.0) + Math.tan(0.0)",
    ),
    (
        "math_predicates_native",
        "Math.isFinite(42.0) && Math.isNaN(0.0 / 0.0)",
    ),
    (
        "math_min_max_native",
        "Math.min(0.0 / 0.0, 5.0) + Math.max(-2.0, 4.0)",
    ),
    ("math_named_unary_native", "Math.floor(x: 3.7)"),
    ("math_named_binary_native", "Math.min(a: 3.0, b: 1.0)"),
    ("math_mixed_binary_native", "Math.max(2.0, b: 5.0)"),
    ("std_math_floor_native", "import std.math\nmath.floor(3.7)"),
    (
        "std_math_alias_native",
        "import std.math as m\nm.max(1.0, 2.0)",
    ),
    (
        "std_math_named_native",
        "import std.math as m\nm.max(a: 1.0, b: 2.0)",
    ),
    ("math_sqrt_boundary", "Math.sqrt(9.0)"),
    ("math_sqrt_error_boundary", "Math.sqrt(-1.0)"),
    (
        "math_parse_float_named_boundary",
        "Math.parseFloat(s: \"  2.5  \")",
    ),
    ("to_int_named_boundary", "toInt(text: \"42\")"),
    (
        "to_int_radix_mixed_boundary",
        "toIntRadix(\"ff\", radix: 16)",
    ),
    ("from_code_point_named_boundary", "fromCodePoint(n: 65)"),
    ("to_float_boundary", "toFloat(42)"),
    ("int_div_floor", "17 / 5"),
    ("int_rem", "17 % 5"),
    ("int_neg", "let n = 5\n-n"),
    // A TYPED `let x: int` binding: the typed HIR records the local at the
    // NAME's span (`x`), not the `x: int` pattern span; the native cross-check
    // keys on the name span, so this confirms the soundness anchor's keying.
    ("typed_let", "let x: int = 40\nlet y: int = 2\nx + y"),
    (
        "bool_logic",
        "let a = true\nlet b = false\na && (b || true)",
    ),
    ("bool_not", "!false"),
    ("int_cmp", "let n = 7\nn > 3 && n <= 7"),
    ("float_cmp", "1.5 < 2.5"),
    ("int_eq", "let a = 4\nlet b = 4\na == b"),
    ("float_eq", "1.0 == 1.0"),
    ("bool_eq", "true == true"),
    ("string_value_native", "\"hello\""),
    ("string_concat_native", "\"a\" + \"b\""),
    (
        "string_compare_native",
        "\"a\" < \"b\" && \"topaz\" == \"topaz\"",
    ),
    ("string_scalars_method_boundary", "\"éA\".scalars()"),
    (
        "string_prefix_method_boundary",
        "\"topaz\".startsWith(prefix: \"top\")",
    ),
    ("string_trim_method_boundary", "\"  topaz\\n\".trim()"),
    (
        "string_split_join_method_boundary",
        "\"a,b,c\".split(sep: \",\").join(\"/\")",
    ),
    (
        "string_replace_named_method_boundary",
        "\"topaz\".replace(old: \"az\", new: \"ology\")",
    ),
    (
        "string_codepoint_named_method_boundary",
        "\"é\".codePointAt(i: 0)",
    ),
    ("string_local_concat_native", "let s = \"topaz\"\ns + \"!\""),
    (
        "fn_string_concat_native",
        "function shout(s: string) -> string { s + \"!\" }\nshout(\"hi\")",
    ),
    (
        "generic_fn_int_native",
        "function id<T>(x: T) -> T { x }\nid(5)",
    ),
    (
        "generic_fn_string_native",
        "function id<T>(x: T) -> T { x }\nid(\"hi\")",
    ),
    (
        "generic_array_first_native",
        "function first<T>(xs: Array<T>) -> T { xs[0] }\nfirst([4, 8, 15])",
    ),
    (
        "generic_fn_named_native",
        "function pick<T>(first: T, second: T) -> T { second }\npick(second: 9, first: 1)",
    ),
    ("print_effect_native", "print(\"hi\")"),
    // --- boxed entry-boundary values ---
    // These values stay out of native locals/functions. Native lowers them only
    // as the final program result, through the same `Value` constructors and
    // shared Bytes leaves as boxed emit.
    ("array_value", "[1, 2, 3]"),
    ("array_get_method_boundary", "[10, 20].get(i: 1)"),
    ("array_of_join_boundary", "Array.of(1, 2, 3).join(\"-\")"),
    ("array_length_property_boundary", "Array.of(1, 2, 3).length"),
    ("record_value", "{ x: 1, y: 2 }"),
    ("option_value", "Some(1)"),
    ("option_okor_some_boundary", "Some(7).okOr(\"e\")"),
    (
        "option_okor_none_named_boundary",
        "let n: Option<int> = None\nn.okOr(error: \"e\")",
    ),
    (
        "optional_member_some_boundary",
        "let r: Option<{ n: int }> = Some({ n: 7 })\nr?.n",
    ),
    (
        "optional_member_none_boundary",
        "let r: Option<{ n: int }> = None\nr?.n",
    ),
    (
        "optional_member_nested_boundary",
        "let r: Option<{ inner: Option<{ n: int }> }> = Some({ inner: Some({ n: 5 }) })\nr?.inner?.n",
    ),
    (
        "optional_member_array_length_boundary",
        "let xs: Option<Array<int>> = Some(Array.of(1, 2, 3))\nxs?.length",
    ),
    (
        "optional_call_some_get_boundary",
        "let xs: Option<Array<int>> = Some(Array.of(10, 20))\nxs?.get(1)",
    ),
    (
        "optional_call_none_lazy_boundary",
        "function bad() -> int { 1 / 0 }\nlet xs: Option<Array<int>> = None\nxs?.get(bad())",
    ),
    (
        "optional_call_string_scalars_boundary",
        "let s: Option<string> = Some(\"abc\")\ns?.scalars()",
    ),
    (
        "optional_call_set_contains_boundary",
        "let s: Option<Set<int>> = Some(Set.of(1, 2))\ns?.contains(2)",
    ),
    ("coalesce_some_boundary", "Some(7) ?? 0"),
    (
        "coalesce_none_boundary",
        "let n: Option<int> = None\nn ?? 7",
    ),
    (
        "coalesce_some_lazy_boundary",
        "function bad() -> int { 1 / 0 }\nSome(9) ?? bad()",
    ),
    (
        "coalesce_array_fallback_boundary",
        "let xs: Option<Array<int>> = None\n(xs ?? []).length",
    ),
    (
        "coalesce_optional_member_boundary",
        "let user: Option<{ profile: Option<{ city: string }> }> = Some({ profile: None })\nuser?.profile?.city ?? \"Unknown\"",
    ),
    (
        "logical_and_true_boundary",
        "Set.of(1, 2).contains(2) && Set.of(3).contains(3)",
    ),
    (
        "logical_and_false_lazy_boundary",
        "function bad() -> bool { (1 / 0) == 0 }\nSet.of(1, 2).contains(3) && bad()",
    ),
    (
        "logical_or_false_boundary",
        "Set.of(1, 2).contains(3) || Set.of(3).contains(3)",
    ),
    (
        "logical_or_true_lazy_boundary",
        "function bad() -> bool { (1 / 0) == 0 }\nSet.of(1, 2).contains(2) || bad()",
    ),
    (
        "if_boxed_array_then_boundary",
        "if Set.of(1).contains(1) { Array.of(1, 2) } else { [] }",
    ),
    (
        "if_boxed_false_branch_boundary",
        "if Set.of(1).contains(2) { \"yes\" } else { \"no\" }",
    ),
    (
        "if_boxed_else_if_boundary",
        "if Set.of(1).contains(2) { \"a\" } else if Set.of(2).contains(2) { \"b\" } else { \"c\" }",
    ),
    (
        "if_boxed_then_lazy_boundary",
        "function bad() -> int { 1 / 0 }\nif Set.of(1).contains(2) { bad() } else { 7 }",
    ),
    (
        "boxed_block_tail_boundary",
        "{ Map.ofEntries([{ key: \"a\", value: 1 }]).getOr(\"a\", 0) }",
    ),
    (
        "record_update_member_boundary",
        "let r = { x: 1, y: 2 }\n(r { x: 9 }).x",
    ),
    (
        "record_update_preserves_field_boundary",
        "let r = { xs: Array.of(1, 2), label: \"a\" }\n(r { label: \"b\" }).xs.length",
    ),
    (
        "record_update_nested_boundary",
        "let r = { user: { name: \"Ada\", age: 1 }, ok: true }\n(r { user: { name: \"Grace\", age: 2 } }).user.name",
    ),
    (
        "record_update_map_field_boundary",
        "let r = { m: map { \"a\": 1 } }\n(r { m: map { \"b\": 2 } }).m.getOr(\"b\", 0)",
    ),
    (
        "interpolated_record_member_boundary",
        "let r = { name: \"Ada\" }\n\"hi {r.name}\"",
    ),
    (
        "interpolated_collection_boundary",
        "\"xs={Array.of(1, 2).length}, set={Set.of(1, 1).length}\"",
    ),
    (
        "interpolated_coalesce_boundary",
        "let city: Option<string> = None\n\"city={city ?? \"Unknown\"}\"",
    ),
    (
        "tagged_template_tag_boundary",
        "(sql\"SELECT {\"name\"}\").tag",
    ),
    (
        "tagged_template_parts_boundary",
        "(sql\"SELECT {\"name\"} FROM users\").parts.length",
    ),
    (
        "array_spread_join_boundary",
        "let xs = Array.of(1, 2)\n[0, ...xs, 3].join(\"-\")",
    ),
    (
        "array_spread_string_boundary",
        "let xs = Array.of(\"b\", \"c\")\n[\"a\", ...xs].join(\"\")",
    ),
    (
        "array_spread_constructor_boundary",
        "[...Array.of(1, 2, 3)].length",
    ),
    (
        "array_of_spread_boundary",
        "let xs = [1, 2]\nArray.of(0, ...xs, 3).join(\"-\")",
    ),
    (
        "set_of_spread_boundary",
        "let xs = [1, 2, 2]\nSet.of(...xs, 3).toArray().join(\"-\")",
    ),
    ("null_literal_boundary", "null"),
    ("record_null_member_boundary", "{ x: null }.x"),
    (
        "json_stringify_record_null_boundary",
        "JSON.stringify({ x: null })",
    ),
    ("json_stringify_null_boundary", "JSON.stringify(null)"),
    ("bytes_static_call", "Bytes.encodeUtf8(\"foobar\").toHex()"),
    (
        "bytes_get_named_method_boundary",
        "Bytes.encodeUtf8(\"AZ\").get(index: 1)",
    ),
    (
        "map_get_or_named_boundary",
        "map { \"name\": \"Ada\" }.getOr(k: \"name\", default: \"none\")",
    ),
    ("set_to_array_boundary", "set { 2, 1, 2 }.toArray()"),
    (
        "set_of_to_array_boundary",
        "Set.of(\"b\", \"a\", \"b\").toArray()",
    ),
    (
        "set_union_to_array_boundary",
        "set { 1, 2 }.union(set { 2, 3 }).toArray()",
    ),
    (
        "map_new_is_empty_boundary",
        "Map.new<string, int>().isEmpty()",
    ),
    (
        "map_of_entries_get_boundary",
        "Map.ofEntries([{ key: \"a\", value: 7 }]).getOr(\"a\", 0)",
    ),
    (
        "map_keys_property_boundary",
        "Map.ofEntries([{ key: \"a\", value: 1 }, { key: \"b\", value: 2 }]).keys",
    ),
    (
        "map_values_property_boundary",
        "Map.ofEntries([{ key: \"a\", value: 1 }, { key: \"b\", value: 2 }]).values",
    ),
    (
        "map_entries_property_boundary",
        "Map.ofEntries([{ key: \"a\", value: 1 }, { key: \"b\", value: 2 }]).entries",
    ),
    ("set_length_property_boundary", "Set.of(1, 2, 1).length"),
    (
        "json_stringify_record_boundary",
        "JSON.stringify({ b: 2, a: 1 })",
    ),
    (
        "json_stringify_named_array_boundary",
        "JSON.stringify(value: [1, 2, 3])",
    ),
    (
        "json_parse_named_error_boundary",
        "JSON.parse(text: \"\\{\")",
    ),
    ("json_parse_error_boundary", "JSON.parse(\"\\{\")"),
    ("path_from_boundary", "Path.from(\"src//./main.tpz\")"),
    (
        "path_cwd_relative_named_boundary",
        "Path.cwdRelative(text: \"logs/app.txt\")",
    ),
    (
        "path_project_error_boundary",
        "Path.project(text: \"../escape\")",
    ),
    (
        "cli_has_flag_boundary",
        "Cli.hasFlag([\"--verbose\", \"input\"], \"--verbose\")",
    ),
    (
        "cli_option_boundary",
        "Cli.option([\"--out\", \"file\", \"input\"], \"--out\")",
    ),
    (
        "cli_options_boundary",
        "Cli.options([\"--include=a\", \"--include\", \"b\"], \"--include\")",
    ),
    (
        "cli_positionals_boundary",
        "Cli.positionals([\"--out\", \"file\", \"input\", \"--\", \"--literal\"]).join(\",\")",
    ),
    (
        "regex_compile_error_boundary",
        "Regex.compile(pattern: \"[abc\")",
    ),
    ("csv_parse_boundary", "CSV.parse(\"name,age\\nAda,36\")"),
    ("csv_parse_named_boundary", "CSV.parse(text: \"a,b\\n\")"),
    (
        "csv_parse_with_header_error_boundary",
        "CSV.parseWithHeader(text: \"name\\nAda,36\")",
    ),
    (
        "csv_stringify_boundary",
        "CSV.stringify([[\"a\", \"b\"], [\"c\", \"d\"]])",
    ),
    (
        "csv_stringify_with_header_boundary",
        "CSV.stringifyWithHeader([map { \"name\": \"Ada\", \"age\": \"36\" }], [\"name\", \"age\"])",
    ),
    ("toml_parse_error_boundary", "TOML.parse(text: \"name = \")"),
    (
        "url_parse_boundary",
        "URL.parse(\"https://example.com/a/b?x=1#top\")",
    ),
    (
        "url_parse_named_error_boundary",
        "URL.parse(text: \"not-a-url\")",
    ),
    (
        "date_from_ymd_named_boundary",
        "Date.fromYmd(year: 2024, month: 2, day: 29)",
    ),
    ("date_parse_error_boundary", "Date.parseIso(\"2024-02-30\")"),
    ("bigint_from_int_boundary", "BigInt.fromInt(42)"),
    (
        "bigint_parse_named_boundary",
        "BigInt.parse(text: \"ff\", radix: 16)",
    ),
    ("decimal_from_int_boundary", "Decimal.fromInt(-7)"),
    (
        "decimal_parse_named_boundary",
        "Decimal.parse(text: \"12.3400\")",
    ),
    (
        "bigint_to_string_method_boundary",
        "BigInt.fromInt(255).toString(16)",
    ),
    (
        "bigint_div_method_boundary",
        "BigInt.fromInt(5).div(BigInt.fromInt(2))",
    ),
    (
        "bigint_mod_named_method_boundary",
        "BigInt.fromInt(5).mod(other: BigInt.fromInt(2))",
    ),
    (
        "decimal_to_string_method_boundary",
        "Decimal.fromInt(12).toString()",
    ),
    (
        "decimal_scale_method_boundary",
        "Decimal.fromInt(12).scale()",
    ),
    (
        "decimal_div_method_boundary",
        "Decimal.fromInt(5).div(Decimal.fromInt(2), 2)",
    ),
    (
        "decimal_round_named_method_boundary",
        "Decimal.fromInt(25).round(scale: 1)",
    ),
    (
        "hash_sha256_boundary",
        "Hash.sha256(Bytes.encodeUtf8(\"abc\")).toHex()",
    ),
    (
        "hash_sha512_local_boundary",
        "let data = Bytes.encodeUtf8(\"abc\")\nHash.sha512(data).toHex()",
    ),
    (
        "hash_hmac_named_boundary",
        "Hash.hmacSha256(key: Bytes.encodeUtf8(\"Jefe\"), message: Bytes.encodeUtf8(\"what do ya want for nothing?\")).toHex()",
    ),
    (
        "hash_crc32_named_boundary",
        "Hash.crc32(data: Bytes.encodeUtf8(\"123456789\"))",
    ),
    (
        "encoding_hex_encode_boundary",
        "Encoding.hexEncode(Encoding.utf8Encode(\"topaz\"))",
    ),
    (
        "encoding_hex_decode_named_boundary",
        "Encoding.hexDecode(text: \"6869\")",
    ),
    (
        "encoding_base64_named_boundary",
        "Encoding.base64Encode(bytes: Encoding.utf8Encode(\"foo\"))",
    ),
    (
        "codec_gzip_boundary",
        "Codec.gzipCompress(Bytes.encodeUtf8(\"hi\"))",
    ),
    (
        "codec_deflate_named_boundary",
        "Codec.deflateCompress(bytes: Bytes.encodeUtf8(\"topaz\"))",
    ),
    (
        "codec_deflate_fixed_named_boundary",
        "Codec.deflateFixedCompress(bytes: Bytes.encodeUtf8(\"topaztopaztopaz\"))",
    ),
    (
        "codec_zlib_fixed_named_boundary",
        "Codec.zlibFixedCompress(bytes: Bytes.encodeUtf8(\"topaztopaztopaz\"))",
    ),
    (
        "codec_reed_solomon_named_boundary",
        "Codec.reedSolomon255223Protect(bytes: Bytes.encodeUtf8(\"topaz\"))",
    ),
    (
        "codec_zstd_default_boundary",
        "Codec.zstdCompress(Bytes.encodeUtf8(\"hello\"))",
    ),
    (
        "codec_zstd_named_boundary",
        "Codec.zstdCompress(level: 7, bytes: Bytes.encodeUtf8(\"hi\"))",
    ),
    (
        "array_nested_read",
        "let arr: Array<Array<int>> = [[1, 2], [3, 4]]\narr[0]",
    ),
    // --- the byte-identity fault fixtures (route through the shared leaf) ---
    // Section 13a checked integer overflow => TPZ4004 at the operator span.
    ("fault_overflow", "9223372036854775807 + 1"),
    // div-by-zero => TPZ4002; rem-by-zero => TPZ4002.
    ("fault_div_zero", "let z = 0\n10 / z"),
    ("fault_rem_zero", "let z = 0\n10 % z"),
    // `i64::MIN / -1` overflow => TPZ4004 (checked_div None). `i64::MIN` is
    // built as `-9223372036854775807 - 1` (the bare literal overflows i64).
    (
        "fault_min_div_neg1",
        "let m = -9223372036854775807 - 1\nm / -1",
    ),
    // `i64::MIN % -1` overflow => TPZ4004.
    (
        "fault_min_rem_neg1",
        "let m = -9223372036854775807 - 1\nm % -1",
    ),
    // `i64::MIN` negation overflow => TPZ4004.
    ("fault_neg_overflow", "let m = -9223372036854775807 - 1\n-m"),
    // negative exponent => TPZ4005.
    ("fault_neg_exp", "let e = -1\n2 ** e"),
    // --- native if/while/let mut, the integer loop programs the slice pins ---
    ("if_value", "let n = 5\nif n > 3 { 100 } else { 200 }"),
    (
        "if_nested",
        "let n = 0\nif n == 0 { if true { 1 } else { 2 } } else { 3 }",
    ),
    (
        "while_sum",
        "let mut i = 1\nlet mut total = 0\nwhile i <= 100 { total += i\ni += 1 }\ntotal",
    ),
    (
        "while_factorial",
        "let mut n = 10\nlet mut acc = 1\nwhile n > 1 { acc = acc * n\nn = n - 1 }\nacc",
    ),
    (
        "while_fibonacci",
        "let mut a = 0\nlet mut b = 1\nlet mut k = 0\nwhile k < 20 { let next = a + b\na = b\nb = next\nk += 1 }\na",
    ),
    (
        "nested_loop_sum",
        "let mut i = 0\nlet mut sum = 0\nwhile i < 10 { let mut j = 0\nwhile j < 10 { sum += i * j\nj += 1 }\ni += 1 }\nsum",
    ),
    (
        "for_array_sum_native",
        "let mut s = 0\nfor x in [1, 2, 3] { s += x }\ns",
    ),
    (
        "for_range_in_fn_native",
        "function sumTo(n: int) -> int { let mut s = 0\nfor x in 1..n { s = s + x }\ns }\nsumTo(5)",
    ),
    (
        "for_range_step_native",
        "let mut s = 0\nfor x in 0..<10 by 2 { s += x }\ns",
    ),
    (
        "match_int_native",
        "let n = 1\nmatch n { case 1 => 10\ncase _ => 20 }",
    ),
    (
        "match_guard_bool_native",
        "let b = true\nmatch b { case true if false => 1\ncase true => 2\ncase _ => 3 }",
    ),
    (
        "match_miss_fault_native",
        "let n = 5\nmatch n { case 1 => 10\ncase 2 => 20 }",
    ),
    (
        "match_binding_native",
        "let x = 7\nmatch x { case 1 => 0\ncase n => n + 1 }",
    ),
    (
        "match_string_binding_native",
        "let s = \"b\"\nmatch s { case n if n == \"a\" => 1\ncase n if n == \"b\" => 2\ncase _ => 3 }",
    ),
    (
        "while_break",
        "let mut i = 0\nwhile true { i += 1\nif i >= 42 { break } }\ni",
    ),
    (
        "while_continue",
        "let mut i = 0\nlet mut evens = 0\nwhile i < 20 { i += 1\nif i % 2 == 1 { continue }\nevens += 1 }\nevens",
    ),
    // overflow INSIDE a loop must fault at the operator, mid-iteration.
    (
        "while_overflow",
        "let mut x = 1\nwhile true { x = x * 2 }\nx",
    ),
    // A COMPOUND-assign overflow must fault at the WHOLE statement's span,
    // exactly the span the boxed backend passes to `binary_value`, so the two
    // engines' fault spans agree (a native compound-assign-span regression fails
    // here, not silently).
    (
        "while_compound_overflow",
        "let mut x = 1\nwhile true { x += x }\nx",
    ),
    // float loop accumulation (IEEE, boxed render at the boundary).
    (
        "while_float",
        "let mut x = 1.0\nlet mut k = 0\nwhile k < 10 { x = x * 2.0\nk += 1 }\nx",
    ),
    // --- checkpoint-ELISION termination + fault parity (the perf-unlock proof) ---
    // A LONG-RUNNING bounded loop (100k iterations): with no `concurrent` in the
    // unit, native ELIDES the back-edge `checkpoint().await` (a plain Rust loop);
    // boxed keeps it. The result + termination must stay byte-identical; this is
    // the case the small bounded fixtures don't stress (many iterations x elision).
    (
        "long_loop_terminates",
        "let mut i = 0\nlet mut acc = 0\nwhile i < 100000 { acc = (acc + i) % 1000000007\ni = i + 1 }\nacc",
    ),
    // A long-running loop that does NOT terminate by condition but FAULTS (integer
    // overflow) deep into iteration: with the checkpoint elided, native must still
    // hit the SAME overflow fault at the SAME span as boxed/interp, proving
    // elision preserves the exact fault (the `?` propagates; no budget is dropped
    // because `checkpoint()` enforces none). Powers of a small base overflow i64
    // after ~39 iterations; the loop "runs past" where a budget would stop it (if
    // one existed), terminating only on the fault, identically across engines.
    (
        "elided_loop_faults_identically",
        "let mut x = 3\nwhile true { x = x * 7 }\nx",
    ),
    // --- direct same-module scalar function calls (slice item 3) ---
    // A scalar function whose body has NO call (so no unguarded native recursion)
    // called directly from the top level: a single native `fn` call. Depth 1 is
    // well below `CALL_DEPTH_LIMIT`, so it is byte-identical to the interpreter.
    (
        "fn_call_simple",
        "function inc(n: int) -> int { n + 1 }\ninc(41)",
    ),
    (
        "fn_call_branch",
        "function clamp(n: int) -> int { if n < 0 { 0 } else { n } }\nclamp(-7)",
    ),
    (
        "fn_call_args",
        "function add(a: int, b: int) -> int { a + b }\nadd(20, 22)",
    ),
    (
        "fn_call_default_param_fully_supplied",
        "function add(a: int, b: int = 1) -> int { a + b }\nadd(20, 22)",
    ),
    (
        "fn_call_default_param_used",
        "function add(a: int, b: int = 10) -> int { a + b }\nadd(5)",
    ),
    (
        "fn_call_all_default_params",
        "function add(a: int = 1, b: int = 2) -> int { a + b }\nadd()",
    ),
    (
        "fn_call_named_skips_default_params",
        "function pack(a: int = 1, b: int = 2, c: int = 3) -> int { a * 100 + b * 10 + c }\npack(c: 9)",
    ),
    (
        "fn_call_named_args",
        "function sub(a: int, b: int) -> int { a - b }\nsub(b: 5, a: 20)",
    ),
    (
        "fn_call_mixed_named_args",
        "function pack(a: int, b: int, c: int) -> int { a * 100 + b * 10 + c }\npack(1, c: 3, b: 2)",
    ),
    (
        "fn_call_named_eval_order",
        "function take(a: int, b: int) -> int { a * 10 + b }\nfunction mark(label: string, n: int) -> int { print(label)\nn }\ntake(b: mark(\"b\", 2), a: mark(\"a\", 1))",
    ),
    (
        "fn_call_float",
        "function half(x: float) -> float { x / 2.0 }\nhalf(9.0)",
    ),
    // A function fault propagates with the body operator's span, byte-identical.
    (
        "fn_call_overflow",
        "function dbl(n: int) -> int { n * 2 }\ndbl(9223372036854775807)",
    ),
    // --- Knuth-Plass-style integer kernel ---
    // `roundDiv`/`absI`/`badness` are pure scalar helpers; the top-level driver
    // runs a Knuth-Plass-style inner loop accumulating a badness sum. The
    // helpers' bodies have no call (native fns); the loop is native scalar, the
    // marketing native-emit use-case, here pinned byte-identical across 3 columns.
    (
        "linebreaker_round_div",
        "function roundDiv(a: int, b: int) -> int { (a + b / 2) / b }\nroundDiv(100, 3)",
    ),
    (
        "linebreaker_abs",
        "function absI(n: int) -> int { if n < 0 { -n } else { n } }\nabsI(-1234)",
    ),
    (
        "linebreaker_dp_inner_loop",
        "let target = 70\nlet mut i = 1\nlet mut sum = 0\nwhile i <= 120 { let d = i - target\nlet pen = d * d\nsum += pen\ni += 1 }\nsum",
    ),
    // --- ASYNC-NATIVE-FNS: fn bodies may now host loops AND calls (acyclic) ---
    // A `while` INSIDE a native fn body (was a refusal pin; now native: the fn is
    // an async fn whose body loop has the checkpoint elided, no concurrent).
    (
        "fn_body_has_while",
        "function count(n: int) -> int { let mut i = 0\nwhile i < n { i += 1 }\ni }\ncount(3)",
    ),
    // A native fn body that CALLS another native fn (was a refusal pin; now native
    // the call is `Box::pin(absI(cx.clone(), span, ...)).await?`, with the
    // recursion guard threaded through the shared `__native_enter_call`.
    // Acyclic: badness -> absI only.
    (
        "fn_body_calls_fn",
        "function absI(n: int) -> int { if n < 0 { -n } else { n } }\nfunction badness(actual: int, target: int) -> int { let d = absI(actual - target)\nd * d }\nbadness(40, 70)",
    ),
    // This line-breaker fixture gives `badness` a nested loop and two helper calls:
    // `absI` and `roundDiv`: the line-breaker hot-path shape (a fn body that loops
    // and calls helpers). Fully native, acyclic call graph
    // (penalty -> absI/roundDiv; top-level driver loop -> penalty).
    (
        "linebreaker_badness_loop_and_calls",
        "function absI(n: int) -> int { if n < 0 { -n } else { n } }\n\
         function roundDiv(a: int, b: int) -> int { (a + b / 2) / b }\n\
         function penalty(width: int, ideal: int) -> int { let mut acc = 0\nlet mut k = 1\nwhile k <= 3 { let slack = absI(width - ideal)\nacc = acc + roundDiv(slack * slack, k)\nk += 1 }\nacc }\n\
         let mut total = 0\nlet mut w = 60\nwhile w <= 80 { total = total + penalty(w, 70)\nw += 1 }\ntotal",
    ),
    // RECURSION-GUARD PARITY: a deep ACYCLIC call chain a -> b -> c (each calls the
    // next once, NOT a cycle) plus a top-level loop, exercising the threaded
    // `__native_enter_call` depth counting on real native calls. The chain is
    // shallow (depth 3, well under CALL_DEPTH_LIMIT=1000) so it completes; the point
    // is that depth bookkeeping is identical across engines for a multi-level chain.
    (
        "fn_chain_depth",
        "function c(n: int) -> int { n + 1 }\nfunction b(n: int) -> int { c(n) + 1 }\nfunction a(n: int) -> int { b(n) + 1 }\na(10)",
    ),
    (
        "recursive_fn",
        "function fib(n: int) -> int { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }\nfib(10)",
    ),
    (
        "mutual_recursion",
        "function isEven(n: int) -> bool { if n == 0 { true } else { isOdd(n - 1) } }\nfunction isOdd(n: int) -> bool { if n == 0 { false } else { isEven(n - 1) } }\nisEven(10)",
    ),
    (
        "deep_recursion_faults",
        "function deep(n: int) -> int { if n == 0 { 0 } else { deep(n - 1) } }\ndeep(2000)",
    ),
    // Long acyclic chains lower natively through boxed callee futures, so they do
    // not build a nested concrete future type tower.
    (
        "chain_below_bound",
        "function f11(x: int) -> int { x + 1 }\n\
         function f10(x: int) -> int { f11(x) + 1 }\n\
         function f9(x: int) -> int { f10(x) + 1 }\n\
         function f8(x: int) -> int { f9(x) + 1 }\n\
         function f7(x: int) -> int { f8(x) + 1 }\n\
         function f6(x: int) -> int { f7(x) + 1 }\n\
         function f5(x: int) -> int { f6(x) + 1 }\n\
         function f4(x: int) -> int { f5(x) + 1 }\n\
         function f3(x: int) -> int { f4(x) + 1 }\n\
         function f2(x: int) -> int { f3(x) + 1 }\n\
         function f1(x: int) -> int { f2(x) + 1 }\n\
         function f0(x: int) -> int { f1(x) + 1 }\n\
         f0(0)",
    ),
    (
        "chain_over_bound",
        "function f19(x: int) -> int { x + 1 }\n\
         function f18(x: int) -> int { f19(x) + 1 }\n\
         function f17(x: int) -> int { f18(x) + 1 }\n\
         function f16(x: int) -> int { f17(x) + 1 }\n\
         function f15(x: int) -> int { f16(x) + 1 }\n\
         function f14(x: int) -> int { f15(x) + 1 }\n\
         function f13(x: int) -> int { f14(x) + 1 }\n\
         function f12(x: int) -> int { f13(x) + 1 }\n\
         function f11(x: int) -> int { f12(x) + 1 }\n\
         function f10(x: int) -> int { f11(x) + 1 }\n\
         function f9(x: int) -> int { f10(x) + 1 }\n\
         function f8(x: int) -> int { f9(x) + 1 }\n\
         function f7(x: int) -> int { f8(x) + 1 }\n\
         function f6(x: int) -> int { f7(x) + 1 }\n\
         function f5(x: int) -> int { f6(x) + 1 }\n\
         function f4(x: int) -> int { f5(x) + 1 }\n\
         function f3(x: int) -> int { f4(x) + 1 }\n\
         function f2(x: int) -> int { f3(x) + 1 }\n\
         function f1(x: int) -> int { f2(x) + 1 }\n\
         function f0(x: int) -> int { f1(x) + 1 }\n\
         f0(0)",
    ),
    // --- NATIVE Array<scalar> READ boundary (index + .length) ---
    // The canonical shape: iterate `i < arr.length`, read `arr[i]`, accumulate.
    // Fully native (boxed array boundary local + native scalar loop), byte-identical.
    (
        "array_sum",
        "let arr: Array<int> = [3, 1, 4, 1, 5]\nlet mut s = 0\nlet mut i = 0\nwhile i < arr.length { s += arr[i]\ni += 1 }\ns",
    ),
    // A native scalar function can now accept a read-only `Array<int>` boundary
    // parameter. The array itself stays boxed; the function body lowers `.length`
    // and `[i]` through the same shared helpers as top-level array boundaries.
    (
        "fn_array_param_sum",
        "function sum(xs: Array<int>) -> int { let mut i = 0\nlet mut s = 0\nwhile i < xs.length { s += xs[i]\ni += 1 }\ns }\nlet xs: Array<int> = [4, 8, 15, 16]\nsum(xs)",
    ),
    // Direct array-literal arguments are boxed exactly once at the call site, then
    // read natively in the callee.
    (
        "fn_array_param_literal_bool",
        "function count(flags: Array<bool>) -> int { let mut i = 0\nlet mut c = 0\nwhile i < flags.length { if flags[i] { c += 1 }\ni += 1 }\nc }\ncount([true, false, true, true])",
    ),
    // A line-breaker cumulative pass reads arr[i], branches on it, and accumulates a
    // running best/sum: the Knuth-Plass line-breaker inner shape (iterate + index
    // + branch over a scalar items array).
    (
        "array_dp_cumulative",
        "let widths: Array<int> = [12, 7, 19, 3, 25, 8, 14]\nlet target = 15\nlet mut i = 0\nlet mut penalty = 0\nlet mut best = 0\nwhile i < widths.length { let d = widths[i] - target\nlet pen = d * d\npenalty = penalty + pen\nif widths[i] > best { best = widths[i] }\ni += 1 }\npenalty + best",
    ),
    // Array<float> element unbox to f64.
    (
        "array_float_sum",
        "let arr: Array<float> = [1.5, 2.5, 3.0]\nlet mut s = 0.0\nlet mut i = 0\nwhile i < arr.length { s = s + arr[i]\ni += 1 }\ns",
    ),
    // Array<bool> element unbox to bool (count the trues).
    (
        "array_bool_count",
        "let flags: Array<bool> = [true, false, true, true]\nlet mut c = 0\nlet mut i = 0\nwhile i < flags.length { if flags[i] { c += 1 }\ni += 1 }\nc",
    ),
    (
        "array_string_concat",
        "let arr: Array<string> = [\"a\", \"b\"]\narr[0] + arr[1]",
    ),
    (
        "for_string_array_concat",
        "let mut out = \"\"\nfor x in [\"a\", \"b\", \"c\"] { out = out + x }\nout",
    ),
    // `.length` of an empty scalar array => 0 (the loop never runs).
    (
        "array_empty_length",
        "let arr: Array<int> = []\nlet mut s = 0\nlet mut i = 0\nwhile i < arr.length { s += arr[i]\ni += 1 }\ns",
    ),
    // OOB FAULT parity: an index past the end faults `FAULT_INDEX` (TPZ4001) at the
    // index-expression span, BYTE-IDENTICAL to interp/boxed; native routes through
    // the SHARED `index_value` leaf. (Native emits fine; the FAULT happens at run.)
    (
        "array_index_oob",
        "let arr: Array<int> = [10, 20, 30]\narr[5]",
    ),
    // A non-constant (computed) index, still a native `int` read.
    (
        "array_index_computed",
        "let arr: Array<int> = [100, 200, 300, 400]\nlet i = 1\narr[i + 2]",
    ),
    (
        "array_index_write",
        "let mut arr: Array<int> = [1, 2, 3]\narr[0] = 9\narr[0]",
    ),
];

/// The native REFUSAL set: well-typed single-module programs OUTSIDE the scalar
/// island for which the build asserts only the structured `TPZ6002` decline.
/// Prefer [`FALLBACK_FIXTURES`] when the boxed fallback can also be run here.
const REFUSALS: &[(&str, &str)] = &[];

/// FALLBACK fixtures: when present, native declines each (asserted TPZ6002) and
/// the program is run END TO END through the BOXED fallback the CLI actually uses
/// on a decline.
type ExternFixtureDef = (
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
);

type FallbackFixtureDef = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
    &'static [ExternFixtureDef],
    &'static str,
);

const FALLBACK_FIXTURES: &[FallbackFixtureDef] = &[
    (
        "byte_buffer_native_fallback_alias_overlap",
        "main.tpz",
        &[(
            "main.tpz",
            "let mut buffer = ByteBuffer.allocate(6, 1)\nbuffer.set(0, 9)\nbuffer.set(1, 8)\nlet mut alias = buffer\nalias.copy(alias, 0, 2, 4)\nalias.toBytes().toHex()",
        )],
        &[],
        "",
    ),
    (
        "byte_buffer_native_fallback_range_fault",
        "main.tpz",
        &[(
            "main.tpz",
            "let mut buffer = ByteBuffer.allocate(4, 1)\nbuffer.fill(1, 4, 9)",
        )],
        &[],
        "",
    ),
    (
        "native_extern_replay_positive_boxed_fallback",
        "main.tpz",
        &[(
            "main.tpz",
            r#"import host.math { twice }
let answer = twice(21)
print("twice={answer}")
answer"#,
        )],
        &[(
            "host.math",
            "host/math.tpz",
            "export function twice(x: int) -> int { x }",
            None,
        )],
        r#"{"module":"host.math","function":"twice","args":[{"$":"int","value":"21"}],"result":{"$":"int","value":"42"}}"#,
    ),
];

type HybridFixtureDef = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
);

/// Whole-unit native declines these multi-module programs, after which the
/// bounded hybrid must retain the boxed envelope and replace only proven scalar
/// top-level functions.
const HYBRID_FIXTURES: &[HybridFixtureDef] = &[
    (
        "hybrid_mixed_value",
        "main.tpz",
        &[
            (
                "main.tpz",
                "import util\nlet values = [1, 2, 3]\nlet word = util.keep(\"ok\")\nlet result = util.twice(values.length)\nprint(\"{result}:{word}\")\nresult",
            ),
            (
                "util.tpz",
                "export function twice(x: int) -> int { x * 2 }\nexport function keep(value: string) -> string { value }",
            ),
        ],
    ),
    (
        "hybrid_fault_identity",
        "main.tpz",
        &[
            (
                "main.tpz",
                "import util\nlet values = [1]\nlet zero = values.length - values.length\nutil.divide(values.length, zero)",
            ),
            (
                "util.tpz",
                "export function divide(a: int, b: int) -> int { a / b }",
            ),
        ],
    ),
    (
        "hybrid_same_name_modules",
        "main.tpz",
        &[
            ("main.tpz", "import a\nimport b\na.same(1) + b.same(2)"),
            ("a.tpz", "export function same(x: int) -> int { x + 10 }"),
            ("b.tpz", "export function same(x: int) -> int { x + 20 }"),
        ],
    ),
    (
        "hybrid_recursive_scalar_call",
        "main.tpz",
        &[
            (
                "main.tpz",
                "import util\nlet seed = [1, 2, 3, 4, 5, 6]\nutil.fib(seed.length)",
            ),
            (
                "util.tpz",
                "export function fib(n: int) -> int {\n  if n <= 1 { n } else { fib(n - 1) + fib(n - 2) }\n}",
            ),
        ],
    ),
    (
        "hybrid_byte_record_projection",
        "main.tpz",
        &[
            (
                "main.tpz",
                "import raster { Image, paint }\n\
                 let mut direct = ByteBuffer.allocate(4, 1)\n\
                 let image = Image { pixels: direct }\n\
                 paint(image, direct)",
            ),
            (
                "raster.tpz",
                "export record Image { pixels: ByteBuffer }\n\
                 export function paint(image: Image, direct: ByteBuffer) -> int {\n\
                   let mut pixels = image.pixels\n\
                   let mut second = direct\n\
                   pixels.set(0, 7)\n\
                   second.set(1, 9)\n\
                   pixels.get(0) + second.get(1)\n\
                 }",
            ),
        ],
    ),
    (
        "hybrid_byte_bounds_fault",
        "main.tpz",
        &[
            (
                "main.tpz",
                "import bytes { read }\nlet mut buffer = ByteBuffer.allocate(2, 4)\nread(buffer, 2)",
            ),
            (
                "bytes.tpz",
                "export function read(buffer: ByteBuffer, index: int) -> int { buffer.get(index) }",
            ),
        ],
    ),
    (
        "hybrid_byte_self_copy_overlap",
        "main.tpz",
        &[
            (
                "main.tpz",
                "import bytes { shift }\n\
                 let mut buffer = ByteBuffer.allocate(5, 0)\n\
                 buffer.set(0, 1); buffer.set(1, 2); buffer.set(2, 3); buffer.set(3, 4); buffer.set(4, 5)\n\
                 shift(buffer)",
            ),
            (
                "bytes.tpz",
                "export function shift(buffer: ByteBuffer) -> int {\n\
                   let mut target = buffer\n\
                   target.copy(target, 0, 1, 4)\n\
                   target.get(1) * 1000 + target.get(2) * 100 + target.get(3) * 10 + target.get(4)\n\
                 }",
            ),
        ],
    ),
    (
        "hybrid_byte_invalid_write_atomic",
        "main.tpz",
        &[
            (
                "main.tpz",
                "import bytes { invalid }\nlet mut buffer = ByteBuffer.allocate(2, 7)\ninvalid(buffer)",
            ),
            (
                "bytes.tpz",
                "export function invalid(buffer: ByteBuffer) -> int { let mut target = buffer; target.fill(0, 2, 999); target.get(0) }",
            ),
        ],
    ),
    (
        "hybrid_bytes_leaf_set",
        "main.tpz",
        &[
            (
                "main.tpz",
                "import bytes { inspect }\nlet mut buffer = ByteBuffer.allocate(3, 65)\ninspect(buffer)",
            ),
            (
                "bytes.tpz",
                "export function inspect(buffer: ByteBuffer) -> int {\n\
                   let frozen = buffer.toBytes()\n\
                   let first = frozen.get(0)\n\
                   let sliced = frozen.slice(0, 2)\n\
                   sliced.length()\n\
                 }",
            ),
        ],
    ),
];

/// Resolve a single-file fixture to a `ResolveOutput` (the unit both backends
/// lower). Panics on a resolution diagnostic: a native fixture must resolve
/// clean.
fn resolve_fixture(name: &str, source: &str) -> ResolveOutput {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", source);
    let unit = resolve(&provider, "main.tpz", None);
    assert!(
        unit.diagnostics.is_empty(),
        "native fixture `{name}` must resolve clean: {:?}",
        unit.diagnostics
    );
    unit
}

struct ExternReplayProvider {
    inner: InMemoryProvider,
    extern_files: BTreeMap<String, String>,
    extern_namespaces: BTreeSet<String>,
    replay_errors: BTreeMap<String, String>,
}

impl ExternReplayProvider {
    fn new() -> Self {
        Self {
            inner: InMemoryProvider::new(),
            extern_files: BTreeMap::new(),
            extern_namespaces: BTreeSet::new(),
            replay_errors: BTreeMap::new(),
        }
    }

    fn add_file(&mut self, path: &'static str, source: &'static str) {
        self.inner.add_file(path, source);
    }

    fn add_extern_file(
        &mut self,
        identity: &'static str,
        path: &'static str,
        source: &'static str,
        replay_error: Option<&'static str>,
    ) {
        self.inner.add_file(path, source);
        self.extern_files
            .insert(path.to_string(), identity.to_string());
        if let Some((root, _)) = identity.split_once('.') {
            self.extern_namespaces.insert(root.to_string());
        }
        if let Some(error) = replay_error {
            self.replay_errors
                .insert(identity.to_string(), error.to_string());
        }
    }
}

impl FileProvider for ExternReplayProvider {
    fn read(&self, path: &str) -> topaz_resolve::SourceRead {
        self.inner.read(path)
    }

    fn is_extern_file(&self, path: &str) -> bool {
        self.extern_files.contains_key(path)
    }

    fn is_extern_namespace(&self, identity: &str) -> bool {
        self.extern_namespaces
            .iter()
            .any(|ns| identity == ns || identity.starts_with(&format!("{ns}.")))
    }

    fn extern_replay_error(&self, identity: &str) -> Option<String> {
        self.replay_errors.get(identity).cloned()
    }

    fn read_directory(&self, dir: &str) -> topaz_resolve::DirectoryRead {
        self.inner.read_directory(dir)
    }

    fn physical_id(&self, path: &str) -> Option<String> {
        self.inner.physical_id(path)
    }
}

fn resolve_fallback_fixture(
    name: &str,
    entry: &str,
    files: &[(&'static str, &'static str)],
    externs: &[ExternFixtureDef],
) -> ResolveOutput {
    let mut provider = ExternReplayProvider::new();
    for (path, source) in files {
        provider.add_file(path, source);
    }
    for (identity, path, source, replay_error) in externs {
        provider.add_extern_file(identity, path, source, *replay_error);
    }
    let unit = resolve(&provider, entry, None);
    assert!(
        unit.diagnostics.is_empty(),
        "native fallback fixture `{name}` must resolve clean: {:?}",
        unit.diagnostics
    );
    unit
}

/// Type-check a resolved unit and return its typed HIR (the native backend's
/// soundness input). Panics if the unit does not type clean: a native fixture
/// must check clean (the native backend only ever sees clean units).
fn checked_of(name: &str, unit: &ResolveOutput) -> topaz_check::CheckedUnit {
    let modules: Vec<UnitModule> = unit
        .modules
        .iter()
        .map(|m| UnitModule {
            identity: m.identity.clone(),
            is_entry: m.is_entry,
            is_extern: m.is_extern,
            is_generated_std: m.is_generated_std,
            extern_replay_error: m.extern_replay_error.clone(),
            src: unit.map.file(m.file).src(),
            program: &m.program,
        })
        .collect();
    let checked = check_unit_typed(&modules);
    assert!(
        checked.diagnostics.is_empty(),
        "native fixture `{name}` must type-check clean: {:?}",
        checked.diagnostics
    );
    assert!(
        checked.typed_hir.is_some(),
        "clean check of `{name}` yields typed HIR"
    );
    checked
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let mut modules = String::new();
    let mut table = String::from(
        "/// Every native-eligible fixture, in source order: its source (the\n\
         /// interpreter column), the BOXED entry, and the NATIVE entry.\n\
         pub static FIXTURES: &[Fixture] = &[\n",
    );

    let mut idx = 0usize;
    for (name, source) in FIXTURES {
        let unit = resolve_fixture(name, source);
        let checked = checked_of(name, &unit);
        let lowered = topaz_lower::lower_checked(&unit, &checked)
            .unwrap_or_else(|error| panic!("native fixture `{name}` failed to lower: {error}"));

        // The boxed column: the SAME `emit_module` output `topaz_difftest`
        // proves, so the two harnesses agree on the boxed lowering.
        let boxed = topaz_emit::emit_module(&lowered)
            .unwrap_or_else(|e| panic!("native fixture `{name}` failed to emit (boxed): {e:?}"));
        let boxed_mod = idx;
        idx += 1;
        modules.push_str(&format!(
            "#[allow(clippy::all, dead_code, unused, unreachable_code, non_snake_case, while_true)]\nmod fixture_{boxed_mod} {{\n{boxed}}}\n\n"
        ));

        // The native column: the typed-HIR-driven monomorphized backend. A native
        // fixture MUST lower (it is in the eligible set); a decline here is a
        // regression, so panic rather than silently fall back.
        let input = NativeInput { unit: &lowered };
        let native = topaz_emit::emit_native_items(&input)
            .unwrap_or_else(|e| panic!("native fixture `{name}` failed to emit (native): {e:?}"));
        let native_mod = idx;
        idx += 1;
        modules.push_str(&format!(
            "#[allow(clippy::all, dead_code, unused, unreachable_code, non_snake_case, while_true)]\nmod fixture_{native_mod} {{\n{native}}}\n\n"
        ));

        table.push_str(&format!(
            "    Fixture {{ name: {name:?}, source: {source:?}, boxed: fixture_{boxed_mod}::run_with_host, native: fixture_{native_mod}::run_with_host }},\n"
        ));
    }
    table.push_str("];\n");

    // The REFUSAL set: assert the native backend DECLINES each (structured
    // `TPZ6002`), so a future change that silently mis-handles one fails the
    // build. Nothing is emitted for these; the assertion IS the proof.
    let mut refusal_count = 0usize;
    for (name, source) in REFUSALS {
        let unit = resolve_fixture(name, source);
        let checked = checked_of(name, &unit);
        let lowered = topaz_lower::lower_checked(&unit, &checked)
            .unwrap_or_else(|error| panic!("refusal fixture `{name}` failed to lower: {error}"));
        let input = NativeInput { unit: &lowered };
        match topaz_emit::emit_native_items(&input) {
            Ok(_) => panic!(
                "native refusal fixture `{name}` UNEXPECTEDLY lowered natively (must decline)"
            ),
            Err(e) => {
                assert!(
                    e.is_native_decline(),
                    "native refusal fixture `{name}` declined with the WRONG error kind: {e:?}"
                );
                refusal_count += 1;
            }
        }
    }

    // The FALLBACK set: native MUST decline each (TPZ6002), AND we emit the BOXED
    // program (the CLI's actual fallback) + a run table, so the test runs interp vs
    // boxed and proves the decline-to-boxed path is byte-identical end to end.
    let mut fallback_table = String::from(
        "/// Native-DECLINE fallback fixtures (run interp vs the boxed fallback).\npub static FALLBACK_FIXTURES: &[FallbackFixture] = &[\n",
    );
    for (name, entry, files, externs, extern_replay_jsonl) in FALLBACK_FIXTURES {
        let unit = resolve_fallback_fixture(name, entry, files, externs);
        let checked = checked_of(name, &unit);
        let lowered = topaz_lower::lower_checked(&unit, &checked)
            .unwrap_or_else(|error| panic!("fallback fixture `{name}` failed to lower: {error}"));
        let input = NativeInput { unit: &lowered };
        // (1) native MUST decline (structured TPZ6002, never a divergent native binary).
        match topaz_emit::emit_native_items(&input) {
            Ok(_) => panic!(
                "fallback fixture `{name}` UNEXPECTEDLY lowered natively (must decline to boxed)"
            ),
            Err(e) => assert!(
                e.is_native_decline(),
                "fallback fixture `{name}` declined with the WRONG error kind: {e:?}"
            ),
        }
        // (2) emit the BOXED program (the actual `--backend native` fallback) and
        // run it in the 3-column test against the interpreter.
        let boxed = topaz_emit::emit_module(&lowered).unwrap_or_else(|e| {
            panic!("fallback fixture `{name}` failed to emit (boxed fallback): {e:?}")
        });
        let m = idx;
        idx += 1;
        modules.push_str(&format!(
            "#[allow(clippy::all, dead_code, unused, unreachable_code, non_snake_case, while_true)]\nmod fixture_{m} {{\n{boxed}}}\n\n"
        ));
        let files_lit = files
            .iter()
            .map(|(p, s)| format!("FixtureFile {{ path: {p:?}, source: {s:?} }}"))
            .collect::<Vec<_>>()
            .join(", ");
        let externs_lit = externs
            .iter()
            .map(|(identity, path, source, replay_error)| {
                format!(
                    "ExternFixtureFile {{ identity: {identity:?}, path: {path:?}, source: {source:?}, replay_error: {replay_error:?} }}"
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        fallback_table.push_str(&format!(
            "    FallbackFixture {{ name: {name:?}, entry: {entry:?}, files: &[{files_lit}], externs: &[{externs_lit}], extern_replay_jsonl: {extern_replay_jsonl:?}, boxed: fixture_{m}::run_with_host }},\n"
        ));
    }
    fallback_table.push_str("];\n");

    let mut hybrid_table = String::from(
        "/// Function-level hybrid fixtures (interp vs boxed vs hybrid).\npub static HYBRID_FIXTURES: &[HybridFixture] = &[\n",
    );
    for (name, entry, files) in HYBRID_FIXTURES {
        let unit = resolve_fallback_fixture(name, entry, files, &[]);
        let checked = checked_of(name, &unit);
        let lowered = topaz_lower::lower_checked(&unit, &checked)
            .unwrap_or_else(|error| panic!("hybrid fixture `{name}` failed to lower: {error}"));
        let input = NativeInput { unit: &lowered };
        assert!(
            topaz_emit::emit_native_items(&input).is_err(),
            "hybrid fixture `{name}` must first decline whole-unit native"
        );
        let boxed = topaz_emit::emit_module(&lowered)
            .unwrap_or_else(|e| panic!("hybrid fixture `{name}` boxed emit failed: {e:?}"));
        let hybrid = topaz_emit::emit_native_or_hybrid(&input)
            .unwrap_or_else(|e| panic!("hybrid fixture `{name}` hybrid emit failed: {e:?}"));
        assert_eq!(
            hybrid.decision.selected_backend, "hybrid-native",
            "hybrid fixture `{name}` selected the wrong backend"
        );
        let boxed_mod = idx;
        idx += 1;
        modules.push_str(&format!(
            "#[allow(clippy::all, dead_code, unused, unreachable_code, non_snake_case, while_true)]\nmod fixture_{boxed_mod} {{\n{boxed}}}\n\n"
        ));
        let hybrid_mod = idx;
        idx += 1;
        modules.push_str(&format!(
            "#[allow(clippy::all, dead_code, unused, unreachable_code, non_snake_case, while_true)]\nmod fixture_{hybrid_mod} {{\n{hybrid_rust}}}\n\n",
            hybrid_rust = hybrid.rust,
        ));
        let files_lit = files
            .iter()
            .map(|(path, source)| format!("FixtureFile {{ path: {path:?}, source: {source:?} }}"))
            .collect::<Vec<_>>()
            .join(", ");
        hybrid_table.push_str(&format!(
            "    HybridFixture {{ name: {name:?}, entry: {entry:?}, files: &[{files_lit}], boxed: fixture_{boxed_mod}::run_with_host, hybrid: fixture_{hybrid_mod}::run_with_host }},\n"
        ));
    }
    hybrid_table.push_str("];\n");

    let generated = format!(
        "{modules}{table}{fallback_table}{hybrid_table}\n/// The number of native-eligible fixtures (3 columns each).\npub const FIXTURE_COUNT: usize = {};\n/// The number of native-REFUSAL fixtures pinned at build time (TPZ6002).\npub const REFUSAL_COUNT: usize = {refusal_count};\n/// The number of native-DECLINE fallback fixtures (run interp vs boxed).\npub const FALLBACK_COUNT: usize = {};\n/// The number of function-level hybrid fixtures.\npub const HYBRID_COUNT: usize = {};\n",
        FIXTURES.len(),
        FALLBACK_FIXTURES.len(),
        HYBRID_FIXTURES.len(),
    );
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR set by cargo");
    fs::write(Path::new(&out_dir).join("fixtures.rs"), generated)
        .expect("write generated native fixtures");
}
