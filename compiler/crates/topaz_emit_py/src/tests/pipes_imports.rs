use super::*;

#[test]
fn imported_module_functions_emit_namespace_and_selected_python_calls() {
    let namespace = emit_source_with_files(
        r#"
import util
print(util.answer(40))
"#,
        &[(
            "util.tpz",
            r#"
export function answer(x: int) -> int {
    x + 2
}
"#,
        )],
    );
    assert!(
        namespace.contains("def _tpz_mod__t_7574696c___t_616e73776572(host, _t_78):  # answer(x)"),
        "{namespace}"
    );
    assert!(
        namespace.contains("host.print(_tpz_mod__t_7574696c___t_616e73776572(host, 40)"),
        "{namespace}"
    );
    assert!(
        !namespace.contains("unsupported imported module function export"),
        "{namespace}"
    );
    assert_generated_python_gates(&namespace)
        .unwrap_or_else(|e| panic!("namespace function import Python gate failed: {e}"));

    let selected = emit_source_with_files(
        r#"
import util { answer as final }
print(final(40))
"#,
        &[(
            "util.tpz",
            r#"
export function answer(x: int) -> int {
    x + 2
}
"#,
        )],
    );
    assert!(
        selected.contains("host.print(_tpz_mod__t_7574696c___t_616e73776572(host, 40)"),
        "{selected}"
    );
    assert_generated_python_gates(&selected)
        .unwrap_or_else(|e| panic!("selected function import Python gate failed: {e}"));
}

#[test]
fn emits_imported_namespace_and_value_alias_hof_callbacks() {
    let namespace_function = emit_source_with_files(
        r#"
import util
let plain = [1].map(util.inc)
concurrent {
    namespaceFunction: {
        let xs = [1]
        xs.map(util.spin)
    }
    fast: 0
}
0
"#,
        &[(
            "util.tpz",
            r#"
export function inc(x: int) -> int {
    x + 1
}

export function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

export let alias = spin
"#,
        )],
    );
    assert!(
        namespace_function.contains(
            "tpz_array_map([1], (lambda __tpz_cb_0: _tpz_mod__t_7574696c___t_696e63(host, __tpz_cb_0))"
        ),
        "namespace function HOF callbacks outside cooperative arms should adapt host: {namespace_function}"
    );
    assert!(
        namespace_function.contains("yield from tpz_array_map__co(")
            && namespace_function
                .contains("_tpz_mod__t_7574696c___t_7370696e__co(host, __tpz_cb_0)"),
        "namespace function HOF callbacks should route through cooperative module functions: {namespace_function}"
    );
    assert_generated_python_gates(&namespace_function).unwrap_or_else(|e| {
        panic!("imported namespace function HOF callback Python gate failed: {e}")
    });

    let selected_alias = emit_source_with_files(
        r#"
import util { alias }
concurrent {
    selectedAlias: {
        let xs = [1]
        xs.map(alias)
    }
    fast: 0
}
0
"#,
        &[(
            "util.tpz",
            r#"
export function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

export let alias = spin
"#,
        )],
    );
    assert!(
        selected_alias.contains("yield from tpz_array_map__co(")
            && selected_alias.contains("_tpz_mod__t_7574696c___t_7370696e__co(host, __tpz_cb_0)"),
        "selected imported function-value aliases should preserve cooperative callback metadata: {selected_alias}"
    );
    assert_generated_python_gates(&selected_alias).unwrap_or_else(|e| {
        panic!("selected imported value-alias HOF callback Python gate failed: {e}")
    });
}

#[test]
fn emits_manual_forwarded_imported_function_value_metadata() {
    let selected_direct = emit_source_with_files(
        r#"
import facade { forwarded }
function main() -> int {
    forwarded(a: 5) + forwarded(10, b: 3)
}
main()
"#,
        &[
            (
                "base.tpz",
                r#"
export function add(a: int, b: int = 2) -> int {
    a + b
}
"#,
            ),
            (
                "facade.tpz",
                r#"
import base { add }
export let forwarded = add
"#,
            ),
        ],
    );
    assert!(
        selected_direct.contains("_t_61=5") && selected_direct.contains("_t_62=3"),
        "selected manual-forwarded function value should preserve named/default params: {selected_direct}"
    );
    assert_generated_python_ok_int(
        &selected_direct,
        20,
        "selected manual-forwarded function-value direct-call parity",
    );

    let namespace_direct = emit_source_with_files(
        r#"
import facade
function main() -> int {
    facade.forwarded(a: 5) + facade.forwarded(10, b: 3)
}
main()
"#,
        &[
            (
                "base.tpz",
                r#"
export function add(a: int, b: int = 2) -> int {
    a + b
}
"#,
            ),
            (
                "facade.tpz",
                r#"
import base { add }
export let forwarded = add
"#,
            ),
        ],
    );
    assert!(
        namespace_direct.contains("tpz_call(tpz_member(")
            && namespace_direct.contains("\"_t_61\": 5")
            && namespace_direct.contains("\"_t_62\": 3"),
        "namespace manual-forwarded function value should call through tpz_call with params: {namespace_direct}"
    );
    assert_generated_python_ok_int(
        &namespace_direct,
        20,
        "namespace manual-forwarded function-value direct-call parity",
    );

    let namespace_alias = emit_source_with_files(
        r#"
import facade
function main() -> int {
    let local = facade.forwarded
    local(a: 5) + local(10, b: 3)
}
main()
"#,
        &[
            (
                "base.tpz",
                r#"
export function add(a: int, b: int = 2) -> int {
    a + b
}
"#,
            ),
            (
                "facade.tpz",
                r#"
import base { add }
export let forwarded = add
"#,
            ),
        ],
    );
    assert!(
        namespace_alias.contains("_t_61=5") && namespace_alias.contains("_t_62=3"),
        "namespace-member aliases of manual-forwarded function values should preserve params: {namespace_alias}"
    );
    assert_generated_python_ok_int(
        &namespace_alias,
        20,
        "namespace-member alias manual-forwarded function-value direct-call parity",
    );

    let statement_lowered_namespace = emit_source_with_files(
        r#"
import facade
function main() -> int {
    facade.forwarded(a: {
        let x = 5
        x
    })
}
main()
"#,
        &[
            (
                "base.tpz",
                r#"
export function add(a: int, b: int = 2) -> int {
    a + b
}
"#,
            ),
            (
                "facade.tpz",
                r#"
import base { add }
export let forwarded = add
"#,
            ),
        ],
    );
    assert!(
        statement_lowered_namespace.contains("tpz_call(")
            && statement_lowered_namespace.contains("\"_t_61\": _"),
        "statement-lowered namespace manual-forwarded function values should preserve named params: {statement_lowered_namespace}"
    );
    assert_generated_python_ok_int(
        &statement_lowered_namespace,
        7,
        "statement-lowered namespace manual-forwarded function-value direct-call parity",
    );
}

#[test]
fn emits_manual_forwarded_imported_function_value_hof_callbacks() {
    let generated = emit_source_with_files(
        r#"
import selected_facade { forwarded }
import facade
let local = facade.forwarded
concurrent {
    selectedValue: {
        let xs = [1]
        xs.map(forwarded)
    }
    namespaceValue: {
        let ys = [1]
        ys.map(facade.forwarded)
    }
    aliasValue: {
        let zs = [1]
        zs.map(local)
    }
    fast: 0
}
0
"#,
        &[
            (
                "base.tpz",
                r#"
export function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
"#,
            ),
            (
                "selected_facade.tpz",
                r#"
import base { spin }
export let forwarded = spin
"#,
            ),
            (
                "facade.tpz",
                r#"
import base { spin }
export let forwarded = spin
"#,
            ),
        ],
    );
    assert!(
        generated.matches("yield from tpz_array_map__co(").count() >= 3
            && generated
                .matches("_tpz_mod__t_62617365___t_7370696e__co(host, __tpz_cb_0)")
                .count()
                >= 3,
        "manual-forwarded function-value HOF callbacks should preserve cooperative metadata: {generated}"
    );
    assert_generated_python_gates(&generated).unwrap_or_else(|e| {
        panic!("manual-forwarded function-value HOF callback Python gate failed: {e}")
    });
}

#[test]
fn emits_selected_imported_array_and_record_function_value_metadata() {
    let direct_calls = emit_source_with_files(
        r#"
import util { callbacks, handlers, base }

function local(a: int, b: int = 2) -> int {
    a - b
}

function main() -> int {
    let spread = [...base, local]
    callbacks[0](a: 5) + callbacks[1](b: 3, a: 10) + handlers.primary(a: 7) + handlers.second(b: 2, a: 4) + spread[1](a: 6) + spread[2](a: 9)
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
        "selected imported function values should still call through runtime storage: {direct_calls}"
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
        "selected imported array/record values should preserve callable params: {direct_calls}"
    );
    assert_generated_python_ok_int(
        &direct_calls,
        69,
        "selected imported array/record function-value call parity",
    );

    let hof_callbacks = emit_source_with_files(
        r#"
import util { callbacks, dynamicCallbacks, handlers }

concurrent {
    arraySlot: {
        let xs = [1]
        xs.map(callbacks[1])
    }
    recordField: {
        let ys = [1]
        ys.map(handlers.primary)
    }
    dynamicPlain: {
        let zs = [1]
        let i = 1
        zs.map(dynamicCallbacks[i])
    }
    fast: 0
}
0
"#,
        &[(
            "util.tpz",
            r#"
function inc(x: int) -> int {
    x + 1
}

function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

export let callbacks = [42, spin]
export let dynamicCallbacks = callbacks
export let handlers = { primary: spin }
"#,
        )],
    );
    assert!(
        hof_callbacks
            .matches("yield from tpz_array_map__co(")
            .count()
            >= 2
            && hof_callbacks
                .matches("_tpz_mod__t_7574696c___t_7370696e__co(host, __tpz_cb_0)")
                .count()
                >= 2
            && !hof_callbacks.contains("tpz_index(_t_63616c6c6261636b73, 1,")
            && !hof_callbacks.contains("tpz_member(_t_68616e646c657273,"),
        "selected imported array/record HOF callbacks should use exported cooperative metadata: {hof_callbacks}"
    );
    assert!(
        hof_callbacks.contains(
            "yield from tpz_array_map__co(_t_7a73, tpz_index(_t_64796e616d696343616c6c6261636b73, _t_69,"
        ),
        "selected imported dynamic array indexes should use the cooperative runtime driver without static callback recovery: {hof_callbacks}"
    );
    assert_generated_python_gates(&hof_callbacks).unwrap_or_else(|e| {
        panic!("selected imported array/record HOF callback Python gate failed: {e}")
    });
}

#[test]
fn emits_namespace_imported_array_and_record_function_value_metadata() {
    let direct_calls = emit_source_with_files(
        r#"
import util

function local(a: int, b: int = 2) -> int {
    a - b
}

function main() -> int {
    let spread = [...util.base, local]
    util.callbacks[0](a: 5) + util.callbacks[1](b: 3, a: 10) + util.handlers.primary(a: 7) + util.handlers.second(b: 2, a: 4) + spread[1](a: 6) + spread[2](a: 9)
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
        "namespace imported function values should still call through runtime storage: {direct_calls}"
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
        "namespace imported array/record values should preserve callable params: {direct_calls}"
    );
    assert_generated_python_ok_int(
        &direct_calls,
        69,
        "namespace imported array/record function-value call parity",
    );

    let hof_callbacks = emit_source_with_files(
        r#"
import util

concurrent {
    arraySlot: {
        let xs = [1]
        xs.map(util.callbacks[1])
    }
    recordField: {
        let ys = [1]
        ys.map(util.handlers.primary)
    }
    spreadSlot: {
        let callbacks = [...util.base]
        let zs = [1]
        zs.map(callbacks[1])
    }
    dynamicPlain: {
        let ws = [1]
        let i = 1
        ws.map(util.callbacks[i])
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
export let base = [42, spin]
"#,
        )],
    );
    assert!(
        hof_callbacks
            .matches("yield from tpz_array_map__co(")
            .count()
            >= 3
            && hof_callbacks
                .matches("_tpz_mod__t_7574696c___t_7370696e__co(host, __tpz_cb_0)")
                .count()
                >= 3,
        "namespace imported array/record HOF callbacks should use exported cooperative metadata: {hof_callbacks}"
    );
    assert!(
        hof_callbacks.contains(
            "yield from tpz_array_map__co(_t_7773, tpz_index(tpz_member(_t_7574696c, \"_t_63616c6c6261636b73\""
        ),
        "namespace imported dynamic array indexes should use the cooperative runtime driver without static callback recovery: {hof_callbacks}"
    );
    assert_generated_python_gates(&hof_callbacks).unwrap_or_else(|e| {
        panic!("namespace imported array/record HOF callback Python gate failed: {e}")
    });

    let namespace_alias_dynamic_spread_hof = emit_source_with_files(
        r#"
import util

function dbl(x: int) -> int {
    x * 2
}

function main() -> int {
    let seeds = [0, 1]
    let i = seeds[1]
    let base = util.base
    let callbacks = [...base, dbl]
    let r = concurrent {
        slow: {
            let xs = [1, 2, 3]
            let out = xs.map(callbacks[i])
            out[0] * 100 + out[1] * 10 + out[2]
        }
        fast: 0
    }
    r.slow + r.fast
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function zero(x: int) -> int {
    0
}

function inc(x: int) -> int {
    x + 1
}

export let base = [zero, inc]
"#,
        )],
    );
    assert!(
        namespace_alias_dynamic_spread_hof.contains("yield from tpz_array_map__co(")
            && namespace_alias_dynamic_spread_hof.contains("tpz_index(")
            && namespace_alias_dynamic_spread_hof
                .matches("__co(host, __tpz_cb_")
                .count()
                == 0
            && !namespace_alias_dynamic_spread_hof
                .contains("_tpz_mod__t_7574696c___t_7a65726f__co(host, __tpz_cb_0)")
            && !namespace_alias_dynamic_spread_hof
                .contains("_tpz_mod__t_7574696c___t_696e63__co(host, __tpz_cb_0)")
            && !namespace_alias_dynamic_spread_hof.contains("_t_64626c__co(host, __tpz_cb_0)"),
        "namespace-member local-alias spread dynamic-index Array.map should keep runtime index reads and avoid static callback recovery: {namespace_alias_dynamic_spread_hof}"
    );
    assert_generated_python_ok_int(
        &namespace_alias_dynamic_spread_hof,
        234,
        "namespace-member local-alias spread dynamic-index Array.map value parity",
    );
}

#[test]
fn imported_option_result_patterns_preserve_wrapped_value_metadata() {
    let module = r#"
function add(a: int, b: int) -> int { a + b }

export let optionArray = Some([3, 1, 2])
export let resultArray: Result<Array<int>, string> = Ok([6, 4, 5])
export let optionCallback = Some(add)
export let resultCallback: Result<(int, int) -> int, string> = Ok(add)
"#;
    let selected = emit_source_with_files(
        r#"
import util { optionArray, resultArray, optionCallback, resultCallback }

function main() -> int {
    let optionArrayScore = match optionArray {
        case Some(xs) => xs.sorted()[0]
        case None => 0
    }
    let resultArrayScore = match resultArray {
        case Ok(xs) => xs.sorted()[0]
        case Err(_) => 0
    }
    let optionCallbackScore = match optionCallback {
        case Some(callback) => callback(b: 3, a: 10)
        case None => 0
    }
    let resultCallbackScore = match resultCallback {
        case Ok(callback) => callback(b: 3, a: 10)
        case Err(_) => 0
    }
    optionArrayScore * 1000 + resultArrayScore * 100 + optionCallbackScore * 10 + resultCallbackScore
}
main()
"#,
        &[("util.tpz", module)],
    );
    assert_generated_python_ok_int(
        &selected,
        1543,
        "selected imported Option and Result pattern metadata parity",
    );

    let namespace = emit_source_with_files(
        r#"
import util

function main() -> int {
    let optionArrayScore = match util.optionArray {
        case Some(xs) => xs.sorted()[0]
        case None => 0
    }
    let resultArrayScore = match util.resultArray {
        case Ok(xs) => xs.sorted()[0]
        case Err(_) => 0
    }
    let optionCallbackScore = match util.optionCallback {
        case Some(callback) => callback(b: 3, a: 10)
        case None => 0
    }
    let resultCallbackScore = match util.resultCallback {
        case Ok(callback) => callback(b: 3, a: 10)
        case Err(_) => 0
    }
    optionArrayScore * 1000 + resultArrayScore * 100 + optionCallbackScore * 10 + resultCallbackScore
}
main()
"#,
        &[("util.tpz", module)],
    );
    assert_generated_python_ok_int(
        &namespace,
        1543,
        "namespace imported Option and Result pattern metadata parity",
    );
}

#[test]
fn emits_imported_spread_array_dynamic_index_named_default_function_value_calls() {
    let selected_direct = emit_source_with_files(
        r#"
import util { base }

function sub(a: int, b: int = 10) -> int {
    a - b
}

function main() -> int {
    let callbacks = [...base, sub]
    let i = 0
    let j = 1
    callbacks[i](a: 5) * 10 + callbacks[j](a: 15)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
        )],
    );
    assert!(
        selected_direct.matches("tpz_call(tpz_index(").count() >= 2
            && selected_direct.contains("\"_t_61\": 5")
            && selected_direct.contains("\"_t_61\": 15"),
        "selected imported spread dynamic direct calls should preserve runtime index and named/default metadata: {selected_direct}"
    );
    assert_generated_python_ok_int(
        &selected_direct,
        75,
        "selected imported spread dynamic-index named/default direct-call parity",
    );

    let namespace_direct = emit_source_with_files(
        r#"
import util

function sub(a: int, b: int = 10) -> int {
    a - b
}

function main() -> int {
    let callbacks = [...util.base, sub]
    let i = 0
    let j = 1
    callbacks[i](a: 5) * 10 + callbacks[j](a: 15)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
        )],
    );
    assert!(
        namespace_direct.matches("tpz_call(tpz_index(").count() >= 2
            && namespace_direct.contains("\"_t_61\": 5")
            && namespace_direct.contains("\"_t_61\": 15"),
        "namespace imported spread dynamic direct calls should preserve runtime index and named/default metadata: {namespace_direct}"
    );
    assert_generated_python_ok_int(
        &namespace_direct,
        75,
        "namespace imported spread dynamic-index named/default direct-call parity",
    );

    let selected_pipe = emit_source_with_files(
        r#"
import util { base }

function sub(a: int, b: int = 10) -> int {
    a - b
}

function main() -> int {
    let callbacks = [...base, sub]
    let i = 0
    let j = 1
    let a = 5 |> callbacks[i](a: _)
    let b = 15 |> callbacks[j](a: _)
    a * 10 + b
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
        )],
    );
    assert!(
        selected_pipe.matches("tpz_call(tpz_index(").count() >= 2
            && selected_pipe.contains("\"_t_61\": __tpz_piped"),
        "selected imported spread dynamic pipe calls should preserve runtime index and named/default metadata: {selected_pipe}"
    );
    assert_generated_python_ok_int(
        &selected_pipe,
        75,
        "selected imported spread dynamic-index named/default pipe parity",
    );

    let namespace_pipe = emit_source_with_files(
        r#"
import util

function sub(a: int, b: int = 10) -> int {
    a - b
}

function main() -> int {
    let callbacks = [...util.base, sub]
    let i = 0
    let j = 1
    let a = 5 |> callbacks[i](a: _)
    let b = 15 |> callbacks[j](a: _)
    a * 10 + b
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
        )],
    );
    assert!(
        namespace_pipe.matches("tpz_call(tpz_index(").count() >= 2
            && namespace_pipe.contains("\"_t_61\": __tpz_piped"),
        "namespace imported spread dynamic pipe calls should preserve runtime index and named/default metadata: {namespace_pipe}"
    );
    assert_generated_python_ok_int(
        &namespace_pipe,
        75,
        "namespace imported spread dynamic-index named/default pipe parity",
    );
}

#[test]
fn emits_imported_spread_array_alias_dynamic_index_named_default_function_value_calls() {
    let selected_direct = emit_source_with_files(
        r#"
import util { base }

function sub(a: int, b: int = 10) -> int {
    a - b
}

function main() -> int {
    let spread = [...base, sub]
    let callbacks = spread
    let i = 0
    let j = 1
    callbacks[i](a: 5) * 10 + callbacks[j](a: 15)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
        )],
    );
    assert!(
        selected_direct.matches("tpz_call(tpz_index(").count() >= 2
            && selected_direct.contains("\"_t_61\": 5")
            && selected_direct.contains("\"_t_61\": 15"),
        "selected imported spread alias dynamic direct calls should preserve runtime index and named/default metadata: {selected_direct}"
    );
    assert_generated_python_ok_int(
        &selected_direct,
        75,
        "selected imported spread alias dynamic-index named/default direct-call parity",
    );

    let selected_pipe = emit_source_with_files(
        r#"
import util { base }

function sub(a: int, b: int = 10) -> int {
    a - b
}

function main() -> int {
    let spread = [...base, sub]
    let callbacks = spread
    let i = 0
    let j = 1
    let a = 5 |> callbacks[i](a: _)
    let b = 15 |> callbacks[j](a: _)
    a * 10 + b
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
        )],
    );
    assert!(
        selected_pipe.matches("tpz_call(tpz_index(").count() >= 2
            && selected_pipe.contains("\"_t_61\": __tpz_piped"),
        "selected imported spread alias dynamic pipe calls should preserve runtime index and named/default metadata: {selected_pipe}"
    );
    assert_generated_python_ok_int(
        &selected_pipe,
        75,
        "selected imported spread alias dynamic-index named/default pipe parity",
    );

    let namespace_direct = emit_source_with_files(
        r#"
import util

function sub(a: int, b: int = 10) -> int {
    a - b
}

function main() -> int {
    let spread = [...util.base, sub]
    let callbacks = spread
    let i = 0
    let j = 1
    callbacks[i](a: 5) * 10 + callbacks[j](a: 15)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
        )],
    );
    assert!(
        namespace_direct.matches("tpz_call(tpz_index(").count() >= 2
            && namespace_direct.contains("\"_t_61\": 5")
            && namespace_direct.contains("\"_t_61\": 15"),
        "namespace imported spread alias dynamic direct calls should preserve runtime index and named/default metadata: {namespace_direct}"
    );
    assert_generated_python_ok_int(
        &namespace_direct,
        75,
        "namespace imported spread alias dynamic-index named/default direct-call parity",
    );

    let namespace_pipe = emit_source_with_files(
        r#"
import util

function sub(a: int, b: int = 10) -> int {
    a - b
}

function main() -> int {
    let spread = [...util.base, sub]
    let callbacks = spread
    let i = 0
    let j = 1
    let a = 5 |> callbacks[i](a: _)
    let b = 15 |> callbacks[j](a: _)
    a * 10 + b
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
        )],
    );
    assert!(
        namespace_pipe.matches("tpz_call(tpz_index(").count() >= 2
            && namespace_pipe.contains("\"_t_61\": __tpz_piped"),
        "namespace imported spread alias dynamic pipe calls should preserve runtime index and named/default metadata: {namespace_pipe}"
    );
    assert_generated_python_ok_int(
        &namespace_pipe,
        75,
        "namespace imported spread alias dynamic-index named/default pipe parity",
    );
}

#[test]
fn keeps_unproven_imported_spread_array_alias_dynamic_index_metadata_plain() {
    let assert_unsupported = |error: PyEmitError, expected: &str, context: &str| {
        assert_eq!(error.code(), "TPZ6PY0001");
        match error.kind {
            PyEmitErrorKind::Unsupported(what) => {
                assert_eq!(what, expected, "{context}")
            }
            other => panic!("{context}: expected unsupported error, got {other:?}"),
        }
    };

    let mutable_spread = emit_source_with_files(
        r#"
import util { base }

function sub(a: int, b: int = 10) -> int {
    a - b
}

function main() -> int {
    let mut spread = [...base, sub]
    let callbacks = spread
    let i = 0
    callbacks[i](a: 5)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
        )],
    );
    assert_generated_python_ok_int(
        &mutable_spread,
        7,
        "tracked mutable spread-built alias dynamic direct-call",
    );

    assert_unsupported(
        emit_error_for_source_with_files(
            r#"
import util { base }

function sub(a: int, b: int = 10) -> int {
    a - b
}

function main() -> int {
    let spread = [...base, sub]
    let mut callbacks = spread
    let i = 0
    callbacks[i](a: 5)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
            )],
        ),
        "call argument shape",
        "mutable alias of spread-built dynamic direct-call",
    );

    assert_unsupported(
        emit_error_for_source_with_files(
            r#"
import util { base }

function sub(a: int, b: int = 10) -> int {
    a - b
}

function main() -> int {
    let spread = [...base, sub]
    let other = [sub]
    let mut callbacks = spread
    callbacks = other
    let i = 0
    callbacks[i](a: 5)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
            )],
        ),
        "call argument shape",
        "post-alias reassigned spread-built dynamic direct-call",
    );

    assert_unsupported(
        emit_error_for_source_with_files(
            r#"
import util { base }

function sub(a: int, b: int = 10) -> int {
    a - b
}

function mul(a: int, b: int) -> int {
    a * b
}

function main() -> int {
    let spread = [...base, sub]
    let other = [mul]
    let callbacks = if true {
        spread
    } else {
        other
    }
    let i = 0
    callbacks[i](a: 5)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
            )],
        ),
        "call argument shape",
        "mismatched conditional alias of spread-built dynamic direct-call",
    );

    assert_unsupported(
        emit_error_for_source_with_files(
            r#"
import util { base }

function sub(a: int, b: int = 10) -> int {
    a - b
}

function mul(a: int, b: int) -> int {
    a * b
}

function main() -> int {
    let spread = [...base, sub]
    let other = [mul]
    let callbacks = match true {
        case true => spread
        case _ => other
    }
    let i = 0
    callbacks[i](a: 5)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
            )],
        ),
        "call argument shape",
        "mismatched match alias of spread-built dynamic direct-call",
    );

    assert_unsupported(
        emit_error_for_source_with_files(
            r#"
import util { base }

function mul(a: int, b: int) -> int {
    a * b
}

function main() -> int {
    let spread = [...base, mul]
    let callbacks = spread
    let i = 0
    callbacks[i](a: 5)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
            )],
        ),
        "call argument shape",
        "heterogeneous spread-built alias dynamic direct-call",
    );
}

#[test]
fn keeps_unproven_namespace_member_alias_spread_array_dynamic_index_metadata_plain() {
    let assert_unsupported = |error: PyEmitError, expected: &str, context: &str| {
        assert_eq!(error.code(), "TPZ6PY0001");
        match error.kind {
            PyEmitErrorKind::Unsupported(what) => {
                assert_eq!(what, expected, "{context}")
            }
            other => panic!("{context}: expected unsupported error, got {other:?}"),
        }
    };

    assert_unsupported(
        emit_error_for_source_with_files(
            r#"
import util

function sub(a: int, b: int = 10) -> int {
    a - b
}

function main() -> int {
    let mut base = util.base
    let spread = [...base, sub]
    let callbacks = spread
    let i = 0
    callbacks[i](a: 5)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
            )],
        ),
        "call argument shape",
        "mutable namespace-member spread source dynamic direct-call",
    );

    let mutable_spread = emit_source_with_files(
        r#"
import util

function sub(a: int, b: int = 10) -> int {
    a - b
}

function main() -> int {
    let base = util.base
    let mut spread = [...base, sub]
    let callbacks = spread
    let i = 0
    callbacks[i](a: 5)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
        )],
    );
    assert_generated_python_ok_int(
        &mutable_spread,
        7,
        "tracked mutable namespace-member spread-built alias dynamic direct-call",
    );

    assert_unsupported(
        emit_error_for_source_with_files(
            r#"
import util

function sub(a: int, b: int = 10) -> int {
    a - b
}

function main() -> int {
    let base = util.base
    let spread = [...base, sub]
    let mut callbacks = spread
    let i = 0
    callbacks[i](a: 5)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
            )],
        ),
        "call argument shape",
        "mutable namespace-member spread alias dynamic direct-call",
    );

    assert_unsupported(
        emit_error_for_source_with_files(
            r#"
import util

function sub(a: int, b: int = 10) -> int {
    a - b
}

function main() -> int {
    let base = util.base
    let spread = [...base, sub]
    let other = [sub]
    let mut callbacks = spread
    callbacks = other
    let i = 0
    callbacks[i](a: 5)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
            )],
        ),
        "call argument shape",
        "post-alias reassigned namespace-member spread-built dynamic direct-call",
    );

    assert_unsupported(
        emit_error_for_source_with_files(
            r#"
import util

function sub(a: int, b: int = 10) -> int {
    a - b
}

function mul(a: int, b: int) -> int {
    a * b
}

function main() -> int {
    let base = util.base
    let spread = [...base, sub]
    let other = [mul]
    let callbacks = if true {
        spread
    } else {
        other
    }
    let i = 0
    callbacks[i](a: 5)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
            )],
        ),
        "call argument shape",
        "mismatched conditional namespace-member spread-built dynamic direct-call",
    );

    assert_unsupported(
        emit_error_for_source_with_files(
            r#"
import util

function sub(a: int, b: int = 10) -> int {
    a - b
}

function mul(a: int, b: int) -> int {
    a * b
}

function main() -> int {
    let base = util.base
    let spread = [...base, sub]
    let other = [mul]
    let callbacks = match true {
        case true => spread
        case _ => other
    }
    let i = 0
    callbacks[i](a: 5)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
            )],
        ),
        "call argument shape",
        "mismatched match namespace-member spread-built dynamic direct-call",
    );
}

#[test]
fn keeps_unproven_imported_spread_array_dynamic_index_metadata_plain() {
    let assert_unsupported = |error: PyEmitError, expected: &str, context: &str| {
        assert_eq!(error.code(), "TPZ6PY0001");
        match error.kind {
            PyEmitErrorKind::Unsupported(what) => {
                assert_eq!(what, expected, "{context}")
            }
            other => panic!("{context}: expected unsupported error, got {other:?}"),
        }
    };

    assert_unsupported(
        emit_error_for_source_with_files(
            r#"
import util { base }

function add(a: int, b: int = 2) -> int {
    a + b
}

function main() -> int {
    let callbacks = [...base, add]
    let i = 1
    callbacks[i](a: 5)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
export let base = [42]
"#,
            )],
        ),
        "call argument shape",
        "selected imported spread non-callable prefix dynamic direct-call",
    );

    assert_unsupported(
        emit_error_for_source_with_files(
            r#"
import util

function add(a: int, b: int = 2) -> int {
    a + b
}

function main() -> int {
    let callbacks = [...util.base, add]
    let i = 1
    callbacks[i](a: 5)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
export let base = [42]
"#,
            )],
        ),
        "call argument shape",
        "namespace imported spread non-callable prefix dynamic direct-call",
    );

    assert_unsupported(
        emit_error_for_source_with_files(
            r#"
import util { base }

function add(a: int, b: int = 2) -> int {
    a + b
}

function main() -> int {
    let callbacks = [...base, add]
    let i = 1
    5 |> callbacks[i](a: _)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
export let base = [42]
"#,
            )],
        ),
        "pipe stage call target",
        "selected imported spread non-callable prefix dynamic pipe-call",
    );

    assert_unsupported(
        emit_error_for_source_with_files(
            r#"
import util

function add(a: int, b: int = 2) -> int {
    a + b
}

function main() -> int {
    let callbacks = [...util.base, add]
    let i = 1
    5 |> callbacks[i](a: _)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
export let base = [42]
"#,
            )],
        ),
        "pipe stage call target",
        "namespace imported spread non-callable prefix dynamic pipe-call",
    );

    assert_unsupported(
        emit_error_for_source_with_files(
            r#"
import util { base }

function mul(a: int, b: int) -> int {
    a * b
}

function main() -> int {
    let callbacks = [...base, mul]
    let i = 0
    callbacks[i](a: 5)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
            )],
        ),
        "call argument shape",
        "selected imported spread heterogeneous dynamic direct-call",
    );

    assert_unsupported(
        emit_error_for_source_with_files(
            r#"
import util

function mul(a: int, b: int) -> int {
    a * b
}

function main() -> int {
    let callbacks = [...util.base, mul]
    let i = 0
    callbacks[i](a: 5)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
            )],
        ),
        "call argument shape",
        "namespace imported spread heterogeneous dynamic direct-call",
    );

    assert_unsupported(
        emit_error_for_source_with_files(
            r#"
import util { base }

function mul(a: int, b: int) -> int {
    a * b
}

function main() -> int {
    let callbacks = [...base, mul]
    let i = 0
    5 |> callbacks[i](a: _)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
            )],
        ),
        "pipe stage call target",
        "selected imported spread heterogeneous dynamic pipe-call",
    );

    assert_unsupported(
        emit_error_for_source_with_files(
            r#"
import util

function mul(a: int, b: int) -> int {
    a * b
}

function main() -> int {
    let callbacks = [...util.base, mul]
    let i = 0
    5 |> callbacks[i](a: _)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

export let base = [add]
"#,
            )],
        ),
        "pipe stage call target",
        "namespace imported spread heterogeneous dynamic pipe-call",
    );

    let (mutable_export, mutable_unit) = emit_unchecked_error_and_unit_for_source_with_files(
        r#"
import util { base }

function add(a: int, b: int = 2) -> int {
    a + b
}

function main() -> int {
    let callbacks = [...base, add]
    let i = 1
    callbacks[i](a: 5)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
export let mut base = [add]

function add(a: int, b: int = 2) -> int {
    a + b
}
"#,
        )],
    );
    assert!(
        !mutable_unit.diagnostics.is_empty(),
        "export let mut spread source must remain a shared resolver diagnostic before normal emission"
    );
    assert_unsupported(
        mutable_export,
        "call argument shape",
        "mutable exported selected spread source dynamic direct-call",
    );
}
