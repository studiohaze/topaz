use super::*;

#[test]
fn emits_collection_comprehensions_with_clauses_and_patterns() {
    let generated = emit_source(
        r#"
function main() -> string {
    let xs = [1, 2, 3, 4]
    let evens = [ for x in xs if x % 2 == 0 => x * 10 ]
    let bumped = evens.map((x) => x + 1)
    let unique = set { for x in [2, 1, 2] => x }
    let table = map { for [k, v] in [[1, 10], [2, 20]] if k > 1 => k: v }
    let shifted = table.mapValues((v) => v + 1)
    "{evens}:{bumped}:{unique.toArray()}:{table.getOr(2, 0)}:{shifted.getOr(2, 0)}"
}
main()
"#,
    );
    assert!(generated.contains("tpz_for_items("), "{generated}");
    assert!(generated.contains("tpz_for_pattern("), "{generated}");
    assert!(generated.contains("tpz_set_of(["), "{generated}");
    assert!(generated.contains("tpz_map_of([("), "{generated}");
    assert!(generated.contains("tpz_array_map("), "{generated}");
    assert!(generated.contains("tpz_map_map_values("), "{generated}");
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("comprehension Python gate failed: {e}"));

    let top_level = emit_source("[ for x in [1, 2] => x ]");
    assert!(top_level.contains("__tpz_value = ["), "{top_level}");
    assert_generated_python_gates(&top_level)
        .unwrap_or_else(|e| panic!("top-level comprehension Python gate failed: {e}"));
}

#[test]
fn comprehension_lambdas_capture_each_iteration_binding() {
    let generated = emit_source(
        r#"
let x = 99
let fs = [ for x in [1, 2, 3] => (() => x) ]
let value = fs[0]() * 100 + fs[1]() * 10 + fs[2]()
value
"#,
    );
    assert_generated_python_ok_int(&generated, 123, "comprehension lambda capture");
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("comprehension capture Python gate failed: {e}"));
}

#[test]
fn comprehension_clauses_can_shadow_earlier_bindings() {
    let generated = emit_source(
        r#"
let fs = [ for x in [1, 2] for x in [3] => (() => x) ]
let value = fs[0]() * 10 + fs[1]()
value
"#,
    );
    assert_generated_python_ok_int(&generated, 33, "comprehension clause shadowing");
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("comprehension shadowing Python gate failed: {e}"));
}

#[test]
fn emits_literal_function_default_parameters() {
    let generated = emit_source(
        r#"
function add(a: int, b: int = 10) -> int {
    a + b
}
function greet(name: string = "Ada") -> string {
    name
}
function flag(x: bool = !false) -> int {
    if x { 1 } else { 0 }
}
function neg(n: int = -1) -> int {
    n
}
function main() -> int {
    add(5) + add(5, 1) + flag() + neg() + if greet() == "Ada" { 2 } else { 0 }
}
main()
"#,
    );
    assert!(
        generated.contains("def __tpz_default__t_616464_1__t_62(host):")
            && generated.contains("return 10")
            && generated.contains("def _t_616464(host, _t_61, _t_62=__tpz_missing):")
            && generated.contains("_t_62 = __tpz_default__t_616464_1__t_62(host)"),
        "{generated}"
    );
    assert!(
        generated.contains("def __tpz_default__t_6772656574_0__t_6e616d65(host):")
            && generated.contains("return \"Ada\"")
            && generated.contains("def _t_6772656574(host, _t_6e616d65=__tpz_missing):"),
        "{generated}"
    );
    assert!(
        generated.contains("def __tpz_default__t_666c6167_0__t_78(host):")
            && generated.contains("return not tpz_condition(False"),
        "{generated}"
    );
    assert!(
        generated.contains("def __tpz_default__t_6e6567_0__t_6e(host):")
            && generated.contains("return tpz_neg(1"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("function default Python gate failed: {e}"));
}

#[test]
fn emits_function_default_scalar_expr_and_required_after_default() {
    let generated = emit_source(
        r#"
function add(a: int = 1 + 2, b: int) -> int {
    a + b
}
function mix(a: int = 1, b: int, c: int = 2) -> int {
    a * 100 + b * 10 + c
}
function main() -> int {
    add(4, 5) + add(b: 6) + mix(1, 2, 3) + mix(b: 4)
}
main()
"#,
    );
    assert!(
        generated.contains("__tpz_missing = object()"),
        "{generated}"
    );
    assert!(
        generated.contains("def __tpz_default__t_616464_0__t_61(host):")
            && generated.contains("return tpz_add(1, 2")
            && generated.contains("def _t_616464(host, _t_61=__tpz_missing, "),
        "{generated}"
    );
    assert!(
        generated.contains("_t_62=__tpz_missing")
            && generated.contains("missing argument for parameter `b`"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("function default expression Python gate failed: {e}"));
}

#[test]
fn emits_function_default_identifier_thunks_in_defining_scope() {
    let top_level = emit_source(
        r#"
let A = 10
function f(A: int, x: int = A) -> int {
    x
}
f(99)
"#,
    );
    assert!(
        top_level.contains("def __tpz_default__t_66_1__t_78(host):")
            && top_level.contains("return _t_41")
            && top_level.contains("def _t_66(host, _t_41, _t_78=__tpz_missing):")
            && top_level.contains("_t_78 = __tpz_default__t_66_1__t_78(host)"),
        "{top_level}"
    );
    assert_generated_python_ok_int(&top_level, 10, "top-level identifier function default");

    let nested = emit_source(
        r#"
function outer() -> int {
    let A = 5
    function inner(A: int, x: int = A) -> int {
        x
    }
    inner(9)
}
outer()
"#,
    );
    assert!(
        nested.contains("def __tpz_default__t_696e6e6572_1__t_78(host):")
            && nested.contains("return _t_41")
            && nested.contains("def _t_696e6e6572(_t_41, _t_78=__tpz_missing):")
            && nested.contains("_t_78 = __tpz_default__t_696e6e6572_1__t_78(host)"),
        "{nested}"
    );
    assert_generated_python_ok_int(&nested, 5, "nested identifier function default");
}

#[test]
fn emits_known_function_named_calls_without_reordering_effects() {
    let generated = emit_source(
        r#"
function mark(label: string, n: int) -> int {
    print(label)
    n
}
function sub(a: int, b: int) -> int {
    a - b
}
function mix(a: int, b: int = 10, c: int = 1) -> int {
    a + b + c
}
function main() -> int {
    sub(b: mark("b", 3), a: mark("a", 10)) + mix(c: mark("c", 3), a: mark("x", 2)) + mix(1, c: 2)
}
main()
"#,
    );
    assert!(
        generated.contains(
            "_t_737562(host, _t_62=_t_6d61726b(host, \"b\", 3), _t_61=_t_6d61726b(host, \"a\", 10))"
        ),
        "{generated}"
    );
    assert!(
        generated.contains(
            "_t_6d6978(host, _t_63=_t_6d61726b(host, \"c\", 3), _t_61=_t_6d61726b(host, \"x\", 2))"
        ),
        "{generated}"
    );
    assert!(
        generated.contains("_t_6d6978(host, 1, _t_63=2)"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("named function call Python gate failed: {e}"));
}

#[test]
fn emits_variadic_function_calls_and_spread_tail() {
    let generated = emit_source(
        r#"
function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function mark(label: string, text: string) -> string {
    print(label)
    text
}
function join(prefix: string = "v", ...xs: int) -> string {
    let mut out = prefix
    for x in xs {
        out = "{out}:{x}"
    }
    out
}
function main() -> string {
    let a = sum(1, 2, 3)
    let b = sum(1, ...[4, 5])
    let c = sum(6, ...[7, 8], 9)
    let d = join("n", 1, 2)
    let e = join(...[3, 4])
    let f = join(...[1, 2], prefix: mark("prefix", "n"))
    let g = 1 |> sum(...[2, 3])
    let h = 10 |> sum(1, ...[2, 3])
    "{a}:{b}:{c}:{d}:{e}:{f}:{g}:{h}"
}
main()
"#,
    );
    assert!(
        generated.contains("def _t_73756d(host, _t_73656564, _t_7873=None):"),
        "{generated}"
    );
    assert!(generated.contains("if _t_7873 is None:"), "{generated}");
    assert!(
        generated.contains(
            "_t_73756d(host, _t_73656564=__tpz_vararg_0, _t_7873=[__tpz_vararg_1, __tpz_vararg_2])"
        ),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_spread_values([4, 5],"),
        "{generated}"
    );
    assert!(
        generated.contains("_t_6a6f696e(host, _t_7873=[*__tpz_vararg_0])"),
        "{generated}"
    );
    assert!(
        generated.contains(
            "_t_6a6f696e(host, _t_707265666978=__tpz_vararg_1, _t_7873=[*__tpz_vararg_0])"
        ),
        "{generated}"
    );
    assert!(
        generated.contains("_t_73756d(host, _t_73656564=__tpz_piped, _t_7873=[*__tpz_vararg_0])"),
        "{generated}"
    );
    assert!(
        generated.contains(
            "_t_73756d(host, _t_73656564=__tpz_piped, _t_7873=[__tpz_vararg_0, *__tpz_vararg_1])"
        ),
        "{generated}"
    );
    let spread_placeholder = emit_source(
        r#"
function sum(seed: int, ...xs: int) -> int {
    seed
}
function main() -> int {
    1 |> sum(..._)
}
main()
"#,
    );
    assert!(
        spread_placeholder.contains("tpz_call_order_fault([__tpz_vararg_0],")
            && spread_placeholder
                .contains("a spread argument cannot skip an unsatisfied fixed parameter (§5)"),
        "{spread_placeholder}"
    );
    assert_generated_python_gates(&spread_placeholder)
        .unwrap_or_else(|e| panic!("variadic pipe spread placeholder Python gate failed: {e}"));
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("variadic function Python gate failed: {e}"));
}

#[test]
fn emits_loop_expression_in_pipe_arguments() {
    let generated = emit_source(
        r#"
function add(a: int, b: int) -> int {
    a + b
}
function sub(a: int, b: int) -> int {
    a - b
}
function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function main() -> int {
	    let a = loop {
	        print("lhs")
	        break 10
	    } |> add(loop {
	        print("arg")
	        break 2
	    })
	    let nested = loop {
	        print("nested-lhs")
	        break 4
	    } |> add(_, loop {
	        print("nested-arg")
	        break 6
	    } + _)
	    let b = 10 |> sub(b: loop {
	        print("named")
	        break 3
    })
    let c = loop {
        print("spread-lhs")
        break 1
    } |> sum(...loop {
        print("spread")
        break [2, 3]
    })
	    print("{a}:{nested}:{b}:{c}")
	    a + nested + b + c
	}
	main()
	"#,
    );
    assert!(
        generated.contains("__tpz_pipe_value_"),
        "statement-lowered pipe should bind the piped value once: {generated}"
    );
    assert!(
        generated.contains("_t_616464(host, __tpz_pipe_value_"),
        "pipe positional call should still insert the piped value first: {generated}"
    );
    assert!(
        generated.contains("tpz_add(__tpz_expr_value_")
            && generated.contains("__tpz_pipe_value_")
            && generated.contains("_t_616464(host, __tpz_pipe_value_"),
        "statement-lowered nested placeholders should combine lowered args with the piped value: {generated}"
    );
    assert!(
        generated.contains("_t_737562(host, __tpz_pipe_value_")
            && generated.contains("_t_62=__tpz_expr_value_"),
        "pipe named call should keep first-arg insertion and lower named loop values: {generated}"
    );
    assert!(
        generated.contains("tpz_spread_values(__tpz_call_arg_")
            && generated.contains("_t_73756d(host, _t_73656564=__tpz_pipe_value_"),
        "pipe variadic spread call should lower spread loop values before the call: {generated}"
    );
    assert!(
        generated.contains("raise TpzLoopBreak(None, 10)")
            && generated.contains("raise TpzLoopBreak(None, 2)")
            && generated.contains("raise TpzLoopBreak(None, [2, 3])"),
        "pipe loop break payloads should stay explicit in generated Python: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("nested loop pipe argument Python gate failed: {e}"));
}

#[test]
fn emits_loop_expression_in_pipe_receiver_hof_stages() {
    let generated = emit_source(
        r#"
function add(a: int, b: int) -> int {
    a + b
}
function inc(x: int) -> int {
    x + 1
}
function main() -> int {
    let xs = [1, 2, 3]
    let m = map { "a": 1, "b": 2 }
    let mut mu = map { "a": 1 }
    let mapped = loop {
        print("map-callback")
        break (x) => x + 1
    } |> xs.map()
    let reduced = loop {
        print("initial")
        break 0
    } |> xs.reduce(add)
    let kept = loop {
        print("filter-callback")
        break (k, v) => v > 1
    } |> m.filter()
    let updateKey = loop {
        print("key")
        break "b"
    } |> mu.update(_, 7, (v) => v / 0)
    let updateInitial = loop {
        print("initial-two")
        break 5
    } |> mu.update("c", _, inc)
    let updateFunction = inc |> mu.update(loop {
        print("function-key")
        break "a"
    }, 0, _)
    let keptB = kept.getOr("b", 0)
    let muA = mu.getOr("a", 0)
    let muB = mu.getOr("b", 0)
    let muC = mu.getOr("c", 0)
    print("{mapped[0]}:{reduced}:{keptB}:{muA}:{muB}:{muC}")
    mapped[0] + reduced + keptB + muA + muB + muC
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_array_map(_t_7873, __tpz_pipe_arg_"),
        "receiver map pipe should use the statement-lowered piped callback value: {generated}"
    );
    assert!(
        generated.contains("tpz_array_reduce(_t_7873, __tpz_pipe_arg_")
            && generated.contains(
                "(lambda __tpz_cb_0, __tpz_cb_1: _t_616464(host, __tpz_cb_0, __tpz_cb_1))"
            ),
        "receiver reduce pipe should bind the piped initial value and adapt the callback argument: {generated}"
    );
    assert!(
        generated.contains("tpz_map_filter(_t_6d, __tpz_pipe_arg_"),
        "Map.filter pipe should use the statement-lowered piped callback value: {generated}"
    );
    assert!(
        generated.contains("tpz_map_update(_t_6d75, __tpz_pipe_arg_")
            && generated.contains(" = \"c\"")
            && generated.contains("tpz_map_update(_t_6d75, __tpz_pipe_arg_"),
        "Map.update pipe should bind loop-produced key and initial placeholders: {generated}"
    );
    assert!(
        generated.contains("(lambda __tpz_cb_0: _t_696e63(host, __tpz_cb_0))"),
        "statement-lowered receiver pipe should still adapt top-level function placeholders: {generated}"
    );
    assert!(
        generated.contains("raise TpzLoopBreak(None, 0)")
            && generated.contains("raise TpzLoopBreak(None, \"b\")")
            && generated.contains("raise TpzLoopBreak(None, 5)"),
        "receiver pipe loop break payloads should stay explicit in generated Python: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("nested loop pipe receiver HOF Python gate failed: {e}"));
}

#[test]
fn emits_loop_expression_in_optional_pipe_receiver_hof_stages() {
    let generated = emit_source(
        r#"
function add(a: int, b: int) -> int {
    a + b
}
function inc(x: int) -> int {
    x + 1
}
function main() -> int {
    let xs: Option<Array<int>> = Some([1, 2, 3])
    let noneXs: Option<Array<int>> = None
    let mut m: Option<Map<string, int>> = Some(map { "a": 1 })
    let mut noneMap: Option<Map<string, int>> = None
    let mapped = loop {
        print("map-callback")
        break (x) => x + 1
    } |> xs?.map()
    let skipped = loop {
        print("none-callback")
        break (x) => x / 0
    } |> noneXs?.map()
    let reduced = loop {
        print("initial")
        break 0
    } |> xs?.reduce(add)
    let updateKey = loop {
        print("key")
        break "b"
    } |> m?.update(_, 7, (v) => v / 0)
    let skippedKey = inc |> noneMap?.update(loop {
        print("skip-key")
        break "z"
    }, 0, _)
    let updateFunction = inc |> m?.update(loop {
        print("function-key")
        break "a"
    }, 0, _)
    let mappedA = match mapped {
        case Some(ys) => ys[0]
        case None => 0
    }
    let skippedValue = match skipped {
        case Some(_) => 100
        case None => 1
    }
    let reducedValue = match reduced {
        case Some(n) => n
        case None => 0
    }
    let mapValue = match m {
        case Some(mm) => mm.getOr("a", 0) + mm.getOr("b", 0)
        case None => 0
    }
    let skippedKeyValue = match skippedKey {
        case Some(_) => 100
        case None => 1
    }
    mappedA + skippedValue + reducedValue + mapValue + skippedKeyValue
}
main()
"#,
    );
    assert!(
        generated.contains("if __tpz_optional_receiver_")
            && generated.contains(" is None:")
            && generated.contains(" = None"),
        "optional pipe receiver HOF should keep a None short-circuit branch: {generated}"
    );
    assert!(
        generated.contains("tpz_wrap_optional(tpz_array_map(")
            && generated.contains("tpz_array_map(")
            && generated.contains("__tpz_pipe_arg_"),
        "optional map pipe should wrap Some inner calls and use the statement-lowered piped callback: {generated}"
    );
    assert!(
        generated.contains("tpz_wrap_optional(tpz_array_reduce(")
            && generated.contains(
                "(lambda __tpz_cb_0, __tpz_cb_1: _t_616464(host, __tpz_cb_0, __tpz_cb_1))"
            ),
        "optional reduce pipe should bind the loop-produced initial value and adapt the callback: {generated}"
    );
    assert!(
        generated.contains("tpz_wrap_optional_unit(tpz_map_update(")
            && generated.contains(", __tpz_pipe_arg_")
            && generated.contains("(lambda __tpz_cb_0: _t_696e63(host, __tpz_cb_0))"),
        "optional Map.update pipe should bind loop-produced key/initial slots and adapt function placeholders: {generated}"
    );
    assert!(
        generated.contains("raise TpzLoopBreak(None, \"z\")")
            && generated.contains("__tpz_optional_receiver_")
            && generated.contains("host.print(\"skip-key\""),
        "optional Map.update explicit loop args should stay branch-local behind the None check: {generated}"
    );
    assert_generated_python_gates(&generated).unwrap_or_else(|e| {
        panic!("nested loop optional pipe receiver HOF Python gate failed: {e}")
    });
}

#[test]
fn emits_loop_expression_in_optional_pipe_receiver_hof_spread_faults() {
    let generated = emit_source(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
    print(label)
    xs
}
function inc(x: int) -> int {
    x + 1
}
function main() -> string {
    let noneXs: Option<Array<int>> = None
    let xs: Option<Array<int>> = Some([1])
    let mut m: Option<Map<string, int>> = Some(map { "a": 1 })
    let skipped = loop {
        print("pipe-none")
        break (x) => x + 1
    } |> noneXs?.map(...loop {
        print("skip-spread")
        break mark("skip", [0])
    })
    let spreadFault = loop {
        print("pipe-some")
        break (x) => x + 1
    } |> xs?.map(...loop {
        print("spread")
        break mark("spread", [0])
    })
    let updateFault = inc |> m?.update(loop {
        print("key")
        break "a"
    }, 0, _, ...loop {
        print("update-spread")
        break mark("update", [0])
    })
    "done"
}
main()
"#,
    );
    assert!(
        generated.contains("if __tpz_optional_receiver_")
            && generated.contains(" = None")
            && generated.contains("tpz_nonvariadic_static_spread_call("),
        "optional pipe spread fault should lower through explicit optional branches: {generated}"
    );
    assert!(
        generated.contains("tpz_spread_values(__tpz_expr_value_")
            && generated.contains("raise TpzLoopBreak(None, _t_6d61726b(host, \"spread\", [0]))"),
        "optional pipe spread operands should be statement-lowered inside the active branch: {generated}"
    );
    assert!(
        generated.contains("(lambda __tpz_cb_0: _t_696e63(host, __tpz_cb_0))"),
        "optional Map.update pipe spread should still adapt top-level function placeholders: {generated}"
    );
    assert!(
        generated.contains("host.print(\"skip-spread\"")
            && generated.contains("if __tpz_optional_receiver_"),
        "None receiver spread operands should remain branch-local in generated code: {generated}"
    );
    assert_generated_python_gates(&generated).unwrap_or_else(|e| {
        panic!("nested loop optional pipe receiver HOF spread Python gate failed: {e}")
    });
}
