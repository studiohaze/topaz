use super::*;

#[test]
fn emits_free_builtin_named_calls_without_reordering_effects() {
    let generated = emit_source(
        r#"
function mark(label: string, n: int) -> int {
    print(label)
    n
}
function add(acc: int, x: int) -> int {
    acc + x
}
function double(x: int) -> int {
    x * 2
}
function main() -> int {
    print(value: "start")
    let xs = map(f: double, xs: [mark("xs", 1), 2])
    let ys = filter(xs: xs, f: (x) => x > 2)
    let total = reduce(f: add, initial: mark("init", 0), xs: ys)
    match toInt(text: "40") {
        case Some(n) => total + n
        case None => total
    }
}
main()
"#,
    );
    assert!(generated.contains("host.print(\"start\","), "{generated}");
    assert!(
        generated.contains("(lambda __tpz_call_arg_0, __tpz_call_arg_1"),
        "{generated}"
    );
    assert!(
        generated
            .contains("tpz_array_reduce(__tpz_call_arg_2, __tpz_call_arg_1, __tpz_call_arg_0,"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("free builtin named call Python gate failed: {e}"));
}

#[test]
fn emits_from_code_point_through_the_pinned_python_runtime_leaf() {
    let generated = emit_source(
        r#"
function main() -> string {
    fromCodePoint(n: 128512) ?? ""
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_from_code_point(128512,"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("fromCodePoint Python gate failed: {e}"));
}

#[test]
fn emits_namespace_builtin_named_calls_without_reordering_effects() {
    let generated = emit_source(
        r#"
function make(label: string, text: string) -> Bytes {
    print(label)
    Bytes.encodeUtf8(s: text)
}
function main() -> Result<string, string> {
    let parsed = JSON.parse(text: "\{\"b\":2,\"a\":1\}")?
    let canonical = JSON.stringify(value: parsed)?
    let joined = Bytes.concat(b: make("b", "B"), a: make("a", "A"))
    let hex = Encoding.hexEncode(bytes: joined)
    let decoded = Encoding.utf8Decode(bytes: Encoding.hexDecode(text: hex)?)?
    Ok(value: "{canonical}:{decoded}")
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_json_parse(\"{\\\"b\\\":2,\\\"a\\\":1}\","),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_bytes_concat(__tpz_call_arg_1, __tpz_call_arg_0,"),
        "{generated}"
    );
    assert!(
        generated.contains("(lambda __tpz_call_arg_0, __tpz_call_arg_1"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("namespace builtin named call Python gate failed: {e}"));
}

#[test]
fn emits_fs_namespace_named_calls_through_host_helpers() {
    let generated = emit_source(
        r#"
function mark(label: string, text: string) -> string {
    print(label)
    text
}
function makeBytes(label: string, text: string) -> Bytes {
    print(label)
    Bytes.encodeUtf8(text)
}
function main() -> Result<string, string> {
    FS.writeBytes(bytes: makeBytes("bytes", "AB"), path: mark("path", "new.txt"))?
    let read = FS.readBytes(path: "new.txt")?
    let text = read.decodeUtf8()?
    let listed = FS.list(path: ".")?
    let item = match listed.get(0) {
        case Some(e) => "{e.name}:{e.kind}:{e.sizeBytes}"
        case None => "missing"
    }
    Ok("{text}:{item}")
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_fs_write_bytes(host, __tpz_call_arg_1, __tpz_call_arg_0,"),
        "{generated}"
    );
    assert!(
        generated.contains("(lambda __tpz_call_arg_0, __tpz_call_arg_1"),
        "{generated}"
    );
    assert!(generated.contains("tpz_fs_read_bytes(host,"), "{generated}");
    assert!(generated.contains("tpz_fs_list(host,"), "{generated}");
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("FS namespace Python gate failed: {e}"));
}

#[test]
fn emits_variadic_namespace_constructor_spread_calls() {
    let generated = emit_source(
        r#"
function main() -> string {
    let xs = Array.of(0, ...[1, 2], 3)
    let dupes = [2, 1, 2]
    let mut total = 0
    for x in Set.of(...dupes, 3) {
        total = total + x
    }
    "{xs.length}:{xs[0]}:{xs[3]}:{total}"
}
main()
"#,
    );
    assert!(
        generated.contains("[0, *tpz_spread_values([1, 2],"),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_set_of([*tpz_spread_values(_t_6475706573,"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("namespace constructor spread Python gate failed: {e}"));
}

#[test]
fn emits_array_literal_spread_through_checked_helper() {
    let generated = emit_source(
        r#"
function main() -> Array<int> {
    let xs = [1, 2]
    [0, ...xs, 3]
}
main()
"#,
    );
    assert!(
        generated.contains("[0, *tpz_spread_values(_t_7873,"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("array literal spread Python gate failed: {e}"));

    let statement_lowered = emit_source(
        r#"
function main() -> Array<int> {
    let xs = [1, 2]
    [0, ...loop {
        print("spread")
        break xs
    }, 3]
}
main()
"#,
    );
    assert!(
        statement_lowered.contains("tpz_spread_values(__tpz_expr_value_"),
        "{statement_lowered}"
    );
    assert!(
        statement_lowered.contains("raise TpzLoopBreak(None, _t_7873)"),
        "{statement_lowered}"
    );
    assert_generated_python_gates(&statement_lowered)
        .unwrap_or_else(|e| panic!("statement-lowered array spread Python gate failed: {e}"));
}

#[test]
fn emits_statementful_block_expression_to_target() {
    let generated = emit_source(
        r#"
function main() -> string {
    let value = {
        let mut n = 1
        n += 2
        let text = loop {
            print("block")
            break "{n}"
        }
        "v:{text}"
    }
    value
}
main()
"#,
    );
    assert!(
        generated.contains("_t_74657874 = __tpz_loop_break_")
            && generated.contains("raise TpzLoopBreak"),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_add(__tpz_assign_current_"),
        "block-local compound assignment should use the shared add helper: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("statementful block expression Python gate failed: {e}"));
}

#[test]
fn unannotated_function_array_literal_tail_return_shape_supports_named_array_get() {
    let generated = emit_source(
        r#"
function make() {
    ([4, 5])
}
function main() -> Option<int> {
    let xs = make()
    xs.get(i: 1)
}
main()
"#,
    );
    assert!(generated.contains("tpz_get(_t_7873, 1,"), "{generated}");
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("unannotated array tail return-shape Python gate failed: {e}"));
}

#[test]
fn direct_literal_tail_return_shape_recognizes_string_template_and_map() {
    let plain_string = Expr {
        kind: ExprKind::String(
            StringLit {
                tag: None,
                multiline: false,
                parts: vec![],
                span: SP,
            }
            .into(),
        ),
        span: SP,
    };
    let tagged_string = Expr {
        kind: ExprKind::String(
            StringLit {
                tag: Some(SP),
                multiline: true,
                parts: vec![],
                span: SP,
            }
            .into(),
        ),
        span: SP,
    };
    let map_literal = Expr {
        kind: ExprKind::MapLiteral(vec![]),
        span: SP,
    };
    let parenthesized_map = Expr {
        kind: ExprKind::Paren(Rc::new(map_literal.clone())),
        span: SP,
    };

    assert_eq!(
        direct_tail_expr_return_shape(&plain_string),
        Some(ReceiverShape::String)
    );
    assert_eq!(
        direct_tail_expr_return_shape(&tagged_string),
        Some(ReceiverShape::Template)
    );
    assert_eq!(
        direct_tail_expr_return_shape(&map_literal),
        Some(ReceiverShape::Map)
    );
    assert_eq!(
        direct_tail_expr_return_shape(&parenthesized_map),
        Some(ReceiverShape::Map)
    );
}

#[test]
fn tagged_templates_emit_runtime_leaf() {
    let generated = emit_source(
        r#"
function main() -> int {
    let segment = "a\\b"
    let path = p"root\\{segment}\\file"
    if "{path}" == "root/a/b/file" { 1 } else { 0 }
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_make_template(\"p\","),
        "tagged template should lower through the runtime leaf: {generated}"
    );
    assert_generated_python_ok_int(&generated, 1, "tagged template runtime leaf");
}

#[test]
fn tagged_templates_emit_registry_tags_and_multiline_parts() {
    let generated = emit_source(
        r#"
function main() -> int {
    let flag = "yes"
    let pattern = r"\\d+{flag}"
    let command = sh"echo {flag}"
    let query = sql"""
select {flag}
from t
"""
    if pattern.tag == "r"
        && command.tag == "sh"
        && query.tag == "sql"
        && pattern.parts.length == 2
        && command.parts.length == 2
        && query.parts.length == 2 { 1 } else { 0 }
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_make_template(\"r\",")
            && generated.contains("tpz_make_template(\"sh\",")
            && generated.contains("tpz_make_template(\"sql\","),
        "every canonical registry tag should lower through the runtime leaf: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        1,
        "tagged template registry and multiline parts",
    );
}

#[test]
fn tagged_templates_preserve_statementful_order_and_fresh_parts() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut order = 0

    function tick(label: int, value: int) -> int {
        order = order * 10 + label
        value
    }

    let template = sh"a {tick(1, 3)} b {loop { break tick(2, 5) }} c"
    let mut first: Array<string> = template.parts
    first.push("extra")
    let second = template.parts
    order * 100 + first.length * 10 + second.length
}
main()
"#,
    );
    let first_value = generated
        .find("_t_7469636b(1, 3)")
        .unwrap_or_else(|| panic!("missing first template interpolation: {generated}"));
    let second_value = generated
        .find("_t_7469636b(2, 5)")
        .unwrap_or_else(|| panic!("missing second template interpolation: {generated}"));
    assert!(
        first_value < second_value,
        "statementful template interpolations should remain left-to-right: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        1243,
        "statementful tagged template order and fresh parts",
    );
}

#[test]
fn tagged_template_final_values_stay_out_of_trace_v1() {
    let fixtures = [
        ("direct", "sql\"\"\"select 1\"\"\"\n"),
        (
            "function return",
            "function make() { sql\"\"\"select 1\"\"\" }\nmake()\n",
        ),
        (
            "statementful function return",
            "function make() { print(\"made\"); sql\"\"\"select 1\"\"\" }\nmake()\n",
        ),
        (
            "local binding",
            "let value = sql\"\"\"select 1\"\"\"\nvalue\n",
        ),
    ];

    for (name, source) in fixtures {
        let generated = emit_source(source);
        assert!(
            generated.contains("return host.trace_ok()")
                && !generated.contains("return host.trace_ok(__tpz_value)"),
            "{name} template final values are outside trace v1: {generated}"
        );
        assert_generated_python_gates(&generated)
            .unwrap_or_else(|e| panic!("{name} final-template Python gate failed: {e}"));
    }
}

#[test]
fn direct_namespace_builtin_tail_return_shape_recognizes_bytes_map_only() {
    assert_eq!(
        direct_tail_shape_for_function(
            r#"
function make() {
    Bytes.empty()
}
"#,
            "make",
        ),
        Some(ReceiverShape::Bytes)
    );
    assert_eq!(
        direct_tail_shape_for_function(
            r#"
function make() {
    Bytes.concat(Bytes.empty(), Bytes.encodeUtf8("x"))
}
"#,
            "make",
        ),
        Some(ReceiverShape::Bytes)
    );
    assert_eq!(
        direct_tail_shape_for_function(
            r#"
function make() {
    (Encoding.utf8Encode(text: "x"))
}
"#,
            "make",
        ),
        Some(ReceiverShape::Bytes)
    );
    assert_eq!(
        direct_tail_shape_for_function(
            r#"
function make() {
    Map.ofEntries(entries: [])
}
"#,
            "make",
        ),
        Some(ReceiverShape::Map)
    );
    assert_eq!(
        direct_tail_shape_for_function(
            r#"
function make() {
    Map.new()
}
"#,
            "make",
        ),
        Some(ReceiverShape::Map)
    );
}

#[test]
fn direct_namespace_builtin_tail_return_shape_recognizes_result_metadata() {
    for (source, ok_shape) in [
        (
            r#"
function make() {
    JSON.parse(text: "null")
}
"#,
            ReceiverShape::Json,
        ),
        (
            r#"
function make() {
    JSON.parse("null")
}
"#,
            ReceiverShape::Json,
        ),
        (
            r#"
function make() {
    Bytes.fromHex(s: "41")
}
"#,
            ReceiverShape::Bytes,
        ),
        (
            r#"
function make() {
    Bytes.fromArray(values: [65])
}
"#,
            ReceiverShape::Bytes,
        ),
        (
            r#"
function make() {
    Bytes.fromBase64(text: "QQ==")
}
"#,
            ReceiverShape::Bytes,
        ),
        (
            r#"
function make() {
    Encoding.hexDecode(text: "41")
}
"#,
            ReceiverShape::Bytes,
        ),
        (
            r#"
function make() {
    Encoding.base64Decode(text: "QQ==")
}
"#,
            ReceiverShape::Bytes,
        ),
        (
            r#"
function make() {
    Encoding.utf8Decode(bytes: Bytes.empty())
}
"#,
            ReceiverShape::String,
        ),
    ] {
        assert_eq!(
            direct_tail_metadata_for_function(source, "make"),
            DirectTailMetadata {
                return_shape: Some(ReceiverShape::Result),
                result_ok_shape: Some(ok_shape),
            },
            "{source}"
        );
    }
}

#[test]
fn direct_namespace_builtin_tail_return_shape_excludes_shadowed_and_indirect_calls() {
    for source in [
        r#"
function make(Bytes: int) {
    Bytes.encodeUtf8(s: "x")
}
"#,
        r#"
let Bytes = 5
function make() {
    Bytes.encodeUtf8(s: "x")
}
"#,
        r#"
let JSON = 5
function make() {
    JSON.parse(text: "null")
}
"#,
        r#"
function make(JSON: string) {
    JSON.parse(text: "null")
}
"#,
        r#"
let Encoding = 5
function make() {
    Encoding.hexDecode(text: "41")
}
"#,
        r#"
function make(Encoding: string) {
    Encoding.utf8Decode(bytes: Bytes.empty())
}
"#,
        r#"
function make() {
    Bytes.fromHex(s: "41")?
}
"#,
        r#"
function make() {
    Map.new(x: 1)
}
"#,
        r#"
function bytes() -> Bytes {
    Bytes.empty()
}
function make() {
    bytes()
}
"#,
        r#"
function make() {
    "null" |> JSON.parse()
}
"#,
        r#"
function make(flag: bool) {
    if flag {
        JSON.parse("null")
    } else {
        JSON.parse("true")
    }
}
"#,
        r#"
function make() {
    let text = "null"
    JSON.parse(text)
}
"#,
    ] {
        assert_eq!(
            direct_tail_metadata_for_function(source, "make"),
            DirectTailMetadata::default(),
            "{source}"
        );
    }
}

#[test]
fn unannotated_function_result_namespace_tail_return_shape_supports_receivers() {
    let generated = emit_source(
        r#"
function makeJson() {
    JSON.parse(text: "\{\"s\":\"AZ\"\}")
}
function makeBytes() {
    Bytes.fromHex("4142")
}
function makeText() {
    Encoding.utf8Decode(Bytes.encodeUtf8("AZ"))
}
function jsonKind(value: JSONValue) -> string {
    value.kind()
}
function main() -> Result<int, string> {
    let mapped = makeJson().map(jsonKind)
    let base = match mapped {
        case Ok(kind) => kind.byteLength()
        case Err(e) => e.message.byteLength()
    }
    let bytes = makeBytes()?
    let text = makeText()?
    Ok(base + bytes.toHex().byteLength() + text.byteLength())
}
main()
"#,
    );
    assert!(generated.contains("tpz_result_map("), "{generated}");
    assert!(generated.contains("tpz_json_kind("), "{generated}");
    assert!(generated.contains("tpz_bytes_from_hex("), "{generated}");
    assert!(generated.contains("tpz_bytes_decode_utf8("), "{generated}");
    assert!(generated.contains("tpz_bytes_to_hex("), "{generated}");
    assert!(generated.contains("tpz_string_byte_length("), "{generated}");
    assert_generated_python_gates(&generated).unwrap_or_else(|e| {
        panic!("unannotated result namespace tail metadata Python gate failed: {e}")
    });
}

#[test]
fn unannotated_function_string_map_literal_tail_return_shape_supports_named_receivers() {
    let generated = emit_source(
        r#"
function makeText() {
    ("abc")
}
function makeMap() {
    (map { "x": 4, "fallback": 9 })
}
function main() -> int {
    let text = makeText()
    let values = makeMap()
    let bonus = if values.containsKey(k: "x") { 10 } else { 0 }
    text.byteLength() + values.getOr(k: "x", default: 0) + bonus
}
main()
"#,
    );
    assert!(generated.contains("tpz_string_byte_length("), "{generated}");
    assert!(generated.contains("tpz_map_contains_key("), "{generated}");
    assert!(generated.contains("tpz_map_get_or("), "{generated}");
    assert_generated_python_gates(&generated).unwrap_or_else(|e| {
        panic!("unannotated string/map tail return-shape Python gate failed: {e}")
    });
}

#[test]
fn unannotated_function_tagged_template_tail_receiver_method_stays_declined() {
    let error = emit_error_for_source(
        r#"
function make() {
    sql"""select 1"""
}
function main() -> int {
    let text = make()
    text.byteLength()
}
main()
"#,
    );
    assert_eq!(error.code(), "TPZ6PY0001");
    assert!(
        matches!(error.kind, PyEmitErrorKind::Unsupported("member call")),
        "known template receiver methods should fail closed: {error:?}"
    );
}
