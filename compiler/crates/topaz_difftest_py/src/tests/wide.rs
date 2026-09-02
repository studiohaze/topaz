use super::*;

#[test]
fn final_float_literals_trace_f64_bits_match_interpreter() {
    let python = cpython_31314();
    let tmp = temp_dir("topaz-difftest-py-floats");
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");

    let cases = [
        ("positive_zero", "0.0"),
        ("negative_zero", "-0.0"),
        ("unary_plus_zero", "+0.0"),
        ("integral_display_domain", "2.0"),
        ("below_1e15", "999999999999999.0"),
        ("at_1e15", "1000000000000000.0"),
        ("above_1e15", "1000000000000001.0"),
        ("round_trip_hard", "0.1"),
        ("large_integral_positional", "10000000000000000.0"),
        ("tiny_decimal", "0.00001"),
    ];

    for (name, source) in cases {
        let mut provider = InMemoryProvider::new();
        provider.add_file("main.tpz", source);
        let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::CURRENT);
        assert!(
            unit.diagnostics.is_empty(),
            "{name}: fixture must resolve cleanly: {:?}",
            unit.diagnostics
        );
        let expected_bits =
            match topaz_interp::Machine::run_unit(&unit, &topaz_interp::TestHost::new())
                .unwrap_or_else(|error| panic!("{name}: interpreter faulted: {error:?}"))
            {
                Value::Float(value) => value.to_bits(),
                other => panic!("{name}: expected final float value, got {other:?}"),
            };

        let script = tmp.join(format!("{name}.py"));
        let generated = emit_module(&unit).expect("float fixture emits to Python");
        fs::write(&script, generated).expect("write generated Python");
        let py_case = Case {
            name: name.to_string(),
            input: String::new(),
        };
        let traces = run_python_batch(&python, &tmp, &script, &[py_case])
            .unwrap_or_else(|error| panic!("{name}: run Python batch: {error}"));
        let trace = traces.get(name).expect("trace for case");
        assert_eq!(trace.status, "ok", "{name}: status");
        assert!(trace.stdout.is_empty(), "{name}: stdout");
        assert_eq!(
            trace.value,
            Some(TraceValue::F64(expected_bits)),
            "{name}: Python f64 trace should match interpreter bits"
        );
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn result_try_trace_values_match_interpreter() {
    let cases = [
        (
            "result_ok_unwrap",
            r#"
function addOne(r: Result<int, string>) -> Result<int, string> {
    let n = r?
    Ok(n + 1)
}
addOne(Ok(41))
"#,
        ),
        (
            "result_err_propagate",
            r#"
function addOne(r: Result<int, string>) -> Result<int, string> {
    let n = r?
    Ok(n + 1)
}
addOne(Err("boom"))
"#,
        ),
        (
            "result_match_ok",
            r#"
function normalize(r: Result<int, string>) -> Result<int, string> {
    match r {
        case Ok(n) => Ok(n + 1)
        case Err(e) => Err(e)
    }
}
normalize(Ok(4))
"#,
        ),
        (
            "result_match_err",
            r#"
function normalize(r: Result<int, string>) -> Result<int, string> {
    match r {
        case Ok(n) => Ok(n + 1)
        case Err(e) => Err(e)
    }
}
normalize(Err("bad"))
"#,
        ),
    ];
    run_value_trace_cases("result", &cases);
}

#[test]
fn structured_fault_trace_matches_interpreter_span_for_try_guard() {
    let source = r#"
function bad() -> int {
    let n = 1?
    n
}
bad()
"#;
    let python = cpython_31314();
    let tmp = temp_dir("topaz-difftest-py-fault");
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");

    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", source);
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::CURRENT);
    assert!(
        unit.diagnostics.is_empty(),
        "fault fixture must resolve cleanly: {:?}",
        unit.diagnostics
    );
    let expected = topaz_interp::Machine::run_unit(&unit, &topaz_interp::TestHost::new())
        .expect_err("interpreter should fault on non-Result try");

    let script = tmp.join("fault_span_helper.py");
    fs::write(
        &script,
        emit_module(&unit).expect("fault fixture emits to Python"),
    )
    .expect("write generated Python");
    let py_case = Case {
        name: "fault_span_helper".to_string(),
        input: String::new(),
    };
    let traces =
        run_python_batch(&python, &tmp, &script, &[py_case]).expect("run Python fault fixture");
    let trace = traces.get("fault_span_helper").expect("trace");
    assert_eq!(trace.status, "fault");
    let fault = trace.fault.as_ref().expect("structured fault");
    assert_eq!(fault.code, expected.code);
    assert_eq!(fault.message, expected.message);
    assert_eq!(fault.span.file, expected.span.file.0 as i64);
    assert_eq!(fault.span.lo, expected.span.lo as i64);
    assert_eq!(fault.span.hi, expected.span.hi as i64);
    assert!(trace.stdout.is_empty());
    assert!(trace.files.is_empty());
    assert!(trace.defer_errors.is_empty());
    assert!(trace.value.is_none());
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn byte_buffer_v56_success_and_fault_match_python() {
    let python = cpython_31314();
    let tmp = temp_dir("topaz-difftest-py-byte-buffer-v56");
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");

    let cases = [
        (
            "byte_buffer_success",
            "let mut buffer = ByteBuffer.allocate(6, 1)\nbuffer.set(0, 9)\nbuffer.set(1, 8)\nlet mut alias = buffer\nalias.copy(alias, 0, 2, 4)\nlet snapshot = alias.toBytes()\nlet mut copied = ByteBuffer.fromBytes(snapshot)\ncopied.set(0, 7)\nalias.toBytes().toHex() + \"|\" + copied.toBytes().toHex()",
        ),
        (
            "byte_buffer_fault",
            "let mut buffer = ByteBuffer.allocate(4, 1)\nbuffer.fill(1, 4, 9)",
        ),
    ];

    for (name, source) in cases {
        let mut provider = InMemoryProvider::new();
        provider.add_file("main.tpz", source);
        let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_6);
        assert!(
            unit.diagnostics.is_empty(),
            "{name}: fixture must resolve cleanly: {:?}",
            unit.diagnostics
        );
        let interpreter = topaz_interp::Machine::run_unit(&unit, &topaz_interp::TestHost::new());
        let script = tmp.join(format!("{name}.py"));
        fs::write(
            &script,
            emit_module(&unit).expect("ByteBuffer emits to Python"),
        )
        .expect("write generated Python");
        let traces = run_python_batch(
            &python,
            &tmp,
            &script,
            &[Case {
                name: name.to_string(),
                input: String::new(),
            }],
        )
        .unwrap_or_else(|error| panic!("{name}: run Python: {error}"));
        let trace = traces.get(name).expect("trace");
        match interpreter {
            Ok(Value::Str(value)) => {
                assert_eq!(trace.status, "ok", "{name}");
                assert_eq!(
                    trace.value,
                    Some(TraceValue::Str(value.to_string())),
                    "{name}"
                );
            }
            Err(expected) => {
                assert_eq!(trace.status, "fault", "{name}");
                let fault = trace.fault.as_ref().expect("structured fault");
                assert_eq!(fault.code, expected.code, "{name}");
                assert_eq!(fault.message, expected.message, "{name}");
                assert_eq!(fault.span.file, expected.span.file.0 as i64, "{name}");
                assert_eq!(fault.span.lo, expected.span.lo as i64, "{name}");
                assert_eq!(fault.span.hi, expected.span.hi as i64, "{name}");
            }
            Ok(other) => panic!("{name}: unexpected interpreter value {other:?}"),
        }
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn defer_lifo_return_and_error_transcripts_match_interpreter() {
    let cases = [
        (
            "defer_lifo",
            r#"
function f() -> int {
    defer print("first")
    defer print("last")
    print("body")
    1
}
f()
"#,
        ),
        (
            "defer_on_return",
            r#"
function f() -> int {
    defer print("cleanup")
    return 1
}
f()
"#,
        ),
        (
            "defer_records_err",
            r#"
function fail() -> Result<int, string> { Err("boom") }
function f() -> Result<int, string> {
    defer fail()
    Ok(1)
}
f()
"#,
        ),
        (
            "defer_top_level",
            r#"
function f() -> int {
    print("main")
    1
}
defer print("end")
f()
"#,
        ),
    ];
    run_transcript_trace_cases("defer", &cases);
}

#[test]
fn virtual_file_trace_matches_interpreter_open_write_corpus() {
    let source = include_str!("../../../../corpus/exec/files/open-write.tpz");
    let python = cpython_31314();
    let tmp = temp_dir("topaz-difftest-py-files");
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");

    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", source);
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::CURRENT);
    assert!(
        unit.diagnostics.is_empty(),
        "file fixture must resolve cleanly: {:?}",
        unit.diagnostics
    );

    let mut seed_files = BTreeMap::new();
    seed_files.insert("config.txt".to_string(), "v=1".to_string());

    let interp_host = topaz_interp::TestHost::new();
    interp_host.add_file("config.txt", "v=1");
    topaz_interp::Machine::run_unit(&unit, &interp_host)
        .unwrap_or_else(|error| panic!("interpreter faulted: {error:?}"));

    let script = tmp.join("open_write.py");
    fs::write(
        &script,
        emit_module(&unit).expect("file fixture emits to Python"),
    )
    .expect("write generated Python");
    let trace = run_python_once_with_files(&python, &tmp, &script, "", &seed_files)
        .unwrap_or_else(|error| panic!("run Python file fixture: {error}"));

    assert_eq!(trace.status, "ok");
    assert_eq!(trace.stdout, interp_host.stdout(), "stdout");
    assert!(trace.defer_errors.is_empty(), "defer errors");
    assert!(trace.fault.is_none(), "fault");
    assert!(trace.value.is_none(), "trace value");
    assert_eq!(
        trace_file_string_map(&trace.files).expect("string file trace"),
        interp_host.files(),
        "final file state"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn json_core_transcripts_match_interpreter() {
    let cases = [
        (
            "json_roundtrip_canonical",
            r#"
function f() -> Result<int, string> {
    let parsed = JSON.parse("\{\"b\":2,\"a\":[true,null,\"한\"]\}")?
    let text = JSON.stringify(parsed)?
    print(text)
    Ok(0)
}
f()
"#,
        ),
        (
            "json_accessors",
            r#"
function f() -> Result<int, string> {
    let root = JSON.parse("\{\"arr\":[false],\"n\":1.0,\"s\":\"x\"\}")?
    let len = match root.length() {
        case Some(n) => n
        case None => -1
    }
    let s = match root.get("s") {
        case Some(v) => match v.asString() {
            case Some(text) => text
            case None => "bad"
        }
        case None => "missing"
    }
    let nText = match root.get("n") {
        case Some(v) => match v.numberText() {
            case Some(text) => text
            case None => "bad"
        }
        case None => "missing"
    }
    let nInt = match root.get("n") {
        case Some(v) => match v.asInt() {
            case Some(n) => n
            case None => -1
        }
        case None => -2
    }
    let b = match root.get("arr") {
        case Some(arr) => match arr.at(0) {
            case Some(v) => match v.asBool() {
                case Some(flag) => flag
                case None => true
            }
            case None => true
        }
        case None => true
    }
    print("{root.kind()}:{len}:{s}:{nText}:{nInt}:{b}")
    Ok(0)
}
f()
"#,
        ),
        (
            "json_duplicate_key_error",
            r#"
function f() -> Result<int, string> {
    match JSON.parse("\{\"a\":1,\"a\":2\}") {
        case Err(e) => print(e.message)
        case Ok(value) => print("?")
    }
    Ok(0)
}
f()
"#,
        ),
        (
            "json_external_number_vectors",
            r#"
function numberSlot(root: JSONValue, index: int) -> string {
    let value = match root.at(index) {
        case Some(v) => v
        case None => return "missing"
    }
    let text = match value.numberText() {
        case Some(t) => t
        case None => "no-text"
    }
    let intText = match value.asInt() {
        case Some(n) => "{n}"
        case None => "none"
    }
    "{text}/{intText}"
}

function hugeExponent(prefix: string) -> string {
    let mut text = prefix
    for i in 0..5000 {
        text = "{text}9"
    }
    text
}

function zeroI128OverflowExponent() -> string {
    "0e9999999999999999999999999999999999999999"
}

function i128MaxExponent(prefix: string) -> string {
    "{prefix}170141183460469231731687303715884105727"
}

function f() -> Result<string, string> {
    let root = match JSON.parse("[-0,9223372036854775807,9223372036854775808,1e2,100e-2,1.50]") {
        case Ok(value) => value
        case Err(e) => return Err(e.message)
    }
    let huge = match JSON.parse(hugeExponent("1e")) {
        case Ok(value) => value
        case Err(e) => return Err(e.message)
    }
    let zeroHuge = match JSON.parse(zeroI128OverflowExponent()) {
        case Ok(value) => value
        case Err(e) => return Err(e.message)
    }
    let atI128Max = match JSON.parse(i128MaxExponent("1e")) {
        case Ok(value) => value
        case Err(e) => return Err(e.message)
    }
    let zeroAtI128Max = match JSON.parse(i128MaxExponent("0e")) {
        case Ok(value) => value
        case Err(e) => return Err(e.message)
    }
    let hugeInt = match huge.asInt() {
        case Some(n) => "{n}"
        case None => "none"
    }
    let zeroHugeInt = match zeroHuge.asInt() {
        case Some(n) => "{n}"
        case None => "none"
    }
    let atI128MaxInt = match atI128Max.asInt() {
        case Some(n) => "{n}"
        case None => "none"
    }
    let zeroAtI128MaxInt = match zeroAtI128Max.asInt() {
        case Some(n) => "{n}"
        case None => "none"
    }
    Ok("{numberSlot(root, 0)}|{numberSlot(root, 1)}|{numberSlot(root, 2)}|{numberSlot(root, 3)}|{numberSlot(root, 4)}|{numberSlot(root, 5)}|huge/{hugeInt}|zeroHuge/{zeroHugeInt}|atI128Max/{atI128MaxInt}|zeroAtI128Max/{zeroAtI128MaxInt}")
}
f()
"#,
        ),
        (
            "json_external_parse_error_vectors",
            r#"
function parseError(text: string) -> string {
    match JSON.parse(text) {
        case Err(e) => "{e.line}:{e.column}:{e.message}"
        case Ok(value) => "ok"
    }
}

function f() -> string {
    let trailingComma = parseError("[1,]")
    let unterminated = parseError("\"abc")
    let highSurrogate = parseError("\"\\uD800\"")
    let lowSurrogate = parseError("\"\\uDC00\"")
    let leadingZero = parseError("01")
    let missingExponent = parseError("1e")
    let escapedDuplicate = parseError("\{\"A\":1,\"\\u0041\":2\}")
    let nonAsciiDigit = parseError("١")
    let superscriptDigit = parseError("1²")
    "{trailingComma}|{unterminated}|{highSurrogate}|{lowSurrogate}|{leadingZero}|{missingExponent}|{escapedDuplicate}|{nonAsciiDigit}|{superscriptDigit}"
}
f()
"#,
        ),
        (
            "json_external_escape_vectors",
            r#"
function f() -> Result<string, string> {
    let root = match JSON.parse("[\"\\b\",\"\\f\",\"\\n\",\"\\r\",\"\\t\",\"\\/\",\"\\\\\",\"\\uD834\\uDD1E\"]") {
        case Ok(value) => value
        case Err(e) => return Err(e.message)
    }
    let text = JSON.stringify(root)?
    print(text)
    Ok(text)
}
f()
"#,
        ),
        (
            "json_stringify_option_boundaries",
            r#"
function f() -> Result<string, string> {
    let one: Option<int> = Some(1)
    let none: Option<int> = None
    let nested: Option<Option<string>> = Some(Some("x"))
    let a = JSON.stringify(one)?
    let b = JSON.stringify(none)?
    let c = JSON.stringify(nested)?
    Ok("{a}|{b}|{c}")
}
f()
"#,
        ),
        (
            "json_server_boundary_request_vectors",
            r#"
function jsonString(root: JSONValue, key: string) -> Result<string, string> {
    let value = match root.get(key) {
        case Some(found) => found
        case None => return Err("missing {key}")
    }
    match value.asString() {
        case Some(text) => Ok(text)
        case None => Err("{key} must be string")
    }
}

function render(status: int, body: string) -> Result<string, string> {
    let raw = "\{\"body\":\"{body}\",\"status\":{status}\}"
    let parsed = match JSON.parse(raw) {
        case Ok(value) => value
        case Err(e) => return Err(e.message)
    }
    JSON.stringify(parsed)
}

function handle(text: string) -> Result<string, string> {
    let root = match JSON.parse(text) {
        case Ok(value) => value
        case Err(e) => return Err(e.message)
    }
    let path = jsonString(root, "path")?
    if path == "/health" {
        render(200, "ok")
    } else {
        render(404, path)
    }
}

function f() -> string {
    let ok = match handle("\{\"method\":\"GET\",\"path\":\"/health\"\}") {
        case Ok(value) => value
        case Err(e) => e
    }
    let missing = match handle("\{\"method\":\"GET\"\}") {
        case Ok(value) => value
        case Err(e) => e
    }
    "{ok}|{missing}"
}
f()
"#,
        ),
        (
            "json_float_stringify_error",
            r#"
function f() -> Result<int, string> {
    match JSON.stringify(1.5) {
        case Err(e) => print(e)
        case Ok(value) => print(value)
    }
    Ok(0)
}
f()
"#,
        ),
    ];
    run_transcript_trace_cases("json", &cases);
}

#[test]
fn bytes_encoding_transcripts_match_interpreter() {
    let cases = [
        (
            "bytes_value_trace_and_accessors",
            r#"
function f() -> Bytes {
    let tail = match Bytes.fromArray([90]) {
        case Ok(v) => v
        case Err(e) => Bytes.empty()
    }
    let b = Bytes.concat(Bytes.encodeUtf8("A"), tail)
    let first = match b.get(0) {
        case Some(n) => n
        case None => -1
    }
    let missing = match b.get(9) {
        case Some(n) => n
        case None => -1
    }
    let arr = b.toArray()
    let arr1 = match arr.get(1) {
        case Some(n) => n
        case None => -1
    }
    print("{b.toHex()}:{b.toBase64()}:{b.length()}:{b.isEmpty()}:{first}:{missing}:{arr1}")
    b
}
f()
"#,
        ),
        (
            "bytes_rfc4648_vectors",
            r#"
function f() -> Result<int, string> {
    let v0 = Bytes.encodeUtf8("").toBase64()
    let v1 = Bytes.encodeUtf8("f").toBase64()
    let v2 = Bytes.encodeUtf8("fo").toBase64()
    let v3 = Bytes.encodeUtf8("foo").toBase64()
    let v4 = Bytes.encodeUtf8("foob").toBase64()
    let v5 = Bytes.encodeUtf8("fooba").toBase64()
    let v6 = Bytes.encodeUtf8("foobar").toBase64()
    print("{v0}:{v1}:{v2}:{v3}:{v4}:{v5}:{v6}")
    Ok(0)
}
f()
"#,
        ),
        (
            "bytes_error_boundaries",
            r#"
function bytesResult(value: Result<Bytes, string>) -> string {
    match value {
        case Ok(b) => b.toHex()
        case Err(e) => e
    }
}

function textResult(value: Result<string, string>) -> string {
    match value {
        case Ok(s) => s
        case Err(e) => e
    }
}

function f() -> Result<int, string> {
    let badUtf8 = match Bytes.fromHex("ff") {
        case Ok(b) => textResult(b.decodeUtf8())
        case Err(e) => e
    }
    let hexOk = bytesResult(Bytes.fromHex("DEADbeef"))
    let hexOdd = bytesResult(Bytes.fromHex("abc"))
    let hexBad = bytesResult(Bytes.fromHex("zz"))
    let b64Len = bytesResult(Bytes.fromBase64("Zg="))
    let b64Char = bytesResult(Bytes.fromBase64("@@@@"))
    let b64Pad = bytesResult(Bytes.fromBase64("Z==="))
    let b64Bits = bytesResult(Bytes.fromBase64("Zh=="))
    print("{hexOk}|{hexOdd}|{hexBad}|{b64Len}|{b64Char}|{b64Pad}|{b64Bits}|{badUtf8}")
    Ok(0)
}
f()
"#,
        ),
        (
            "encoding_facade_roundtrip",
            r#"
function textResult(value: Result<string, string>) -> string {
    match value {
        case Ok(s) => s
        case Err(e) => e
    }
}

function f() -> Result<int, string> {
    let decoded = match Encoding.base64Decode(Encoding.base64Encode(Encoding.utf8Encode("hé"))) {
        case Ok(b) => textResult(Encoding.utf8Decode(b))
        case Err(e) => e
    }
    let hexed = Encoding.hexEncode(Encoding.utf8Encode("topaz"))
    let fromHex = match Encoding.hexDecode("6869") {
        case Ok(b) => Encoding.base64Encode(b)
        case Err(e) => e
    }
    print("{hexed}:{fromHex}:{decoded}")
    Ok(0)
}
f()
"#,
        ),
        (
            "bytes_slice_clamp",
            r#"
function f() -> Bytes {
    let b = Bytes.encodeUtf8("foobar")
    print("{b.slice(1, 3).toHex()}:{b.slice(0, 99).toHex()}:{b.slice(4, 2).toHex()}")
    b.slice(4, 2)
}
f()
"#,
        ),
    ];
    run_transcript_trace_cases("bytes", &cases);
}

#[test]
fn map_set_transcripts_match_interpreter() {
    let cases = [
        (
            "map_snapshot_order_and_trace",
            r#"
function f() -> Map<Array<int>, string> {
    let mut key = [1, 2]
    let mut m = Map.new<Array<int>, string>()
    m.insert(key, "hit")
    key.push(3)
    m.insert([9], "nine")
    m.insert([1, 2], "updated")
    let oldKey = m.getOr([1, 2], "missing")
    let mutatedKey = m.getOr(key, "missing")
    print("{m.length}:{m.keys.length}:{oldKey}:{mutatedKey}:{m.containsKey([9])}")
    m
}
f()
"#,
        ),
        (
            "map_bytes_key_and_literal_render",
            r#"
function f() -> Map<Bytes, int> {
    let mut m = map {
        Bytes.encodeUtf8("b"): 2,
        Bytes.encodeUtf8("a"): 1,
    }
    m.insert(Bytes.encodeUtf8("b"), 20)
    let bValue = m.getOr(Bytes.encodeUtf8("b"), 0)
    print("{m}:{bValue}:{m.values.length}")
    m
}
f()
"#,
        ),
        (
            "set_dedupe_order_and_algebra",
            r#"
function f() -> Set<int> {
    let a = Set.of(2, 1, 2)
    let b = Set.of(1, 3)
    let u = a.union(b)
    let i = a.intersection(b)
    let d = a.difference(b)
    print("{a.length}:{a.contains(2)}:{a.contains(9)}:{u.toArray()}:{i.toArray()}:{d.toArray()}")
    u
}
f()
"#,
        ),
    ];
    run_transcript_trace_cases("map_set", &cases);
}

#[test]
fn checker_probe_records_generic_map_equality_runtime_guard_path() {
    let mut provider = InMemoryProvider::new();
    provider.add_file(
        "main.tpz",
        r#"
function same<T>(a: T, b: T) -> bool {
    a == b
}
function main() -> bool {
    let m = Map.new<string, int>()
    same(m, m)
}
main()
"#,
    );
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::CURRENT);
    assert!(
        unit.diagnostics.is_empty(),
        "if the checker now rejects generic Map equality, remove the runtime-guard fixture: {:?}",
        unit.diagnostics
    );
}

#[test]
fn std_map_and_set_are_shared_resolve_errors_not_python_surface() {
    for module in ["std.map", "std.set"] {
        let mut provider = InMemoryProvider::new();
        provider.add_file("main.tpz", format!("import {module}\n0\n"));
        let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::CURRENT);
        assert!(
            !unit.diagnostics.is_empty(),
            "{module} must stay outside the accepted Topaz surface"
        );
        let rendered = format!("{:?}", unit.diagnostics);
        assert!(
            rendered.contains(module) || rendered.contains("std"),
            "{module} should fail as a shared resolver error, not a Python decline: {rendered}"
        );
    }
}

#[test]
fn wide_core_python_corpus_matches_interpreter_and_boxed_rust() {
    assert_eq!(
        WIDE_CORE_FIXTURES.len(),
        PYTHON_WIDE_CORE_FIXTURE_COUNT,
        "Python wide core corpus count drifted"
    );

    let python = cpython_31314();
    let tmp = temp_dir("topaz-difftest-py-wide-core");
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");

    let mut failures = Vec::new();
    for fixture in WIDE_CORE_FIXTURES {
        let interp = match run_wide_interpreter(fixture) {
            Ok(receipt) => receipt,
            Err(error) => {
                failures.push(format!(
                    "{}: interpreter setup failed: {error}",
                    fixture.name
                ));
                continue;
            }
        };
        let rust = run_wide_boxed_rust(fixture);
        compare_wide_receipts(fixture.name, &interp, &rust, &mut failures);

        match run_wide_python(&python, &tmp, fixture) {
            Ok(trace) => {
                compare_python_trace_to_wide_receipt(fixture, &trace, &interp, &mut failures);
            }
            Err(error) => failures.push(format!("{}: Python failed: {error}", fixture.name)),
        }
    }

    let _ = fs::remove_dir_all(&tmp);
    assert!(
        failures.is_empty(),
        "wide Python core corpus mismatches ({} of {}):\n{}",
        failures.len(),
        WIDE_CORE_FIXTURES.len(),
        failures.join("\n")
    );
}
