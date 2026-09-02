use super::*;

#[test]
fn emits_namespace_member_alias_array_and_record_function_value_metadata() {
    let direct_calls = emit_source_with_files(
        r#"
import util

function local(a: int, b: int = 2) -> int {
    a - b
}

function main() -> int {
    let callbacks = util.callbacks
    let chained = callbacks
    let handlers = util.handlers
    let chainedHandlers = handlers
    let base = util.base
    let spread = [...base, local]
    callbacks[0](a: 5) + chained[1](b: 3, a: 10) + handlers.primary(a: 7) + chainedHandlers.second(b: 2, a: 4) + spread[1](a: 6) + spread[2](a: 9)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

function mul(a: int, b: int) -> int {
    a * b
}

export let callbacks = [add, mul]
export let handlers = { primary: add, second: mul }
export let base = [42, add]
"#,
        )],
    );
    assert!(
        direct_calls.contains("tpz_call(tpz_index(")
            && direct_calls.contains("tpz_call(tpz_member("),
        "namespace member aliases should still call through runtime storage: {direct_calls}"
    );
    assert!(
        direct_calls.contains("\"_t_61\": 5")
            && direct_calls.contains("\"_t_62\": 3")
            && direct_calls.contains("\"_t_61\": 10")
            && direct_calls.contains("\"_t_61\": 7")
            && direct_calls.contains("\"_t_62\": 2")
            && direct_calls.contains("\"_t_61\": 4")
            && direct_calls.contains("\"_t_61\": 6")
            && direct_calls.contains("\"_t_61\": 9"),
        "namespace member aliases should preserve callable params: {direct_calls}"
    );
    assert_generated_python_ok_int(
        &direct_calls,
        69,
        "namespace member alias array/record function-value call parity",
    );

    let hof_callbacks = emit_source_with_files(
        r#"
import util

let callbacks = util.callbacks
let chained = callbacks
let handlers = util.handlers
let chainedHandlers = handlers
let spread = [...callbacks]

concurrent {
    arraySlot: {
        let xs = [1]
        xs.map(callbacks[1])
    }
    chainedSlot: {
        let cs = [1]
        cs.map(chained[1])
    }
    recordField: {
        let ys = [1]
        ys.map(chainedHandlers.primary)
    }
    spreadSlot: {
        let zs = [1]
        zs.map(spread[1])
    }
    dynamicPlain: {
        let ws = [1]
        let i = 1
        ws.map(callbacks[i])
    }
    fast: 0
}
0
"#,
        &[(
            "util.tpz",
            r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

export let callbacks = [42, spin]
export let handlers = { primary: spin }
"#,
        )],
    );
    assert!(
        hof_callbacks
            .matches("yield from tpz_array_map__co(")
            .count()
            >= 4
            && hof_callbacks
                .matches("_tpz_mod__t_7574696c___t_7370696e__co(host, __tpz_cb_0)")
                .count()
                >= 4,
        "namespace member alias HOF callbacks should use exported cooperative metadata: {hof_callbacks}"
    );
    assert!(
        hof_callbacks.contains(
            "yield from tpz_array_map__co(_t_7773, tpz_index(_t_63616c6c6261636b73, _t_69,"
        ),
        "namespace member alias dynamic array indexes should use the cooperative runtime driver without static callback recovery: {hof_callbacks}"
    );
    assert_generated_python_gates(&hof_callbacks)
        .unwrap_or_else(|e| panic!("namespace member alias HOF callback Python gate failed: {e}"));
}

#[test]
fn emits_conditional_namespace_member_alias_function_value_metadata() {
    let direct_calls = emit_source_with_files(
        r#"
import util

function main() -> int {
    let f = if true {
        util.add
    } else {
        util.add
    }
    let callbacks = if true {
        util.callbacks
    } else {
        util.callbacks
    }
    let handlers = if false {
        util.handlers
    } else {
        util.handlers
    }
    f(a: 5) + callbacks[1](b: 3, a: 10) + handlers.primary(a: 7)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}

function mul(a: int, b: int) -> int {
    a * b
}

export let add = addImpl
export let callbacks = [42, mul]
export let handlers = { primary: addImpl }
"#,
        )],
    );
    assert!(
        direct_calls.contains("_t_66(_t_61=5)")
            && direct_calls.contains("\"_t_62\": 3")
            && direct_calls.contains("\"_t_61\": 10")
            && direct_calls.contains("\"_t_61\": 7"),
        "conditional namespace-member aliases should preserve identical branch callable metadata: {direct_calls}"
    );
    assert_generated_python_ok_int(
        &direct_calls,
        46,
        "conditional namespace-member alias direct-call parity",
    );

    let forwarded_direct_call = emit_source_with_files(
        r#"
import facade
function main() -> int {
    let f = if true {
        facade.forwarded
    } else {
        facade.forwarded
    }
    f(a: 5)
}
main()
"#,
        &[
            (
                "base.tpz",
                r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
export let forwarded = add
"#,
            ),
            (
                "facade.tpz",
                r#"
import base { forwarded as baseForwarded }
export let forwarded = baseForwarded
"#,
            ),
        ],
    );
    assert!(
        forwarded_direct_call.contains("_t_66(_t_61=5)"),
        "conditional manual-forwarded namespace-member aliases should preserve callable metadata: {forwarded_direct_call}"
    );
    assert_generated_python_ok_int(
        &forwarded_direct_call,
        7,
        "conditional manual-forwarded namespace-member alias direct-call parity",
    );

    let distinct_default_direct_call = emit_source_with_files(
        r#"
import util
function main() -> int {
    let seeds = [1, 0]
    let f = if seeds[0] == 1 {
        util.add
    } else {
        util.sub
    }
    let g = if seeds[1] == 1 {
        util.add
    } else {
        util.sub
    }
    f(a: 5) * 1000 + (g(a: 5) + 100)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}

function subImpl(a: int, b: int = 100) -> int {
    a - b
}

export let add = addImpl
export let sub = subImpl
"#,
        )],
    );
    assert!(
        distinct_default_direct_call.matches("(_t_61=5)").count() >= 2,
        "same-shape conditional namespace-member aliases should keep named direct-call shape: {distinct_default_direct_call}"
    );
    assert_generated_python_ok_int(
        &distinct_default_direct_call,
        7005,
        "conditional namespace-member alias distinct-target default parity",
    );

    let distinct_default_pipe_call = emit_source_with_files(
        r#"
import util
function main() -> int {
    let seeds = [1, 0]
    let f = if seeds[0] == 1 {
        util.add
    } else {
        util.sub
    }
    let g = if seeds[1] == 1 {
        util.add
    } else {
        util.sub
    }
    let a = 5 |> f(a: _)
    let b = 5 |> g(a: _)
    a * 1000 + (b + 100)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}

function subImpl(a: int, b: int = 100) -> int {
    a - b
}

export let add = addImpl
export let sub = subImpl
"#,
        )],
    );
    assert!(
        distinct_default_pipe_call.contains("lambda __tpz_piped")
            && distinct_default_pipe_call
                .matches("_t_61=__tpz_piped")
                .count()
                >= 2,
        "same-shape conditional namespace-member aliases should keep named pipe-call shape: {distinct_default_pipe_call}"
    );
    assert_generated_python_ok_int(
        &distinct_default_pipe_call,
        7005,
        "conditional namespace-member alias distinct-target default pipe parity",
    );

    let variadic_default_pipe_call = emit_source_with_files(
        r#"
import util
function main() -> int {
    let seeds = [1, 0]
    let f = if seeds[0] == 1 {
        util.sum
    } else {
        util.pack
    }
    let g = if seeds[1] == 1 {
        util.sum
    } else {
        util.pack
    }
    let a = 5 |> f(_, ...[1, 2])
    let b = 5 |> g(_, ...[1, 2])
    a * 1000 + b
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function sumImpl(seed: int, base: int = 2, ...xs: int) -> int {
    let mut total = seed + base
    for x in xs {
        total = total + x
    }
    total
}

function packImpl(seed: int, base: int = 100, ...xs: int) -> int {
    let mut total = base - seed
    for x in xs {
        total = total - x
    }
    total
}

export let sum = sumImpl
export let pack = packImpl
"#,
        )],
    );
    assert!(
        variadic_default_pipe_call.contains("lambda __tpz_piped")
            && variadic_default_pipe_call
                .matches("_t_73656564=__tpz_vararg_0")
                .count()
                >= 2
            && variadic_default_pipe_call
                .matches("_t_7873=[*__tpz_vararg_1")
                .count()
                >= 2
            && variadic_default_pipe_call
                .matches("tpz_spread_values(")
                .count()
                >= 2,
        "same-shape conditional namespace-member aliases should keep variadic spread pipe-call shape: {variadic_default_pipe_call}"
    );
    assert_generated_python_ok_int(
        &variadic_default_pipe_call,
        10092,
        "conditional namespace-member alias variadic default pipe parity",
    );

    let named_tail_variadic_pipe_call = emit_source_with_files(
        r#"
import util
function main() -> int {
    let seeds = [1, 0]
    let f = if seeds[0] == 1 {
        util.sum
    } else {
        util.pack
    }
    let g = if seeds[1] == 1 {
        util.sum
    } else {
        util.pack
    }
    let a = 5 |> f(...[1, 2], seed: 9, base: _)
    let b = 5 |> g(...[1, 2], seed: 9, base: _)
    a * 1000 + (b + 100)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function sumImpl(seed: int = 0, base: int = 2, ...xs: int) -> int {
    let mut total = seed + base
    for x in xs {
        total = total + x
    }
    total
}

function packImpl(seed: int = 0, base: int = 100, ...xs: int) -> int {
    let mut total = base - seed
    for x in xs {
        total = total - x
    }
    total
}

export let sum = sumImpl
export let pack = packImpl
"#,
        )],
    );
    assert!(
        named_tail_variadic_pipe_call.contains("lambda __tpz_piped")
            && named_tail_variadic_pipe_call
                .matches("_t_73656564=__tpz_vararg_")
                .count()
                >= 2
            && named_tail_variadic_pipe_call
                .matches("_t_62617365=__tpz_vararg_")
                .count()
                >= 2
            && named_tail_variadic_pipe_call
                .matches(", 9, __tpz_piped")
                .count()
                >= 2
            && named_tail_variadic_pipe_call
                .matches("_t_7873=[*__tpz_vararg_")
                .count()
                >= 2
            && named_tail_variadic_pipe_call
                .matches("tpz_spread_values(")
                .count()
                >= 2,
        "same-shape conditional namespace-member aliases should keep named-tail variadic spread pipe-call shape: {named_tail_variadic_pipe_call}"
    );
    assert_generated_python_ok_int(
        &named_tail_variadic_pipe_call,
        17093,
        "conditional namespace-member alias named-tail variadic pipe parity",
    );

    let hof_callbacks = emit_source_with_files(
        r#"
import util

let cb = if true {
    util.spin
} else {
    util.spin
}
let callbacks = if true {
    util.callbacks
} else {
    util.callbacks
}
let handlers = if false {
    util.handlers
} else {
    util.handlers
}

concurrent {
    singleValue: {
        let xs = [1]
        xs.map(cb)
    }
    arraySlot: {
        let ys = [1]
        ys.map(callbacks[1])
    }
    recordField: {
        let zs = [1]
        zs.map(handlers.primary)
    }
    fast: 0
}
0
"#,
        &[(
            "util.tpz",
            r#"
function spinImpl(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

export let spin = spinImpl
export let callbacks = [42, spinImpl]
export let handlers = { primary: spinImpl }
"#,
        )],
    );
    assert!(
        hof_callbacks
            .matches("yield from tpz_array_map__co(")
            .count()
            >= 3
            && hof_callbacks
                .matches("_tpz_mod__t_7574696c___t_7370696e496d706c__co(host, __tpz_cb_0)")
                .count()
                >= 3,
        "conditional namespace-member aliases should preserve identical branch cooperative metadata: {hof_callbacks}"
    );
    assert_generated_python_gates(&hof_callbacks).unwrap_or_else(|e| {
        panic!("conditional namespace-member alias HOF callback Python gate failed: {e}")
    });
}

#[test]
fn emits_match_namespace_member_alias_function_value_metadata() {
    let direct_calls = emit_source_with_files(
        r#"
import util

function main() -> int {
    let f = match 1 {
        case n if n == 0 => util.add
        case _ => util.add
    }
    let callbacks = match true {
        case true => util.callbacks
        case _ => util.callbacks
    }
    let handlers = match "ko" {
        case "en" => util.handlers
        case _ => util.handlers
    }
    f(a: 5) + callbacks[1](b: 3, a: 10) + handlers.primary(a: 7)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}

function mul(a: int, b: int) -> int {
    a * b
}

export let add = addImpl
export let callbacks = [42, mul]
export let handlers = { primary: addImpl }
"#,
        )],
    );
    assert!(
        direct_calls.contains("_t_66(_t_61=5)")
            && direct_calls.contains("\"_t_62\": 3")
            && direct_calls.contains("\"_t_61\": 10")
            && direct_calls.contains("\"_t_61\": 7"),
        "match namespace-member aliases should preserve identical arm callable metadata: {direct_calls}"
    );
    assert_generated_python_ok_int(
        &direct_calls,
        46,
        "match namespace-member alias direct-call parity",
    );

    let non_catch_all_direct_call = emit_source_with_files(
        r#"
import util
function main() -> int {
    let f = match true {
        case true => util.add
        case false => util.add
    }
    f(a: 5)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}
export let add = addImpl
"#,
        )],
    );
    assert!(
        non_catch_all_direct_call.contains("(_t_61=5)"),
        "non-catch-all match namespace-member aliases should keep named direct-call shape: {non_catch_all_direct_call}"
    );
    assert_generated_python_ok_int(
        &non_catch_all_direct_call,
        7,
        "non-catch-all match namespace-member alias direct-call parity",
    );

    let non_catch_all_distinct_default_direct_call = emit_source_with_files(
        r#"
import util
function main() -> int {
    let seeds = [1, 2]
    let f = match seeds[0] {
        case n if n == 1 => util.add
        case n if n == 2 => util.sub
    }
    let g = match seeds[1] {
        case n if n == 1 => util.add
        case n if n == 2 => util.sub
    }
    f(a: 5) * 1000 + (g(a: 5) + 100)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}

function subImpl(a: int, b: int = 100) -> int {
    a - b
}

export let add = addImpl
export let sub = subImpl
"#,
        )],
    );
    assert!(
        non_catch_all_distinct_default_direct_call
            .matches("(_t_61=5)")
            .count()
            >= 2,
        "same-shape non-catch-all match namespace-member aliases should keep named direct-call shape: {non_catch_all_distinct_default_direct_call}"
    );
    assert_generated_python_ok_int(
        &non_catch_all_distinct_default_direct_call,
        7005,
        "non-catch-all match namespace-member alias distinct-target default parity",
    );

    let non_catch_all_positional_pipe_call = emit_source_with_files(
        r#"
import util
function main() -> int {
    let seeds = [1, 2]
    let f = match seeds[0] {
        case n if n == 1 => util.inc
        case n if n == 2 => util.double
    }
    let g = match seeds[1] {
        case n if n == 1 => util.inc
        case n if n == 2 => util.double
    }
    let a = 5 |> f
    let b = 5 |> g
    a * 1000 + b
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function incImpl(a: int) -> int {
    a + 1
}

function doubleImpl(a: int) -> int {
    a * 2
}

export let inc = incImpl
export let double = doubleImpl
"#,
        )],
    );
    assert!(
        non_catch_all_positional_pipe_call.contains("lambda __tpz_piped")
            && non_catch_all_positional_pipe_call
                .matches("(__tpz_piped)")
                .count()
                >= 2,
        "non-catch-all match namespace-member aliases should keep positional pipe-call shape: {non_catch_all_positional_pipe_call}"
    );
    assert_generated_python_ok_int(
        &non_catch_all_positional_pipe_call,
        6010,
        "non-catch-all match namespace-member alias positional pipe parity",
    );

    let non_catch_all_distinct_default_pipe_call = emit_source_with_files(
        r#"
import util
function main() -> int {
    let seeds = [1, 2]
    let f = match seeds[0] {
        case n if n == 1 => util.add
        case n if n == 2 => util.sub
    }
    let g = match seeds[1] {
        case n if n == 1 => util.add
        case n if n == 2 => util.sub
    }
    let a = 5 |> f(a: _)
    let b = 5 |> g(a: _)
    a * 1000 + (b + 100)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}

function subImpl(a: int, b: int = 100) -> int {
    a - b
}

export let add = addImpl
export let sub = subImpl
"#,
        )],
    );
    assert!(
        non_catch_all_distinct_default_pipe_call.contains("lambda __tpz_piped")
            && non_catch_all_distinct_default_pipe_call
                .matches("_t_61=__tpz_piped")
                .count()
                >= 2,
        "same-shape non-catch-all match namespace-member aliases should keep named pipe-call shape: {non_catch_all_distinct_default_pipe_call}"
    );
    assert_generated_python_ok_int(
        &non_catch_all_distinct_default_pipe_call,
        7005,
        "non-catch-all match namespace-member alias distinct-target default pipe parity",
    );

    let non_catch_all_variadic_default_pipe_call = emit_source_with_files(
        r#"
import util
function main() -> int {
    let seeds = [1, 2]
    let f = match seeds[0] {
        case n if n == 1 => util.sum
        case n if n == 2 => util.pack
    }
    let g = match seeds[1] {
        case n if n == 1 => util.sum
        case n if n == 2 => util.pack
    }
    let a = 5 |> f(_, ...[1, 2])
    let b = 5 |> g(_, ...[1, 2])
    a * 1000 + b
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function sumImpl(seed: int, base: int = 2, ...xs: int) -> int {
    let mut total = seed + base
    for x in xs {
        total = total + x
    }
    total
}

function packImpl(seed: int, base: int = 100, ...xs: int) -> int {
    let mut total = base - seed
    for x in xs {
        total = total - x
    }
    total
}

export let sum = sumImpl
export let pack = packImpl
"#,
        )],
    );
    assert!(
        non_catch_all_variadic_default_pipe_call.contains("lambda __tpz_piped")
            && non_catch_all_variadic_default_pipe_call
                .matches("_t_73656564=__tpz_vararg_0")
                .count()
                >= 2
            && non_catch_all_variadic_default_pipe_call
                .matches("_t_7873=[*__tpz_vararg_1")
                .count()
                >= 2
            && non_catch_all_variadic_default_pipe_call
                .matches("tpz_spread_values(")
                .count()
                >= 2,
        "same-shape non-catch-all match namespace-member aliases should keep variadic spread pipe-call shape: {non_catch_all_variadic_default_pipe_call}"
    );
    assert_generated_python_ok_int(
        &non_catch_all_variadic_default_pipe_call,
        10092,
        "non-catch-all match namespace-member alias variadic default pipe parity",
    );

    let non_catch_all_named_tail_variadic_pipe_call = emit_source_with_files(
        r#"
import util
function main() -> int {
    let seeds = [1, 2]
    let f = match seeds[0] {
        case n if n == 1 => util.sum
        case n if n == 2 => util.pack
    }
    let g = match seeds[1] {
        case n if n == 1 => util.sum
        case n if n == 2 => util.pack
    }
    let a = 5 |> f(...[1, 2], seed: 9, base: _)
    let b = 5 |> g(...[1, 2], seed: 9, base: _)
    a * 1000 + (b + 100)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function sumImpl(seed: int = 0, base: int = 2, ...xs: int) -> int {
    let mut total = seed + base
    for x in xs {
        total = total + x
    }
    total
}

function packImpl(seed: int = 0, base: int = 100, ...xs: int) -> int {
    let mut total = base - seed
    for x in xs {
        total = total - x
    }
    total
}

export let sum = sumImpl
export let pack = packImpl
"#,
        )],
    );
    assert!(
        non_catch_all_named_tail_variadic_pipe_call.contains("lambda __tpz_piped")
            && non_catch_all_named_tail_variadic_pipe_call
                .matches("_t_73656564=__tpz_vararg_")
                .count()
                >= 2
            && non_catch_all_named_tail_variadic_pipe_call
                .matches("_t_62617365=__tpz_vararg_")
                .count()
                >= 2
            && non_catch_all_named_tail_variadic_pipe_call
                .matches(", 9, __tpz_piped")
                .count()
                >= 2
            && non_catch_all_named_tail_variadic_pipe_call
                .matches("_t_7873=[*__tpz_vararg_")
                .count()
                >= 2
            && non_catch_all_named_tail_variadic_pipe_call
                .matches("tpz_spread_values(")
                .count()
                >= 2,
        "same-shape non-catch-all match namespace-member aliases should keep named-tail variadic spread pipe-call shape: {non_catch_all_named_tail_variadic_pipe_call}"
    );
    assert_generated_python_ok_int(
        &non_catch_all_named_tail_variadic_pipe_call,
        17093,
        "non-catch-all match namespace-member alias named-tail variadic pipe parity",
    );

    let distinct_default_direct_call = emit_source_with_files(
        r#"
import util
function main() -> int {
    let seeds = [1, 0]
    let f = match seeds[0] {
        case n if n == 1 => util.add
        case _ => util.sub
    }
    let g = match seeds[1] {
        case n if n == 1 => util.add
        case _ => util.sub
    }
    f(a: 5) * 1000 + (g(a: 5) + 100)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}

function subImpl(a: int, b: int = 100) -> int {
    a - b
}

export let add = addImpl
export let sub = subImpl
"#,
        )],
    );
    assert!(
        distinct_default_direct_call.matches("(_t_61=5)").count() >= 2,
        "same-shape match namespace-member aliases should keep named direct-call shape: {distinct_default_direct_call}"
    );
    assert_generated_python_ok_int(
        &distinct_default_direct_call,
        7005,
        "match namespace-member alias distinct-target default parity",
    );

    let distinct_default_pipe_call = emit_source_with_files(
        r#"
import util
function main() -> int {
    let seeds = [1, 0]
    let f = match seeds[0] {
        case n if n == 1 => util.add
        case _ => util.sub
    }
    let g = match seeds[1] {
        case n if n == 1 => util.add
        case _ => util.sub
    }
    let a = 5 |> f(a: _)
    let b = 5 |> g(a: _)
    a * 1000 + (b + 100)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}

function subImpl(a: int, b: int = 100) -> int {
    a - b
}

export let add = addImpl
export let sub = subImpl
"#,
        )],
    );
    assert!(
        distinct_default_pipe_call.contains("lambda __tpz_piped")
            && distinct_default_pipe_call
                .matches("_t_61=__tpz_piped")
                .count()
                >= 2,
        "same-shape match namespace-member aliases should keep named pipe-call shape: {distinct_default_pipe_call}"
    );
    assert_generated_python_ok_int(
        &distinct_default_pipe_call,
        7005,
        "match namespace-member alias distinct-target default pipe parity",
    );

    let variadic_default_pipe_call = emit_source_with_files(
        r#"
import util
function main() -> int {
    let seeds = [1, 0]
    let f = match seeds[0] {
        case n if n == 1 => util.sum
        case _ => util.pack
    }
    let g = match seeds[1] {
        case n if n == 1 => util.sum
        case _ => util.pack
    }
    let a = 5 |> f(_, ...[1, 2])
    let b = 5 |> g(_, ...[1, 2])
    a * 1000 + b
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function sumImpl(seed: int, base: int = 2, ...xs: int) -> int {
    let mut total = seed + base
    for x in xs {
        total = total + x
    }
    total
}

function packImpl(seed: int, base: int = 100, ...xs: int) -> int {
    let mut total = base - seed
    for x in xs {
        total = total - x
    }
    total
}

export let sum = sumImpl
export let pack = packImpl
"#,
        )],
    );
    assert!(
        variadic_default_pipe_call.contains("lambda __tpz_piped")
            && variadic_default_pipe_call
                .matches("_t_73656564=__tpz_vararg_0")
                .count()
                >= 2
            && variadic_default_pipe_call
                .matches("_t_7873=[*__tpz_vararg_1")
                .count()
                >= 2
            && variadic_default_pipe_call
                .matches("tpz_spread_values(")
                .count()
                >= 2,
        "same-shape match namespace-member aliases should keep variadic spread pipe-call shape: {variadic_default_pipe_call}"
    );
    assert_generated_python_ok_int(
        &variadic_default_pipe_call,
        10092,
        "match namespace-member alias variadic default pipe parity",
    );

    let named_tail_variadic_pipe_call = emit_source_with_files(
        r#"
import util
function main() -> int {
    let seeds = [1, 0]
    let f = match seeds[0] {
        case n if n == 1 => util.sum
        case _ => util.pack
    }
    let g = match seeds[1] {
        case n if n == 1 => util.sum
        case _ => util.pack
    }
    let a = 5 |> f(...[1, 2], seed: 9, base: _)
    let b = 5 |> g(...[1, 2], seed: 9, base: _)
    a * 1000 + (b + 100)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function sumImpl(seed: int = 0, base: int = 2, ...xs: int) -> int {
    let mut total = seed + base
    for x in xs {
        total = total + x
    }
    total
}

function packImpl(seed: int = 0, base: int = 100, ...xs: int) -> int {
    let mut total = base - seed
    for x in xs {
        total = total - x
    }
    total
}

export let sum = sumImpl
export let pack = packImpl
"#,
        )],
    );
    assert!(
        named_tail_variadic_pipe_call.contains("lambda __tpz_piped")
            && named_tail_variadic_pipe_call
                .matches("_t_73656564=__tpz_vararg_")
                .count()
                >= 2
            && named_tail_variadic_pipe_call
                .matches("_t_62617365=__tpz_vararg_")
                .count()
                >= 2
            && named_tail_variadic_pipe_call
                .matches(", 9, __tpz_piped")
                .count()
                >= 2
            && named_tail_variadic_pipe_call
                .matches("_t_7873=[*__tpz_vararg_")
                .count()
                >= 2
            && named_tail_variadic_pipe_call
                .matches("tpz_spread_values(")
                .count()
                >= 2,
        "same-shape match namespace-member aliases should keep named-tail variadic spread pipe-call shape: {named_tail_variadic_pipe_call}"
    );
    assert_generated_python_ok_int(
        &named_tail_variadic_pipe_call,
        17093,
        "match namespace-member alias named-tail variadic pipe parity",
    );

    let hof_callbacks = emit_source_with_files(
        r#"
import util

let cb = match 1 {
    case n if n == 0 => util.spin
    case _ => util.spin
}
let callbacks = match true {
    case true => util.callbacks
    case _ => util.callbacks
}
let handlers = match "ko" {
    case "en" => util.handlers
    case _ => util.handlers
}

concurrent {
    singleValue: {
        let xs = [1]
        xs.map(cb)
    }
    arraySlot: {
        let ys = [1]
        ys.map(callbacks[1])
    }
    recordField: {
        let zs = [1]
        zs.map(handlers.primary)
    }
    fast: 0
}
0
"#,
        &[(
            "util.tpz",
            r#"
function spinImpl(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

export let spin = spinImpl
export let callbacks = [42, spinImpl]
export let handlers = { primary: spinImpl }
"#,
        )],
    );
    assert!(
        hof_callbacks
            .matches("yield from tpz_array_map__co(")
            .count()
            >= 3
            && hof_callbacks
                .matches("_tpz_mod__t_7574696c___t_7370696e496d706c__co(host, __tpz_cb_0)")
                .count()
                >= 3,
        "match namespace-member aliases should preserve identical arm cooperative metadata: {hof_callbacks}"
    );
    assert_generated_python_gates(&hof_callbacks).unwrap_or_else(|e| {
        panic!("match namespace-member alias HOF callback Python gate failed: {e}")
    });

    let mismatched_match_hof_alias = emit_source_with_files(
        r#"
import util
let seeds = [1]
let callbacks = match seeds[0] {
    case n if n == 1 => util.fastCallbacks
    case _ => util.slowCallbacks
}
concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks[0])
    }
    idle: 0
}
0
"#,
        &[(
            "util.tpz",
            r#"
function fast(x: int) -> int {
    x + 40
}
function spin(x: int) -> int {
    1 / 0
}
export let fastCallbacks = [fast]
export let slowCallbacks = [spin]
"#,
        )],
    );
    assert!(
        mismatched_match_hof_alias.contains("yield from tpz_array_map__co(")
            && !mismatched_match_hof_alias
                .contains("_tpz_mod__t_7574696c___t_66617374__co(host, __tpz_cb_0)")
            && !mismatched_match_hof_alias
                .contains("_tpz_mod__t_7574696c___t_7370696e__co(host, __tpz_cb_0)"),
        "mismatched match namespace-member HOF aliases should stay on the runtime cooperative driver without direct static callback recovery: {mismatched_match_hof_alias}"
    );
    assert_generated_python_gates(&mismatched_match_hof_alias).unwrap_or_else(|e| {
        panic!("mismatched match namespace-member alias HOF callback Python gate failed: {e}")
    });

    let non_catch_all_match_hof_alias = emit_source_with_files(
        r#"
import util
let seeds = [1]
let callbacks = match seeds[0] {
    case n if n == 1 => util.fastCallbacks
    case n if n == 2 => util.slowCallbacks
}
let result = concurrent {
    value: {
        let xs = [1]
        let ys = xs.map(callbacks[0])
        ys[0]
    }
    idle: 0
}
result.value
"#,
        &[(
            "util.tpz",
            r#"
function fast(x: int) -> int {
    x + 40
}
function spin(x: int) -> int {
    1 / 0
}
export let fastCallbacks = [fast]
export let slowCallbacks = [spin]
"#,
        )],
    );
    assert!(
        non_catch_all_match_hof_alias.contains("yield from tpz_array_map__co(")
            && !non_catch_all_match_hof_alias
                .contains("_tpz_mod__t_7574696c___t_66617374__co(host, __tpz_cb_0)")
            && !non_catch_all_match_hof_alias
                .contains("_tpz_mod__t_7574696c___t_7370696e__co(host, __tpz_cb_0)"),
        "non-catch-all match namespace-member HOF aliases should stay on the runtime cooperative driver without direct static callback recovery: {non_catch_all_match_hof_alias}"
    );
    assert_generated_python_ok_int(
        &non_catch_all_match_hof_alias,
        41,
        "non-catch-all match namespace-member HOF callback runtime-driver parity",
    );
}

#[test]
fn emits_mutable_namespace_member_alias_function_value_metadata() {
    let direct_calls = emit_source_with_files(
        r#"
import util

function main() -> int {
    let mut f = util.add
    let mut callbacks = util.callbacks
    let mut handlers = util.handlers
    f(a: 5) + f(10, b: 3) + callbacks[0](a: 7) + handlers.primary(a: 11)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}

export let add = addImpl
export let callbacks = [addImpl]
export let handlers = { primary: addImpl }
"#,
        )],
    );
    assert!(
        direct_calls.contains("_t_66(_t_61=5)")
            && direct_calls.contains("_t_66(10, _t_62=3)")
            && direct_calls.contains("\"_t_61\": 7")
            && direct_calls.contains("\"_t_61\": 11"),
        "mutable namespace-member aliases should preserve callable params before mutation: {direct_calls}"
    );
    assert_generated_python_ok_int(
        &direct_calls,
        42,
        "mutable namespace-member alias function-value direct-call parity",
    );

    let hof_callbacks = emit_source_with_files(
        r#"
import util

let mut cb = util.spin
let base = util.callbacks
let mut callbacks = base
let mut handlers = util.handlers

concurrent {
    singleValue: {
        let xs = [1]
        xs.map(cb)
    }
    arraySlot: {
        let ys = [1]
        ys.map(callbacks[1])
    }
    recordField: {
        let zs = [1]
        zs.map(handlers.primary)
    }
    fast: 0
}
0
"#,
        &[(
            "util.tpz",
            r#"
function spinImpl(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

export let spin = spinImpl
export let callbacks = [42, spinImpl]
export let handlers = { primary: spinImpl }
"#,
        )],
    );
    assert!(
        hof_callbacks
            .matches("yield from tpz_array_map__co(")
            .count()
            >= 3
            && hof_callbacks
                .matches("_tpz_mod__t_7574696c___t_7370696e496d706c__co(host, __tpz_cb_0)")
                .count()
                >= 3,
        "mutable namespace-member aliases should preserve cooperative metadata before mutation: {hof_callbacks}"
    );
    assert_generated_python_gates(&hof_callbacks).unwrap_or_else(|e| {
        panic!("mutable namespace-member alias HOF callback Python gate failed: {e}")
    });

    let reserved_direct_calls = emit_source_with_files(
        r#"
import util

function main() -> int {
    let mut f = util.add
    f(a: 5) + f(10, b: 3)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
export function add(a: int, b: int = 2) -> int {
    a + b
}
"#,
        )],
    );
    assert!(
        reserved_direct_calls.contains("_t_66(_t_61=5)")
            && reserved_direct_calls.contains("_t_66(10, _t_62=3)")
            && reserved_direct_calls.contains("tpz_host_callable("),
        "reserved-name mutable namespace-member function aliases should preserve callable params: {reserved_direct_calls}"
    );
    assert_generated_python_ok_int(
        &reserved_direct_calls,
        20,
        "reserved-name mutable namespace-member function alias direct-call parity",
    );

    let reserved_hof_callbacks = emit_source_with_files(
        r#"
import util

let mut cb = util.sortedBy
concurrent {
    slow: {
        let xs = [1]
        xs.map(cb)
    }
    fast: 0
}
0
"#,
        &[(
            "util.tpz",
            r#"
export function sortedBy(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
"#,
        )],
    );
    assert!(
        reserved_hof_callbacks.contains("yield from tpz_array_map__co(")
            && reserved_hof_callbacks
                .contains("_tpz_mod__t_7574696c___t_736f727465644279__co(host, __tpz_cb_0)"),
        "reserved-name mutable namespace-member function aliases should preserve cooperative metadata: {reserved_hof_callbacks}"
    );
    assert_generated_python_gates(&reserved_hof_callbacks).unwrap_or_else(|e| {
        panic!("reserved-name mutable namespace-member alias HOF callback Python gate failed: {e}")
    });

    let cross_arm_reassigned_hof_callbacks = emit_source_with_files(
        r#"
import util

function main() -> int {
    let mut cb = util.spin
    let r = concurrent {
        slow: {
            let xs = [1, 2]
            let ys = xs.map(cb)
            ys[0] + ys[1]
        }
        flip: {
            cb = util.same
            0
        }
    }
    r.slow
}
main()
"#,
        &[(
            "util.tpz",
            r#"
export function spin(x: int) -> int {
    let mut i = 0
    while i < 200 {
        i = i + 1
    }
    x
}

export function same(x: int) -> int {
    x
}
"#,
        )],
    );
    assert!(
        cross_arm_reassigned_hof_callbacks.contains("yield from tpz_array_map__co(")
            && !cross_arm_reassigned_hof_callbacks
                .contains("_tpz_mod__t_7574696c___t_7370696e__co(host, __tpz_cb_0)"),
        "cross-arm writers should use the runtime cooperative driver without baking namespace-member alias metadata: {cross_arm_reassigned_hof_callbacks}"
    );
    assert_generated_python_ok_int(
        &cross_arm_reassigned_hof_callbacks,
        3,
        "cross-arm-mutated namespace-member HOF callback alias",
    );

    let reassigned_source_direct_calls = emit_source_with_files(
        r#"
import util

function main() -> int {
    let mut first = util.plus
    let mut reserved = util.add
    first = util.bump
    reserved = util.grow
    first(x: 5) + first(10, y: 3) + reserved(x: 7) + reserved(8, y: 4)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function plusImpl(a: int, b: int = 2) -> int {
    a + b
}

function bumpImpl(x: int, y: int = 9) -> int {
    x * 10 + y
}

export let plus = plusImpl
export let bump = bumpImpl

export function add(a: int, b: int = 2) -> int {
    a + b
}

export function grow(x: int, y: int = 9) -> int {
    x * 10 + y
}
"#,
        )],
    );
    assert!(
        reassigned_source_direct_calls.contains("_t_6669727374(_t_78=5)")
            && reassigned_source_direct_calls.contains("_t_6669727374(10, _t_79=3)")
            && reassigned_source_direct_calls.contains("_t_7265736572766564(_t_78=7)")
            && reassigned_source_direct_calls.contains("_t_7265736572766564(8, _t_79=4)"),
        "reassigned mutable namespace-member function aliases should use current-value callable params: {reassigned_source_direct_calls}"
    );
    assert_generated_python_ok_int(
        &reassigned_source_direct_calls,
        325,
        "reassigned mutable namespace-member current-value direct-call parity",
    );

    let reassigned_source_hof_callbacks = emit_source_with_files(
        r#"
import util

let mut first = util.fast
let mut reserved = util.otherSorted
first = util.spin
reserved = util.sortedBy

concurrent {
    valueCurrent: {
        let xs = [1]
        xs.map(first)
    }
    reservedCurrent: {
        let ys = [1]
        ys.map(reserved)
    }
    boom: {
        let zs = [1]
        zs[2]
    }
}
0
"#,
        &[(
            "util.tpz",
            r#"
function fastImpl(x: int) -> int {
    x
}

function spinImpl(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    1 / 0
}

export let fast = fastImpl
export let spin = spinImpl

export function otherSorted(x: int) -> int {
    x
}

export function sortedBy(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    1 / 0
}
"#,
        )],
    );
    assert!(
        reassigned_source_hof_callbacks
            .matches("yield from tpz_array_map__co(")
            .count()
            >= 2
            && reassigned_source_hof_callbacks
                .contains("_tpz_mod__t_7574696c___t_7370696e496d706c__co(host, __tpz_cb_0)")
            && reassigned_source_hof_callbacks
                .contains("_tpz_mod__t_7574696c___t_736f727465644279__co(host, __tpz_cb_0)"),
        "reassigned mutable namespace-member function aliases should use current-value cooperative metadata: {reassigned_source_hof_callbacks}"
    );
    assert_generated_python_gates(&reassigned_source_hof_callbacks).unwrap_or_else(|e| {
        panic!(
            "reassigned mutable namespace-member current-value HOF callback Python gate failed: {e}"
        )
    });

    let reassigned_storage_direct_calls = emit_source_with_files(
        r#"
import util

function main() -> int {
    let mut callbacks = util.oldCallbacks
    let mut handlers = util.oldHandlers
    callbacks = util.callbacks
    handlers = util.handlers
    callbacks[0](a: 5) + callbacks[1](y: 3, x: 10) + handlers.primary(y: 4, x: 7)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}

function mulImpl(x: int, y: int) -> int {
    x * y
}

export let oldCallbacks = [addImpl]
export let oldHandlers = { primary: addImpl }
export let callbacks = [addImpl, mulImpl]
export let handlers = { primary: mulImpl }
"#,
        )],
    );
    assert!(
        reassigned_storage_direct_calls.contains("\"_t_61\": 5")
            && reassigned_storage_direct_calls.contains("\"_t_79\": 3")
            && reassigned_storage_direct_calls.contains("\"_t_78\": 10")
            && reassigned_storage_direct_calls.contains("\"_t_79\": 4")
            && reassigned_storage_direct_calls.contains("\"_t_78\": 7"),
        "reassigned mutable namespace-member storage aliases should use assignment-point RHS callable params: {reassigned_storage_direct_calls}"
    );
    assert_generated_python_ok_int(
        &reassigned_storage_direct_calls,
        65,
        "reassigned mutable namespace-member storage direct-call parity",
    );

    let reassigned_storage_hof_callbacks = emit_source_with_files(
        r#"
import util

let mut callbacks = util.fastCallbacks
let mut handlers = util.fastHandlers
callbacks = util.slowCallbacks
handlers = util.slowHandlers

concurrent {
    arraySlot: {
        let xs = [1]
        xs.map(callbacks[1])
    }
    recordField: {
        let ys = [1]
        ys.map(handlers.primary)
    }
    boom: {
        let zs = [1]
        zs[2]
    }
}
0
"#,
        &[(
            "util.tpz",
            r#"
function fast(x: int) -> int {
    x
}

function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    1 / 0
}

export let fastCallbacks = [42, fast]
export let slowCallbacks = [42, spin]
export let fastHandlers = { primary: fast }
export let slowHandlers = { primary: spin }
"#,
        )],
    );
    assert!(
        reassigned_storage_hof_callbacks
            .matches("yield from tpz_array_map__co(")
            .count()
            >= 2
            && reassigned_storage_hof_callbacks
                .matches("_tpz_mod__t_7574696c___t_7370696e__co(host, __tpz_cb_0)")
                .count()
                >= 2,
        "reassigned mutable namespace-member storage aliases should use assignment-point RHS cooperative metadata: {reassigned_storage_hof_callbacks}"
    );
    assert_generated_python_gates(&reassigned_storage_hof_callbacks).unwrap_or_else(|e| {
        panic!("reassigned mutable namespace-member storage HOF callback Python gate failed: {e}")
    });

    let alias_chain_direct_calls = emit_source_with_files(
        r#"
import util

function main() -> int {
    let first = util.plus
    let second = first
    let mut third = second
    let reserved = util.add
    let chainedReserved = reserved
    let mut mutableReserved = chainedReserved
    second(a: 5) + third(10, b: 3) + chainedReserved(a: 7) + mutableReserved(8, b: 4)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function plusImpl(a: int, b: int = 2) -> int {
    a + b
}

export let plus = plusImpl

export function add(a: int, b: int = 2) -> int {
    a + b
}
"#,
        )],
    );
    assert!(
        alias_chain_direct_calls.contains("_t_7365636f6e64(_t_61=5)")
            && alias_chain_direct_calls.contains("_t_7468697264(10, _t_62=3)")
            && alias_chain_direct_calls.contains("_t_636861696e65645265736572766564(_t_61=7)")
            && alias_chain_direct_calls.contains("_t_6d757461626c655265736572766564(8, _t_62=4)")
            && alias_chain_direct_calls
                .matches("tpz_host_callable(")
                .count()
                >= 2,
        "namespace-member single-function alias chains should preserve callable params: {alias_chain_direct_calls}"
    );
    assert_generated_python_ok_int(
        &alias_chain_direct_calls,
        41,
        "namespace-member single-function alias chain direct-call parity",
    );

    let alias_chain_hof_callbacks = emit_source_with_files(
        r#"
import util

let first = util.spin
let second = first
let mut third = second
let reserved = util.sortedBy
let chainedReserved = reserved
let mut mutableReserved = chainedReserved

concurrent {
    valueChain: {
        let xs = [1]
        xs.map(second)
    }
    mixedValueChain: {
        let ys = [1]
        ys.map(third)
    }
    reservedChain: {
        let zs = [1]
        zs.map(chainedReserved)
    }
    mixedReservedChain: {
        let ws = [1]
        ws.map(mutableReserved)
    }
    fast: 0
}
0
"#,
        &[(
            "util.tpz",
            r#"
function spinImpl(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

export let spin = spinImpl

export function sortedBy(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
"#,
        )],
    );
    assert!(
        alias_chain_hof_callbacks
            .matches("yield from tpz_array_map__co(")
            .count()
            >= 4
            && alias_chain_hof_callbacks
                .matches("_tpz_mod__t_7574696c___t_7370696e496d706c__co(host, __tpz_cb_0)")
                .count()
                >= 2
            && alias_chain_hof_callbacks
                .matches("_tpz_mod__t_7574696c___t_736f727465644279__co(host, __tpz_cb_0)")
                .count()
                >= 2,
        "namespace-member single-function alias chains should preserve cooperative metadata: {alias_chain_hof_callbacks}"
    );
    assert_generated_python_gates(&alias_chain_hof_callbacks).unwrap_or_else(|e| {
        panic!("namespace-member single-function alias chain HOF callback Python gate failed: {e}")
    });
}
