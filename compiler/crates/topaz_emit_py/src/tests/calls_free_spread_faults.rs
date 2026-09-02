use super::*;

#[test]
fn emits_nonvariadic_user_function_spread_faults() {
    let generated = emit_source(
        r#"
function f(a: int, b: int = 0) -> int {
    a + b
}
function mark(label: string, xs: Array<int>) -> Array<int> {
    print(label)
    xs
}
function main() -> int {
    let xs = mark("spread", [1])
    f(...xs)
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_nonvariadic_spread_call("),
        "{generated}"
    );
    assert!(
        generated.contains("*tpz_spread_values(_t_7873,"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("nonvariadic spread fault Python gate failed: {e}"));

    let local = emit_source(
        r#"
function main() -> int {
    let f = (a: int) => a
    let xs = [1]
    f(...xs)
}
main()
"#,
    );
    assert!(local.contains("tpz_nonvariadic_spread_call("), "{local}");
    assert_generated_python_gates(&local)
        .unwrap_or_else(|e| panic!("local nonvariadic spread fault Python gate failed: {e}"));

    let order_fault = emit_unchecked_source(
        r#"
function f(a: int, b: int) -> int {
    a + b
}
function markInt(label: string, value: int) -> int {
    print(label)
    value
}
function main() -> int {
    f(b: markInt("named", 2), markInt("pos", 1))
}
main()
"#,
    );
    assert!(
        order_fault.contains("tpz_call_order_fault(")
            && order_fault.contains("positional arguments may not follow named arguments (§5)"),
        "{order_fault}"
    );
    assert_generated_python_gates(&order_fault)
        .unwrap_or_else(|e| panic!("known function order fault Python gate failed: {e}"));
}

#[test]
fn emits_nonvariadic_user_function_pipe_spread_faults() {
    let generated = emit_source(
        r#"
function f(a: int, b: int = 0) -> int {
    a + b
}
function mark(label: string, xs: Array<int>) -> Array<int> {
    print(label)
    xs
}
function main() -> int {
    let x = 1
    x |> f(...mark("spread", [2]))
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_nonvariadic_spread_call([__tpz_piped"),
        "nonvariadic pipe spread should use the shared direct-call fault helper: {generated}"
    );
    assert!(
        generated.contains("*tpz_spread_values(_t_6d61726b(host, \"spread\", [2])"),
        "pipe spread operands should still be checked before the nonvariadic fault helper: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("nonvariadic pipe spread fault Python gate failed: {e}"));

    let arity_fault = emit_source(
        r#"
function f(a: int, b: int) -> int {
    a + b
}
function mark(label: string, xs: Array<int>) -> Array<int> {
    print(label)
    xs
}
function main() -> int {
    1 |> f(2, 3, ...mark("spread", [4]))
}
main()
"#,
    );
    assert!(
        arity_fault.contains("tpz_nonvariadic_spread_call([__tpz_piped, 2, 3]"),
        "pipe spread should place the piped value in the first fixed slot before arity faulting: {arity_fault}"
    );
    assert_generated_python_gates(&arity_fault)
        .unwrap_or_else(|e| panic!("nonvariadic pipe spread arity fault Python gate failed: {e}"));
}

#[test]
fn emits_loop_expression_in_nonvariadic_pipe_spread_fault_arguments() {
    let generated = emit_source(
        r#"
function f(a: int, b: int = 0) -> int {
    a + b
}
function main() -> int {
    loop {
        print("piped")
        break 1
    } |> f(loop {
        print("prefix")
        break 2
    }, ...loop {
        print("spread")
        break [3]
    })
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_nonvariadic_spread_call(")
            && generated.contains("call_arg")
            && generated.contains("call_spread"),
        "statement-lowered nonvariadic pipe spread should bind arguments before the shared helper: {generated}"
    );
    assert!(
        generated.contains("raise TpzLoopBreak(None, 1)")
            && generated.contains("raise TpzLoopBreak(None, 2)")
            && generated.contains("raise TpzLoopBreak(None, [3])"),
        "piped, prefix, and spread loop values should all lower in source order: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("nested loop nonvariadic pipe spread Python gate failed: {e}"));
}

#[test]
fn emits_nonvariadic_user_function_pipe_spread_placeholders_and_named_tail() {
    let placeholder_spread = emit_source(
        r#"
function f(a: int, b: int) -> int {
    a + b
}
function main() -> int {
    [1, 2] |> f(..._)
}
main()
"#,
    );
    assert!(
        placeholder_spread
            .contains("tpz_nonvariadic_spread_call([], [*tpz_spread_values(__tpz_piped"),
        "placeholder spread should not insert an implicit first argument or re-evaluate lhs: {placeholder_spread}"
    );
    assert_generated_python_gates(&placeholder_spread)
        .unwrap_or_else(|e| panic!("nonvariadic pipe spread placeholder Python gate failed: {e}"));

    let named_tail = emit_source(
        r#"
function f(a: int, b: int = 0) -> int {
    a + b
}
function main() -> int {
    1 |> f(...[2], b: 3)
}
main()
"#,
    );
    assert!(
        named_tail.contains("tpz_nonvariadic_spread_call([__tpz_piped], [*tpz_spread_values([2],")
            && named_tail.contains("[(\"b\", 3)]"),
        "nonvariadic pipe spread should preserve a named tail through the shared helper: {named_tail}"
    );
    assert_generated_python_gates(&named_tail)
        .unwrap_or_else(|e| panic!("nonvariadic pipe spread named tail Python gate failed: {e}"));

    let statement_lowered = emit_source(
        r#"
function f(a: int, b: int) -> int {
    a + b
}
function main() -> int {
    loop {
        print("piped")
        break [1, 2]
    } |> f(..._)
}
main()
"#,
    );
    assert!(
        statement_lowered.contains("tpz_nonvariadic_spread_call(")
            && statement_lowered.contains("= __tpz_pipe_value_")
            && statement_lowered.contains("tpz_spread_values(__tpz_call_arg_"),
        "statement-lowered placeholder spread should use the single-evaluated pipe temp: {statement_lowered}"
    );
    assert_generated_python_gates(&statement_lowered).unwrap_or_else(|e| {
        panic!("statement-lowered pipe spread placeholder Python gate failed: {e}")
    });

    let order_fault = emit_unchecked_source(
        r#"
function f(a: int, b: int = 0) -> int {
    a + b
}
function main() -> int {
    1 |> f(b: _, ...[2])
}
main()
"#,
    );
    assert!(
        order_fault.contains("tpz_call_order_fault(")
            && order_fault.contains("named arguments must follow spread arguments (§5)"),
        "named-before-spread pipe stage should remain a call-order fault: {order_fault}"
    );
}

#[test]
fn emits_nonvariadic_static_spread_faults() {
    let generated = emit_source(
        r#"
function mark(label: string, xs: Array<string>) -> Array<string> {
    print(label)
    xs
}
function main() -> () {
    print(...mark("spread", ["x"]))
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_nonvariadic_static_spread_call("),
        "{generated}"
    );
    assert!(
        generated.contains("*tpz_spread_values(_t_6d61726b(host, \"spread\", [\"x\"]),"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("static spread fault Python gate failed: {e}"));

    let named_tail = emit_source(
        r#"
function markArray(label: string, xs: Array<string>) -> Array<string> {
    print(label)
    xs
}
function markString(label: string, value: string) -> string {
    print(label)
    value
}
function main() -> () {
    print(...markArray("spread", ["x"]), value: markString("named", "y"))
}
main()
"#,
    );
    assert!(
        named_tail.contains("tpz_nonvariadic_static_spread_call(")
            && named_tail.contains("[(\"value\","),
        "{named_tail}"
    );
    assert_generated_python_gates(&named_tail)
        .unwrap_or_else(|e| panic!("static spread+named Python gate failed: {e}"));

    let order_fault = emit_unchecked_source(
        r#"
function markArray(label: string, xs: Array<string>) -> Array<string> {
    print(label)
    xs
}
function markString(label: string, value: string) -> string {
    print(label)
    value
}
function main() -> () {
    print(value: markString("named", "y"), ...markArray("spread", ["x"]))
}
main()
"#,
    );
    assert!(
        order_fault.contains("tpz_call_order_fault(")
            && order_fault.contains("named arguments must follow spread arguments (§5)")
            && !order_fault.contains("*tpz_spread_values("),
        "{order_fault}"
    );
    assert_generated_python_gates(&order_fault)
        .unwrap_or_else(|e| panic!("static named-before-spread Python gate failed: {e}"));

    let positional_after_named = emit_unchecked_source(
        r#"
function markString(label: string, value: string) -> string {
    print(label)
    value
}
function main() -> () {
    print(value: markString("named", "x"), markString("pos", "y"))
}
main()
"#,
    );
    assert!(
        positional_after_named.contains("tpz_call_order_fault(")
            && positional_after_named
                .contains("positional arguments may not follow named arguments (§5)"),
        "{positional_after_named}"
    );
    assert_generated_python_gates(&positional_after_named)
        .unwrap_or_else(|e| panic!("static positional-after-named Python gate failed: {e}"));

    let namespace = emit_source(
        r#"
function markString(label: string, xs: Array<string>) -> Array<string> {
    print(label)
    xs
}
function markBytes(label: string, xs: Array<Bytes>) -> Array<Bytes> {
    print(label)
    xs
}
function markOneBytes(label: string, value: Bytes) -> Bytes {
    print(label)
    value
}
function markText(label: string, value: string) -> string {
    print(label)
    value
}
function markInt(label: string, xs: Array<int>) -> Array<int> {
    print(label)
    xs
}
function main() -> string {
    let a = JSON.parse(...markString("json", ["null"]))
    let b = Bytes.concat(...markBytes("bytes", [Bytes.encodeUtf8("x")]))
    let c = Map.new(...markInt("map", [1]))
    let d = Bytes.concat(Bytes.encodeUtf8("prefix"), ...markBytes("bytes-named", [Bytes.encodeUtf8("x")]), b: markOneBytes("named", Bytes.encodeUtf8("y")))
    let e = Encoding.utf8Encode(...markString("encoding-named", ["x"]), text: markText("encoding-text", "y"))
    let f = JSON.parse(...markString("json-named", ["null"]), text: markText("json-text", "null"))
    "{a}:{b}:{c}:{d}:{e}:{f}"
}
main()
"#,
    );
    assert!(
        namespace
            .matches("tpz_nonvariadic_static_spread_call(")
            .count()
            >= 6,
        "{namespace}"
    );
    assert_generated_python_gates(&namespace)
        .unwrap_or_else(|e| panic!("namespace spread fault Python gate failed: {e}"));

    let namespace_order_fault = emit_unchecked_source(
        r#"
function markString(label: string, value: string) -> string {
    print(label)
    value
}
function main() -> JSONValue {
    JSON.parse(text: markString("named", "null"), markString("pos", "null"))
}
main()
"#,
    );
    assert!(
        namespace_order_fault.contains("tpz_call_order_fault(")
            && namespace_order_fault
                .contains("positional arguments may not follow named arguments (§5)"),
        "{namespace_order_fault}"
    );
    assert_generated_python_gates(&namespace_order_fault)
        .unwrap_or_else(|e| panic!("namespace positional-after-named Python gate failed: {e}"));
}

#[test]
fn emits_receiver_readonly_spread_faults() {
    let generated = emit_source(
        r#"
function markInt(label: string, xs: Array<int>) -> Array<int> {
    print(label)
    xs
}
function markString(label: string, xs: Array<string>) -> Array<string> {
    print(label)
    xs
}
function makeMap() -> Map<string, int> {
    map { "x": 1 }
}
function makeJson() -> JSONValue {
    JSON.parse("\{\"x\":1\}")?
}
function main() -> string {
    let a = [10].get(...markInt("arr", [0]))
    let b = "a,b".split(...markString("str", [","]))
    let c = Bytes.encodeUtf8("x").decodeUtf8(...markString("bytes", []))
    let d = makeMap().containsKey(...markString("map", ["x"]))
    let e = makeJson().kind(...markString("json", []))
    "{a}:{b}:{c}:{d}:{e}"
}
main()
"#,
    );
    assert!(
        generated
            .matches("tpz_nonvariadic_static_spread_call(")
            .count()
            >= 5,
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("receiver readonly spread Python gate failed: {e}"));

    let named_tail = emit_source(
        r#"
function markArray(label: string, xs: Array<int>) -> Array<int> {
    print(label)
    xs
}
function markInt(label: string, value: int) -> int {
    print(label)
    value
}
function main() -> Option<int> {
    [10].get(...markArray("spread", [0]), i: markInt("named", 0))
}
main()
"#,
    );
    assert!(
        named_tail.contains("tpz_nonvariadic_static_spread_call(")
            && named_tail.contains("[(\"i\","),
        "{named_tail}"
    );
    assert_generated_python_gates(&named_tail)
        .unwrap_or_else(|e| panic!("receiver spread+named Python gate failed: {e}"));

    let order_fault = emit_unchecked_source(
        r#"
function markArray(label: string, xs: Array<int>) -> Array<int> {
    print(label)
    xs
}
function markInt(label: string, value: int) -> int {
    print(label)
    value
}
function main() -> Option<int> {
    [10].get(i: markInt("named", 0), ...markArray("spread", [0]))
}
main()
"#,
    );
    assert!(
        order_fault.contains("tpz_call_order_fault(")
            && order_fault.contains("named arguments must follow spread arguments (§5)")
            && !order_fault.contains("*tpz_spread_values("),
        "{order_fault}"
    );
    assert_generated_python_gates(&order_fault)
        .unwrap_or_else(|e| panic!("receiver named-before-spread Python gate failed: {e}"));
}

#[test]
fn emits_receiver_callback_spread_faults() {
    let generated = emit_source(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
    print(label)
    xs
}
function main() -> string {
    let a = [1].map(...mark("map", [0]))
    let b = [1].filter(...mark("filter", [0]))
    let c = [1].reduce(0, ...mark("reduce", [0]))
    let d = [1].sortedBy(...mark("sorted", [0]))
    let e = Some(1).map(...mark("option-map", [0]))
    let f = Ok(1).map(...mark("result-map", [0]))
    let g = Some(1).flatMap(...mark("option-flat", [0]))
    let h = Ok(1).flatMap(...mark("result-flat", [0]))
    let k = None.okOrElse(...mark("ok", [0]))
    let m = map { "x": 1 }
    let i = m.mapValues(...mark("values", [0]))
    let j = m.filter(...mark("map-filter", [0]))
    "{a}:{b}:{c}:{d}:{e}:{f}:{g}:{h}:{k}:{i}:{j}"
}
main()
"#,
    );
    assert!(
        generated
            .matches("tpz_nonvariadic_static_spread_call(")
            .count()
            >= 11,
        "{generated}"
    );
    assert!(
        generated.contains("(lambda __tpz_call_recv:"),
        "receiver should evaluate before spread fault: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("receiver callback spread Python gate failed: {e}"));
}

#[test]
fn emits_receiver_mutating_spread_faults() {
    let generated = emit_source(
        r#"
function markInt(label: string, xs: Array<int>) -> Array<int> {
    print(label)
    xs
}
function markString(label: string, xs: Array<string>) -> Array<string> {
    print(label)
    xs
}
function markKey(label: string, xs: Array<(int) -> int>) -> Array<(int) -> int> {
    print(label)
    xs
}
function markKeep(label: string, xs: Array<(int) -> bool>) -> Array<(int) -> bool> {
    print(label)
    xs
}
function key(x: int) -> int { x }
function keep(x: int) -> bool { x > 0 }
function inc(x: int) -> int { x + 1 }
function main() -> string {
    let xs = [1]
    xs.push(...markInt("push", [2]))
    xs.sortBy(...markKey("sort", [key]))
    xs.retain(...markKeep("retain", [keep]))
    let m = map { "x": 1 }
    m.insert("y", ...markInt("insert", [2]))
    m.remove(...markString("map-remove", ["x"]))
    m.clear(...markInt("map-clear", []))
    m.update("x", 0, ...markKey("update", [inc]))
    let s = Set.of(1)
    s.add(...markInt("set-add", [2]))
    s.remove(...markInt("set-remove", [1]))
    s.clear(...markInt("set-clear", []))
    "{xs}:{m}:{s}"
}
main()
"#,
    );
    assert!(
        generated
            .matches("tpz_nonvariadic_static_spread_call(")
            .count()
            >= 10,
        "{generated}"
    );
    assert!(
        generated.contains("(lambda __tpz_call_recv:"),
        "receiver should evaluate before mutating spread fault: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("receiver mutating spread Python gate failed: {e}"));
}
