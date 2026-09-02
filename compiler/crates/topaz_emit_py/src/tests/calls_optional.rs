use super::*;

#[test]
fn emits_optional_callback_hofs() {
    let generated = emit_source(
        r#"
function inc(x: int) -> int {
    x + 1
}
function maybeInc(x: int) -> Option<int> {
    Some(x + 1)
}
function main() -> int {
    let xs: Option<Array<int>> = Some([1, 2])
    let noneXs: Option<Array<int>> = None
    let opt: Option<Option<int>> = Some(Some(3))
    let res: Option<Result<int, string>> = Some(Ok(4))
    let mapped = xs?.map(inc)
    let skipped = noneXs?.map((x) => x / 0)
    let flat = opt?.flatMap(maybeInc)
    let out = res?.map(inc)
    let a = match mapped {
        case Some(ys) => ys[0] + ys[1]
        case None => 0
    }
    let b = match skipped {
        case Some(_) => 100
        case None => 7
    }
    let c = match flat {
        case Some(n) => n
        case None => 0
    }
    let d = match out {
        case Some(Ok(n)) => n
        case _ => 0
    }
    a + b + c + d
}
main()
"#,
    );
    assert!(
        generated.contains("None if __tpz_obj is None else"),
        "optional HOF should short-circuit None before callback args: {generated}"
    );
    assert!(
        generated.contains("tpz_wrap_optional(tpz_array_map(__tpz_obj.value"),
        "Option<Array>.map should call array map on Some inner and rewrap: {generated}"
    );
    assert!(
        generated.contains("tpz_wrap_optional(tpz_option_flat_map(__tpz_obj.value"),
        "Option<Option>.flatMap should call option flatMap on Some inner: {generated}"
    );
    assert!(
        generated.contains("tpz_wrap_optional(tpz_result_map(__tpz_obj.value"),
        "Option<Result>.map should call result map on Some inner: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("optional callback HOF Python gate failed: {e}"));
}

#[test]
fn emits_optional_receiver_callback_spread_faults() {
    let generated = emit_source(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
    print(label)
    xs
}
function main() -> string {
    let noneXs: Option<Array<int>> = None
    let xs: Option<Array<int>> = Some([1])
    let opt: Option<Option<int>> = Some(Some(1))
    let res: Option<Result<int, string>> = Some(Ok(1))
    let m: Option<Map<string, int>> = Some(map { "x": 1 })
    let a = noneXs?.map(...mark("none", [0]))
    let b = xs?.map(...mark("map", [0]))
    let c = xs?.filter(...mark("filter", [0]))
    let d = xs?.reduce(0, ...mark("reduce", [0]))
    let e = xs?.sortedBy(...mark("sorted", [0]))
    let f = opt?.map(...mark("option-map", [0]))
    let g = res?.map(...mark("result-map", [0]))
    let h = opt?.flatMap(...mark("option-flat", [0]))
    let i = res?.flatMap(...mark("result-flat", [0]))
    let j = opt?.okOrElse(...mark("ok", [0]))
    let k = m?.mapValues(...mark("values", [0]))
    let l = m?.filter(...mark("map-filter", [0]))
    "{a}:{b}:{c}:{d}:{e}:{f}:{g}:{h}:{i}:{j}:{k}:{l}"
}
main()
"#,
    );
    assert!(
        generated.contains("None if __tpz_obj is None else"),
        "optional receiver should short-circuit before spread fault: {generated}"
    );
    assert!(
        generated
            .matches("tpz_nonvariadic_static_spread_call(")
            .count()
            >= 12,
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("optional receiver callback spread Python gate failed: {e}"));
}

#[test]
fn emits_optional_receiver_mutating_spread_faults() {
    let generated = emit_source(
        r#"
function markKey(label: string, fs: Array<(int) -> int>) -> Array<(int) -> int> {
    print(label)
    fs
}
function markKeep(label: string, fs: Array<(int) -> bool>) -> Array<(int) -> bool> {
    print(label)
    fs
}
function key(x: int) -> int { x }
function keep(x: int) -> bool { x > 0 }
function inc(x: int) -> int { x + 1 }
function main() -> string {
    let noneXs: Option<Array<int>> = None
    let mut xs: Option<Array<int>> = Some([2, 1])
    let mut kept: Option<Array<int>> = Some([1])
    let mut m: Option<Map<string, int>> = Some(map { "x": 1 })
    let a = noneXs?.sortBy(...markKey("none-sort", [key]))
    let b = xs?.sortBy(...markKey("sort", [key]))
    let c = kept?.retain(...markKeep("retain", [keep]))
    let d = m?.update("x", 0, ...markKey("update", [inc]))
    "{a}:{b}:{c}:{d}"
}
main()
"#,
    );
    assert!(
        generated.contains("None if __tpz_obj is None else"),
        "optional mutator spread should short-circuit None before spread fault: {generated}"
    );
    assert!(
        generated.contains("tpz_wrap_optional_unit("),
        "optional mutator spread should rewrap Some branch unit: {generated}"
    );
    assert!(
        generated
            .matches("tpz_nonvariadic_static_spread_call(")
            .count()
            >= 4,
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("optional mutating receiver spread Python gate failed: {e}"));
}

#[test]
fn emits_optional_mutating_callback_hofs() {
    let generated = emit_source(
        r#"
function neg(x: int) -> int {
    0 - x
}
function large(x: int) -> bool {
    x > 2
}
function main() -> int {
    let mut xs: Option<Array<int>> = Some([3, 1, 2])
    let sortedResult = xs?.sortBy(neg)
    let sorted = match xs {
        case Some(ys) => ys[0] + ys[2]
        case None => 0
    }
    let sortedReturn = match sortedResult {
        case Some(_) => 1
        case None => 0
    }
    let mut kept: Option<Array<int>> = Some([1, 2, 3, 4])
    let retainedResult = kept?.retain(large)
    let retained = match kept {
        case Some(ys) => ys.length + ys[0]
        case None => 0
    }
    let retainedReturn = match retainedResult {
        case Some(_) => 1
        case None => 0
    }
    let mut noneXs: Option<Array<int>> = None
    let skippedResult = noneXs?.retain((x) => x / 0 == 0)
    let skippedReturn = match skippedResult {
        case Some(_) => 100
        case None => 1
    }
    let mut m = Map.new()
    m.insert("a", 1)
    let mut om: Option<Map<string, int>> = Some(m)
    let updatedResult = om?.update("a", 0, (v) => v + 5)
    let insertedResult = om?.update("b", 7, (v) => v / 0)
    let updated = match om {
        case Some(mm) => mm.getOr("a", 0) + mm.getOr("b", 0)
        case None => 0
    }
    let updatedReturn = match updatedResult {
        case Some(_) => 1
        case None => 0
    }
    let insertedReturn = match insertedResult {
        case Some(_) => 1
        case None => 0
    }
    sorted + sortedReturn + retained + retainedReturn + skippedReturn + updated + updatedReturn + insertedReturn
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_wrap_optional_unit(tpz_array_sort_by(__tpz_obj.value"),
        "optional sortBy should mutate Some inner through tpz_array_sort_by: {generated}"
    );
    assert!(
        generated.contains("tpz_wrap_optional_unit(tpz_array_retain(__tpz_obj.value"),
        "optional retain should mutate Some inner through tpz_array_retain: {generated}"
    );
    assert!(
        generated.contains("tpz_wrap_optional_unit(tpz_map_update(__tpz_obj.value"),
        "optional Map.update should mutate Some inner through tpz_map_update: {generated}"
    );
    assert!(
        generated.contains("None if __tpz_obj is None else"),
        "optional mutating HOFs should short-circuit None before callback args: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("optional mutating callback HOF Python gate failed: {e}"));
}

#[test]
fn emits_pipe_callback_hof_stages() {
    let generated = emit_source(
        r#"
function add(a: int, b: int) -> int {
    a + b
}
function inc(x: int) -> int {
    x + 1
}
function isBig(x: int) -> bool {
    x > 1
}
function keep(k: string, v: int) -> bool {
    v > 1
}
function main() -> int {
    let xs = [1, 2, 3]
    let m = map { "a": 1, "b": 2 }
    let mut mu = map { "a": 1 }
    let mapped = ((x) => x + 1) |> xs.map()
    let filtered = isBig |> xs.filter()
    let reduced = 0 |> xs.reduce(add)
    let reducedNamed = 0 |> xs.reduce(initial: _, f: add)
    let reducedInserted = 0 |> xs.reduce(f: add)
    let sorted = ((x) => 0 - x) |> xs.sortedBy()
    let mappedNamed = inc |> xs.map(f: _)
    let values = inc |> m.mapValues()
    let kept = keep |> m.filter()
    let updateF = inc |> mu.update("a", 0, _)
    let updateK = "b" |> mu.update(_, 7, (v) => v / 0)
    let updateInitial = 9 |> mu.update("c", _, (v) => v / 0)
    let updateNamedInitial = 11 |> mu.update(k: "d", initial: _, f: inc)
    let updateNamedKey = "e" |> mu.update(k: _, initial: 5, f: inc)
    let updateNamedF = inc |> mu.update(k: "f", initial: 1, f: _)
    mapped[0] + filtered.length + reduced + reducedNamed + reducedInserted + sorted[0] + mappedNamed[1] + values.getOr("a", 0) + kept.getOr("b", 0) + mu.getOr("a", 0) + mu.getOr("b", 0) + mu.getOr("c", 0) + mu.getOr("d", 0) + mu.getOr("e", 0) + mu.getOr("f", 0)
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_array_map(_t_7873, __tpz_piped,"),
        "lambda pipe callback should become the map callback slot: {generated}"
    );
    assert!(
        generated.contains("(lambda __tpz_cb_0: _t_6973426967(host, __tpz_cb_0))"),
        "top-level function pipe callback should adapt host argument: {generated}"
    );
    assert!(
        generated.contains("tpz_array_reduce(_t_7873, __tpz_piped, (lambda __tpz_cb_0, __tpz_cb_1: _t_616464(host, __tpz_cb_0, __tpz_cb_1))"),
        "reduce pipe stage should keep first-argument insertion for initial and adapt callback: {generated}"
    );
    assert!(
        generated.contains(
            "tpz_array_reduce(_t_7873, __tpz_piped, (lambda __tpz_cb_0, __tpz_cb_1: _t_616464(host, __tpz_cb_0, __tpz_cb_1))"
        ),
        "named reduce pipe placeholder should bind initial and adapt callback: {generated}"
    );
    assert!(
        generated
            .contains("tpz_array_map(_t_7873, (lambda __tpz_cb_0: _t_696e63(host, __tpz_cb_0))"),
        "named map pipe placeholder should adapt the callback slot: {generated}"
    );
    assert!(
        generated.contains("tpz_map_filter(_t_6d, (lambda __tpz_cb_0, __tpz_cb_1: _t_6b656570(host, __tpz_cb_0, __tpz_cb_1))"),
        "Map.filter pipe callback should adapt two callback args: {generated}"
    );
    assert!(
        generated
            .matches("tpz_map_update(_t_6d75, __tpz_pipe_arg_")
            .count()
            >= 6
            && generated.contains("= (lambda __tpz_cb_0: _t_696e63(host, __tpz_cb_0))")
            && generated.contains("= (lambda _t_76:"),
        "Map.update pipe placeholders should preserve slot order and callback adaptation: {generated}"
    );
    let callback_nested_placeholder = emit_error_for_source(
        r#"
	function add(a: int, b: int) -> int {
	    a + b
	}
	function main() -> int {
	    let xs = [1, 2]
	    add |> xs.reduce(initial: 0, f: (_))
	}
	main()
	"#,
    );
    assert_eq!(
        callback_nested_placeholder.kind,
        PyEmitErrorKind::Unsupported("pipe placeholder")
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("pipe callback HOF Python gate failed: {e}"));
    assert_generated_python_ok_int(&generated, 67, "pipe callback HOF value parity");
}

#[test]
fn emits_statementful_callback_value_in_pipe_static_slot() {
    let generated = emit_source(
        r#"
function add(a: int, b: int) -> int {
    a + b
}
function mark(label: string, n: int) -> int {
    print(label)
    n
}
function main() -> int {
    let xs = [1, 2]
    10 |> xs.reduce(initial: mark("initial", 1) + _, f: {
        print("f")
        add
    })
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_array_reduce(_t_7873")
            && generated.contains("tpz_host_callable(_t_616464, host")
            && generated.contains("initial")
            && generated.contains("f"),
        "statementful callback pipe slot should lower before the reduce call: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("statementful callback pipe slot Python gate failed: {e}"));
}

#[test]
fn emits_optional_pipe_callback_hof_stages() {
    let generated = emit_source(
        r#"
function add(a: int, b: int) -> int {
    a + b
}
function inc(x: int) -> int {
    x + 1
}
function isBig(x: int) -> bool {
    x > 1
}
function keep(k: string, v: int) -> bool {
    v > 1
}
function maybeInc(x: int) -> Option<int> {
    Some(x + 1)
}
function main() -> int {
    let xs: Option<Array<int>> = Some([1, 2, 3])
    let mut muts: Option<Array<int>> = Some([3, 1, 2])
    let mut keptMut: Option<Array<int>> = Some([1, 2, 3, 4])
    let mut noneMut: Option<Array<int>> = None
    let noneXs: Option<Array<int>> = None
    let opt: Option<Option<int>> = Some(Some(3))
    let res: Option<Result<int, string>> = Some(Ok(4))
    let mut m: Option<Map<string, int>> = Some(map { "a": 1, "b": 2 })
    let mut noneMap: Option<Map<string, int>> = None
    let mapped = ((x) => x + 1) |> xs?.map()
    let skipped = ((x) => x / 0) |> noneXs?.map()
    let mappedNamed = inc |> xs?.map(f: _)
    let filtered = isBig |> xs?.filter()
    let reduced = 0 |> xs?.reduce(add)
    let sorted = ((x) => 0 - x) |> xs?.sortedBy()
    let sortResult = ((x) => 0 - x) |> muts?.sortBy()
    let retainResult = isBig |> keptMut?.retain()
    let skippedMut = ((x) => x / 0 == 0) |> noneMut?.retain()
    let flat = maybeInc |> opt?.flatMap()
    let out = inc |> res?.map()
    let values = inc |> m?.mapValues()
    let kept = keep |> m?.filter()
    let updateF = inc |> m?.update("a", 0, _)
    let updateK = "c" |> m?.update(_, 9, (v) => v / 0)
    let updateInitial = 11 |> m?.update("d", _, (v) => v / 0)
    let skippedUpdate = ((v) => v / 0) |> noneMap?.update("z", 0, _)
    let a = match mapped {
        case Some(ys) => ys[0]
        case None => 0
    }
    let b = match skipped {
        case Some(_) => 100
        case None => 7
    }
    let c = match filtered {
        case Some(ys) => ys.length
        case None => 0
    }
    let cNamed = match mappedNamed {
        case Some(ys) => ys[1]
        case None => 0
    }
    let d = match sorted {
        case Some(ys) => ys[0]
        case None => 0
    }
    let e = match flat {
        case Some(n) => n
        case None => 0
    }
    let f = match out {
        case Some(Ok(n)) => n
        case _ => 0
    }
    let g = match values {
        case Some(mm) => mm.getOr("a", 0)
        case None => 0
    }
    let h = match kept {
        case Some(mm) => mm.getOr("b", 0)
        case None => 0
    }
    let i = match muts {
        case Some(ys) => ys[0] + ys[2]
        case None => 0
    }
    let j = match sortResult {
        case Some(_) => 1
        case None => 0
    }
    let k = match keptMut {
        case Some(ys) => ys.length + ys[0]
        case None => 0
    }
    let l = match retainResult {
        case Some(_) => 1
        case None => 0
    }
    let n = match skippedMut {
        case Some(_) => 100
        case None => 1
    }
    let o = match m {
        case Some(mm) => mm.getOr("a", 0) + mm.getOr("c", 0) + mm.getOr("d", 0)
        case None => 0
    }
    let p = match updateF {
        case Some(_) => 1
        case None => 0
    }
    let q = match updateK {
        case Some(_) => 1
        case None => 0
    }
    let r = match updateInitial {
        case Some(_) => 1
        case None => 0
    }
    let s = match skippedUpdate {
        case Some(_) => 100
        case None => 1
    }
    a + b + c + cNamed + reduced + d + e + f + g + h + i + j + k + l + n + o + p + q + r + s
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_wrap_optional(tpz_array_map(__tpz_obj.value, __tpz_piped,"),
        "optional pipe lambda callback should map over Some inner and rewrap: {generated}"
    );
    assert!(
        generated.contains("None if __tpz_obj is None else"),
        "optional pipe callback HOF should preserve optional short-circuit skeleton: {generated}"
    );
    assert!(
        generated.contains("tpz_array_reduce(__tpz_obj.value, __tpz_piped, (lambda __tpz_cb_0, __tpz_cb_1: _t_616464(host, __tpz_cb_0, __tpz_cb_1))"),
        "optional pipe reduce should keep first-argument insertion and adapt callback: {generated}"
    );
    assert!(
        generated.contains(
            "tpz_wrap_optional(tpz_array_map(__tpz_obj.value, (lambda __tpz_cb_0: _t_696e63(host, __tpz_cb_0))"
        ),
        "optional named map pipe placeholder should adapt the callback slot: {generated}"
    );
    assert!(
        generated.contains("tpz_map_filter(__tpz_obj.value, (lambda __tpz_cb_0, __tpz_cb_1: _t_6b656570(host, __tpz_cb_0, __tpz_cb_1))"),
        "optional pipe Map.filter should adapt two callback args: {generated}"
    );
    assert!(
        generated
            .contains("tpz_wrap_optional_unit(tpz_array_sort_by(__tpz_obj.value, __tpz_piped,"),
        "optional pipe sortBy should mutate Some inner and preserve optional-unit: {generated}"
    );
    assert!(
        generated.contains("tpz_wrap_optional_unit(tpz_array_retain(__tpz_obj.value, (lambda __tpz_cb_0: _t_6973426967(host, __tpz_cb_0))"),
        "optional pipe retain should adapt function callback and preserve optional-unit: {generated}"
    );
    assert!(
        generated.contains("tpz_wrap_optional_unit(tpz_map_update(__tpz_obj.value, \"a\", 0, (lambda __tpz_cb_0: _t_696e63(host, __tpz_cb_0))"),
        "optional Map.update pipe callback placeholder should adapt the callback slot: {generated}"
    );
    assert!(
        generated.contains(
            "tpz_wrap_optional_unit(tpz_map_update(__tpz_obj.value, __tpz_piped, 9, (lambda _t_76"
        ),
        "optional Map.update pipe key placeholder should bind the key slot: {generated}"
    );
    assert!(
        generated.contains("tpz_wrap_optional_unit(tpz_map_update(__tpz_obj.value, \"d\", __tpz_piped, (lambda _t_76"),
        "optional Map.update pipe initial placeholder should bind the initial slot: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("optional pipe callback HOF Python gate failed: {e}"));
}

#[test]
fn emits_optional_pipe_receiver_hof_spread_faults() {
    let generated = emit_source(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
    print(label)
    xs
}
function add(a: int, b: int) -> int { a + b }
function inc(x: int) -> int { x + 1 }
function isBig(x: int) -> bool { x > 1 }
function maybeInc(x: int) -> Option<int> { Some(x + 1) }
function fallback() -> int { 7 }
function keep(k: string, v: int) -> bool { v > 0 }
function main() -> string {
    let noneXs: Option<Array<int>> = None
    let xs: Option<Array<int>> = Some([1])
    let opt: Option<Option<int>> = Some(Some(1))
    let res: Option<Result<int, string>> = Some(Ok(1))
    let m: Option<Map<string, int>> = Some(map { "x": 1 })
    let a = inc |> noneXs?.map(...mark("none", [0]))
    let b = inc |> xs?.map(...mark("map", [0]))
    let c = isBig |> xs?.filter(...mark("filter", [0]))
    let d = 0 |> xs?.reduce(add, ...mark("reduce", [0]))
    let e = inc |> xs?.sortedBy(...mark("sorted", [0]))
    let f = inc |> opt?.map(...mark("option-map", [0]))
    let g = inc |> res?.map(...mark("result-map", [0]))
    let h = maybeInc |> opt?.flatMap(...mark("option-flat", [0]))
    let i = maybeInc |> res?.flatMap(...mark("result-flat", [0]))
    let j = fallback |> opt?.okOrElse(...mark("ok", [0]))
    let k = inc |> m?.mapValues(...mark("values", [0]))
    let l = keep |> m?.filter(...mark("map-filter", [0]))
    "{a}:{b}:{c}:{d}:{e}:{f}:{g}:{h}:{i}:{j}:{k}:{l}"
}
main()
"#,
    );
    assert!(
        generated.contains("(lambda __tpz_piped:"),
        "optional pipe HOF spread should keep the pipe lambda: {generated}"
    );
    assert!(
        generated.contains("None if __tpz_obj is None else"),
        "optional pipe HOF spread should short-circuit None before spread fault: {generated}"
    );
    assert!(
        generated
            .matches("tpz_nonvariadic_static_spread_call(")
            .count()
            >= 12,
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("optional pipe receiver HOF spread Python gate failed: {e}"));

    let direct_named = emit_error_for_source(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
    print(label)
    xs
}
function inc(x: int) -> int { x + 1 }
function main() -> string {
    let xs = [1]
    inc |> xs.map(...mark("map", [0]), f: inc)
    "unreachable"
}
main()
"#,
    );
    assert_eq!(
        direct_named.kind,
        PyEmitErrorKind::Unsupported("pipe stage argument")
    );

    let optional_named = emit_error_for_source(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
    print(label)
    xs
}
function inc(x: int) -> int { x + 1 }
function main() -> string {
    let xs: Option<Array<int>> = Some([1])
    inc |> xs?.map(...mark("map", [0]), f: inc)
    "unreachable"
}
main()
"#,
    );
    assert_eq!(
        optional_named.kind,
        PyEmitErrorKind::Unsupported("call argument shape")
    );
}

#[test]
fn unannotated_function_namespace_builtin_tail_return_shape_supports_named_receivers() {
    let generated = emit_source(
        r#"
function makeBytes() {
    Bytes.encodeUtf8("AZ")
}
function makeMap() {
    Map.new()
}
function main() -> int {
    let bytes = makeBytes()
    let values = makeMap()
    let hex = bytes.toHex()
    let bonus = if values.containsKey(k: "x") { 10 } else { 0 }
    hex.byteLength() + values.getOr(k: "x", default: 7) + bonus
}
main()
"#,
    );
    assert!(generated.contains("tpz_bytes_to_hex("), "{generated}");
    assert!(generated.contains("tpz_map_contains_key("), "{generated}");
    assert!(generated.contains("tpz_map_get_or("), "{generated}");
    assert_generated_python_gates(&generated).unwrap_or_else(|e| {
        panic!("unannotated namespace-builtin tail return-shape Python gate failed: {e}")
    });
}

#[test]
fn emits_optional_pipe_mutating_spread_faults() {
    let generated = emit_source(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
    print(label)
    xs
}
function key(x: int) -> int { x }
function keep(x: int) -> bool { x > 0 }
function inc(x: int) -> int { x + 1 }
function main() -> string {
    let mut noneXs: Option<Array<int>> = None
    let mut xs: Option<Array<int>> = Some([2, 1])
    let mut kept: Option<Array<int>> = Some([1])
    let mut m: Option<Map<string, int>> = Some(map { "x": 1 })
    let a = key |> noneXs?.sortBy(...mark("none-sort", [0]))
    let b = key |> xs?.sortBy(...mark("sort", [0]))
    let c = keep |> kept?.retain(...mark("retain", [0]))
    let d = inc |> m?.update("x", 0, _, ...mark("update", [0]))
    "{a}:{b}:{c}:{d}"
}
main()
"#,
    );
    assert!(
        generated.contains("(lambda __tpz_piped:"),
        "optional pipe spread should keep the pipe lambda: {generated}"
    );
    assert!(
        generated.contains("None if __tpz_obj is None else"),
        "optional pipe spread should short-circuit None before spread fault: {generated}"
    );
    assert!(
        generated
            .matches("tpz_nonvariadic_static_spread_call(")
            .count()
            >= 4,
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("optional pipe mutating spread Python gate failed: {e}"));
}

#[test]
fn emits_map_callback_hofs() {
    let generated = emit_source(
        r#"
function inc(x: int) -> int {
    x + 1
}
function keep(name: string, value: int) -> bool {
    name == "b" || value > 10
}
function main() -> int {
    let m = map { "a": 1, "b": 2 }
    let mapped = m.mapValues(inc)
    let filtered = m.filter(keep)
    let filtered2 = m.filter((key, value) => key == "a" && value == 1)
    mapped.getOr("a", 0) + mapped.getOr("b", 0) + filtered.length + filtered2.length
}
main()
"#,
    );
    assert!(
        generated
            .contains("tpz_map_map_values(_t_6d, (lambda __tpz_cb_0: _t_696e63(host, __tpz_cb_0))"),
        "Map.mapValues function callback should adapt host argument: {generated}"
    );
    assert!(
        generated.contains("tpz_map_filter(_t_6d, (lambda __tpz_cb_0, __tpz_cb_1: _t_6b656570(host, __tpz_cb_0, __tpz_cb_1))"),
        "Map.filter function callback should adapt host and two args: {generated}"
    );
    assert!(
        generated.contains("tpz_map_filter(_t_6d, (lambda _t_6b6579, _t_76616c7565"),
        "Map.filter lambda callback should lower through tpz_map_filter: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("Map callback HOF Python gate failed: {e}"));
}

#[test]
fn emits_map_update_lazy_callback() {
    let generated = emit_source(
        r#"
function bump(x: int) -> int {
    x + 5
}
function main() -> int {
    let mut m = Map.new()
    m.insert("a", 1)
    m.update("a", 0, bump)
    m.update("b", 7, (v) => v / 0)
    m.getOr("a", 0) + m.getOr("b", 0)
}
main()
"#,
    );
    assert!(
        generated.contains(
            "tpz_map_update(_t_6d, \"a\", 0, (lambda __tpz_cb_0: _t_62756d70(host, __tpz_cb_0))"
        ),
        "Map.update function callback should adapt host argument: {generated}"
    );
    assert!(
        generated.contains("tpz_map_update(_t_6d, \"b\", 7, (lambda _t_76"),
        "Map.update lambda callback should lower through tpz_map_update: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("Map.update Python gate failed: {e}"));
}
