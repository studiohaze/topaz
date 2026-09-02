use super::*;

#[test]
fn emits_receiver_readonly_named_calls_without_reordering_effects() {
    let generated = emit_source(
        r#"
function mark(label: string, n: int) -> int {
    print(label)
    n
}
function makeArray() -> Array<int> {
    [8, 9]
}
function makeMap() -> Map<string, int> {
    map { "late": 4 }
}
function makeBytes() -> Bytes {
    Bytes.encodeUtf8(s: "AZ")
}
function idJson(value: JSONValue) -> JSONValue {
    value
}
function makeArrayResult() -> Result<Array<int>, string> {
    Ok(value: [10, 11])
}
function makeMapResult() -> Result<Map<string, int>, string> {
    Ok(value: map { "try": 6 })
}
function makeBytesResult() -> Result<Bytes, string> {
    Ok(value: Bytes.encodeUtf8(s: "BY"))
}
function idJsonResult(value: JSONValue) -> Result<JSONValue, string> {
    Ok(value: value)
}
function main() -> Result<string, string> {
    let parts = "a,b,c".split(sep: ",")
    let cp = match "Topaz".codePointAt(i: 1) {
        case Some(n) => n
        case None => 0
    }
    let arrGot = match [4, 5].get(i: 1) {
        case Some(n) => n
        case None => 0
    }
    let madeArray = makeArray()
    let arrFnGot = match madeArray.get(i: 1) {
        case Some(n) => n
        case None => 0
    }
    let madeArrayResult = makeArrayResult()?
    let arrTryGot = match madeArrayResult.get(i: 1) {
        case Some(n) => n
        case None => 0
    }
    let bytes = Bytes.fromArray(values: [65, 66, 67])?
    let byteGot = match bytes.get(index: 1) {
        case Some(n) => n
        case None => 0
    }
    let madeBytes = makeBytes()
    let byteFnGot = match madeBytes.get(index: 1) {
        case Some(n) => n
        case None => 0
    }
    let madeBytesResult = makeBytesResult()?
    let byteTryGot = match madeBytesResult.get(index: 1) {
        case Some(n) => n
        case None => 0
    }
    let sliced = bytes.slice(end: mark("end", 2), start: mark("start", 0))
    let decoded = sliced.decodeUtf8()?
    let m = map { "x": 1 }
    let mapGot = match m.get(k: "x") {
        case Some(n) => n
        case None => 0
    }
    let madeMap = makeMap()
    let mapFnGot = match madeMap.get(k: "late") {
        case Some(n) => n
        case None => 0
    }
    let madeMapResult = makeMapResult()?
    let mapTryGot = match madeMapResult.get(k: "try") {
        case Some(n) => n
        case None => 0
    }
    let got = m.getOr(default: mark("default", 9), k: "x")
    let has = m.containsKey(k: "x")
    let both = Set.of(1, 2).union(other: Set.of(2, 3))
    let parsed = JSON.parse(text: "[10,20]")?
    let item = match parsed.at(index: 1) {
        case Some(value) => JSON.stringify(value: value)?
        case None => "none"
    }
    let parsedObj = JSON.parse(text: "\{\"x\":3\}")?
    let jsonGot = match parsedObj.get(key: "x") {
        case Some(value) => match value.asInt() {
            case Some(n) => n
            case None => 0
        }
        case None => 0
    }
    let madeJson = idJson(parsedObj)
    let jsonFnGot = match madeJson.get(key: "x") {
        case Some(value) => match value.asInt() {
            case Some(n) => n
            case None => 0
        }
        case None => 0
    }
    let madeJsonResult = idJsonResult(value: parsedObj)?
    let jsonTryGot = match madeJsonResult.get(key: "x") {
        case Some(value) => match value.asInt() {
            case Some(n) => n
            case None => 0
        }
        case None => 0
    }
    Ok(value: "{parts[1]}:{cp}:{arrGot}:{arrFnGot}:{arrTryGot}:{byteGot}:{byteFnGot}:{byteTryGot}:{decoded}:{mapGot}:{mapFnGot}:{mapTryGot}:{got}:{has}:{both.length}:{item}:{jsonGot}:{jsonFnGot}:{jsonTryGot}")
}
main()
"#,
    );
    assert!(generated.contains("tpz_get([4, 5], 1,"), "{generated}");
    assert!(
        generated.contains("tpz_get(_t_6d6164654172726179, 1,"),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_get(_t_6d6164654172726179526573756c74, 1,"),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_get(_t_6279746573, 1,"),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_get(_t_6d6164654279746573, 1,"),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_get(_t_6d6164654279746573526573756c74, 1,"),
        "{generated}"
    );
    assert!(
        generated.contains(
            "(lambda __tpz_call_recv, __tpz_call_arg_0, __tpz_call_arg_1: tpz_bytes_slice(__tpz_call_recv, __tpz_call_arg_1, __tpz_call_arg_0,"
        ),
        "{generated}"
    );
    assert!(
        generated.contains(
            "(lambda __tpz_call_recv, __tpz_call_arg_0, __tpz_call_arg_1: tpz_map_get_or(__tpz_call_recv, __tpz_call_arg_1, __tpz_call_arg_0,"
        ),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_string_split(\"a,b,c\", \",\","),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_json_at(_t_706172736564, 1,"),
        "{generated}"
    );
    assert!(generated.contains("tpz_get(_t_6d, \"x\","), "{generated}");
    assert!(
        generated.contains("tpz_get(_t_6d6164654d6170, \"late\","),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_get(_t_6d6164654d6170526573756c74, \"try\","),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_get(_t_7061727365644f626a, \"x\","),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_get(_t_6d6164654a736f6e, \"x\","),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_get(_t_6d6164654a736f6e526573756c74, \"x\","),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("receiver read-only named call Python gate failed: {e}"));
}

#[test]
fn emits_receiver_callback_named_calls_without_reordering_effects() {
    let generated = emit_source(
        r#"
function double(x: int) -> int {
    x * 2
}
function add(acc: int, x: int) -> int {
    acc + x
}
function keepEntry(key: string, value: int) -> bool {
    key == "b" || value > 10
}
function maybeInc(x: int) -> Option<int> {
    Some(x + 1)
}
function okInc(x: int) -> Result<int, string> {
    Ok(x + 1)
}
function late() -> string {
    "late"
}
function main() -> int {
    let xs = [1, 2, 3]
    let ys = xs.map(f: double)
    let zs = xs.filter(f: (x) => x > 1)
    let total = xs.reduce(f: add, initial: 0)
    let sorted = [3, 1, 2].sortedBy(f: (x) => 0 - x)
    let opt = match Some(2).flatMap(f: maybeInc) {
        case Some(n) => n
        case None => 0
    }
    let res = match Ok(3).flatMap(f: okInc) {
        case Ok(n) => n
        case Err(_) => 0
    }
    let none: Option<int> = None
    let err = match none.okOrElse(f: late) {
        case Ok(_) => 0
        case Err(e) => if e == "late" { 1 } else { 0 }
    }
    let mut m = map { "a": 1, "b": 2 }
    let mapped = m.mapValues(f: double)
    let filtered = m.filter(f: keepEntry)
    m.update(f: double, initial: 0, k: "a")
    total + ys[0] + zs[0] + sorted[0] + opt + res + err + mapped.getOr("b", 0) + filtered.getOr("b", 0) + m.getOr("a", 0)
}
main()
"#,
    );
    assert!(
        generated.contains(
            "(lambda __tpz_call_recv, __tpz_call_arg_0, __tpz_call_arg_1: tpz_array_reduce(__tpz_call_recv, __tpz_call_arg_1, __tpz_call_arg_0,"
        ),
        "{generated}"
    );
    assert!(
        generated.contains(
            "tpz_map_update(_t_6d, \"a\", 0, (lambda __tpz_cb_0: _t_646f75626c65(host, __tpz_cb_0))"
        ),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_option_ok_or_else(_t_6e6f6e65, (lambda : _t_6c617465(host)),"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("receiver callback named call Python gate failed: {e}"));
    assert_generated_python_ok_int(&generated, 29, "receiver callback named-call parity");
}
