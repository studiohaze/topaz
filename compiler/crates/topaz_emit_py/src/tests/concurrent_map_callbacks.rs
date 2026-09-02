use super::*;

#[test]
fn emits_concurrent_no_timeout_map_get_composed_hof_callbacks() {
    let generated = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    1 / 0
}
function same(x: int) -> int {
    x
}

let callbacks = map { "both": spin >> same }

concurrent {
    slow: {
        let xs = [1]
        match callbacks.get("both") {
            case Some(cb) => xs.map(cb)
            case None => xs
        }
    }
    fast: 0
}
0
"#,
    );
    assert!(
        generated.contains("yield from tpz_array_map__co(")
            && generated.contains(
                "tpz_compose(tpz_host_callable(_t_7370696e, host, _t_7370696e__co), tpz_host_callable(_t_73616d65, host, _t_73616d65__co)"
            )
            && !generated.contains(".__call_cooperative__(__tpz_cb_0)"),
        "Map.get composed HOF callbacks should carry cooperative operands and use the runtime cooperative driver: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("Map.get composed HOF callback Python gate failed: {e}"));
}

#[test]
fn emits_concurrent_no_timeout_map_get_lambda_hof_callbacks() {
    let generated = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    1 / 0
}

let callbacks = map { "plain": (x) => spin(x) }

concurrent {
    slow: {
        let xs = [1]
        match callbacks.get("plain") {
            case Some(cb) => xs.map(cb)
            case None => xs
        }
    }
    fast: 0
}
0
"#,
    );
    assert!(
        generated.contains("yield from tpz_array_map__co(")
            && generated.contains(
                "tpz_cooperative_callable((lambda _t_78: _t_7370696e(host, _t_78)), (lambda _t_78: (yield from _t_7370696e__co(host, _t_78))))"
            )
            && !generated.contains("tpz_host_callable(")
            && !generated.contains(".__call_cooperative__(__tpz_cb_0)"),
        "Map.get lambda HOF callbacks should store a hostless cooperative callable and use the runtime cooperative driver: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("Map.get lambda HOF callback Python gate failed: {e}"));
}

#[test]
fn emits_concurrent_no_timeout_map_get_map_values_hof_callbacks() {
    let function_carrier = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    1 / 0
}

let callbacks = map { "primary": spin }

concurrent {
    slow: {
        let values = map { "a": 1 }
        match callbacks.get("primary") {
            case Some(cb) => values.mapValues(cb)
            case None => values
        }
    }
    fast: 0
}
0
"#,
    );
    assert!(
        function_carrier.contains("yield from tpz_map_map_values__co(")
            && function_carrier.contains("tpz_get(_t_63616c6c6261636b73")
            && function_carrier.contains("tpz_host_callable(_t_7370696e, host, _t_7370696e__co)")
            && !function_carrier.contains(".__call_cooperative__(__tpz_cb_0)")
            && !function_carrier.contains("yield from _t_7370696e__co(host, __tpz_cb_0)"),
        "Map.get function callbacks feeding Map.mapValues should store a host callable and use the runtime cooperative driver: {function_carrier}"
    );
    assert_generated_python_gates(&function_carrier).unwrap_or_else(|e| {
        panic!("Map.get Map.mapValues function carrier Python gate failed: {e}")
    });

    let alias_carrier = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    1 / 0
}

let aliasSpin = spin
let callbacks = map { "alias": aliasSpin }

concurrent {
    slow: {
        let values = map { "a": 1 }
        match callbacks.get("alias") {
            case Some(cb) => values.mapValues(cb)
            case None => values
        }
    }
    fast: 0
}
0
"#,
    );
    assert!(
        alias_carrier.contains("yield from tpz_map_map_values__co(")
            && alias_carrier.contains("tpz_get(_t_63616c6c6261636b73")
            && alias_carrier.contains("_t_616c6961735370696e")
            && alias_carrier.contains("tpz_host_callable(_t_7370696e, host, _t_7370696e__co)")
            && !alias_carrier.contains(".__call_cooperative__(__tpz_cb_0)")
            && !alias_carrier.contains("yield from _t_7370696e__co(host, __tpz_cb_0)"),
        "Map.get immutable-alias callbacks feeding Map.mapValues should keep alias carrier storage and use the runtime cooperative driver: {alias_carrier}"
    );
    assert_generated_python_gates(&alias_carrier)
        .unwrap_or_else(|e| panic!("Map.get Map.mapValues alias carrier Python gate failed: {e}"));

    let composed_carrier = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    1 / 0
}
function same(x: int) -> int {
    x
}

let callbacks = map { "both": spin >> same }

concurrent {
    slow: {
        let values = map { "a": 1 }
        match callbacks.get("both") {
            case Some(cb) => values.mapValues(cb)
            case None => values
        }
    }
    fast: 0
}
0
"#,
    );
    assert!(
        composed_carrier.contains("yield from tpz_map_map_values__co(")
            && composed_carrier.contains("tpz_get(_t_63616c6c6261636b73")
            && composed_carrier.contains(
                "tpz_compose(tpz_host_callable(_t_7370696e, host, _t_7370696e__co), tpz_host_callable(_t_73616d65, host, _t_73616d65__co)"
            )
            && !composed_carrier.contains(".__call_cooperative__(__tpz_cb_0)")
            && !composed_carrier.contains("yield from _t_7370696e__co(host, __tpz_cb_0)")
            && !composed_carrier.contains("yield from _t_73616d65__co(host, __tpz_cb_0)"),
        "Map.get composed callbacks feeding Map.mapValues should carry cooperative operands and use the runtime cooperative driver: {composed_carrier}"
    );
    assert_generated_python_gates(&composed_carrier).unwrap_or_else(|e| {
        panic!("Map.get Map.mapValues composed carrier Python gate failed: {e}")
    });

    let lambda_carrier = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    1 / 0
}

let callbacks = map { "plain": (x) => spin(x) }

concurrent {
    slow: {
        let values = map { "a": 1 }
        match callbacks.get("plain") {
            case Some(cb) => values.mapValues(cb)
            case None => values
        }
    }
    fast: 0
}
0
"#,
    );
    assert!(
        lambda_carrier.contains("yield from tpz_map_map_values__co(")
            && lambda_carrier.contains("tpz_get(_t_63616c6c6261636b73")
            && lambda_carrier.contains(
                "tpz_cooperative_callable((lambda _t_78: _t_7370696e(host, _t_78)), (lambda _t_78: (yield from _t_7370696e__co(host, _t_78))))"
            )
            && !lambda_carrier.contains("tpz_host_callable(")
            && !lambda_carrier.contains(".__call_cooperative__(__tpz_cb_0)"),
        "Map.get lambda callbacks feeding Map.mapValues should store a hostless cooperative callable and use the runtime cooperative driver: {lambda_carrier}"
    );
    assert_generated_python_gates(&lambda_carrier)
        .unwrap_or_else(|e| panic!("Map.get Map.mapValues lambda carrier Python gate failed: {e}"));
}

#[test]
fn emits_concurrent_no_timeout_map_get_map_update_hof_callbacks() {
    let function_carrier = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    1 / 0
}

let callbacks = map { "primary": spin }

concurrent {
    slow: {
        let mut values = map { "a": 1 }
        match callbacks.get("primary") {
            case Some(cb) => values.update("a", 0, cb)
            case None => ()
        }
    }
    fast: 0
}
0
"#,
    );
    assert!(
        function_carrier.contains("yield from tpz_map_update__co(")
            && function_carrier.contains("tpz_get(_t_63616c6c6261636b73")
            && function_carrier.contains("tpz_host_callable(_t_7370696e, host, _t_7370696e__co)")
            && !function_carrier.contains(".__call_cooperative__(__tpz_cb_0)")
            && !function_carrier.contains("yield from _t_7370696e__co(host, __tpz_cb_0)"),
        "Map.get function callbacks feeding Map.update should store a host callable and use the runtime cooperative driver: {function_carrier}"
    );
    assert_generated_python_gates(&function_carrier)
        .unwrap_or_else(|e| panic!("Map.get Map.update function carrier Python gate failed: {e}"));

    let alias_carrier = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    1 / 0
}

let aliasSpin = spin
let callbacks = map { "alias": aliasSpin }

concurrent {
    slow: {
        let mut values = map { "a": 1 }
        match callbacks.get("alias") {
            case Some(cb) => values.update("a", 0, cb)
            case None => ()
        }
    }
    fast: 0
}
0
"#,
    );
    assert!(
        alias_carrier.contains("yield from tpz_map_update__co(")
            && alias_carrier.contains("tpz_get(_t_63616c6c6261636b73")
            && alias_carrier.contains("_t_616c6961735370696e")
            && alias_carrier.contains("tpz_host_callable(_t_7370696e, host, _t_7370696e__co)")
            && !alias_carrier.contains(".__call_cooperative__(__tpz_cb_0)")
            && !alias_carrier.contains("yield from _t_7370696e__co(host, __tpz_cb_0)"),
        "Map.get immutable-alias callbacks feeding Map.update should keep alias carrier storage and use the present-key runtime cooperative driver: {alias_carrier}"
    );
    assert_generated_python_gates(&alias_carrier)
        .unwrap_or_else(|e| panic!("Map.get Map.update alias carrier Python gate failed: {e}"));

    let composed_carrier = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    1 / 0
}
function same(x: int) -> int {
    x
}

let callbacks = map { "both": spin >> same }

concurrent {
    slow: {
        let mut values = map { "a": 1 }
        match callbacks.get("both") {
            case Some(cb) => values.update("a", 0, cb)
            case None => ()
        }
    }
    fast: 0
}
0
"#,
    );
    assert!(
        composed_carrier.contains("yield from tpz_map_update__co(")
            && composed_carrier.contains("tpz_get(_t_63616c6c6261636b73")
            && composed_carrier.contains(
                "tpz_compose(tpz_host_callable(_t_7370696e, host, _t_7370696e__co), tpz_host_callable(_t_73616d65, host, _t_73616d65__co)"
            )
            && !composed_carrier.contains(".__call_cooperative__(__tpz_cb_0)")
            && !composed_carrier.contains("yield from _t_7370696e__co(host, __tpz_cb_0)")
            && !composed_carrier.contains("yield from _t_73616d65__co(host, __tpz_cb_0)"),
        "Map.get composed callbacks feeding Map.update should carry cooperative operands and use the runtime cooperative driver: {composed_carrier}"
    );
    assert_generated_python_gates(&composed_carrier)
        .unwrap_or_else(|e| panic!("Map.get Map.update composed carrier Python gate failed: {e}"));

    let lambda_carrier = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    1 / 0
}

let callbacks = map { "plain": (x) => spin(x) }

concurrent {
    slow: {
        let mut values = map { "a": 1 }
        match callbacks.get("plain") {
            case Some(cb) => values.update("a", 0, cb)
            case None => ()
        }
    }
    fast: 0
}
0
"#,
    );
    assert!(
        lambda_carrier.contains("yield from tpz_map_update__co(")
            && lambda_carrier.contains("tpz_get(_t_63616c6c6261636b73")
            && lambda_carrier.contains(
                "tpz_cooperative_callable((lambda _t_78: _t_7370696e(host, _t_78)), (lambda _t_78: (yield from _t_7370696e__co(host, _t_78))))"
            )
            && !lambda_carrier.contains("tpz_host_callable(")
            && !lambda_carrier.contains(".__call_cooperative__(__tpz_cb_0)"),
        "Map.get lambda callbacks feeding Map.update should store a hostless cooperative callable and use the runtime cooperative driver: {lambda_carrier}"
    );
    assert_generated_python_gates(&lambda_carrier)
        .unwrap_or_else(|e| panic!("Map.get Map.update lambda carrier Python gate failed: {e}"));
}

#[test]
fn emits_concurrent_no_timeout_map_get_map_filter_hof_callbacks() {
    let direct_co_callback_recovery_count =
        |generated: &str| generated.matches("__co(host, __tpz_cb_").count();

    let function_carrier = emit_source(
        r#"
function keep(key: string, value: int) -> bool {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    key != "a" && value > 1
}

let callbacks = map { "primary": keep }

let r = concurrent {
    slow: {
        let values = map { "a": 2, "b": 3, "c": 1 }
        match callbacks.get("primary") {
            case Some(cb) => {
                let kept = values.filter(cb)
                kept.getOr("a", 0) + kept.getOr("b", 0) + kept.getOr("c", 0)
            }
            case None => -1
        }
    }
    fast: 0
}
r.slow + r.fast
"#,
    );
    assert!(
        function_carrier.contains("yield from tpz_map_filter__co(")
            && function_carrier.contains("tpz_get(_t_63616c6c6261636b73")
            && function_carrier.contains("tpz_host_callable(_t_6b656570, host, _t_6b656570__co)")
            && direct_co_callback_recovery_count(&function_carrier) == 0,
        "Map.get function callbacks feeding Map.filter should use the arity-2 runtime cooperative driver and retain/drop witness: {function_carrier}"
    );
    assert_generated_python_ok_int(&function_carrier, 3, "Map.get Map.filter function carrier");

    let alias_carrier = emit_source(
        r#"
function keep(key: string, value: int) -> bool {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    key != "a" && value > 1
}

let aliasKeep = keep
let callbacks = map { "alias": aliasKeep }

let r = concurrent {
    slow: {
        let values = map { "a": 2, "b": 3, "c": 1 }
        match callbacks.get("alias") {
            case Some(cb) => {
                let kept = values.filter(cb)
                kept.getOr("a", 0) + kept.getOr("b", 0) + kept.getOr("c", 0)
            }
            case None => -1
        }
    }
    fast: 0
}
r.slow + r.fast
"#,
    );
    assert!(
        alias_carrier.contains("yield from tpz_map_filter__co(")
            && alias_carrier.contains("tpz_get(_t_63616c6c6261636b73")
            && alias_carrier.contains("_t_616c6961734b656570")
            && alias_carrier.contains("tpz_host_callable(_t_6b656570, host, _t_6b656570__co)")
            && direct_co_callback_recovery_count(&alias_carrier) == 0,
        "Map.get immutable-alias callbacks feeding Map.filter should keep alias carrier storage and use the runtime cooperative driver: {alias_carrier}"
    );
    assert_generated_python_ok_int(&alias_carrier, 3, "Map.get Map.filter alias carrier");

    let lambda_carrier = emit_source(
        r#"
function keep(key: string, value: int) -> bool {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    key != "a" && value > 1
}

let callbacks = map { "plain": (key, value) => keep(key, value) }

let r = concurrent {
    slow: {
        let values = map { "a": 2, "b": 3, "c": 1 }
        match callbacks.get("plain") {
            case Some(cb) => {
                let kept = values.filter(f: cb)
                kept.getOr("a", 0) + kept.getOr("b", 0) + kept.getOr("c", 0)
            }
            case None => -1
        }
    }
    fast: 0
}
r.slow + r.fast
"#,
    );
    assert!(
        lambda_carrier.contains("yield from tpz_map_filter__co(")
            && lambda_carrier.contains("tpz_get(_t_63616c6c6261636b73")
            && lambda_carrier.contains(
                "tpz_cooperative_callable((lambda _t_6b6579, _t_76616c7565: _t_6b656570(host, _t_6b6579, _t_76616c7565)), (lambda _t_6b6579, _t_76616c7565: (yield from _t_6b656570__co(host, _t_6b6579, _t_76616c7565))))"
            )
            && !lambda_carrier.contains("tpz_host_callable(")
            && direct_co_callback_recovery_count(&lambda_carrier) == 0,
        "Map.get lambda callbacks feeding named Map.filter should store an arity-2 hostless cooperative callable and use the runtime cooperative driver: {lambda_carrier}"
    );
    assert_generated_python_ok_int(&lambda_carrier, 3, "Map.get Map.filter lambda carrier");
}

#[test]
fn emits_concurrent_no_timeout_map_get_array_reduce_hof_callbacks() {
    let direct_co_callback_recovery_count =
        |generated: &str| generated.matches("__co(host, __tpz_cb_").count();

    let function_carrier = emit_source(
        r#"
function mix(acc: int, value: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    acc * 10 + value
}

let callbacks = map { "mix": mix }

let r = concurrent {
    slow: {
        let xs = [2, 3]
        match callbacks.get("mix") {
            case Some(cb) => xs.reduce(1, cb)
            case None => -1
        }
    }
    fast: 0
}
r.slow + r.fast
"#,
    );
    assert!(
        function_carrier.contains("yield from tpz_array_reduce__co(")
            && function_carrier.contains("tpz_get(_t_63616c6c6261636b73")
            && function_carrier.contains("tpz_host_callable(_t_6d6978, host, _t_6d6978__co)")
            && direct_co_callback_recovery_count(&function_carrier) == 0,
        "Map.get function callbacks feeding Array.reduce should use the arity-2 accumulator runtime cooperative driver: {function_carrier}"
    );
    assert_generated_python_ok_int(
        &function_carrier,
        123,
        "Map.get Array.reduce function carrier",
    );

    let alias_carrier = emit_source(
        r#"
function mix(acc: int, value: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    acc * 10 + value
}

let aliasMix = mix
let callbacks = map { "alias": aliasMix }

let r = concurrent {
    slow: {
        let xs = [2, 3]
        match callbacks.get("alias") {
            case Some(cb) => xs.reduce(1, cb)
            case None => -1
        }
    }
    fast: 0
}
r.slow + r.fast
"#,
    );
    assert!(
        alias_carrier.contains("yield from tpz_array_reduce__co(")
            && alias_carrier.contains("tpz_get(_t_63616c6c6261636b73")
            && alias_carrier.contains("_t_616c6961734d6978")
            && alias_carrier.contains("tpz_host_callable(_t_6d6978, host, _t_6d6978__co)")
            && direct_co_callback_recovery_count(&alias_carrier) == 0,
        "Map.get immutable-alias callbacks feeding Array.reduce should preserve alias storage and use the arity-2 accumulator runtime cooperative driver: {alias_carrier}"
    );
    assert_generated_python_ok_int(&alias_carrier, 123, "Map.get Array.reduce alias carrier");

    let lambda_carrier = emit_source(
        r#"
function mix(acc: int, value: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    acc * 10 + value
}

let callbacks = map { "plain": (acc, value) => mix(acc, value) }

let r = concurrent {
    slow: {
        let xs = [2, 3]
        match callbacks.get("plain") {
            case Some(cb) => xs.reduce(initial: 1, f: cb)
            case None => -1
        }
    }
    fast: 0
}
r.slow + r.fast
"#,
    );
    assert!(
        lambda_carrier.contains("yield from tpz_array_reduce__co(")
            && lambda_carrier.contains("tpz_get(_t_63616c6c6261636b73")
            && lambda_carrier.contains(
                "tpz_cooperative_callable((lambda _t_616363, _t_76616c7565: _t_6d6978(host, _t_616363, _t_76616c7565)), (lambda _t_616363, _t_76616c7565: (yield from _t_6d6978__co(host, _t_616363, _t_76616c7565))))"
            )
            && !lambda_carrier.contains("tpz_host_callable(")
            && direct_co_callback_recovery_count(&lambda_carrier) == 0,
        "Map.get lambda callbacks feeding named Array.reduce should store an arity-2 hostless cooperative callable and use the accumulator runtime cooperative driver: {lambda_carrier}"
    );
    assert_generated_python_ok_int(&lambda_carrier, 123, "Map.get Array.reduce lambda carrier");
}

#[test]
fn emits_concurrent_no_timeout_map_get_array_copy_mutating_hof_callbacks() {
    let direct_co_callback_recovery_count =
        |generated: &str| generated.matches("__co(host, __tpz_cb_").count();
    let assert_case = |label: &str,
                       source: &str,
                       driver: &str,
                       expected: i64,
                       required_fragments: &[&str],
                       forbidden_fragments: &[&str]| {
        let generated = emit_source(source);
        assert!(
            generated.contains(driver)
                && generated.contains("tpz_get(_t_63616c6c6261636b73")
                && direct_co_callback_recovery_count(&generated) == 0,
            "Map.get Array HOF {label} should recover through Map.get and use {driver}: {generated}"
        );
        for fragment in required_fragments {
            assert!(
                generated.contains(fragment),
                "Map.get Array HOF {label} missing required carrier fragment {fragment}: {generated}"
            );
        }
        for fragment in forbidden_fragments {
            assert!(
                !generated.contains(fragment),
                "Map.get Array HOF {label} should not contain forbidden fragment {fragment}: {generated}"
            );
        }
        assert_generated_python_ok_int(&generated, expected, label);
    };

    assert_case(
        "Array.filter Map.get function carrier",
        r#"
function keep(x: int) -> bool {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x > 2
}

let callbacks = map { "keep": keep }

let r = concurrent {
    slow: {
        let xs = [1, 2, 3, 4]
        match callbacks.get("keep") {
            case Some(cb) => {
                let kept = xs.filter(cb)
                (kept[0] + kept[1]) * 100 + (xs[0] + xs[1] + xs[2] + xs[3])
            }
            case None => -1
        }
    }
    fast: 0
}
r.slow + r.fast
"#,
        "yield from tpz_array_filter__co(",
        710,
        &["tpz_host_callable(_t_6b656570, host, _t_6b656570__co)"],
        &[],
    );
    assert_case(
        "Array.filter Map.get alias carrier",
        r#"
function keep(x: int) -> bool {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x > 2
}

let aliasKeep = keep
let callbacks = map { "alias": aliasKeep }

let r = concurrent {
    slow: {
        let xs = [1, 2, 3, 4]
        match callbacks.get("alias") {
            case Some(cb) => {
                let kept = xs.filter(cb)
                (kept[0] + kept[1]) * 100 + (xs[0] + xs[1] + xs[2] + xs[3])
            }
            case None => -1
        }
    }
    fast: 0
}
r.slow + r.fast
"#,
        "yield from tpz_array_filter__co(",
        710,
        &[
            "_t_616c6961734b656570",
            "tpz_host_callable(_t_6b656570, host, _t_6b656570__co)",
        ],
        &[],
    );
    assert_case(
        "Array.filter Map.get lambda carrier",
        r#"
function keep(x: int) -> bool {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x > 2
}

let callbacks = map { "plain": (x) => keep(x) }

let r = concurrent {
    slow: {
        let xs = [1, 2, 3, 4]
        match callbacks.get("plain") {
            case Some(cb) => {
                let kept = xs.filter(f: cb)
                (kept[0] + kept[1]) * 100 + (xs[0] + xs[1] + xs[2] + xs[3])
            }
            case None => -1
        }
    }
    fast: 0
}
r.slow + r.fast
"#,
        "yield from tpz_array_filter__co(",
        710,
        &[
            "tpz_cooperative_callable((lambda _t_78: _t_6b656570(host, _t_78)), (lambda _t_78: (yield from _t_6b656570__co(host, _t_78))))",
        ],
        &["tpz_host_callable("],
    );

    assert_case(
        "Array.sortedBy Map.get function carrier",
        r#"
function rank(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    10 - x
}

let callbacks = map { "rank": rank }

let r = concurrent {
    slow: {
        let xs = [1, 3, 2]
        match callbacks.get("rank") {
            case Some(cb) => {
                let sorted = xs.sortedBy(cb)
                sorted[0] * 100000 + sorted[1] * 10000 + sorted[2] * 1000 + xs[0] * 100 + xs[1] * 10 + xs[2]
            }
            case None => -1
        }
    }
    fast: 0
}
r.slow + r.fast
"#,
        "yield from tpz_array_sorted_by__co(",
        321132,
        &["tpz_host_callable(_t_72616e6b, host, _t_72616e6b__co)"],
        &[],
    );
    assert_case(
        "Array.sortedBy Map.get alias carrier",
        r#"
function rank(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    10 - x
}

let aliasRank = rank
let callbacks = map { "alias": aliasRank }

let r = concurrent {
    slow: {
        let xs = [1, 3, 2]
        match callbacks.get("alias") {
            case Some(cb) => {
                let sorted = xs.sortedBy(cb)
                sorted[0] * 100000 + sorted[1] * 10000 + sorted[2] * 1000 + xs[0] * 100 + xs[1] * 10 + xs[2]
            }
            case None => -1
        }
    }
    fast: 0
}
r.slow + r.fast
"#,
        "yield from tpz_array_sorted_by__co(",
        321132,
        &[
            "_t_616c69617352616e6b",
            "tpz_host_callable(_t_72616e6b, host, _t_72616e6b__co)",
        ],
        &[],
    );
    assert_case(
        "Array.sortedBy Map.get lambda carrier",
        r#"
function rank(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    10 - x
}

let callbacks = map { "plain": (x) => rank(x) }

let r = concurrent {
    slow: {
        let xs = [1, 3, 2]
        match callbacks.get("plain") {
            case Some(cb) => {
                let sorted = xs.sortedBy(f: cb)
                sorted[0] * 100000 + sorted[1] * 10000 + sorted[2] * 1000 + xs[0] * 100 + xs[1] * 10 + xs[2]
            }
            case None => -1
        }
    }
    fast: 0
}
r.slow + r.fast
"#,
        "yield from tpz_array_sorted_by__co(",
        321132,
        &[
            "tpz_cooperative_callable((lambda _t_78: _t_72616e6b(host, _t_78)), (lambda _t_78: (yield from _t_72616e6b__co(host, _t_78))))",
        ],
        &["tpz_host_callable("],
    );

    assert_case(
        "Array.sortBy Map.get function carrier",
        r#"
function rank(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    10 - x
}

let callbacks = map { "rank": rank }

let r = concurrent {
    slow: {
        let mut xs = [1, 3, 2]
        match callbacks.get("rank") {
            case Some(cb) => xs.sortBy(cb)
            case None => ()
        }
        xs[0] * 100 + xs[1] * 10 + xs[2]
    }
    fast: 0
}
r.slow + r.fast
"#,
        "yield from tpz_array_sort_by__co(",
        321,
        &["tpz_host_callable(_t_72616e6b, host, _t_72616e6b__co)"],
        &[],
    );
    assert_case(
        "Array.sortBy Map.get alias carrier",
        r#"
function rank(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    10 - x
}

let aliasRank = rank
let callbacks = map { "alias": aliasRank }

let r = concurrent {
    slow: {
        let mut xs = [1, 3, 2]
        match callbacks.get("alias") {
            case Some(cb) => xs.sortBy(cb)
            case None => ()
        }
        xs[0] * 100 + xs[1] * 10 + xs[2]
    }
    fast: 0
}
r.slow + r.fast
"#,
        "yield from tpz_array_sort_by__co(",
        321,
        &[
            "_t_616c69617352616e6b",
            "tpz_host_callable(_t_72616e6b, host, _t_72616e6b__co)",
        ],
        &[],
    );
    assert_case(
        "Array.sortBy Map.get lambda carrier",
        r#"
function rank(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    10 - x
}

let callbacks = map { "plain": (x) => rank(x) }

let r = concurrent {
    slow: {
        let mut xs = [1, 3, 2]
        match callbacks.get("plain") {
            case Some(cb) => xs.sortBy(f: cb)
            case None => ()
        }
        xs[0] * 100 + xs[1] * 10 + xs[2]
    }
    fast: 0
}
r.slow + r.fast
"#,
        "yield from tpz_array_sort_by__co(",
        321,
        &[
            "tpz_cooperative_callable((lambda _t_78: _t_72616e6b(host, _t_78)), (lambda _t_78: (yield from _t_72616e6b__co(host, _t_78))))",
        ],
        &["tpz_host_callable("],
    );

    assert_case(
        "Array.retain Map.get function carrier",
        r#"
function keep(x: int) -> bool {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x > 2
}

let callbacks = map { "keep": keep }

let r = concurrent {
    slow: {
        let mut xs = [1, 2, 3, 4]
        let before = xs[0] + xs[1] + xs[2] + xs[3]
        match callbacks.get("keep") {
            case Some(cb) => xs.retain(cb)
            case None => ()
        }
        before * 10 + xs[0] + xs[1]
    }
    fast: 0
}
r.slow + r.fast
"#,
        "yield from tpz_array_retain__co(",
        107,
        &["tpz_host_callable(_t_6b656570, host, _t_6b656570__co)"],
        &[],
    );
    assert_case(
        "Array.retain Map.get alias carrier",
        r#"
function keep(x: int) -> bool {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x > 2
}

let aliasKeep = keep
let callbacks = map { "alias": aliasKeep }

let r = concurrent {
    slow: {
        let mut xs = [1, 2, 3, 4]
        let before = xs[0] + xs[1] + xs[2] + xs[3]
        match callbacks.get("alias") {
            case Some(cb) => xs.retain(cb)
            case None => ()
        }
        before * 10 + xs[0] + xs[1]
    }
    fast: 0
}
r.slow + r.fast
"#,
        "yield from tpz_array_retain__co(",
        107,
        &[
            "_t_616c6961734b656570",
            "tpz_host_callable(_t_6b656570, host, _t_6b656570__co)",
        ],
        &[],
    );
    assert_case(
        "Array.retain Map.get lambda carrier",
        r#"
function keep(x: int) -> bool {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x > 2
}

let callbacks = map { "plain": (x) => keep(x) }

let r = concurrent {
    slow: {
        let mut xs = [1, 2, 3, 4]
        let before = xs[0] + xs[1] + xs[2] + xs[3]
        match callbacks.get("plain") {
            case Some(cb) => xs.retain(f: cb)
            case None => ()
        }
        before * 10 + xs[0] + xs[1]
    }
    fast: 0
}
r.slow + r.fast
"#,
        "yield from tpz_array_retain__co(",
        107,
        &[
            "tpz_cooperative_callable((lambda _t_78: _t_6b656570(host, _t_78)), (lambda _t_78: (yield from _t_6b656570__co(host, _t_78))))",
        ],
        &["tpz_host_callable("],
    );
}
