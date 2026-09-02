use super::*;

#[test]
fn emits_loop_expression_value_and_labeled_control() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut sum = 0
    let mut o = 0
    loop 'outer {
        o = o + 1
        if o > 3 {
            break
        }
        let mut i = 0
        loop {
            i = i + 1
            if i > 5 {
                break
            }
            if i == 2 {
                continue 'outer
            }
            sum = sum + i
        }
    }
    let value = loop {
        break sum
    }
    value
}
main()
"#,
    );
    assert!(
        generated.contains("raise TpzLoopBreak(None, _t_73756d)"),
        "loop break value should raise through the Topaz loop boundary: {generated}"
    );
    assert!(
        generated.contains("raise TpzLoopContinue(\"outer\")"),
        "labeled continue should be carried to the labeled loop boundary: {generated}"
    );
    assert!(
        generated.contains("except TpzLoopBreak as"),
        "loop expression should install a break boundary: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("loop expression Python gate failed: {e}"));
}

#[test]
fn emits_defer_scope_drain_across_loop_break_value() {
    let generated = emit_source(
        r#"
function breakValue() -> int {
    print("eval-value")
    7
}
function main() -> int {
    let r = loop {
        defer print("defer")
        break breakValue()
    }
    print("r={r}")
    0
}
main()
"#,
    );
    assert!(
        generated.contains("__tpz_run_defers_to("),
        "defer scopes should drain to a saved mark: {generated}"
    );
    assert!(
        generated.contains("except (TpzReturn, TpzLoopBreak, TpzLoopContinue):"),
        "defer scopes should drain Topaz control-flow exits: {generated}"
    );
    assert!(
        generated.contains("raise TpzLoopBreak(None, _t_627265616b56616c7565(host))"),
        "break values should be evaluated before Topaz loop control unwinds: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("loop defer-scope Python gate failed: {e}"));
}

#[test]
fn statement_lowered_defer_action_runs_at_scope_drain_with_outer_writes() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut value = 0
    if true {
        defer {
            value = 2
            value
        }
        if value == 0 {
            value = 1
        } else {
            value = 99
        }
    }
    value
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        2,
        "statement-lowered defer registration, scope drain, and outer write parity",
    );
}

#[test]
fn block_local_const_binds_in_place_and_is_visible_to_nested_functions() {
    let generated = emit_source(
        r#"
function main() -> int {
    const base: int = 40
    function addTwo() -> int {
        base + 2
    }
    addTwo()
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        42,
        "block-local const evaluation, binding, and immutable capture parity",
    );
}

#[test]
fn emits_loop_expression_in_nested_value_positions() {
    let generated = emit_source(
        r#"
function nested() -> int {
    return 1 + loop {
        break 2
    }
}
function main() -> int {
    let a = 10 + loop {
        break 5
    }
    let xs = [loop {
        break a - 12
    }, 4]
    let r = {
        left: xs[0],
        right: loop {
            break xs[1] + nested()
        }
    }
    print("{r.left}:{r.right}")
    r.left + r.right
}
main()
"#,
    );
    assert!(
        generated.contains("raise TpzLoopBreak(None, 2)"),
        "nested return loop expression should still lower through loop control: {generated}"
    );
    assert!(
        generated.contains("_tr_6c656674_7269676874("),
        "record fields around nested loop values should construct the structural record: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("nested loop expression Python gate failed: {e}"));
}

#[test]
fn emits_loop_expression_in_known_call_arguments() {
    let generated = emit_source(
        r#"
function combine(a: int, b: int) -> int {
    print("combine {a}:{b}")
    a * 10 + b
}
function main() -> int {
    let r = combine(loop {
        print("arg-a")
        break 2
    }, b: loop {
        print("arg-b")
        break 3
    })
    print("{r}")
    r
}
main()
"#,
    );
    assert!(
        generated.contains("_t_636f6d62696e65(host"),
        "nested loop arguments should lower into a known function call: {generated}"
    );
    assert!(
        generated.contains("raise TpzLoopBreak(None, 2)")
            && generated.contains("raise TpzLoopBreak(None, 3)"),
        "both call arguments should still lower through loop-control values: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("nested loop call argument Python gate failed: {e}"));
}

#[test]
fn emits_loop_expression_in_variadic_call_arguments() {
    let generated = emit_source(
        r#"
function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function main() -> int {
    let a = sum(loop {
        print("seed")
        break 1
    }, loop {
        print("tail-a")
        break 2
    }, loop {
        print("tail-b")
        break 3
    })
    let b = sum(loop {
        print("seed-b")
        break 4
    }, ...loop {
        print("spread-b")
        break [5, 6]
    }, loop {
        print("tail-c")
        break 7
    })
    let c = sum(...loop {
        print("spread-c")
        break [8, 9]
    }, seed: loop {
        print("seed-c")
        break 10
    })
    print("{a}:{b}:{c}")
    a + b + c
}
main()
"#,
    );
    assert!(
        generated.contains("_t_73756d(host"),
        "nested loop variadic arguments should lower into known variadic function calls: {generated}"
    );
    assert!(
        generated.contains("call_arg") && generated.contains("call_spread"),
        "variadic arguments should bind source-order temporaries before the final call: {generated}"
    );
    assert!(
        generated.contains("tpz_spread_values("),
        "spread tail values should be checked before the variadic call: {generated}"
    );
    assert!(
        generated.contains("raise TpzLoopBreak(None, 1)")
            && generated.contains("raise TpzLoopBreak(None, 7)")
            && generated.contains("raise TpzLoopBreak(None, 10)"),
        "fixed, tail, and named variadic arguments should lower loop-control values: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("nested loop variadic argument Python gate failed: {e}"));
}

#[test]
fn emits_loop_expression_in_nonvariadic_spread_fault_arguments() {
    let generated = emit_source(
        r#"
function f(a: int, b: int = 0) -> int {
    a + b
}
function main() -> int {
    f(loop {
        print("prefix")
        break 1
    }, ...loop {
        print("spread")
        break [2]
    }, b: loop {
        print("named")
        break 3
    })
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_nonvariadic_spread_call("),
        "nonvariadic known-function spread with nested loops should lower to the shared fault helper: {generated}"
    );
    assert!(
        generated.contains("tpz_spread_values("),
        "spread values should be checked before the nonvariadic fault helper: {generated}"
    );
    assert!(
        generated.contains("raise TpzLoopBreak(None, 1)")
            && generated.contains("raise TpzLoopBreak(None, [2])")
            && generated.contains("raise TpzLoopBreak(None, 3)"),
        "prefix, spread, and named nonvariadic spread arguments should lower loop values: {generated}"
    );
    assert_generated_python_gates(&generated).unwrap_or_else(|e| {
        panic!("nested loop nonvariadic spread argument Python gate failed: {e}")
    });

    let static_generated = emit_source(
        r#"
function main() -> () {
    print(...loop {
        print("static-spread")
        break ["x"]
    })
}
main()
"#,
    );
    assert!(
        static_generated.contains("tpz_nonvariadic_static_spread_call(")
            && static_generated.contains("tpz_spread_values("),
        "static free-builtin spread with nested loops should lower to the shared static fault helper: {static_generated}"
    );
    assert_generated_python_gates(&static_generated)
        .unwrap_or_else(|e| panic!("nested loop static spread argument Python gate failed: {e}"));

    let order_fault = emit_unchecked_source(
        r#"
function main() -> () {
    print(value: loop {
        print("named")
        break "x"
    }, ...loop {
        print("spread")
        break ["y"]
    })
}
main()
"#,
    );
    assert!(
        order_fault.contains("tpz_call_order_fault(")
            && order_fault.contains("named arguments must follow spread arguments (§5)")
            && !order_fault.contains("*tpz_spread_values("),
        "named-before-spread order faults should evaluate the spread expression but not spread it: {order_fault}"
    );
    assert_generated_python_gates(&order_fault).unwrap_or_else(|e| {
        panic!("nested loop static spread order fault Python gate failed: {e}")
    });
}

#[test]
fn emits_loop_expression_in_receiver_spread_fault_arguments() {
    let read_only = emit_source(
        r#"
function makeArray() -> Array<int> {
    print("receiver")
    [10]
}
function main() -> Option<int> {
    makeArray().get(...loop {
        print("spread")
        break [0]
    })
}
main()
"#,
    );
    assert!(
        read_only.contains("call_recv")
            && read_only.contains("tpz_nonvariadic_static_spread_call(")
            && read_only.contains("tpz_spread_values("),
        "receiver read-only spread fault with nested loops should evaluate the receiver then route through the shared static fault helper: {read_only}"
    );
    assert!(
        read_only.contains("raise TpzLoopBreak(None, [0])"),
        "receiver read-only spread argument should lower the nested loop value: {read_only}"
    );
    assert_generated_python_gates(&read_only).unwrap_or_else(|e| {
        panic!("nested loop receiver read-only spread fault Python gate failed: {e}")
    });

    let callback = emit_source(
        r#"
function main() -> Array<int> {
    [1].map(...loop {
        print("callback-spread")
        break [0]
    })
}
main()
"#,
    );
    assert!(
        callback.contains("call_recv")
            && callback.contains("tpz_nonvariadic_static_spread_call(")
            && callback.contains("tpz_spread_values("),
        "receiver callback spread fault with nested loops should lower to the shared static fault helper: {callback}"
    );
    assert_generated_python_gates(&callback).unwrap_or_else(|e| {
        panic!("nested loop receiver callback spread fault Python gate failed: {e}")
    });

    let optional = emit_source(
        r#"
function main() -> string {
    let none: Option<Array<int>> = None
    let some: Option<Array<int>> = Some([10])
    let skipped = none?.get(...loop {
        print("skip")
        break [0]
    })
    let hit = some?.get(...loop {
        print("hit")
        break [0]
    })
    "{skipped}:{hit}"
}
main()
"#,
    );
    assert!(
        optional.contains("if ")
            && optional.contains(" is None:")
            && optional
                .matches("tpz_nonvariadic_static_spread_call(")
                .count()
                >= 2
            && optional.contains("tpz_spread_values("),
        "optional receiver spread fault should keep None short-circuit structure and lower Some/direct spread faults: {optional}"
    );
    assert_generated_python_gates(&optional).unwrap_or_else(|e| {
        panic!("nested loop optional receiver spread fault Python gate failed: {e}")
    });

    let mutating = emit_source(
        r#"
function main() -> Array<int> {
    let mut xs = [1]
    xs.push(...loop {
        print("push-spread")
        break [2]
    })
    xs
}
main()
"#,
    );
    assert!(
        mutating.contains("call_recv")
            && mutating.contains("tpz_nonvariadic_static_spread_call(")
            && mutating.contains("tpz_spread_values("),
        "mutating receiver spread fault with nested loops should evaluate the receiver then route through the shared static fault helper: {mutating}"
    );
    assert_generated_python_gates(&mutating).unwrap_or_else(|e| {
        panic!("nested loop mutating receiver spread fault Python gate failed: {e}")
    });
}

#[test]
fn emits_loop_expression_in_static_builtin_arguments() {
    let generated = emit_source(
        r#"
function main() -> int {
    print(loop {
        print("print-arg")
        break "hello"
    })
    let parsedOption = toInt(text: loop {
        print("to-int")
        break "42"
    })
    let parsed = parsedOption ?? 0
    let maybe = Some(loop {
        break 7
    })
    let unwrapped = match maybe {
        case Some(n) => n
        case None => 0
    }
    let encoded = Bytes.encodeUtf8(s: loop {
        break "az"
    })
    let joined = Bytes.concat(b: loop {
        print("bytes-b")
        break Encoding.utf8Encode(text: "b")
    }, a: loop {
        print("bytes-a")
        break encoded
    })
    print("{parsed}:{unwrapped}:{Encoding.hexEncode(bytes: joined)}")
    parsed + unwrapped
}
main()
"#,
    );
    assert!(
        generated.contains("host.print(") && generated.contains("tpz_to_int("),
        "free builtin nested loop arguments should lower into static calls: {generated}"
    );
    assert!(
        generated.contains("tpz_bytes_encode_utf8(")
            && generated.contains("tpz_bytes_concat(")
            && generated.contains("tpz_bytes_to_hex("),
        "namespace builtin nested loop arguments should lower into static calls: {generated}"
    );
    assert!(
        generated.contains("raise TpzLoopBreak(None, \"hello\")")
            && generated.contains("raise TpzLoopBreak(None, \"42\")")
            && generated.contains("raise TpzLoopBreak(None, 7)"),
        "builtin call arguments should still lower loop break values: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("nested loop builtin call argument Python gate failed: {e}"));
}

#[test]
fn emits_loop_expression_in_receiver_call_arguments() {
    let generated = emit_source(
        r#"
function add(a: int, b: int) -> int {
    a + b
}
function inc(x: int) -> int {
    x + 1
}
function main() -> int {
    let xs = [10, 20, 30]
    let pickedOption = xs.get(i: loop {
        print("idx")
        break 1
    })
    let picked = pickedOption ?? 0
    let m = map { "a": 1, "b": 2 }
    let got = m.getOr(default: loop {
        print("default")
        break 9
    }, k: loop {
        print("key")
        break "b"
    })
    let bytes = Bytes.encodeUtf8(s: "abcd")
    let sliced = bytes.slice(end: loop {
        print("end")
        break 3
    }, start: loop {
        print("start")
        break 1
    })
    let text = match sliced.decodeUtf8() {
        case Ok(s) => s
        case Err(_) => ""
    }
    let reduced = xs.reduce(initial: loop {
        print("initial")
        break 5
    }, f: add)
    let mut mu = map { "a": 1 }
    mu.update(k: loop {
        print("update-key")
        break "b"
    }, initial: loop {
        print("update-initial")
        break 4
    }, f: inc)
    let updated = mu.getOr("b", 0)
    print("{picked}:{got}:{text}:{reduced}:{updated}")
    picked + got + reduced + updated
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_get(")
            && generated.contains("tpz_map_get_or(")
            && generated.contains("tpz_bytes_slice("),
        "read-only receiver nested loop arguments should lower into receiver calls: {generated}"
    );
    assert!(
        generated.contains("tpz_array_reduce(") && generated.contains("tpz_map_update("),
        "callback receiver non-callback nested loop arguments should lower into receiver calls: {generated}"
    );
    assert!(
        generated.contains("raise TpzLoopBreak(None, 1)")
            && generated.contains("raise TpzLoopBreak(None, \"b\")")
            && generated.contains("raise TpzLoopBreak(None, 5)"),
        "receiver call arguments should still lower loop break values: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("nested loop receiver call argument Python gate failed: {e}"));
}

#[test]
fn emits_loop_expression_in_optional_receiver_call_arguments() {
    let generated = emit_source(
        r#"
function add(a: int, b: int) -> int {
    a + b
}
function inc(x: int) -> int {
    x + 1
}
function main() -> int {
    let xs: Option<Array<int>> = Some([10, 20, 30])
    let noneXs: Option<Array<int>> = None
    let pickedOption = xs?.get(i: loop {
        print("idx")
        break 1
    })
    let picked = pickedOption ?? 0
    let skippedOption = noneXs?.get(i: loop {
        print("skip")
        break 0
    })
    let skipped = skippedOption ?? 7
    let m: Option<Map<string, int>> = Some(map { "a": 1, "b": 2 })
    let gotOption = m?.getOr(loop {
        print("key")
        break "b"
    }, loop {
        print("default")
        break 9
    })
    let got = gotOption ?? 0
    let bytes: Option<Bytes> = Some(Bytes.encodeUtf8(s: "abcd"))
    let sliced = bytes?.slice(loop {
        print("start")
        break 1
    }, loop {
        print("end")
        break 3
    })
    let text = match sliced {
        case Some(b) => match b.decodeUtf8() {
            case Ok(s) => s
            case Err(_) => ""
        }
        case None => ""
    }
    let reducedOption = xs?.reduce(loop {
        print("initial")
        break 5
    }, add)
    let reduced = reducedOption ?? 0
    let mut om: Option<Map<string, int>> = Some(map { "a": 1 })
    om?.update(loop {
        print("update-key")
        break "b"
    }, loop {
        print("update-initial")
        break 4
    }, inc)
    let updated = match om {
        case Some(mm) => mm.getOr("b", 0)
        case None => 0
    }
    print("{picked}:{skipped}:{got}:{text}:{reduced}:{updated}")
    picked + skipped + got + reduced + updated
}
main()
"#,
    );
    assert!(
        generated.contains("if isinstance(") && generated.contains(" is None:"),
        "optional receiver nested loop arguments should lower through explicit short-circuit branches: {generated}"
    );
    assert!(
        generated.contains("tpz_wrap_optional(tpz_get(")
            && generated.contains("tpz_wrap_optional(tpz_map_get_or(")
            && generated.contains("tpz_wrap_optional(tpz_bytes_slice("),
        "optional read-only receiver calls should rewrap Some branch values: {generated}"
    );
    assert!(
        generated.contains("tpz_wrap_optional(tpz_array_reduce(")
            && generated.contains("tpz_wrap_optional_unit(tpz_map_update("),
        "optional callback receiver calls should rewrap value/unit branches: {generated}"
    );
    assert!(
        generated.contains("raise TpzLoopBreak(None, \"b\")")
            && generated.contains("raise TpzLoopBreak(None, 5)")
            && generated.contains("raise TpzLoopBreak(None, 4)"),
        "optional receiver call arguments should still lower loop break values: {generated}"
    );
    assert_generated_python_gates(&generated).unwrap_or_else(|e| {
        panic!("nested loop optional receiver call argument Python gate failed: {e}")
    });
}

#[test]
fn while_statement_reevaluates_statement_lowered_condition_each_iteration() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut count = 0
    while loop {
        count += 1
        break count < 4
    } {
        count += 0
    }
    count
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        4,
        "statement-lowered while condition per-iteration parity",
    );
}

#[test]
fn if_statement_accepts_statement_lowered_condition() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut touched = 0
    if loop {
        touched = 1
        break true
    } {
        touched += 2
    } else {
        touched = 100
    }
    touched
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        3,
        "statement-lowered if condition order and branch parity",
    );
}

#[test]
fn conditional_value_lowering_isolates_callback_metadata_per_branch() {
    let generated = emit_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function sum(a: int, b: int) -> int { a + b }
function main() -> int {
    let mut fromIf = [first]
    let ifValue = if false {
        fromIf[0] = second
        0
    } else {
        [0].map(fromIf[0])[0]
    }

    let mut fromAnd = [first]
    let ignoredAnd = false && {
        fromAnd[0] = second
        true
    }
    let andValue = [0].map(fromAnd[0])[0]

    let mut fromOr = [first]
    let ignoredOr = true || {
        fromOr[0] = second
        false
    }
    let orValue = [0].map(fromOr[0])[0]

    let mut fromCoalesce = [first]
    let ignoredCoalesce = Some(1) ?? {
        fromCoalesce[0] = second
        0
    }
    let coalesceValue = [0].map(fromCoalesce[0])[0]

    let mut fromMatch = [first]
    let matchValue = match 2 {
        case 1 => {
            fromMatch[0] = second
            0
        }
        case 2 => [0].map(fromMatch[0])[0]
        case _ => 0
    }

    ifValue * 10000 + andValue * 1000 + orValue * 100 + coalesceValue * 10 + matchValue
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        11_111,
        "conditional value branch callback metadata isolation",
    );
}

#[test]
fn optional_receiver_call_branches_isolate_argument_metadata() {
    let generated = emit_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function main() -> int {
    let noneXs: Option<Array<int>> = None

    let mut directCallbacks = [first]
    let skippedDirect = noneXs?.get(i: {
        directCallbacks[0] = second
        0
    })
    let directValue = [0].map(directCallbacks[0])[0]

    let mut pipeCallbacks = [first]
    let skippedPipe = 0 |> noneXs?.reduce({
        pipeCallbacks[0] = second
        sum
    })
    let pipeValue = [0].map(pipeCallbacks[0])[0]

    directValue * 10 + pipeValue
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        11,
        "optional receiver call branch argument metadata isolation",
    );
}

#[test]
fn value_iteration_paths_isolate_zero_and_filtered_body_metadata() {
    let generated = emit_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function main() -> int {
    let empty: Array<int> = []

    let mut fromValueFor = { callback: first }
    let ignoredValueFor = for x in empty {
        fromValueFor.callback = second
        x
    }
    let valueForResult = [0].map(fromValueFor.callback)[0]

    let mut fromEmptyComprehension = { callback: first }
    let ignoredEmptyComprehension = [ for x in empty => {
        fromEmptyComprehension.callback = second
        x
    } ]
    let emptyComprehensionResult = [0].map(fromEmptyComprehension.callback)[0]

    let mut fromFilteredBody = { callback: first }
    let ignoredFilteredBody = [ for x in [1] if false => {
        fromFilteredBody.callback = second
        x
    } ]
    let filteredBodyResult = [0].map(fromFilteredBody.callback)[0]

    let mut fromNestedIterator = { callback: first }
    let ignoredNestedIterator = [ for x in empty for y in {
        fromNestedIterator.callback = second
        [x]
    } => y ]
    let nestedIteratorResult = [0].map(fromNestedIterator.callback)[0]

    valueForResult * 1000 + emptyComprehensionResult * 100 + filteredBodyResult * 10 + nestedIteratorResult
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        1_111,
        "value iteration zero and filtered body metadata isolation",
    );
}
